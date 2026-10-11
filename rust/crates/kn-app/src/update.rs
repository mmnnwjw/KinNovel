//! 检查新版本 (只提示, 不自动安装)。
//!
//! 启动后在后台问 GitHub Releases API 最新的发布版本 (设置「自动检查更新」, 默认开, 每天最多一次),
//! 结果存在 `data/update.json` = `{"checked": unix 秒, "latest": "1.3.0"}`, 「我的」页读它显示提示。
//! 只访问 api.github.com, 不碰轻书架服务器; 失败 (例如连不上 GitHub) 静默忽略。

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::store::{self, atomic_write, Paths};

const LATEST_URL: &str = "https://api.github.com/repos/mmnnwjw/KinNovel/releases/latest";
pub const PROJECT_PAGE: &str = "github.com/mmnnwjw/KinNovel";
const CHECK_INTERVAL_SECS: i64 = 24 * 3600;

/// 本次运行里已知的最新版本 (启动时从文件读, 后台检查完更新)。
static LATEST: Mutex<Option<String>> = Mutex::new(None);

impl Paths {
    fn update_file(&self) -> PathBuf {
        self.data_dir().join("update.json")
    }
}

/// "v1.2.10" / "1.2.10" → (1, 2, 10); 预发布后缀 ("-rc1") 忽略。
fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let text = text.trim().trim_start_matches('v');
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

/// `latest` 比当前程序新时返回它 (去掉 "v")。
pub fn newer_than_current(latest: &str) -> Option<String> {
    let current = parse_version(env!("CARGO_PKG_VERSION"))?;
    (parse_version(latest)? > current).then(|| latest.trim().trim_start_matches('v').to_string())
}

/// 已知的更新版本 (没有或不比当前新则 None)。
pub fn available() -> Option<String> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).as_deref().and_then(newer_than_current)
}

fn load(paths: &Paths) -> (i64, Option<String>) {
    let v: Value = std::fs::read(paths.update_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    (v.get("checked").and_then(Value::as_i64).unwrap_or(0), v.get("latest").and_then(Value::as_str).map(str::to_string))
}

/// 启动时调用: 读出上次的结果; 需要时 (`enabled` 且超过一天) 在后台重新检查。
pub fn start(paths: &Paths, net: Option<kn_net::Client>, enabled: bool) {
    let (checked, latest) = load(paths);
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = latest;
    let Some(net) = net.filter(|_| enabled) else { return };
    if (store::unix_now() - checked).abs() < CHECK_INTERVAL_SECS {
        return;
    }
    let paths = paths.clone();
    std::thread::spawn(move || match check(&net) {
        Ok(tag) => {
            eprintln!("[update] 最新版本 {tag} (当前 {})", env!("CARGO_PKG_VERSION"));
            let text = json!({"checked": store::unix_now(), "latest": tag}).to_string();
            if let Err(e) = atomic_write(&paths.update_file(), text.as_bytes()) {
                eprintln!("[update] 无法保存检查结果: {e}");
            }
            *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(tag);
        }
        Err(e) => eprintln!("[update] 检查失败: {e}"),
    });
}

/// 立即检查 (「关于」里的按钮), 在后台线程调用。返回最新的版本号。
pub fn check_now(paths: &Paths, net: Option<&kn_net::Client>) -> Result<String, String> {
    let net = net.ok_or("离线模式")?;
    let tag = check(net)?;
    let text = json!({"checked": store::unix_now(), "latest": tag}).to_string();
    let _ = atomic_write(&paths.update_file(), text.as_bytes());
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(tag.clone());
    Ok(tag)
}

fn check(net: &kn_net::Client) -> Result<String, String> {
    let body = net.download(LATEST_URL, 256 * 1024).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let tag = v.get("tag_name").and_then(Value::as_str).ok_or("响应里没有 tag_name")?;
    Ok(tag.trim_start_matches('v').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.2.10"), Some((1, 2, 10)));
        assert_eq!(parse_version("2.0"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.3.0-rc1"), Some((1, 3, 0)));
        assert_eq!(parse_version("x"), None);
        assert!(newer_than_current("v999.0.0").is_some());
        assert!(newer_than_current(env!("CARGO_PKG_VERSION")).is_none());
        assert!(newer_than_current("0.1.0").is_none());
    }
}
