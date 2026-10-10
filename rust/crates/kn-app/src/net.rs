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

/// 配置里的账号 (邮箱, 密码); 启动时设置一次。
static ACCOUNT: Mutex<Option<(String, String)>> = Mutex::new(None);
/// 串行化登录: 启动时的后台登录与页面加载可能同时需要会话。
static LOGIN_LOCK: Mutex<()> = Mutex::new(());

pub fn set_account(account: Option<(String, String)>) {
    *ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()) = account;
}

pub fn has_account() -> bool {
    ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// 需要登录的请求之前调用: 已有会话直接返回; 否则用配置里的账号登录 (阻塞, 只在后台线程用)。
pub fn ensure_login(net: &Client) -> Result<(), String> {
    if net.user().is_some() {
        return Ok(());
    }
    let account = ACCOUNT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let _guard = LOGIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if net.user().is_some() {
        return Ok(());
    }
    if net.has_refresh_token() {
        // 有刷新令牌但缺用户信息: 补一次 GetMyInfo
        if let Ok(Some(_)) = net.refresh_user() {
            return Ok(());
        }
    }
    let Some((email, password)) = account else { return Err("未登录 (在 bin/config.json 填写账号)".into()) };
    net.login(&email, &password).map(|_| ()).map_err(|e| format!("登录失败: {e}"))
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

/// 章节 JSON 字节 (Python `_load_chapter` 的缓存语义, 但不再为过期缓存阻塞):
/// - 有缓存: 立即返回。缓存过期 (> 12 h) 且服务器未被标记为不可用时, 后台线程重新下载写回,
///   下次打开生效 —— 打开章节永远不用等网络。
/// - 无缓存: 下载并写缓存 (必须等)。
/// `net` 为 None (离线/预览) 时只读缓存。
pub fn chapter_bytes(paths: &Paths, net: Option<&Client>, book_id: i64, sort_num: i64, convert: &str) -> Result<Vec<u8>, String> {
    let file = paths.chapter_file(book_id, sort_num, convert);
    if let Ok(bytes) = std::fs::read(&file) {
        if let Some(net) = net {
            if !is_fresh(&file) && !net.server_down() {
                let (paths, net, convert) = (paths.clone(), net.clone(), convert.to_string());
                std::thread::spawn(move || {
                    if let Err(e) = download_chapter(&paths, &net, book_id, sort_num, &convert, BACKGROUND_PRIORITY) {
                        eprintln!("[net] 章节后台刷新 {book_id}#{sort_num} 失败: {e}");
                    }
                });
            }
        }
        return Ok(bytes);
    }
    let Some(net) = net else {
        return Err("章节未缓存 (离线)".into());
    };
    download_chapter(paths, net, book_id, sort_num, convert, 0)
}

/// 后台请求的优先级: 让给用户正在等待的请求 (turn 调度器里数字越小越优先)。
pub const BACKGROUND_PRIORITY: i32 = 5;

/// 下载章节并写缓存。
pub fn download_chapter(paths: &Paths, net: &Client, book_id: i64, sort_num: i64, convert: &str, priority: i32) -> Result<Vec<u8>, String> {
    let convert_opt = (!convert.is_empty()).then_some(convert);
    let value = net.get_novel_content(book_id, sort_num, convert_opt, priority).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    if let Err(e) = store::atomic_write(&paths.chapter_file(book_id, sort_num, convert), &bytes) {
        eprintln!("[net] 章节缓存写入失败: {e}");
    }
    Ok(bytes)
}

/// 章节字体文件: 已缓存直接返回, 否则下载 (Python `ensure_font`)。
pub fn ensure_font(paths: &Paths, net: Option<&Client>, api_server: &str, font_url: &str) -> Option<PathBuf> {
    let file = paths.font_file(api_server, font_url)?;
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > 0) {
        store::touch(&file);
        return Some(file);
    }
    let net = net?;
    let url = kn_text::absolute_url(api_server, font_url);
    match net.download(&url, FONT_LIMIT) {
        Ok(bytes) if !bytes.is_empty() => {
            store::atomic_write(&file, &bytes).ok()?;
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
    if let Some(bytes) = crate::api::fixture_file(url) {
        return Ok(Some(bytes));
    }
    let file = paths.image_file(url, height);
    if let Ok(bytes) = std::fs::read(&file) {
        store::touch(&file);
        return Ok(Some(bytes));
    }
    let Some(net) = net else { return Ok(None) };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(None);
    }
    let bytes = net.download(&scaled_image_url(url, height), IMAGE_LIMIT).map_err(|e| e.to_string())?;
    if let Err(e) = store::atomic_write(&file, &bytes) {
        eprintln!("[net] 图片缓存写入失败: {e}");
    }
    Ok(Some(bytes))
}

/// CDN 能否给出比阅读页所用版本更高的分辨率: 只有带 `size=` 的站内图片支持缩放参数,
/// 其它 URL 阅读页拿到的就是原图。
pub fn has_original_variant(url: &str) -> bool {
    scaled_image_url(url, 0) != url
}

/// 插图原图字节 (不带缩放参数, 全屏预览的 "原图"): 先查缓存, 缺失时下载并写缓存。
/// 离线且未缓存返回 Ok(None)。
pub fn original_image_bytes(paths: &Paths, net: Option<&Client>, url: &str) -> Result<Option<Vec<u8>>, String> {
    let file = paths.original_image_file(url);
    if let Ok(bytes) = std::fs::read(&file) {
        store::touch(&file);
        return Ok(Some(bytes));
    }
    let Some(net) = net else { return Ok(None) };
    let original = strip_height(url);
    let bytes = net.download(&original, IMAGE_LIMIT).map_err(|e| e.to_string())?;
    if let Err(e) = store::atomic_write(&file, &bytes) {
        eprintln!("[net] 原图缓存写入失败: {e}");
    }
    Ok(Some(bytes))
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
        std::fs::write(&file, b"{\"cached\":true}").unwrap();
        let old = SystemTime::now() - CHAPTER_TTL - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(old).unwrap();
        // 不可路由的地址: 真去连接的话要等到超时
        let net = Client::new(kn_net::ClientConfig {
            server: "http://10.255.255.1:9".into(),
            session_path: dir.join("session.json"),
            ..Default::default()
        });
        let started = std::time::Instant::now();
        let bytes = chapter_bytes(&paths, Some(&net), 1, 2, "").unwrap();
        assert_eq!(bytes, b"{\"cached\":true}");
        assert!(started.elapsed() < Duration::from_millis(500), "{:?}", started.elapsed());
        net.shutdown();
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
