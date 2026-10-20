//! 设备上的数据: 配置、章节缓存、阅读进度、字体缓存。缓存格式与 Python 版 (0.8.x) 一致, 从 0.8 升级不丢进度:
//! - 设置 `data/settings.json` (保留未知键与键顺序; 键名沿用 Python 版), 账号另存 (见 `account.rs`)
//! - 章节 `cache/content/sha256("{book}:{sort}:{convert}").json` = 服务器 GetChapterContent 响应
//! - 进度 `cache/progress/{book}-{sort}[-{convert}].json` = `{"path", "offset", "page"}`
//! - 字体 `cache/fonts/sha256(absolute_url(api_server, Font)).woff2`; 解码后的 TTF 另存为 `<同名>.ttf`
//!   (WOFF2 解码在设备上要 ~350 ms, 只做一次)
//!
//! 1.x 新增、Python 版没有的数据 (书签、阅读统计、更新检查、休眠封面) 放在 `data/` (见 `userdata.rs` 等), 不随缓存清理。

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const DEFAULT_APP_DIR: &str = "/mnt/us/extensions/kinnovel";
pub const DEFAULT_API_SERVER: &str = "https://api.lightnovel.life";
/// 章节 HTML 里相对地址 (正文图片等) 的站点根。Python 版用的旧域名 www.lightnovel.app 在国内网络下 DNS 被污染 (解析到
/// 108.160.x 一类地址, 连接超时), 网页版现在的域名是 www.lightnovel.life (Cloudflare)。
pub const SITE_BASE: &str = "https://www.lightnovel.life";

pub fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 临时文件名里的标记: `<文件名>.tmp-kn<pid>-<序号>`; 1.6.3 及以前是 `<去掉扩展名>.tmp-kn`。
/// 启动修剪缓存时删掉两种残留 (见 [`is_tmp_leftover`])。
const TMP_MARK: &str = ".tmp-kn";

/// 用户数据 (`data/` 下的设置、账号、书签、统计、更新状态, 以及阅读进度): 写临时文件、fsync 后再改名,
/// 断电 / 强制重启后要么是旧内容要么是新内容 (`/mnt/us` 是 vfat, 不 fsync 可能留下 0 字节文件)。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_via_tmp(path, bytes, true)
}

/// 可再下载的缓存 (章节、字体、图片、漫画清单与页面): 同样经临时文件改名, 但不 fsync (vfat 上 fsync 会刷整个块设备)。
/// 断电后可能留下半截文件, 读取方解析失败时删掉重下 (章节见 `pages/reader.rs`, 字体见 [`load_chapter_font`])。
pub fn cache_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_via_tmp(path, bytes, false)
}

fn write_via_tmp(path: &Path, bytes: &[u8], durable: bool) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent();
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)?;
    }
    // 临时文件名唯一: 同一文件可能被两个线程同时写 (过期章节的后台刷新与前台下载)
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!("{TMP_MARK}{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp = path.with_file_name(name);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        if durable {
            f.sync_all()?;
        }
        drop(f);
        std::fs::rename(&tmp, path)?;
        // 改名本身也落盘 (Linux 上目录可以 fsync; 失败无妨, 最坏是断电后还是旧内容)
        #[cfg(unix)]
        if durable {
            if let Some(dir) = dir {
                if let Ok(d) = std::fs::File::open(dir) {
                    let _ = d.sync_all();
                }
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// 是不是 [`atomic_write`] / [`cache_write`] 残留的临时文件 (新旧两种命名)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn is_tmp_leftover(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    match name.rfind(TMP_MARK) {
        Some(i) => {
            let rest = &name[i + TMP_MARK.len()..];
            // 旧: 以 ".tmp-kn" 结尾; 新: ".tmp-kn<pid>-<序号>"
            rest.is_empty() || rest.split_once('-').is_some_and(|(a, b)| !a.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) && !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
        }
        None => false,
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub app_dir: PathBuf,
}

impl Paths {
    /// 启动脚本通过 `KN_APP_DIR` 传入扩展目录; 主机上可指向一份设备缓存的拷贝。
    pub fn from_env() -> Self {
        let app_dir = std::env::var_os("KN_APP_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_APP_DIR));
        Paths { app_dir }
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.app_dir.join("cache")
    }

    pub fn chapter_file(&self, book_id: i64, sort_num: i64, convert: &str) -> PathBuf {
        let key = format!("{book_id}:{sort_num}:{convert}");
        self.cache_dir().join("content").join(sha256_hex(&key) + ".json")
    }

    /// 插图/封面的灰度 JPEG 缓存 (与 Python `ImageCache._path` 一致: sha256("{url}#h={档位}").jpg)。
    pub fn image_file(&self, url: &str, height: u32) -> PathBuf {
        let key = format!("{url}#h={}", kn_render::height_bucket(height));
        self.cache_dir().join("covers").join(sha256_hex(&key) + ".jpg")
    }

    /// 插图原图 (不带 CDN 缩放参数) 的缓存: 只有 1.0 的全屏预览用, 键与缩放档位区分开。
    pub fn original_image_file(&self, url: &str) -> PathBuf {
        self.cache_dir().join("covers").join(sha256_hex(&format!("{url}#original")) + ".jpg")
    }

    /// 漫画缓存根目录 (见 `crate::comic`)。
    pub fn comic_dir(&self) -> PathBuf {
        self.cache_dir().join("comic")
    }

    pub fn comic_chapter_file(&self, chapter_id: i64) -> PathBuf {
        self.comic_dir().join("chapters").join(format!("{chapter_id}.json"))
    }

    pub fn comic_book_file(&self, book_id: i64) -> PathBuf {
        self.comic_dir().join("books").join(format!("{book_id}.json"))
    }

    /// 漫画页面图片 (键的规则与插图缓存相同, 但单独放一个目录、单独的配额)。
    pub fn comic_image_file(&self, url: &str, height: u32) -> PathBuf {
        let key = format!("{url}#h={}", kn_render::height_bucket(height));
        self.comic_dir().join("pages").join(sha256_hex(&key) + ".img")
    }

    pub fn comic_progress_file(&self, book_id: i64) -> PathBuf {
        self.cache_dir().join("progress").join(format!("comic-{book_id}.json"))
    }

    pub fn progress_file(&self, book_id: i64, sort_num: i64, convert: &str) -> PathBuf {
        let suffix = if convert.is_empty() { String::new() } else { format!("-{convert}") };
        self.cache_dir().join("progress").join(format!("{book_id}-{sort_num}{suffix}.json"))
    }

    /// 章节字体缓存文件 (与 Python `ensure_font` 的命名一致)。
    pub fn font_file(&self, api_server: &str, font_url: &str) -> Option<PathBuf> {
        if font_url.is_empty() {
            return None;
        }
        let url = kn_text::absolute_url(api_server, font_url);
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return None;
        }
        let name = url.split('?').next().unwrap_or("").rsplit('/').next().unwrap_or("");
        let suffix = match name.rsplit_once('.') {
            Some((_, ext)) if ["ttf", "otf", "woff", "woff2"].contains(&ext.to_lowercase().as_str()) => format!(".{}", ext.to_lowercase()),
            _ => ".font".to_string(),
        };
        Some(self.cache_dir().join("fonts").join(sha256_hex(&url) + &suffix))
    }
}

/// 设置 (`data/settings.json`)。只读写需要的键, 其余原样保留; 缺的键用代码里的默认值。
pub struct Config {
    path: PathBuf,
    data: Map<String, Value>,
}

impl Config {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default();
        Config { path, data }
    }

    pub fn in_memory() -> Self {
        Config { path: PathBuf::new(), data: Map::new() }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.data.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(default)
    }

    pub fn float(&self, key: &str, default: f64) -> f64 {
        self.data.get(key).and_then(Value::as_f64).unwrap_or(default)
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.data.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    pub fn string(&self, key: &str, default: &str) -> String {
        match self.data.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) => String::new(),
            _ => default.to_string(),
        }
    }

    /// 字符串数组 (非字符串项跳过); 键不存在或不是数组时为空。
    pub fn strings(&self, key: &str) -> Vec<String> {
        match self.data.get(key) {
            Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(str::to_string).collect(),
            _ => Vec::new(),
        }
    }

    /// 设置并立即落盘 (缩进 2 空格、非 ASCII 原样, 与 Python `json.dump(indent=2, ensure_ascii=False)` 一致)。
    pub fn set(&mut self, key: &str, value: Value) {
        if self.data.get(key) == Some(&value) {
            return;
        }
        self.data.insert(key.to_string(), value);
        if self.path.as_os_str().is_empty() {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.data) {
            if let Err(e) = atomic_write(&self.path, text.as_bytes()) {
                eprintln!("[config] 保存失败: {e}");
            }
        }
    }

    pub fn api_server(&self) -> String {
        let s = self.string("api_server", DEFAULT_API_SERVER);
        let s = s.trim_end_matches('/');
        if s.is_empty() { DEFAULT_API_SERVER.to_string() } else { s.to_string() }
    }

    pub fn convert(&self) -> String {
        self.string("convert", "")
    }
}

/// 缓存的章节 (GetChapterContent 响应)。
#[derive(Clone, Debug, Default)]
pub struct Chapter {
    pub book_id: i64,
    pub chapter_id: i64,
    pub sort_num: i64,
    pub title: String,
    pub book_name: String,
    pub content: String,
    pub font: String,
    pub chapters: Vec<String>,
    /// 服务器记录的阅读位置 (只在 ChapterId 是本章时有意义)
    pub server_position: Option<(i64, String)>,
}

fn as_i64(v: Option<&Value>) -> i64 {
    v.and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0)
}

fn as_string(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

impl Chapter {
    pub fn parse(bytes: &[u8], with_content: bool) -> Option<Chapter> {
        let root: Value = serde_json::from_slice(bytes).ok()?;
        Self::from_value(&root, with_content)
    }

    /// 同 [`Chapter::parse`], 但输入是已解好的 JSON (刚下载的章节不用再从字节解析一遍)。
    pub fn from_value(root: &Value, with_content: bool) -> Option<Chapter> {
        let c = root.get("Chapter").unwrap_or(root);
        let chapters = c
            .get("Chapters")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect())
            .unwrap_or_default();
        let server_position = root.get("ReadPosition").map(|p| (as_i64(p.get("ChapterId")), as_string(p.get("Position"))));
        Some(Chapter {
            book_id: as_i64(c.get("BookId")),
            chapter_id: as_i64(c.get("Id")),
            sort_num: as_i64(c.get("SortNum")),
            title: as_string(c.get("Title")),
            book_name: as_string(c.get("BookName")),
            content: if with_content { as_string(c.get("Content")) } else { String::new() },
            font: as_string(c.get("Font")),
            chapters,
            server_position,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub path: String,
    pub offset: usize,
    pub page: usize,
}

pub fn load_progress(file: &Path) -> Option<Progress> {
    let v: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let path = v.get("path")?.as_str()?.to_string();
    if path.is_empty() {
        return None;
    }
    let offset = v.get("offset").and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
    let page = v.get("page").and_then(Value::as_i64).unwrap_or(0).max(0) as usize;
    Some(Progress { path, offset, page })
}

pub fn save_progress(file: &Path, p: &Progress) -> std::io::Result<()> {
    // 与 Python json.dumps(..., ensure_ascii=False) 的格式逐字节一致 (", " / ": " 分隔)
    let path = serde_json::to_string(&p.path).unwrap_or_else(|_| "\".\"".into());
    let text = format!("{{\"path\": {path}, \"offset\": {}, \"page\": {}}}", p.offset, p.page);
    atomic_write(file, text.as_bytes())
}

/// 最近一次阅读 (书架顶部的 "继续阅读")。Rust 版自己的文件, Python 版忽略它。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastRead {
    pub book_id: i64,
    pub sort_num: i64,
    /// 漫画 (用漫画阅读页打开, 按 `chapter_id` 定位话)
    pub comic: bool,
    pub chapter_id: i64,
    pub book_name: String,
    pub chapter_title: String,
    /// 0 起的页号与总页数 (总页数可能是排版到一半时的下限)
    pub page: usize,
    pub pages: usize,
    /// Unix 秒
    pub time: i64,
    /// 封面地址 (书目接口给的; 不知道时为空, 书架卡片改用第一章的卷首图)
    pub cover: String,
}

impl Paths {
    pub fn last_read_file(&self) -> PathBuf {
        self.cache_dir().join("kn_last_read.json")
    }
}

pub fn load_last_read(paths: &Paths) -> Option<LastRead> {
    let v: Value = serde_json::from_slice(&std::fs::read(paths.last_read_file()).ok()?).ok()?;
    Some(LastRead {
        book_id: as_i64(v.get("book_id")),
        sort_num: as_i64(v.get("sort_num")),
        comic: v.get("comic").and_then(Value::as_bool).unwrap_or(false),
        chapter_id: as_i64(v.get("chapter_id")),
        book_name: as_string(v.get("book_name")),
        chapter_title: as_string(v.get("chapter_title")),
        page: as_i64(v.get("page")).max(0) as usize,
        pages: as_i64(v.get("pages")).max(0) as usize,
        time: as_i64(v.get("time")),
        cover: as_string(v.get("cover")),
    })
    .filter(|r| r.book_id > 0 && if r.comic { r.chapter_id > 0 } else { r.sort_num > 0 })
}

/// 保存最近阅读。`cover` 为空时沿用上一条记录里同一本书的封面地址。
pub fn save_last_read(paths: &Paths, r: &LastRead) -> std::io::Result<()> {
    let cover = if r.cover.is_empty() {
        load_last_read(paths).filter(|old| old.book_id == r.book_id).map(|old| old.cover).unwrap_or_default()
    } else {
        r.cover.clone()
    };
    let v = serde_json::json!({
        "book_id": r.book_id, "sort_num": r.sort_num, "comic": r.comic, "chapter_id": r.chapter_id, "book_name": r.book_name,
        "chapter_title": r.chapter_title, "page": r.page, "pages": r.pages, "time": r.time, "cover": cover,
    });
    atomic_write(&paths.last_read_file(), v.to_string().as_bytes())
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 读取章节字体并得到可直接加载的 TTF 字节: 优先用解码好的 `.ttf` 旁路缓存, 否则解码 WOFF2 并写回旁路。
/// 在后台线程调用。
/// 缓存坏了 (半截文件、解码失败) 时删掉原文件和旁路缓存, 下次打开重新下载 (`net::ensure_font`)。
pub fn load_chapter_font(file: &Path) -> Result<Vec<u8>, String> {
    let sidecar = font_sidecar(file);
    if let Ok(bytes) = std::fs::read(&sidecar) {
        if sfnt_complete(&bytes) {
            return Ok(bytes);
        }
        // 半截的旁路缓存: 删掉, 从 WOFF2 重新解码
        eprintln!("[font] 解码缓存 {} 不完整, 重新解码", sidecar.display());
        let _ = std::fs::remove_file(&sidecar);
    }
    let raw = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if raw.starts_with(b"wOF2") {
        let ttf = match kn_render::decode_woff2(&raw) {
            Ok(ttf) if sfnt_complete(&ttf) => ttf,
            Ok(_) => {
                forget_chapter_font(file);
                return Err(format!("{}: 解码结果不完整", file.display()));
            }
            Err(e) => {
                forget_chapter_font(file);
                return Err(format!("{}: {e}", file.display()));
            }
        };
        if let Err(e) = cache_write(&sidecar, &ttf) {
            eprintln!("[font] 无法写入解码缓存 {}: {e}", sidecar.display());
        }
        Ok(ttf)
    } else if sfnt_complete(&raw) {
        Ok(raw)
    } else {
        forget_chapter_font(file);
        Err(format!("{}: 字体文件不完整", file.display()))
    }
}

/// 解码后的 TTF 旁路缓存 `<字体文件>.ttf`。
pub fn font_sidecar(file: &Path) -> PathBuf {
    PathBuf::from(format!("{}.ttf", file.display()))
}

/// 删掉章节字体与它的旁路缓存 (字体加载失败时调用)。
pub fn forget_chapter_font(file: &Path) {
    eprintln!("[font] 字体缓存 {} 损坏, 删除", file.display());
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_file(font_sidecar(file));
}

/// TTF/OTF 是否完整: 表目录里每张表都在文件范围内 (截断的文件最后几张表会越界)。
fn sfnt_complete(bytes: &[u8]) -> bool {
    let be32 = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    if !matches!(be32(0), Some(0x0001_0000 | 0x4F54_544F | 0x7472_7565)) {
        // 0x00010000 / 'OTTO' / 'true'
        return false;
    }
    let tables = match bytes.get(4..6) {
        Some(b) => u16::from_be_bytes([b[0], b[1]]) as usize,
        None => return false,
    };
    tables > 0
        && (0..tables).all(|i| {
            let rec = 12 + i * 16;
            match (be32(rec + 8), be32(rec + 12)) {
                (Some(offset), Some(len)) => (offset as u64 + len as u64) <= bytes.len() as u64,
                _ => false,
            }
        })
}

/// 把目录 (递归) 修剪到 `limit` 字节以内: 按修改时间从旧到新删除, 再删掉空子目录。
/// 返回删除的字节数。与 Python `utils.prune_cache` 一致 (缓存命中时 touch mtime, 所以是 LRU)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn prune_dir(dir: &Path, limit: u64) -> u64 {
    fn walk(dir: &Path, files: &mut Vec<(std::time::SystemTime, u64, PathBuf)>, dirs: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, files, dirs);
                dirs.push(path);
            } else if meta.is_file() {
                files.push((meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len(), path));
            }
        }
    }
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    walk(dir, &mut files, &mut dirs);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= limit {
        return 0;
    }
    files.sort();
    let mut removed = 0;
    for (_, size, path) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += size;
            total -= size;
        }
    }
    for d in dirs {
        let _ = std::fs::remove_dir(d); // 非空时失败, 正好
    }
    removed
}

/// 启动时按 `cache_limit_mb` 修剪缓存 (Python `PageContext.prune_cache`: 四个目录各占四分之一);
/// 漫画页面图片单独按 `comic_mb` 修剪 (清单与书目很小, 不清理)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn prune_cache(paths: &Paths, limit_mb: i64, comic_mb: i64) -> u64 {
    // 先删掉断电 / 崩溃留下的临时文件 (缓存与用户数据目录)
    let leftovers = remove_tmp_leftovers(&paths.cache_dir()) + remove_tmp_leftovers(&paths.data_dir());
    let per_dir = (limit_mb.max(16) as u64 * 1024 * 1024 / 4).max(1);
    let novel: u64 = ["covers", "fonts", "images", "content"].iter().map(|name| prune_dir(&paths.cache_dir().join(name), per_dir)).sum();
    leftovers + novel + prune_dir(&paths.comic_dir().join("pages"), comic_mb.max(16) as u64 * 1024 * 1024)
}

/// 递归删掉目录里残留的临时文件, 返回删除的字节数。本进程正在写的 (名字里是自己的 pid) 不动:
/// 修剪在后台线程里跑, 同时可能有下载在写缓存。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn remove_tmp_leftovers(dir: &Path) -> u64 {
    let own = format!("{TMP_MARK}{}-", std::process::id());
    let mut removed = 0;
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            removed += remove_tmp_leftovers(&path);
        } else if is_tmp_leftover(&path)
            && !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(&own))
            && std::fs::remove_file(&path).is_ok()
        {
            removed += meta.len();
        }
    }
    removed
}

/// 缓存命中时刷新修改时间, 让修剪顺序成为真正的 LRU。
pub fn touch(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

/// 一本书缓存的第一章 (序号 1) 里的第一张图 (卷首图, 通常就是封面); 没缓存或没有图返回 None。
/// 直接按文件名找 (当前的繁简转换优先, 再试其余几种), 不扫描整个缓存目录。
pub fn first_image_of_book(paths: &Paths, book_id: i64, convert: &str) -> Option<String> {
    let mut converts = vec![convert];
    converts.extend(["", "t2s", "s2t"].into_iter().filter(|c| *c != convert));
    let chapter = converts.into_iter().find_map(|c| {
        let ch = Chapter::parse(&std::fs::read(paths.chapter_file(book_id, 1, c)).ok()?, true)?;
        (ch.book_id == book_id && ch.sort_num == 1).then_some(ch)
    })?;
    kn_text::extract_blocks(&chapter.content, SITE_BASE)
        .into_iter()
        .find(|b| b.kind == kn_text::BlockKind::Image && !b.source_url.is_empty())
        .map(|b| b.source_url)
}

/// 书架列表用的章节信息 (缓存文件里只取这几项)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CachedChapter {
    pub book_id: i64,
    pub sort_num: i64,
    pub title: String,
    pub book_name: String,
}

/// 章节缓存里书架要的字段 (原始 JSON 值, 取值规则与 [`Chapter::from_value`] 相同)。
#[derive(Default)]
struct HeadFields {
    book_id: Option<Value>,
    sort_num: Option<Value>,
    title: Option<Value>,
    book_name: Option<Value>,
}

/// 响应根对象: 有 `Chapter` 键时字段取自它, 否则取自根 (同 `root.get("Chapter").unwrap_or(root)`)。
struct ChapterHead {
    root: HeadFields,
    chapter: Option<HeadFields>,
}

/// 只取 [`HeadFields`] 的解析器; 其余键 (正文 `Content`、目录 `Chapters` 等) 用 `IgnoredAny` 跳过, 只扫描不分配。
struct HeadVisitor {
    root: bool,
}

impl<'de> serde::de::Visitor<'de> for HeadVisitor {
    type Value = ChapterHead;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("章节对象")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<ChapterHead, A::Error> {
        let mut head = ChapterHead { root: HeadFields::default(), chapter: None };
        while let Some(key) = map.next_key::<String>()? {
            let f = &mut head.root;
            match key.as_str() {
                "Chapter" if self.root => head.chapter = Some(map.next_value_seed(HeadVisitor { root: false })?.root),
                "BookId" => f.book_id = Some(map.next_value()?),
                "SortNum" => f.sort_num = Some(map.next_value()?),
                "Title" => f.title = Some(map.next_value()?),
                "BookName" => f.book_name = Some(map.next_value()?),
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(head)
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for HeadVisitor {
    type Value = ChapterHead;

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<ChapterHead, D::Error> {
        d.deserialize_map(self)
    }
}

impl CachedChapter {
    /// 从缓存文件的字节里取书架要的字段。不是合法的 JSON 对象时返回 None, 与 [`Chapter::parse`] 一样跳过;
    /// 结果与 `Chapter::parse(bytes, false)` 的对应字段相同。
    pub fn parse(bytes: &[u8]) -> Option<CachedChapter> {
        use serde::de::DeserializeSeed;
        let mut de = serde_json::Deserializer::from_slice(bytes);
        let head = HeadVisitor { root: true }.deserialize(&mut de).ok()?;
        de.end().ok()?;
        let f = head.chapter.unwrap_or(head.root);
        Some(CachedChapter {
            book_id: as_i64(f.book_id.as_ref()),
            sort_num: as_i64(f.sort_num.as_ref()),
            title: as_string(f.title.as_ref()),
            book_name: as_string(f.book_name.as_ref()),
        })
    }
}

/// 缓存目录里的章节 (书名、章节名、序号), 供离线书架使用。按书 id、序号排序。
pub fn list_cached_chapters(paths: &Paths) -> Vec<CachedChapter> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(paths.cache_dir().join("content")) else {
        return out;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(ch) = std::fs::read(&path).ok().and_then(|b| CachedChapter::parse(&b)) {
            if ch.book_id > 0 {
                out.push(ch);
            }
        }
    }
    out.sort_by(|a, b| (a.book_id, a.sort_num).cmp(&(b.book_id, b.sort_num)));
    out
}

#[cfg(test)]
mod prune_tests {
    use super::*;

    #[test]
    fn prune_removes_oldest_first_until_under_limit() {
        let dir = std::env::temp_dir().join(format!("kn_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let base = std::time::SystemTime::now() - std::time::Duration::from_secs(1000);
        for (i, name) in ["a", "b", "sub/c"].iter().enumerate() {
            let p = dir.join(name);
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
            f.set_modified(base + std::time::Duration::from_secs(i as u64 * 10)).unwrap();
        }
        assert_eq!(prune_dir(&dir, 1000), 0);
        assert_eq!(prune_dir(&dir, 150), 200);
        assert!(!dir.join("a").exists() && !dir.join("b").exists());
        assert!(dir.join("sub/c").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn truncated_fonts_are_detected_and_deleted() {
        let ttf: &[u8] = include_bytes!("../data/KinNovelSymbols.ttf");
        assert!(sfnt_complete(ttf));
        assert!(!sfnt_complete(&ttf[..ttf.len() - 1]));
        assert!(!sfnt_complete(&ttf[..ttf.len() / 2]));
        assert!(!sfnt_complete(&ttf[..10]));
        assert!(!sfnt_complete(b""));

        let dir = std::env::temp_dir().join(format!("kn_font_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f.ttf");
        // 半截的旁路缓存被丢弃, 改用原文件
        std::fs::write(&file, ttf).unwrap();
        std::fs::write(font_sidecar(&file), &ttf[..ttf.len() / 3]).unwrap();
        assert_eq!(load_chapter_font(&file).unwrap().len(), ttf.len());
        assert!(!font_sidecar(&file).exists());
        // 半截的字体文件: 报错并删掉, 下次重新下载
        std::fs::write(&file, &ttf[..ttf.len() / 2]).unwrap();
        assert!(load_chapter_font(&file).is_err());
        assert!(!file.exists());
        // 解不开的 WOFF2: 连同旁路一起删
        std::fs::write(&file, b"wOF2\0\0\0\0garbage").unwrap();
        std::fs::write(font_sidecar(&file), b"x").unwrap();
        assert!(load_chapter_font(&file).is_err());
        assert!(!file.exists() && !font_sidecar(&file).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tmp_leftovers_old_and_new_names_are_removed() {
        let dir = std::env::temp_dir().join(format!("kn_tmpclean_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("content")).unwrap();
        let own = format!("x.json.tmp-kn{}-3", std::process::id());
        let names = ["content/abc.tmp-kn", "content/abc.json.tmp-kn99999999-0", "content/abc.json", "1-2.json", "session.json.tmp-7", "a.tmp-knx", own.as_str()];
        for name in names {
            std::fs::write(dir.join(name), b"12345").unwrap();
        }
        assert_eq!(remove_tmp_leftovers(&dir), 10);
        assert!(!dir.join("content/abc.tmp-kn").exists());
        assert!(!dir.join("content/abc.json.tmp-kn99999999-0").exists());
        for keep in ["content/abc.json", "1-2.json", "session.json.tmp-7", "a.tmp-knx", own.as_str()] {
            assert!(dir.join(keep).exists(), "{keep}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_leave_no_tmp_files() {
        let dir = std::env::temp_dir().join(format!("kn_atomic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        atomic_write(&dir.join("d/a.json"), b"one").unwrap();
        atomic_write(&dir.join("d/a.json"), b"two").unwrap();
        cache_write(&dir.join("d/b.jpg"), b"img").unwrap();
        assert_eq!(std::fs::read(dir.join("d/a.json")).unwrap(), b"two");
        assert_eq!(std::fs::read(dir.join("d/b.jpg")).unwrap(), b"img");
        assert_eq!(std::fs::read_dir(dir.join("d")).unwrap().count(), 2);
        // 写不进去 (目标是目录) 时临时文件也删掉
        std::fs::create_dir_all(dir.join("d/c.json")).unwrap();
        assert!(atomic_write(&dir.join("d/c.json"), b"x").is_err());
        assert_eq!(std::fs::read_dir(dir.join("d")).unwrap().count(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn concurrent_writes_to_one_file_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("kn_atomic_mt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("c.json");
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let file = file.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        // Windows 上目标被另一个线程改名占用时可能拒绝, 只要求不留半截内容
                        let _ = cache_write(&file, format!("{{\"v\":{i}}}").as_bytes());
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("{\"v\":") && text.ends_with('}'), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_names_match_python() {
        // 与 Python stable_cache_name / _chapter_cache_path / ensure_font 对照 (设备缓存实测文件名)
        let paths = Paths { app_dir: PathBuf::from("/x") };
        let f = paths.chapter_file(20644, 4, "t2s");
        assert!(f.ends_with("aa3455fd7b607165624bc933215d20735ca784e1b2160b64f8c430833138e386.json"), "{}", f.display());
        let p = paths.progress_file(20644, 4, "t2s");
        assert!(p.ends_with("20644-4-t2s.json"));
        assert!(paths.progress_file(1, 2, "").ends_with("1-2.json"));
    }

    #[test]
    fn config_keeps_unknown_keys_and_order() {
        let dir = std::env::temp_dir().join(format!("kn-config-test-{}", std::process::id()));
        let file = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "{\n  \"zeta\": 1,\n  \"font_size\": 48,\n  \"alpha\": \"中文\"\n}").unwrap();
        let mut c = Config::load(file.clone());
        assert_eq!(c.int("font_size", 0), 48);
        c.set("font_size", Value::from(52));
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, "{\n  \"zeta\": 1,\n  \"font_size\": 52,\n  \"alpha\": \"中文\"\n}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 书架扫描用的轻量解析与完整解析取到的字段一致 (包括各种不规整的输入)。
    #[test]
    fn cached_chapter_head_matches_full_parse() {
        let cases: &[&str] = &[
            r#"{"Chapter":{"BookId":7,"Id":70,"SortNum":3,"Title":"第三章","BookName":"书","Content":"<p>正文\"\\u4e2d</p>","Chapters":["a","b"]},"ReadPosition":{"ChapterId":70}}"#,
            r#"{"ReadPosition":null,"Chapter":{"Content":"x","BookId":"12","SortNum":"4","Title":null,"BookName":5}}"#,
            r#"{"BookId":9,"SortNum":1,"Title":"根上的","BookName":"根","Content":"y"}"#,
            r#"{"Chapter":{"BookId":1,"Chapter":{"BookId":2},"SortNum":2.5}}"#,
            r#"{"Chapter":{"BookId":1,"SortNum":1},"Chapter":{"BookId":3,"SortNum":6,"Title":"后一个"}}"#,
            r#"{"Chapter":{"BookId":1,"Title":"t"},"BookId":99}"#,
            r#"{"BookId":1,"BookId":2}"#,
            r#"{}"#,
        ];
        for text in cases {
            let full = Chapter::parse(text.as_bytes(), false).expect(text);
            let head = CachedChapter::parse(text.as_bytes()).expect(text);
            assert_eq!(head, CachedChapter { book_id: full.book_id, sort_num: full.sort_num, title: full.title, book_name: full.book_name }, "{text}");
        }
        // 两边都不认的输入
        for bad in [&b"{\"Chapter\":{\"BookId\":1}"[..], b"{\"BookId\":1} x", b"", b"[1,2]", b"{\"Chapter\":{\"BookId\":1,\"Content\":\"abc}}"] {
            assert!(Chapter::parse(bad, false).is_none_or(|c| c.book_id == 0));
            assert!(CachedChapter::parse(bad).is_none_or(|c| c.book_id == 0));
        }
    }

    /// 合成的章节缓存: `books` 本书, 每本 `per_book` 章, 正文约 `content_kb` KB。
    fn synthetic_cache(dir: &Path, books: i64, per_book: i64, content_kb: usize) -> Paths {
        let paths = Paths { app_dir: dir.to_path_buf() };
        let para = "<p>　　这是一段用来凑长度的正文，里面有“引号”和\\n转义。</p>";
        let content: String = std::iter::repeat_n(para, content_kb * 1024 / para.len() + 1).collect();
        for book in 1..=books {
            let titles: Vec<String> = (1..=per_book.max(40)).map(|i| format!("第{i}章 标题")).collect();
            for sort in 1..=per_book {
                let body = if sort == 1 { format!("<img src=\"https://img.example/{book}.jpg\">{content}") } else { content.clone() };
                let v = serde_json::json!({
                    "Chapter": {"Id": book * 1000 + sort, "BookId": 30000 + book, "SortNum": sort, "Title": format!("第{sort}章"),
                                "BookName": format!("书{book}"), "Content": body, "Font": "/font/x.woff2", "Chapters": titles},
                    "ReadPosition": {"ChapterId": 0, "Position": ""},
                });
                std::fs::create_dir_all(paths.cache_dir().join("content")).unwrap();
                std::fs::write(paths.chapter_file(30000 + book, sort, ""), serde_json::to_vec(&v).unwrap()).unwrap();
            }
        }
        paths
    }

    #[test]
    fn scan_and_first_image_read_the_cache() {
        let dir = std::env::temp_dir().join(format!("kn-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = synthetic_cache(&dir, 3, 4, 2);
        // 损坏的文件和非 json 文件跳过
        std::fs::write(paths.cache_dir().join("content/broken.json"), b"{\"Chapter\":{\"BookId\":5").unwrap();
        std::fs::write(paths.cache_dir().join("content/x.tmp"), b"{}").unwrap();
        let list = list_cached_chapters(&paths);
        assert_eq!(list.len(), 12);
        assert_eq!(list[0], CachedChapter { book_id: 30001, sort_num: 1, title: "第1章".into(), book_name: "书1".into() });
        assert!(list.windows(2).all(|w| (w[0].book_id, w[0].sort_num) < (w[1].book_id, w[1].sort_num)));
        assert_eq!(first_image_of_book(&paths, 30002, "").as_deref(), Some("https://img.example/2.jpg"));
        // 当前设置是繁转简, 但只缓存了不转换的版本
        assert_eq!(first_image_of_book(&paths, 30003, "t2s").as_deref(), Some("https://img.example/3.jpg"));
        assert_eq!(first_image_of_book(&paths, 30009, ""), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 计时: `cargo test -p kn-app --release -- --ignored --nocapture scan_timing`
    #[test]
    #[ignore]
    fn scan_timing() {
        let dir = std::env::temp_dir().join(format!("kn-scan-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 300 章, 每章正文约 30 KB (约一万汉字), 目录 100 项
        let paths = synthetic_cache(&dir, 3, 100, 30);
        let full = || {
            let mut n = 0;
            for e in std::fs::read_dir(paths.cache_dir().join("content")).unwrap().flatten() {
                n += Chapter::parse(&std::fs::read(e.path()).unwrap(), false).is_some() as usize;
            }
            n
        };
        for round in 0..3 {
            let t = std::time::Instant::now();
            let a = full();
            let full_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            let b = list_cached_chapters(&paths).len();
            let head_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            let reads: usize = std::fs::read_dir(paths.cache_dir().join("content")).unwrap().flatten().map(|e| std::fs::read(e.path()).unwrap().len()).sum();
            let read_ms = t.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(a, b);
            println!("round {round}: {b} 章 {} MB: 完整解析 {full_ms:.1} ms, 只取字段 {head_ms:.1} ms, 只读文件 {read_ms:.1} ms", reads / 1_000_000);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn progress_round_trip() {
        let dir = std::env::temp_dir().join(format!("kn-progress-test-{}", std::process::id()));
        let file = dir.join("1-2.json");
        let p = Progress { path: "./p[3]".into(), offset: 7, page: 2 };
        save_progress(&file, &p).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"path\": \"./p[3]\", \"offset\": 7, \"page\": 2}");
        assert_eq!(load_progress(&file), Some(p));
        std::fs::remove_dir_all(&dir).ok();
    }
}
