//! 网络与缓存的结合: 章节/字体/插图 "先查缓存, 不新鲜或缺失时下载并写回缓存", 语义与 Python 版一致。
//! 这里的函数都是阻塞的, 只在后台任务线程里调用。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use kn_net::Client;

use crate::store::{self, Paths};

/// 章节缓存的新鲜期 (Python `_CHAPTER_CACHE_TTL`)。
const CHAPTER_TTL: Duration = Duration::from_secs(12 * 3600);
const FONT_LIMIT: usize = 20 * 1024 * 1024;
const IMAGE_LIMIT: usize = 16 * 1024 * 1024;
/// 封面/插图下载失败后, 再次显示时隔多久重试 (网络可能已经恢复)。
pub const RETRY_AFTER: Duration = Duration::from_secs(20);

/// 串行化登录: 启动时的后台登录与页面加载可能同时需要会话。
static LOGIN_LOCK: Mutex<()> = Mutex::new(());

/// 需要登录的请求之前调用: 已有会话直接返回; 否则用保存的账号 (`Client::set_account`) 登录
/// (阻塞, 只在后台线程用)。
pub fn ensure_login(net: &Client) -> Result<(), String> {
    if net.user().is_some() {
        return Ok(());
    }
    let _guard = LOGIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 锁里再读账号: 同时等着的另一次登录可能刚发现它被拒并删掉了
    let account = net.account();
    if net.user().is_some() {
        return Ok(());
    }
    if net.has_refresh_token() {
        // 有刷新令牌但缺用户信息: 补一次 GetMyInfo
        if let Ok(Some(_)) = net.refresh_user() {
            return Ok(());
        }
    }
    if account.is_none() { return Err("未登录".into()); }
    net.login_saved().map(|_| ()).map_err(|e| format!("登录失败: {e}"))
}

/// 由配置建网络客户端 (session.json / visitor-id 与 Python 版共用)。
pub fn client(paths: &Paths, config: &store::Config) -> Client {
    let visitor_file = paths.cache_dir().join("visitor-id");
    let visitor_id = std::fs::read_to_string(&visitor_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let id = store::sha256_hex(&format!("{:?}{}", SystemTime::now(), std::process::id()))[..32].to_string();
            let _ = store::atomic_write(&visitor_file, id.as_bytes());
            id
        });
    Client::new(kn_net::ClientConfig {
        server: config.api_server(),
        strict_tls: config.bool("strict_tls", true),
        request_limit: config.int("request_limit", 9).max(1) as u32,
        request_window: Duration::from_millis(config.int("request_window_ms", 5500).max(500) as u64),
        session_path: paths.cache_dir().join("session.json"),
        visitor_id: Some(visitor_id),
        ..Default::default()
    })
}

fn is_fresh(file: &std::path::Path) -> bool {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < CHAPTER_TTL)
}

/// [`cached_chapter`] 的结果。
pub enum CachedChapter {
    Hit(store::Chapter),
    /// 没有能用的缓存, 要下载 (corrupt = 缓存解析不了, 已删除)
    Miss { corrupt: bool },
}

/// 读章节分两步 (Python `_load_chapter` 的缓存语义, 但不再为过期缓存阻塞): 先 `cached_chapter` (本地队列),
/// 未命中且在线时再 [`download_and_parse`] (网络队列); 离线时报 [`missing_chapter_error`]。
/// - 有缓存: 立即返回。缓存过期 (> 12 h) 且服务器未被标记为不可用时, 后台线程重新下载写回,
///   下次打开生效 —— 打开章节永远不用等网络。
/// - 缓存解析不了 (断电留下的半截文件等): 删掉, 按无缓存处理 —— 不然新的 mtime 让它 12 小时内都不会刷新。
/// - 无缓存: 下载并写缓存 (必须等); 直接用解好的 JSON, 不再从写盘的字节重新解析。
pub fn cached_chapter(paths: &Paths, net: Option<&Client>, book_id: i64, sort_num: i64, convert: &str, with_content: bool) -> CachedChapter {
    let file = paths.chapter_file(book_id, sort_num, convert);
    let Ok(bytes) = std::fs::read(&file) else { return CachedChapter::Miss { corrupt: false } };
    if let Some(chapter) = store::Chapter::parse(&bytes, with_content) {
        if let Some(net) = net {
            if !is_fresh(&file) && !net.server_down() {
                spawn_refresh(paths, net, book_id, sort_num, convert);
            }
        }
        return CachedChapter::Hit(chapter);
    }
    eprintln!("[net] 章节缓存 {book_id}#{sort_num} 损坏, 删除后重新下载");
    let _ = std::fs::remove_file(&file);
    CachedChapter::Miss { corrupt: true }
}

/// 缓存未命中又不能下载 (离线) 时的提示。
pub fn missing_chapter_error(corrupt: bool) -> String {
    if corrupt { "章节缓存损坏, 已删除, 联网后重试" } else { "章节未缓存 (离线)" }.into()
}

/// 下载章节、写缓存并解析 (阻塞等网络, 只在网络队列里调用)。
pub fn download_and_parse(paths: &Paths, net: &Client, book_id: i64, sort_num: i64, convert: &str, with_content: bool) -> Result<store::Chapter, String> {
    let value = download_chapter_value(paths, net, book_id, sort_num, convert, 0)?;
    store::Chapter::from_value(&value, with_content).ok_or_else(|| "章节数据损坏".into())
}

/// 已缓存的章节字体文件 (不下载; 有就更新访问时间)。
pub fn cached_font(paths: &Paths, api_server: &str, font_url: &str) -> Option<PathBuf> {
    let file = paths.font_file(api_server, font_url)?;
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > 0) {
        store::touch(&file);
        return Some(file);
    }
    None
}

/// 正在后台刷新的章节 (书, 序号, 繁简转换): 来回切章时同一章只刷新一次。
static REFRESHING: Mutex<Vec<(i64, i64, String)>> = Mutex::new(Vec::new());

/// 占住一个后台刷新名额; 刷新结束 (含出错、panic 展开) 时随 drop 释放。
struct RefreshGuard((i64, i64, String));

impl RefreshGuard {
    fn try_begin(key: (i64, i64, String)) -> Option<RefreshGuard> {
        let mut list = REFRESHING.lock().unwrap_or_else(|e| e.into_inner());
        if list.contains(&key) {
            return None;
        }
        list.push(key.clone());
        Some(RefreshGuard(key))
    }
}

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESHING.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| *k != self.0);
    }
}

fn spawn_refresh(paths: &Paths, net: &Client, book_id: i64, sort_num: i64, convert: &str) {
    let Some(guard) = RefreshGuard::try_begin((book_id, sort_num, convert.to_string())) else { return };
    let (paths, net, convert) = (paths.clone(), net.clone(), convert.to_string());
    std::thread::spawn(move || {
        let _guard = guard;
        if let Err(e) = download_chapter_value(&paths, &net, book_id, sort_num, &convert, BACKGROUND_PRIORITY) {
            eprintln!("[net] 章节后台刷新 {book_id}#{sort_num} 失败: {e}");
        }
    });
}

/// 后台请求的优先级: 让给用户正在等待的请求 (turn 调度器里数字越小越优先)。
pub const BACKGROUND_PRIORITY: i32 = 5;

/// 下载章节并写缓存, 返回响应字节。
pub fn download_chapter(paths: &Paths, net: &Client, book_id: i64, sort_num: i64, convert: &str, priority: i32) -> Result<Vec<u8>, String> {
    let value = download_chapter_value(paths, net, book_id, sort_num, convert, priority)?;
    serde_json::to_vec(&value).map_err(|e| e.to_string())
}

/// 下载章节并写缓存, 返回解好的 JSON。
fn download_chapter_value(paths: &Paths, net: &Client, book_id: i64, sort_num: i64, convert: &str, priority: i32) -> Result<serde_json::Value, String> {
    let convert_opt = (!convert.is_empty()).then_some(convert);
    let value = net.get_novel_content(book_id, sort_num, convert_opt, priority).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    if let Err(e) = store::cache_write(&paths.chapter_file(book_id, sort_num, convert), &bytes) {
        eprintln!("[net] 章节缓存写入失败: {e}");
    }
    Ok(value)
}

/// 章节字体文件: 已缓存直接返回, 否则下载 (Python `ensure_font`)。
/// 缓存的字体解析不了时由 [`store::load_chapter_font`] / [`store::forget_chapter_font`] 删掉, 下次进来重新下载。
pub fn ensure_font(paths: &Paths, net: Option<&Client>, api_server: &str, font_url: &str) -> Option<PathBuf> {
    if let Some(file) = cached_font(paths, api_server, font_url) {
        return Some(file);
    }
    let file = paths.font_file(api_server, font_url)?;
    let net = net?;
    let url = kn_text::absolute_url(api_server, font_url);
    match net.download(&url, FONT_LIMIT) {
        Ok(bytes) if !bytes.is_empty() => {
            store::cache_write(&file, &bytes).ok()?;
            // 旧的解码旁路缓存 (来自被删掉的坏文件) 作废
            let _ = std::fs::remove_file(store::font_sidecar(&file));
            Some(file)
        }
        Ok(_) => None,
        Err(e) => {
            eprintln!("[net] 字体下载失败: {e}");
            None
        }
    }
}

/// CDN 缩放参数: 只有带 `size=WxH` 的站内图片支持, 设置/替换 `height=档位` (Python `scaled_image_url`)。
pub fn scaled_image_url(url: &str, height: u32) -> String {
    let Some((base, query)) = url.split_once('?') else { return url.to_string() };
    let has_size = query.split('&').any(|kv| {
        kv.strip_prefix("size=").is_some_and(|v| {
            let mut it = v.splitn(2, 'x');
            matches!((it.next(), it.next()), (Some(w), Some(h)) if w.parse::<u32>().is_ok_and(|n| n > 0) && h.parse::<u32>().is_ok_and(|n| n > 0))
        })
    });
    if !has_size {
        return url.to_string();
    }
    let bucket = kn_render::height_bucket(height);
    let mut parts: Vec<String> = query.split('&').filter(|kv| !kv.is_empty() && !kv.starts_with("height=")).map(str::to_string).collect();
    parts.push(format!("height={bucket}"));
    format!("{base}?{}", parts.join("&"))
}

/// 插图/封面原始字节: 先查缓存 (与 Python 共用文件名), 缺失时按缩放后的 URL 下载并写缓存。
/// 缓存里存的是 CDN 原始字节 (Python 版用 Pillow 读, 不依赖扩展名), 不重新编码。
pub fn image_bytes(paths: &Paths, net: Option<&Client>, url: &str, height: u32) -> Result<Option<Vec<u8>>, String> {
    image_source(paths, net, url, height).map(|c| c.map(|c| c.bytes))
}

/// 图片字节和它所在的缓存文件 (测试夹具没有文件)。解码用 [`CachedImage::decode`]: 文件坏了会被删掉。
pub struct CachedImage {
    pub bytes: Vec<u8>,
    pub file: Option<PathBuf>,
}

impl CachedImage {
    pub fn new(bytes: Vec<u8>, file: Option<PathBuf>) -> Self {
        CachedImage { bytes, file }
    }

    /// 解码为灰度图。缓存文件解码失败 (半截文件、服务器给的错误页) 时删掉它,
    /// 这样 [`RETRY_AFTER`] 后的重试会重新下载, 而不是再读同一份坏字节。
    pub fn decode(&self, target: Option<(u32, u32)>) -> Result<kn_render::Bitmap, kn_render::ImageError> {
        self.decode_with_budget(target, kn_render::ImageBudget::Normal)
    }

    pub fn decode_with_budget(&self, target: Option<(u32, u32)>, budget: kn_render::ImageBudget) -> Result<kn_render::Bitmap, kn_render::ImageError> {
        let result = kn_render::decode_gray_with_budget(&self.bytes, target, budget);
        if let (Err(e), Some(file)) = (&result, &self.file) {
            if is_corrupt_image(&self.bytes, e) {
                eprintln!("[net] 图片缓存 {} 无法解码 ({e}), 删除", file.display());
                let _ = std::fs::remove_file(file);
            }
        }
        result
    }
}

/// 重新下载能不能修好: 解码出错、截断或认不出格式的才算坏; 图太大、确实是不支持的格式 (GIF / AVIF / HEIC) 删了也没用。
fn is_corrupt_image(bytes: &[u8], err: &kn_render::ImageError) -> bool {
    match err {
        kn_render::ImageError::Decode(_) => true,
        kn_render::ImageError::TooLarge => false,
        kn_render::ImageError::Unsupported => !(bytes.starts_with(b"GIF8") || bytes.get(4..8) == Some(b"ftyp")),
    }
}

/// 同 [`image_bytes`], 但带上缓存文件路径 (见 [`CachedImage`])。
pub fn image_source(paths: &Paths, net: Option<&Client>, url: &str, height: u32) -> Result<Option<CachedImage>, String> {
    if let Some(bytes) = crate::api::fixture_file(url) {
        return Ok(Some(CachedImage::new(bytes, None)));
    }
    let file = paths.image_file(url, height);
    if let Ok(bytes) = std::fs::read(&file) {
        store::touch(&file);
        return Ok(Some(CachedImage::new(bytes, Some(file))));
    }
    let Some(net) = net else { return Ok(any_cached_size(paths, url)) };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(None);
    }
    let bytes = match net.download(&scaled_image_url(url, height), IMAGE_LIMIT) {
        Ok(bytes) => bytes,
        Err(e) => return any_cached_size(paths, url).map(Some).ok_or_else(|| e.to_string()),
    };
    if let Err(e) = store::cache_write(&file, &bytes) {
        eprintln!("[net] 图片缓存写入失败: {e}");
    }
    Ok(Some(CachedImage::new(bytes, Some(file))))
}

/// 同一张图缓存过的其它尺寸 (大的优先): 离线或下载失败时用, 例如排版改动后插图换了尺寸档位。
fn any_cached_size(paths: &Paths, url: &str) -> Option<CachedImage> {
    kn_render::HEIGHT_BUCKETS.iter().rev().find_map(|&h| {
        let file = paths.image_file(url, h);
        let bytes = std::fs::read(&file).ok()?;
        store::touch(&file);
        Some(CachedImage::new(bytes, Some(file)))
    })
}

/// CDN 能否给出比阅读页所用版本更高的分辨率: 只有带 `size=` 的站内图片支持缩放参数,
/// 其它 URL 阅读页拿到的就是原图。
pub fn has_original_variant(url: &str) -> bool {
    scaled_image_url(url, 0) != url
}

/// 插图原图字节 (不带缩放参数, 全屏预览的 "原图"): 先查缓存, 缺失时下载并写缓存。
/// 离线且未缓存返回 Ok(None)。
pub fn original_image_bytes(paths: &Paths, net: Option<&Client>, url: &str) -> Result<Option<CachedImage>, String> {
    let file = paths.original_image_file(url);
    if let Ok(bytes) = std::fs::read(&file) {
        store::touch(&file);
        return Ok(Some(CachedImage::new(bytes, Some(file))));
    }
    let Some(net) = net else { return Ok(None) };
    let original = strip_height(url);
    let bytes = net.download(&original, IMAGE_LIMIT).map_err(|e| e.to_string())?;
    if let Err(e) = store::cache_write(&file, &bytes) {
        eprintln!("[net] 原图缓存写入失败: {e}");
    }
    Ok(Some(CachedImage::new(bytes, Some(file))))
}

/// 去掉 URL 里的 `height=` 缩放参数 (章节 HTML 里的地址通常不带, 防御性处理)。
fn strip_height(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else { return url.to_string() };
    let parts: Vec<&str> = query.split('&').filter(|kv| !kv.is_empty() && !kv.starts_with("height=")).collect();
    if parts.is_empty() { base.to_string() } else { format!("{base}?{}", parts.join("&")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 过期缓存 + 服务器不可达: 必须立即返回缓存, 不能等网络失败。
    #[test]
    fn stale_chapter_returns_cache_without_waiting() {
        let dir = std::env::temp_dir().join(format!("kn-swr-test-{}", std::process::id()));
        let paths = Paths { app_dir: dir.clone() };
        let file = paths.chapter_file(1, 2, "");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{\"Chapter\": {\"Title\": \"缓存\"}}").unwrap();
        let old = SystemTime::now() - CHAPTER_TTL - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(old).unwrap();
        // 不可路由的地址: 真去连接的话要等到超时
        let net = Client::new(kn_net::ClientConfig {
            server: "http://10.255.255.1:9".into(),
            session_path: dir.join("session.json"),
            ..Default::default()
        });
        let started = std::time::Instant::now();
        let CachedChapter::Hit(chapter) = cached_chapter(&paths, Some(&net), 1, 2, "", true) else { panic!("应命中缓存") };
        assert_eq!(chapter.title, "缓存");
        assert!(started.elapsed() < Duration::from_millis(500), "{:?}", started.elapsed());
        // 后台刷新还在连 (不可路由地址): 再打开同一章不会再起一个刷新线程
        assert!(RefreshGuard::try_begin((1, 2, String::new())).is_none());
        net.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 截断的章节缓存: 删掉; 离线时报错 (下次联网重新下载), 不再 12 小时内一直读同一个坏文件。
    #[test]
    fn corrupt_chapter_cache_is_deleted() {
        let dir = std::env::temp_dir().join(format!("kn-corrupt-ch-{}", std::process::id()));
        let paths = Paths { app_dir: dir.clone() };
        let file = paths.chapter_file(3, 4, "t2s");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"{\"Chapter\": {\"Title\": \"x\", \"Cont").unwrap();
        assert!(matches!(cached_chapter(&paths, None, 3, 4, "t2s", true), CachedChapter::Miss { corrupt: true }));
        assert!(missing_chapter_error(true).contains("损坏"));
        assert!(!file.exists());
        assert!(matches!(cached_chapter(&paths, None, 3, 4, "t2s", true), CachedChapter::Miss { corrupt: false }));
        assert_eq!(missing_chapter_error(false), "章节未缓存 (离线)");
        // 完好的缓存照常读
        std::fs::write(&file, "{\"Chapter\": {\"Title\": \"好\", \"SortNum\": 4, \"Content\": \"<p>a</p>\"}}").unwrap();
        let CachedChapter::Hit(chapter) = cached_chapter(&paths, None, 3, 4, "t2s", true) else { panic!() };
        assert_eq!((chapter.title.as_str(), chapter.sort_num, chapter.content.as_str()), ("好", 4, "<p>a</p>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_guard_releases_on_drop_and_panic() {
        let key = (77, 1, "x".to_string());
        let g = RefreshGuard::try_begin(key.clone()).unwrap();
        assert!(RefreshGuard::try_begin(key.clone()).is_none());
        assert!(RefreshGuard::try_begin((77, 2, "x".into())).is_some());
        drop(g);
        let g = RefreshGuard::try_begin(key.clone()).unwrap();
        let result = std::thread::spawn(move || {
            let _g = g;
            panic!("刷新线程出错");
        })
        .join();
        assert!(result.is_err());
        assert!(RefreshGuard::try_begin(key).is_some());
    }

    #[test]
    fn undecodable_cached_image_is_deleted() {
        let dir = std::env::temp_dir().join(format!("kn-corrupt-img-{}", std::process::id()));
        let paths = Paths { app_dir: dir.clone() };
        let url = "https://x/a.jpg?size=10x20";
        let file = paths.image_file(url, 300);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        // 截断的 JPEG
        std::fs::write(&file, [0xFF, 0xD8, 0xFF, 0xE0, 0, 16]).unwrap();
        let cached = image_source(&paths, None, url, 300).unwrap().unwrap();
        assert_eq!(cached.file.as_deref(), Some(file.as_path()));
        assert!(cached.decode(None).is_err());
        assert!(!file.exists());
        // 0 字节文件 (认不出格式) 也删
        std::fs::write(&file, b"").unwrap();
        assert!(image_source(&paths, None, url, 300).unwrap().unwrap().decode(None).is_err());
        assert!(!file.exists());
        // 确实不支持的格式 (GIF) 留着: 重新下载也一样
        std::fs::write(&file, b"GIF89a\x01\x00\x01\x00").unwrap();
        assert!(image_source(&paths, None, url, 300).unwrap().unwrap().decode(None).is_err());
        assert!(file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn original_variant_detection() {
        assert!(has_original_variant("https://x/a.jpg?size=1089x1600&t=5"));
        assert!(!has_original_variant("https://x/a.jpg"));
        assert!(!has_original_variant("https://x/a.jpg?t=1"));
        assert_eq!(strip_height("https://x/a.jpg?size=1x2&height=512&t=5"), "https://x/a.jpg?size=1x2&t=5");
        assert_eq!(strip_height("https://x/a.jpg?height=512"), "https://x/a.jpg");
    }

    #[test]
    fn scaled_url_only_for_system_images() {
        assert_eq!(scaled_image_url("https://x/a.jpg", 900), "https://x/a.jpg");
        assert_eq!(scaled_image_url("https://x/a.jpg?t=1", 900), "https://x/a.jpg?t=1");
        assert_eq!(scaled_image_url("https://x/a.jpg?size=1089x1600&t=5", 900), "https://x/a.jpg?size=1089x1600&t=5&height=1024");
        assert_eq!(scaled_image_url("https://x/a.jpg?height=256&size=10x20", 300), "https://x/a.jpg?size=10x20&height=384");
    }
}
