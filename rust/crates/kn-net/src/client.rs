//! Port of `bin/src/kinnovel/api.py`'s `ApiClient`: login/refresh-token over
//! plain HTTP, a response cache + in-flight request de-duplication layered
//! on top of `transport::SignalRClient`, and the typed-ish public methods
//! (`get_book_list`, `get_novel_content`, ...) the UI pages call.
//!
//! `Client` is cheap to clone (an `Arc` around the real state) and
//! `Send + Sync`; every method blocks the calling thread. The intended
//! caller is a `kn-ui` worker-pool thread, never the UI/render thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::error::NetError;
use crate::health::ServerHealth;
use crate::helpers::{self, dict_items, int_items};
use crate::http;
use crate::session::SessionStore;
use crate::stream;
use crate::transport::{SignalRClient, SignalRConfig};

/// Methods that are not safe to blindly retry/coalesce: replaying them on a
/// transport error could double-apply a side effect (buy an item twice,
/// post a comment twice, ...). Exact set ported from `api.py`'s
/// `NON_IDEMPOTENT_METHODS`.
const NON_IDEMPOTENT_METHODS: &[&str] = &[
    "BuyShopItem",
    "SignIn",
    "SaveBookShelf",
    "MarkNotifications",
    "ClearReadHistory",
    "PostComment",
    "ReplyComment",
    "DeleteComment",
    "SendDirectMessage",
    "UseSignMakeupCard",
];

fn is_non_idempotent(method: &str) -> bool {
    NON_IDEMPOTENT_METHODS.contains(&method)
}

fn now_epoch_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

#[derive(Clone)]
pub struct ClientConfig {
    pub server: String,
    pub strict_tls: bool,
    pub request_limit: u32,
    pub request_window: Duration,
    pub timeout: Duration,
    pub session_path: PathBuf,
    pub visitor_id: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            server: "https://api.lightnovel.life".to_string(),
            strict_tls: true,
            request_limit: 9,
            request_window: Duration::from_millis(5500),
            timeout: Duration::from_secs(30),
            session_path: PathBuf::from("cache/session.json"),
            visitor_id: None,
        }
    }
}

enum InflightState {
    Pending,
    Done(Result<Value, NetError>),
}

struct Inflight {
    state: Mutex<InflightState>,
    cv: Condvar,
}

struct CacheEntry {
    expires: Instant,
    value: Value,
}

/// The real client state, always reached through `Client`'s `Arc`. Split
/// out so `self_weak` can hand a `TokenProvider` closure a way to call back
/// into `get_access_token` without `Client::new` needing the hub before the
/// client exists (classic `Arc::new_cyclic` use).
struct Inner {
    strict_tls: bool,
    timeout: Duration,
    visitor_id: String,
    session: SessionStore,
    hub: Arc<SignalRClient>,
    refresh_lock: Mutex<()>,
    flight: Mutex<HashMap<String, Arc<Inflight>>>,
    cache: Mutex<HashMap<String, CacheEntry>>,
    health: ServerHealth,
}

/// Blocking, thread-safe API client. Cheap to `clone()` (shares one `Arc`),
/// `Send + Sync`. Mirrors `api.py`'s `ApiClient`.
#[derive(Clone)]
pub struct Client(Arc<Inner>);

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        let session = SessionStore::load(config.session_path);
        let visitor_id = config.visitor_id.unwrap_or_else(crate::transport::random_hex_id);
        let server = config.server.clone();
        let strict_tls = config.strict_tls;
        let timeout = config.timeout;

        let inner = Arc::new_cyclic(|weak: &Weak<Inner>| {
            let weak_for_provider = weak.clone();
            let token_provider: crate::transport::TokenProvider = Arc::new(move || {
                match weak_for_provider.upgrade() {
                    Some(inner) => get_access_token(&inner),
                    None => Ok(None),
                }
            });
            let hub_config = SignalRConfig {
                server,
                strict_tls,
                request_limit: config.request_limit,
                request_window: config.request_window,
                timeout,
                visitor_id: visitor_id.clone(),
                ..SignalRConfig::default()
            };
            Inner {
                strict_tls,
                timeout,
                visitor_id,
                session,
                hub: SignalRClient::new(hub_config, Some(token_provider)),
                refresh_lock: Mutex::new(()),
                flight: Mutex::new(HashMap::new()),
                cache: Mutex::new(HashMap::new()),
                health: ServerHealth::default(),
            }
        });
        Client(inner)
    }

    /// 下载任意 http(s) 资源 (字体、封面、插图), 使用与 API 相同的 TLS 设置。不经过限频
    /// (CDN 与 API 不同主机), 调用方自行控制并发。
    pub fn download(&self, url: &str, limit: usize) -> Result<Vec<u8>, NetError> {
        crate::http::download(url, limit, self.0.timeout, crate::stream::tls_config(self.0.strict_tls))
    }

    pub fn set_server(&self, server: &str) {
        let new_server = server.trim_end_matches('/').to_string();
        if new_server != self.0.hub.server() && self.0.session.has_refresh_token() {
            // Different servers don't share tokens: don't hand an old
            // site's credentials to a new one.
            let _ = self.0.session.clear_credentials();
        }
        self.0.hub.set_server(&new_server);
    }

    pub fn server(&self) -> String {
        self.0.hub.server()
    }

    pub fn user(&self) -> Option<Value> {
        self.0.session.get("User")
    }

    pub fn user_id(&self) -> i64 {
        self.user()
            .and_then(|u| u.get("Id").and_then(Value::as_i64))
            .unwrap_or(0)
    }

    /// 服务器最近不可用且还没到下一次探测时间: 自动请求 (后台刷新、进度上传、云端书架)
    /// 应直接用本地缓存。用户主动打开的页面照常请求。
    pub fn server_down(&self) -> bool {
        self.0.health.is_down()
    }

    /// 最近一次 API 请求因服务器故障失败 (可能已到探测时间)。
    pub fn server_degraded(&self) -> bool {
        self.0.health.degraded()
    }

    pub fn has_refresh_token(&self) -> bool {
        self.0.session.has_refresh_token()
    }

    fn tls_config(&self) -> Arc<rustls::ClientConfig> {
        stream::tls_config(self.0.strict_tls)
    }

    /// Device sleeps: drop the hub connection and stop the keepalive
    /// (saves the radio). Mirrors `api.py`'s `suspend()`.
    pub fn suspend(&self) {
        self.0.hub.close();
    }

    /// Device wakes: nothing to do immediately, the next `invoke()`
    /// reconnects lazily. Mirrors `api.py`'s `resume()`.
    pub fn resume(&self) {}

    /// Final shutdown: stop the hub for good (no further reconnects).
    pub fn shutdown(&self) {
        self.0.hub.shutdown();
    }

    // ---- plain HTTP (login / refresh token) ----

    fn http_call(
        &self,
        path: &str,
        payload: Option<Value>,
        method: &str,
        token: Option<&str>,
    ) -> Result<Value, NetError> {
        let result = self.http_call_inner(path, payload, method, token);
        self.0.health.record(&result);
        result
    }

    fn http_call_inner(
        &self,
        path: &str,
        payload: Option<Value>,
        method: &str,
        token: Option<&str>,
    ) -> Result<Value, NetError> {
        let server = self.0.hub.server();
        let mut full_path = path.to_string();
        let mut body = Vec::new();
        let mut headers: Vec<(&str, String)> = vec![
            ("Accept", "application/json".to_string()),
            ("User-Agent", "KinNovel/1.0".to_string()),
            ("x-id", self.0.visitor_id.clone()),
        ];
        if method == "GET" {
            if let Some(Value::Object(map)) = &payload {
                let parts: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(&value_to_query(v))))
                    .collect();
                if !parts.is_empty() {
                    full_path.push(if full_path.contains('?') { '&' } else { '?' });
                    full_path.push_str(&parts.join("&"));
                }
            }
        } else if let Some(p) = &payload {
            body = serde_json::to_vec(p).unwrap_or_default();
            headers.push(("Content-Type", "application/json".to_string()));
        }
        if let Some(t) = token {
            headers.push(("Authorization", format!("Bearer {t}")));
        }

        let mut response = None;
        for attempt in 0..2 {
            self.0.hub.rate_limit.wait();
            let resp = http::request(&server, &full_path, method, &headers, &body, self.0.timeout, self.tls_config())?;
            if resp.status == 429 && attempt == 0 {
                let retry_after = resp
                    .header("Retry-After")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(5.0)
                    .clamp(1.0, 15.0);
                std::thread::sleep(Duration::from_secs_f64(retry_after));
                continue;
            }
            response = Some(resp);
            break;
        }
        let response = response.ok_or_else(|| NetError::network("HTTP 请求失败: 重试次数耗尽"))?;

        let content: Value = serde_json::from_slice(&response.body).map_err(|_| {
            if response.status >= 400 {
                NetError::api(format!("HTTP {}", response.status), response.status as i32)
            } else {
                NetError::protocol("响应不是 JSON")
            }
        })?;
        let Value::Object(obj) = &content else {
            return if response.status >= 400 {
                Err(NetError::api(format!("HTTP {}", response.status), response.status as i32))
            } else {
                Err(NetError::protocol("响应 JSON 不是对象"))
            };
        };
        let success = obj.get("Success").or_else(|| obj.get("success"));
        if response.status >= 400 || success == Some(&Value::Bool(false)) {
            let message = obj
                .get("Msg")
                .or_else(|| obj.get("msg"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("HTTP {}", response.status));
            let status = obj
                .get("Status")
                .or_else(|| obj.get("status"))
                .and_then(Value::as_i64)
                .unwrap_or(response.status as i64);
            return Err(NetError::api(message, status as i32));
        }
        Ok(obj.get("Response").or_else(|| obj.get("response")).cloned().unwrap_or(Value::Null))
    }

    pub fn login(&self, email: &str, password: &str) -> Result<Value, NetError> {
        let credentials = self.http_call(
            "/api/user/login",
            Some(json!({"email": email, "password": sha256_hex(password)})),
            "POST",
            None,
        )?;
        self.store_credentials(&credentials)?;
        let user = match self.get_my_info() {
            Ok(u) => u,
            Err(e) => {
                let _ = self.0.session.clear_credentials();
                return Err(e);
            }
        };
        self.0.session.set_many(vec![("User", user.clone())])?;
        Ok(user)
    }

    fn store_credentials(&self, credentials: &Value) -> Result<(), NetError> {
        let Some(obj) = credentials.as_object() else {
            return Err(NetError::api("登录响应缺少凭据", 500));
        };
        let token = obj.get("Token").or_else(|| obj.get("token")).and_then(Value::as_str);
        let refresh = obj.get("RefreshToken").or_else(|| obj.get("refreshToken")).and_then(Value::as_str);
        let (Some(token), Some(refresh)) = (token, refresh) else {
            return Err(NetError::api("登录响应缺少 Token 或 RefreshToken", 500));
        };
        self.0.session.set_many(vec![
            ("Token", Value::String(token.to_string())),
            ("RefreshToken", Value::String(refresh.to_string())),
            ("TokenUpdatedAt", json!(now_epoch_secs())),
        ])
    }

    pub fn refresh_access_token(&self) -> Result<Option<String>, NetError> {
        let Some(refresh) = self.0.session.get_str("RefreshToken").filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let _guard = self.0.refresh_lock.lock().unwrap();
        let token = self.0.session.get_str("Token").filter(|s| !s.is_empty());
        let updated = self.0.session.get("TokenUpdatedAt").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if let Some(t) = &token {
            if now_epoch_secs() - updated < 25.0 {
                return Ok(Some(t.clone()));
            }
        }
        match self.http_call("/api/user/refresh_token", Some(json!({"token": refresh})), "POST", None) {
            Ok(value) => match value.as_str() {
                Some(t) if !t.is_empty() => {
                    self.0
                        .session
                        .set_many(vec![("Token", Value::String(t.to_string())), ("TokenUpdatedAt", json!(now_epoch_secs()))])?;
                    Ok(Some(t.to_string()))
                }
                _ => Ok(None),
            },
            Err(e) => {
                // A 401 just means this refresh attempt was rejected, not
                // that the refresh token itself is dead; only drop
                // credentials on an explicit "invalid" status.
                if e.is_refresh_invalid() {
                    let _ = self.0.session.clear_credentials();
                }
                Err(e)
            }
        }
    }

    pub fn get_access_token(&self) -> Result<Option<String>, NetError> {
        get_access_token(&self.0)
    }

    pub fn refresh_user(&self) -> Result<Option<Value>, NetError> {
        if !self.has_refresh_token() {
            return Ok(None);
        }
        let user = self.get_my_info()?;
        self.0.session.set_many(vec![("User", user.clone())])?;
        Ok(Some(user))
    }

    // ---- hub invoke: cache + in-flight de-dup + 401 refresh-and-retry ----

    fn flight_key(method: &str, params: &Value) -> String {
        format!("{method}:{}", canonical_json(params))
    }

    fn cache_get(&self, key: &str) -> Option<Value> {
        let mut cache = self.0.cache.lock().unwrap();
        match cache.get(key) {
            Some(entry) if Instant::now() < entry.expires => Some(entry.value.clone()),
            Some(_) => {
                cache.remove(key);
                None
            }
            None => None,
        }
    }

    fn cache_set(&self, key: String, value: Value, ttl: Duration) {
        let mut cache = self.0.cache.lock().unwrap();
        cache.insert(key, CacheEntry { expires: Instant::now() + ttl, value });
        if cache.len() > 128 {
            let drop_keys: Vec<String> = cache.keys().take(64).cloned().collect();
            for k in drop_keys {
                cache.remove(&k);
            }
        }
    }

    pub fn invoke(&self, method: &str, params: Value, priority: i32, cache_ttl: Duration) -> Result<Value, NetError> {
        self.invoke_ex(method, params, priority, cache_ttl, None)
    }

    pub fn invoke_ex(
        &self,
        method: &str,
        params: Value,
        priority: i32,
        cache_ttl: Duration,
        coalesce: Option<bool>,
    ) -> Result<Value, NetError> {
        let coalesce = coalesce.unwrap_or(!is_non_idempotent(method));
        let key = if coalesce || !cache_ttl.is_zero() { Some(Self::flight_key(method, &params)) } else { None };
        if !cache_ttl.is_zero() {
            if let Some(k) = &key {
                if let Some(cached) = self.cache_get(k) {
                    return Ok(cached);
                }
            }
        }
        if coalesce {
            let key = key.clone().unwrap();
            let (is_leader, entry) = {
                let mut flight = self.0.flight.lock().unwrap();
                if let Some(existing) = flight.get(&key) {
                    (false, existing.clone())
                } else {
                    let entry = Arc::new(Inflight { state: Mutex::new(InflightState::Pending), cv: Condvar::new() });
                    flight.insert(key.clone(), entry.clone());
                    (true, entry)
                }
            };
            if !is_leader {
                // Merge identical concurrent reads into one hub call; fall
                // back to a direct call if the leader takes implausibly
                // long rather than waiting forever (mirrors api.py's 50s
                // `entry["event"].wait(50)`).
                let state = entry.state.lock().unwrap();
                let (state, timeout_result) = entry
                    .cv
                    .wait_timeout_while(state, Duration::from_secs(50), |s| matches!(s, InflightState::Pending))
                    .unwrap();
                if timeout_result.timed_out() {
                    drop(state);
                    return self.invoke_direct(method, params, priority);
                }
                return match &*state {
                    InflightState::Done(result) => result.clone(),
                    InflightState::Pending => unreachable!(),
                };
            }
            return self.invoke_leader(method, params, priority, cache_ttl, Some(key));
        }
        self.invoke_leader(method, params, priority, cache_ttl, key)
    }

    fn invoke_leader(
        &self,
        method: &str,
        params: Value,
        priority: i32,
        cache_ttl: Duration,
        key: Option<String>,
    ) -> Result<Value, NetError> {
        let result = self.invoke_direct(method, params, priority);
        if let Some(k) = &key {
            if let Some(entry) = self.0.flight.lock().unwrap().remove(k) {
                let mut state = entry.state.lock().unwrap();
                *state = InflightState::Done(result.clone());
                entry.cv.notify_all();
            }
        }
        if let (Ok(value), false, Some(k)) = (&result, cache_ttl.is_zero(), &key) {
            self.cache_set(k.clone(), value.clone(), cache_ttl);
        }
        result
    }

    fn invoke_direct(&self, method: &str, params: Value, priority: i32) -> Result<Value, NetError> {
        let result = self.invoke_direct_inner(method, params, priority);
        self.0.health.record(&result);
        result
    }

    fn invoke_direct_inner(&self, method: &str, params: Value, priority: i32) -> Result<Value, NetError> {
        let retry = !is_non_idempotent(method);
        match self.0.hub.invoke(method, params.clone(), true, None, retry, priority) {
            Ok(v) => Ok(v),
            Err(e) if !e.is_unauthorized() => Err(e),
            Err(unauthorized_err) => {
                // Access token expired mid-flight: drop it, refresh, retry
                // exactly once on a fresh connection. Mirrors api.py's
                // `_invoke_direct`.
                self.0.session.set_many(vec![("Token", Value::String(String::new())), ("TokenUpdatedAt", json!(0))]).ok();
                match self.refresh_access_token() {
                    Ok(Some(_)) => {
                        self.0.hub.close();
                        self.0.hub.invoke(method, params, true, None, retry, priority)
                    }
                    Ok(None) => Err(unauthorized_err),
                    Err(refresh_err) => Err(refresh_err),
                }
            }
        }
    }

    // ---- public catalogue methods (mirrors api.py 1:1) ----

    pub fn get_book_list(
        &self,
        page: i64,
        size: i64,
        keywords: Option<&str>,
        order: &str,
        ignore_japanese: bool,
        ignore_ai: bool,
        category_id: Option<i64>,
    ) -> Result<Value, NetError> {
        let mut params = helpers::object(vec![
            ("Page", json!(page)),
            ("Size", json!(size)),
            ("Order", json!(order)),
            ("IgnoreJapanese", json!(ignore_japanese)),
            ("IgnoreAI", json!(ignore_ai)),
        ]);
        if let Some(k) = keywords {
            params["KeyWords"] = json!(k);
        }
        if let Some(c) = category_id {
            params["CategoryId"] = json!(c);
        }
        let result = self.invoke("GetBookList", params, 0, Duration::ZERO)?;
        Ok(helpers::novel_data(result, "Data"))
    }

    pub fn get_book_categories(&self, book_type: &str) -> Result<Value, NetError> {
        let result = self.invoke("GetBookCategories", json!({"Type": book_type}), 0, Duration::from_secs(300))?;
        Ok(helpers::normalize_list(result, "Data"))
    }

    pub fn get_rank(&self, days: i64) -> Result<Value, NetError> {
        let result = self.invoke("GetRank", json!({"Days": days}), 0, Duration::ZERO)?;
        Ok(helpers::novel_data(result, "Data"))
    }

    pub fn get_announcement_list(&self, page: i64, size: i64) -> Result<Value, NetError> {
        let result = self.invoke(
            "GetAnnouncementList",
            json!({"Page": page, "Size": size}),
            0,
            Duration::from_secs(60),
        )?;
        Ok(helpers::normalize_data(result, "Data"))
    }

    pub fn get_announcement_detail(&self, announcement_id: i64) -> Result<Value, NetError> {
        self.invoke("GetAnnouncementDetail", json!({"Id": announcement_id}), 0, Duration::ZERO)
    }

    pub fn get_book_info(&self, book_id: i64) -> Result<Value, NetError> {
        self.invoke("GetBookInfo", json!({"Id": book_id}), 0, Duration::ZERO)
    }

    pub fn get_book_list_by_ids(&self, ids: &[i64], book_type: Option<&str>) -> Result<Value, NetError> {
        if ids.len() > 24 {
            return Err(NetError::api("单次最多请求 24 本书", 400));
        }
        let mut params = helpers::object(vec![("Ids", json!(ids))]);
        // Novels go through the default branch: the web client only sends
        // Type=Comic for a comic series. Sending Type=Novel makes the
        // server return an empty list.
        //   Some("Novel") -> default branch, comics dropped
        //   None          -> default branch, everything kept (shelf: novels + comic volumes)
        //   Some("Comic") -> Type=Comic, the server aggregates by series ({Data: [ComicListItem]})
        match book_type {
            Some(t) if t.eq_ignore_ascii_case("novel") => {
                let result = self.invoke("GetBookListByIds", params, 0, Duration::ZERO)?;
                Ok(helpers::novel_data(result, "Data"))
            }
            Some(t) => {
                params["Type"] = json!(t);
                let result = self.invoke("GetBookListByIds", params, 0, Duration::ZERO)?;
                Ok(helpers::normalize_list(result, "Data"))
            }
            None => {
                let result = self.invoke("GetBookListByIds", params, 0, Duration::ZERO)?;
                Ok(helpers::normalize_list(result, "Data"))
            }
        }
    }

    pub fn get_book_list_by_ids_chunked(
        &self,
        ids: &[i64],
        book_type: Option<&str>,
        chunk_size: usize,
    ) -> Result<Vec<Value>, NetError> {
        let size = chunk_size.max(1);
        let mut output = Vec::new();
        for chunk in ids.chunks(size) {
            if chunk.is_empty() {
                continue;
            }
            let result = self.get_book_list_by_ids(chunk, book_type)?;
            let list = match &result {
                Value::Object(obj) => obj.get("Data").or_else(|| obj.get("data")).cloned().unwrap_or(Value::Array(vec![])),
                other => other.clone(),
            };
            output.extend(dict_items(&list));
        }
        Ok(output)
    }

    pub fn get_books_by_series(
        &self,
        series_name: &str,
        page: i64,
        size: i64,
        order: &str,
        ignore_japanese: bool,
        ignore_ai: bool,
    ) -> Result<Value, NetError> {
        let params = json!({
            "SeriesName": series_name,
            "Page": page,
            "Size": size,
            "Order": order,
            "IgnoreJapanese": ignore_japanese,
            "IgnoreAI": ignore_ai,
        });
        let result = self.invoke("GetBooksBySeries", params, 0, Duration::ZERO)?;
        Ok(helpers::novel_data(result, "Data"))
    }

    pub fn get_novel_content(&self, book_id: i64, sort_num: i64, convert: Option<&str>, priority: i32) -> Result<Value, NetError> {
        let mut params = helpers::object(vec![("Bid", json!(book_id)), ("SortNum", json!(sort_num))]);
        if let Some(c) = convert {
            params["Convert"] = json!(c);
        }
        self.invoke("GetNovelContent", params, priority, Duration::ZERO)
    }

    /// `priority`: 0 = 交互级; 数字越大越让位给其它请求 (换章时的上传用后台优先级)。
    pub fn save_read_position(&self, book_id: i64, chapter_id: i64, xpath: &str, priority: i32) -> Result<Value, NetError> {
        let xpath = if xpath.is_empty() { "." } else { xpath };
        self.invoke(
            "SaveReadPosition",
            json!({"Bid": book_id, "Cid": chapter_id, "XPath": xpath}),
            priority,
            Duration::ZERO,
        )
    }

    pub fn get_read_history(&self) -> Result<Value, NetError> {
        let mut result = self.invoke("GetReadHistory", json!({}), 0, Duration::ZERO)?;
        if let Value::Object(obj) = &mut result {
            for key in ["Novel", "Comic"] {
                let ids = obj.get(key).cloned().unwrap_or(Value::Null);
                obj.insert(key.to_string(), json!(int_items(&ids)));
            }
        }
        Ok(result)
    }

    // ---- comics (web client `services/manga`) ----

    /// Comic series list. `order`: `latest` | `new` | `view`.
    /// Returns `{Data: [{Id, Title, Cover, Count, LastUpdatedAt}], TotalPages}`.
    pub fn get_comic_list(&self, page: i64, size: i64, order: &str) -> Result<Value, NetError> {
        let result = self.invoke("GetComicList", json!({"Page": page, "Size": size, "Order": order}), 0, Duration::ZERO)?;
        Ok(helpers::normalize_list(result, "Data"))
    }

    /// Novel search, as the web client's `pages/Search.vue`: `method` is one of
    /// `GetBookList` (fuzzy; exact = keywords wrapped in quotes), `GetBookListByTitle`,
    /// `GetBookListByAuthor`, `GetBookListByName` (series) or `GetBookListByTags`
    /// (comma-separated, AND). No `Order`: the server ranks the matches.
    pub fn search_books(&self, method: &str, keywords: &str, page: i64, size: i64, ignore_japanese: bool, ignore_ai: bool) -> Result<Value, NetError> {
        let params = json!({
            "Page": page,
            "Size": size,
            "KeyWords": keywords,
            "IgnoreJapanese": ignore_japanese,
            "IgnoreAI": ignore_ai,
        });
        let result = self.invoke(method, params, 0, Duration::ZERO)?;
        Ok(helpers::novel_data(result, "Data"))
    }

    /// Comic search (`SearchComicSeries`): same `Mode` values as the novel search
    /// (fuzzy | exact | title | author | name | tags), results aggregated by series
    /// (`{Data: [ComicListItem]}` like `GetComicList`).
    pub fn search_comic_series(&self, keywords: &str, mode: &str, page: i64, size: i64, ignore_japanese: bool, ignore_ai: bool) -> Result<Value, NetError> {
        let params = json!({
            "KeyWords": keywords,
            "Mode": mode,
            "Page": page,
            "Size": size,
            "IgnoreJapanese": ignore_japanese,
            "IgnoreAI": ignore_ai,
        });
        let result = self.invoke("SearchComicSeries", params, 0, Duration::ZERO)?;
        Ok(helpers::normalize_list(result, "Data"))
    }

    /// One batch of page image URLs of a comic chapter:
    /// `{Chapter: {Id, BookId, BookName, Title, SortNum, Total, Skip, Images}, ReadPosition?}`.
    /// The server only returns `ReadPosition` (`Position` = 1-based page) when `skip == 0`.
    pub fn get_comic_content(&self, chapter_id: i64, skip: i64, take: i64, priority: i32) -> Result<Value, NetError> {
        self.invoke("GetComicContent", json!({"Cid": chapter_id, "Skip": skip, "Take": take}), priority, Duration::ZERO)
    }

    pub fn clear_read_history(&self) -> Result<Value, NetError> {
        self.invoke("ClearReadHistory", json!({}), 0, Duration::ZERO)
    }

    pub fn get_my_info(&self) -> Result<Value, NetError> {
        self.invoke("GetMyInfo", json!({}), 0, Duration::ZERO)
    }

    pub fn get_notifications(&self, page: i64, size: i64) -> Result<Value, NetError> {
        let result = self.invoke("GetNotifications", json!({"Page": page, "Size": size}), 0, Duration::ZERO)?;
        Ok(helpers::normalize_data(result, "Data"))
    }

    pub fn mark_notifications(&self, ids: &[i64]) -> Result<Value, NetError> {
        self.invoke("MarkNotifications", json!({"Ids": ids}), 0, Duration::ZERO)
    }

    pub fn get_book_shelf(&self) -> Result<Value, NetError> {
        // The shelf is written back as a full-table overwrite; normalize
        // only, never drop comics/folders here.
        let envelope = self.invoke("GetBookShelf", json!({}), 0, Duration::ZERO)?;
        match envelope {
            Value::Object(mut obj) => {
                let key = if obj.contains_key("data") { "data" } else { "Data" };
                let data = obj.get(key).cloned().unwrap_or(Value::Null);
                obj.insert(key.to_string(), Value::Array(dict_items(&data)));
                Ok(Value::Object(obj))
            }
            other => Ok(Value::Array(dict_items(&other))),
        }
    }

    pub fn save_book_shelf(&self, items: Value, version: &str) -> Result<Value, NetError> {
        self.invoke("SaveBookShelf", json!({"data": items, "ver": version}), 0, Duration::ZERO)
    }

    pub fn sign_in(&self) -> Result<Value, NetError> {
        self.invoke("SignIn", json!({}), 0, Duration::ZERO)
    }

    pub fn get_shop(&self) -> Result<Value, NetError> {
        let result = self.invoke("GetShop", json!({}), 0, Duration::ZERO)?;
        Ok(helpers::normalize_list(result, "Data"))
    }

    pub fn get_my_items(&self) -> Result<Value, NetError> {
        let result = self.invoke("GetMyItems", json!({}), 0, Duration::ZERO)?;
        Ok(helpers::normalize_list(result, "Data"))
    }

    pub fn buy_shop_item(&self, key: &str, quantity: i64) -> Result<Value, NetError> {
        self.invoke("BuyShopItem", json!({"Key": key, "Quantity": quantity}), 0, Duration::ZERO)
    }

    pub fn get_comments(&self, comment_type: &str, target_id: i64, page: i64) -> Result<Value, NetError> {
        self.invoke("GetComments", json!({"Type": comment_type, "Id": target_id, "Page": page}), 0, Duration::ZERO)
    }
}

fn get_access_token(inner: &Arc<Inner>) -> Result<Option<String>, NetError> {
    {
        let _guard = inner.refresh_lock.lock().unwrap();
        let token = inner.session.get_str("Token").filter(|s| !s.is_empty());
        let updated = inner.session.get("TokenUpdatedAt").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if token.is_some() && now_epoch_secs() - updated < 25.0 {
            return Ok(token);
        }
    }
    if !inner.session.has_refresh_token() {
        return Ok(None);
    }
    match refresh_access_token_inner(inner) {
        Ok(opt) => Ok(opt),
        Err(e) => {
            if e.is_refresh_invalid() {
                let _ = inner.session.clear_credentials();
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

fn refresh_access_token_inner(inner: &Arc<Inner>) -> Result<Option<String>, NetError> {
    // `Client::refresh_access_token` is the single implementation; wrap
    // `inner` back into a `Client` handle (cheap: one Arc clone) so the
    // free-function token-provider closure and the public method share one
    // code path.
    Client(inner.clone()).refresh_access_token()
}

fn sha256_hex(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn value_to_query(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Deterministic JSON text for use as a de-dup/cache key, independent of
/// object key insertion order (mirrors `api.py`'s
/// `json.dumps(params, sort_keys=True)`).
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys.iter().map(|k| format!("{:?}:{}", k, canonical_json(&map[*k]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_ignores_key_order() {
        let a = json!({"b": 1, "a": 2});
        let b = json!({"a": 2, "b": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
    }

    #[test]
    fn sha256_matches_known_vector() {
        // sha256("") well-known test vector.
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
