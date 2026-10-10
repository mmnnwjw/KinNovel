//! 设备上的数据: 配置、章节缓存、阅读进度、字体缓存。格式与 Python 版 (0.8.x) 完全一致, 两个版本可以共用
//! 同一个扩展目录 (`/mnt/us/extensions/kinnovel`):
//! - 配置 `bin/config.json` (保留未知键与键顺序; 含账号信息, 不要打印)
//! - 章节 `cache/content/sha256("{book}:{sort}:{convert}").json` = 服务器 GetChapterContent 响应
//! - 进度 `cache/progress/{book}-{sort}[-{convert}].json` = `{"path", "offset", "page"}`
//! - 字体 `cache/fonts/sha256(absolute_url(api_server, Font)).woff2`; 解码后的 TTF 另存为 `<同名>.ttf`
//!   (WOFF2 解码在设备上要 ~350 ms, 只做一次)
//!
//! 以后会拆成 kn-store crate (Phase 3, 连同会话与网络缓存)。

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const DEFAULT_APP_DIR: &str = "/mnt/us/extensions/kinnovel";
pub const DEFAULT_API_SERVER: &str = "https://api.lightnovel.life";
/// 正文图片等相对链接的站点根 (与 Python ReaderDocument 一致)
pub const SITE_BASE: &str = "https://www.lightnovel.app";

pub fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 先写临时文件再改名, 断电不留半个文件。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp-kn");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
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

    pub fn config_file(&self) -> PathBuf {
        self.app_dir.join("bin").join("config.json")
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

/// `bin/config.json`。只读写需要的键, 其余原样保留。
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
        let c = root.get("Chapter").unwrap_or(&root);
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
    pub book_name: String,
    pub chapter_title: String,
    /// 0 起的页号与总页数 (总页数可能是排版到一半时的下限)
    pub page: usize,
    pub pages: usize,
    /// Unix 秒
    pub time: i64,
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
        book_name: as_string(v.get("book_name")),
        chapter_title: as_string(v.get("chapter_title")),
        page: as_i64(v.get("page")).max(0) as usize,
        pages: as_i64(v.get("pages")).max(0) as usize,
        time: as_i64(v.get("time")),
    })
    .filter(|r| r.book_id > 0 && r.sort_num > 0)
}

pub fn save_last_read(paths: &Paths, r: &LastRead) -> std::io::Result<()> {
    let v = serde_json::json!({
        "book_id": r.book_id, "sort_num": r.sort_num, "book_name": r.book_name,
        "chapter_title": r.chapter_title, "page": r.page, "pages": r.pages, "time": r.time,
    });
    atomic_write(&paths.last_read_file(), v.to_string().as_bytes())
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 读取章节字体并得到可直接加载的 TTF 字节: 优先用解码好的 `.ttf` 旁路缓存, 否则解码 WOFF2 并写回旁路。
/// 在后台线程调用。
pub fn load_chapter_font(file: &Path) -> Result<Vec<u8>, String> {
    let sidecar = PathBuf::from(format!("{}.ttf", file.display()));
    if let Ok(bytes) = std::fs::read(&sidecar) {
        if bytes.len() > 12 {
            return Ok(bytes);
        }
    }
    let raw = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if raw.starts_with(b"wOF2") {
        let ttf = kn_render::decode_woff2(&raw).map_err(|e| e.to_string())?;
        if let Err(e) = atomic_write(&sidecar, &ttf) {
            eprintln!("[font] 无法写入解码缓存 {}: {e}", sidecar.display());
        }
        Ok(ttf)
    } else {
        Ok(raw)
    }
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

/// 启动时按 `cache_limit_mb` 修剪缓存 (Python `PageContext.prune_cache`: 四个目录各占四分之一)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn prune_cache(paths: &Paths, limit_mb: i64) -> u64 {
    let per_dir = (limit_mb.max(16) as u64 * 1024 * 1024 / 4).max(1);
    ["covers", "fonts", "images", "content"].iter().map(|name| prune_dir(&paths.cache_dir().join(name), per_dir)).sum()
}

/// 缓存命中时刷新修改时间, 让修剪顺序成为真正的 LRU。
pub fn touch(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

/// 缓存目录里的章节 (书名、章节名、序号), 供离线书架使用。按书名、序号排序。
pub fn list_cached_chapters(paths: &Paths) -> Vec<(Chapter, PathBuf)> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(paths.cache_dir().join("content")) else {
        return out;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(ch) = std::fs::read(&path).ok().and_then(|b| Chapter::parse(&b, false)) {
            if ch.book_id > 0 {
                out.push((ch, path));
            }
        }
    }
    out.sort_by(|a, b| (a.0.book_id, a.0.sort_num).cmp(&(b.0.book_id, b.0.sort_num)));
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
