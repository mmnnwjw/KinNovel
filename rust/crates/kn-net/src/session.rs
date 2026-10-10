use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value};

use crate::error::NetError;

/// Port of `api.py`'s `SessionStore`: a small JSON file
/// (`cache/session.json`) holding `Token`/`RefreshToken`/`TokenUpdatedAt`/
/// `User`. Deliberately keeps the exact same keys and JSON shape (2-space
/// indented, UTF-8, trailing newline from `json.dumps(..., indent=2)` does
/// NOT add one -- Python's `json.dumps` never appends a trailing newline
/// either) so the Python and Rust builds can share one cache directory.
pub struct SessionStore {
    path: PathBuf,
    data: Mutex<Map<String, Value>>,
}

impl SessionStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        SessionStore { path, data: Mutex::new(data) }
    }

    pub fn save(&self) -> Result<(), NetError> {
        let data = self.data.lock().unwrap();
        let text = serde_json::to_string_pretty(&*data)
            .map_err(|e| NetError::protocol(format!("序列化 session.json 失败: {e}")))?;
        atomic_write(&self.path, text.as_bytes())
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.lock().unwrap().get(key).cloned()
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str().map(str::to_string))
    }

    pub fn set_many(&self, values: Vec<(&str, Value)>) -> Result<(), NetError> {
        {
            let mut data = self.data.lock().unwrap();
            for (key, value) in values {
                data.insert(key.to_string(), value);
            }
        }
        self.save()
    }

    pub fn clear_credentials(&self) -> Result<(), NetError> {
        {
            let mut data = self.data.lock().unwrap();
            data.remove("Token");
            data.remove("RefreshToken");
            data.remove("TokenUpdatedAt");
            data.remove("User");
        }
        self.save()
    }

    pub fn has_refresh_token(&self) -> bool {
        matches!(self.get("RefreshToken"), Some(Value::String(s)) if !s.is_empty())
    }
}

/// Same durability shape as `utils.py`'s `atomic_write`: write to a sibling
/// temp file, fsync, then rename over the destination.
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), NetError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(NetError::from)?;
    let mut tmp_path = dir.to_path_buf();
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("session");
    tmp_path.push(format!("{file_name}.tmp-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp_path).map_err(NetError::from)?;
        file.write_all(data).map_err(NetError::from)?;
        file.flush().map_err(NetError::from)?;
        file.sync_all().map_err(NetError::from)?;
    }
    std::fs::rename(&tmp_path, path).map_err(NetError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_python_session_shape() {
        let dir = std::env::temp_dir().join(format!("kn-net-session-test-{}", random_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");

        let store = SessionStore::load(&path);
        assert!(store.get("Token").is_none());
        store
            .set_many(vec![
                ("Token", Value::String("abc".into())),
                ("RefreshToken", Value::String("def".into())),
                ("TokenUpdatedAt", serde_json::json!(1234.5)),
            ])
            .unwrap();
        store.set_many(vec![("User", serde_json::json!({"Id": 7, "Name": "测试"}))]).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let reparsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(reparsed["Token"], "abc");
        assert_eq!(reparsed["RefreshToken"], "def");
        assert_eq!(reparsed["User"]["Name"], "测试");

        let reloaded = SessionStore::load(&path);
        assert_eq!(reloaded.get_str("Token"), Some("abc".to_string()));
        assert!(reloaded.has_refresh_token());

        reloaded.clear_credentials().unwrap();
        assert!(!reloaded.has_refresh_token());
        assert!(reloaded.get("Token").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn random_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }
}
