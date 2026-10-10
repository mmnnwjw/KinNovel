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

/// 章节 JSON 字节: 新鲜缓存直接用; 否则下载并写缓存; 下载失败时退回旧缓存 (Python `_load_chapter`)。
/// `net` 为 None (离线/预览) 时只读缓存。
pub fn chapter_bytes(paths: &Paths, net: Option<&Client>, book_id: i64, sort_num: i64, convert: &str) -> Result<Vec<u8>, String> {
    let file = paths.chapter_file(book_id, sort_num, convert);
    let cached = std::fs::read(&file).ok();
    if let Some(bytes) = &cached {
        if net.is_none() || is_fresh(&file) {
            return Ok(bytes.clone());
        }
    }
    let Some(net) = net else {
        return Err("章节未缓存 (离线)".into());
    };
    let convert_opt = (!convert.is_empty()).then_some(convert);
    match net.get_novel_content(book_id, sort_num, convert_opt, 0) {
        Ok(value) => {
            let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
            if let Err(e) = store::atomic_write(&file, &bytes) {
                eprintln!("[net] 章节缓存写入失败: {e}");
            }
            Ok(bytes)
        }
        Err(e) => cached.ok_or_else(|| e.to_string()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_url_only_for_system_images() {
        assert_eq!(scaled_image_url("https://x/a.jpg", 900), "https://x/a.jpg");
        assert_eq!(scaled_image_url("https://x/a.jpg?t=1", 900), "https://x/a.jpg?t=1");
        assert_eq!(scaled_image_url("https://x/a.jpg?size=1089x1600&t=5", 900), "https://x/a.jpg?size=1089x1600&t=5&height=1024");
        assert_eq!(scaled_image_url("https://x/a.jpg?height=256&size=10x20", 300), "https://x/a.jpg?size=10x20&height=384");
    }
}
