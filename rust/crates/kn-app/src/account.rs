//! 账号与配置文件的位置。
//!
//! - 设置 `data/settings.json` (`store::Config`); 账号 `data/account.json` = `{"email", "password_sha256"}`。
//!   只存密码的 SHA-256: 登录接口本来就只收这个值 (网页版同样在本地哈希), 不必保存明文密码。
//! - 1.4 及以前用 0.x Python 版留下的 `bin/config.json` (设置与明文账号混在一起, 而且在程序目录里,
//!   覆盖安装会把它换成默认值)。第一次启动时迁移过来, 然后删掉旧文件。
//!
//! 会话令牌仍在 `cache/session.json` (kn-net 管理)。

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::store::{atomic_write, Paths};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub email: String,
    /// `kn_net::password_hash(密码)`
    pub password_hash: String,
}

impl Paths {
    pub fn settings_file(&self) -> PathBuf {
        self.data_dir().join("settings.json")
    }

    pub fn account_file(&self) -> PathBuf {
        self.data_dir().join("account.json")
    }

    /// 1.4 及以前的配置 (Python 版格式), 只在迁移时读一次。
    pub fn legacy_config_file(&self) -> PathBuf {
        self.app_dir.join("bin").join("config.json")
    }
}

pub fn load(paths: &Paths) -> Option<Account> {
    let v: Value = serde_json::from_slice(&std::fs::read(paths.account_file()).ok()?).ok()?;
    let email = v.get("email")?.as_str()?.trim().to_string();
    let password_hash = v.get("password_sha256")?.as_str()?.to_string();
    (!email.is_empty() && !password_hash.is_empty()).then_some(Account { email, password_hash })
}

pub fn save(paths: &Paths, account: &Account) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(&json!({"email": account.email, "password_sha256": account.password_hash}))?;
    atomic_write(&paths.account_file(), text.as_bytes())
}

pub fn clear(paths: &Paths) {
    let _ = std::fs::remove_file(paths.account_file());
}

/// Python 版才用、1.x 不再读的键。
const OBSOLETE_KEYS: &[&str] = &["account_email", "account_password", "screen_protocol", "framebuffer", "reader_guide_dismissed", "prefetch_reading_target", "home_order", "justify"];

/// 旧版安装包自带的 `bin/config.json` 默认值。迁移时去掉与之相同的项, 让程序里的默认值生效
/// (例如正文字号的默认值按屏幕 ppi 换算, 旧文件里固定写着 300 ppi 的 48)。
fn shipped_default(key: &str) -> Option<Value> {
    Some(match key {
        "api_server" => json!("https://api.lightnovel.life"),
        "font_path" => json!("/usr/java/lib/fonts/STHeitiMedium.ttf"),
        "font_size" => json!(48),
        "line_spacing" => json!(1.42),
        "reader_margin" => json!(34),
        "page_flash" => json!(false),
        "full_refresh_every" => json!(6),
        "page_turn_animation" => json!(true),
        "night_mode" => json!(false),
        "first_line_indent" => json!(true),
        "convert" => Value::Null,
        "ignore_japanese" => json!(false),
        "ignore_ai" => json!(false),
        "prefetch_chapters" => json!(false),
        "request_limit" => json!(9),
        "request_window_ms" => json!(5500),
        "cache_limit_mb" => json!(192),
        "strict_tls" => json!(true),
        _ => return None,
    })
}

fn same_number(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// 还没有 `data/settings.json` 而旧的 `bin/config.json` 在: 把设置和账号搬过去, 删掉旧文件。
/// 返回是否做了迁移。
pub fn migrate_legacy_config(paths: &Paths) -> bool {
    let legacy = paths.legacy_config_file();
    if paths.settings_file().exists() || !legacy.exists() {
        return false;
    }
    let old: Map<String, Value> = match std::fs::read(&legacy).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let text = |key: &str| old.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let email = text("account_email").trim().to_string();
    let password = text("account_password");
    if !email.is_empty() && !password.is_empty() && load(paths).is_none() {
        if let Err(e) = save(paths, &Account { email, password_hash: kn_net::password_hash(&password) }) {
            eprintln!("[config] 保存账号失败: {e}");
            return false;
        }
    }
    let settings: Map<String, Value> = old
        .into_iter()
        .filter(|(k, v)| !OBSOLETE_KEYS.contains(&k.as_str()) && !shipped_default(k).is_some_and(|d| same_number(v, &d)))
        .collect();
    let text = serde_json::to_string_pretty(&Value::Object(settings)).unwrap_or_else(|_| "{}".into());
    if let Err(e) = atomic_write(&paths.settings_file(), text.as_bytes()) {
        eprintln!("[config] 迁移设置失败: {e}");
        return false;
    }
    // 旧文件里有明文密码, 迁移成功后删除
    if let Err(e) = std::fs::remove_file(&legacy) {
        eprintln!("[config] 删除 {} 失败: {e}", legacy.display());
    }
    eprintln!("[config] 已把 bin/config.json 迁移到 data/");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_settings_and_hashes_the_password() {
        let dir = std::env::temp_dir().join(format!("kn-migrate-test-{}", std::process::id()));
        let paths = Paths { app_dir: dir.clone() };
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(
            paths.legacy_config_file(),
            r#"{"api_server": "https://api.lightnovel.life", "account_email": " a@b.c ", "account_password": "pw",
                "font_size": 48, "line_spacing": 1.6, "full_refresh_every": 6.0, "home_order": {"shelf": 0},
                "night_mode": true, "search_history": ["x"]}"#,
        )
        .unwrap();
        assert!(migrate_legacy_config(&paths));
        assert!(!paths.legacy_config_file().exists());
        assert_eq!(load(&paths), Some(Account { email: "a@b.c".into(), password_hash: kn_net::password_hash("pw") }));
        let settings: Value = serde_json::from_slice(&std::fs::read(paths.settings_file()).unwrap()).unwrap();
        assert_eq!(settings, json!({"line_spacing": 1.6, "night_mode": true, "search_history": ["x"]}));
        // 已经迁移过: 不再动
        std::fs::write(paths.legacy_config_file(), "{}").unwrap();
        assert!(!migrate_legacy_config(&paths));
        assert!(paths.legacy_config_file().exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
