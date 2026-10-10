//! Port of `bin/src/kinnovel/transport.py`'s `SignalRClient`: negotiate over
//! HTTP, connect a WebSocket, do the SignalR JSON protocol handshake, send
//! invocations and match responses by `invocationId`, keep the connection
//! alive with a background ping thread that also closes the socket after it
//! has idled too long, and transparently reconnect on the next `invoke()`.
//!
//! Blocking, thread-safe: many worker threads may call `invoke()`
//! concurrently; `rate_limit` throttles how many actually reach the network,
//! and `turn` ensures only one invocation is in flight on the shared socket
//! at a time (with interactive calls, priority 0, always jumping ahead of
//! lower-priority ones that are already waiting).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::Message;

use crate::error::NetError;
use crate::gzip::{gunzip_limited, GzipError};
use crate::http::{self, ServerUrl};
use crate::rate_limit::RateLimit;
use crate::stream::{self, NetStream};
use crate::turn::TurnScheduler;

const RECORD_SEPARATOR: u8 = 0x1e;
/// Matches `transport.py`'s `MAX_SIGNALR_RECORD_BYTES` / response gzip cap.
const MAX_SIGNALR_RECORD_BYTES: usize = 16 * 1024 * 1024;
const MAX_GUNZIP_BYTES: usize = 8 * 1024 * 1024;

/// Returns the current access token to send with a negotiate/invoke call,
/// or `Err` if refreshing it failed with something other than "refresh
/// token is dead" (mirrors `api.py`'s `get_access_token`, which swallows
/// only the invalid-refresh-token case and lets any other error propagate).
pub type TokenProvider = Arc<dyn Fn() -> Result<Option<String>, NetError> + Send + Sync>;

#[derive(Clone)]
pub struct SignalRConfig {
    pub server: String,
    pub strict_tls: bool,
    pub request_limit: u32,
    pub request_window: Duration,
    pub timeout: Duration,
    pub visitor_id: String,
    /// Reconnect if the socket has sat idle (no successful read/write) for
    /// longer than this when a new invoke wants to use it. Python: 20s.
    pub idle_reconnect: Duration,
    /// How often the keepalive thread sends a protocol ping. Python: 10s.
    pub keepalive_interval: Duration,
    /// Proactively close the socket once it has not carried a *real*
    /// invoke (keepalive pings don't count) for this long. Python: 120s.
    pub keepalive_idle_limit: Duration,
}

impl Default for SignalRConfig {
    fn default() -> Self {
        SignalRConfig {
            server: String::new(),
            strict_tls: true,
            request_limit: 9,
            request_window: Duration::from_millis(5500),
            timeout: Duration::from_secs(30),
            visitor_id: String::new(),
            idle_reconnect: Duration::from_secs(20),
            keepalive_interval: Duration::from_secs(10),
            keepalive_idle_limit: Duration::from_secs(120),
        }
    }
}

struct ConnState {
    socket: Option<tungstenite::WebSocket<NetStream>>,
    record_buffer: Vec<u8>,
    last_used: Option<Instant>,
    /// Only bumped by a real `invoke()`, not by the keepalive ping -- used
    /// to decide whether the connection has been truly idle long enough to
    /// drop (saves the radio on the device).
    last_real_use: Option<Instant>,
}

struct KeepaliveSignal {
    stop: Mutex<bool>,
    cv: Condvar,
}

pub struct SignalRClient {
    server: Mutex<String>,
    strict_tls: bool,
    token_provider: Option<TokenProvider>,
    timeout: Duration,
    connect_timeout: Duration,
    visitor_id: String,
    idle_reconnect: Duration,
    keepalive_interval: Duration,
    keepalive_idle_limit: Duration,

    pub rate_limit: RateLimit,
    turn: TurnScheduler,

    state: Mutex<ConnState>,
    /// A cheap clone of the current connection's raw TCP handle, kept
    /// outside `state` so `close()`/`shutdown()` can interrupt a blocked
    /// read in another thread immediately, without waiting on `state`
    /// (mirrors `transport.py`'s `close()`, which touches `self._socket`
    /// without `self._lock` for the same reason).
    raw: Mutex<Option<std::net::TcpStream>>,
    shutdown: AtomicBool,

    keepalive_signal: Arc<KeepaliveSignal>,
    keepalive_handle: Mutex<Option<std::thread::JoinHandle<()>>>,

    self_weak: Weak<SignalRClient>,
}

impl SignalRClient {
    pub fn new(config: SignalRConfig, token_provider: Option<TokenProvider>) -> Arc<Self> {
        let connect_timeout = config.timeout.min(Duration::from_secs(10));
        Arc::new_cyclic(|weak| SignalRClient {
            server: Mutex::new(config.server.trim_end_matches('/').to_string()),
            strict_tls: config.strict_tls,
            token_provider,
            timeout: config.timeout,
            connect_timeout,
            visitor_id: config.visitor_id,
            idle_reconnect: config.idle_reconnect,
            keepalive_interval: config.keepalive_interval,
            keepalive_idle_limit: config.keepalive_idle_limit,
            rate_limit: RateLimit::new(config.request_limit, config.request_window),
            turn: TurnScheduler::new(),
            state: Mutex::new(ConnState {
                socket: None,
                record_buffer: Vec::new(),
                last_used: None,
                last_real_use: None,
            }),
            raw: Mutex::new(None),
            shutdown: AtomicBool::new(false),
            keepalive_signal: Arc::new(KeepaliveSignal { stop: Mutex::new(true), cv: Condvar::new() }),
            keepalive_handle: Mutex::new(None),
            self_weak: weak.clone(),
        })
    }

    pub fn server(&self) -> String {
        self.server.lock().unwrap().clone()
    }

    pub fn set_server(&self, server: &str) {
        let mut state = self.state.lock().unwrap();
        self.close_locked(&mut state);
        *self.server.lock().unwrap() = server.trim_end_matches('/').to_string();
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    fn closed_err() -> NetError {
        NetError::network("SignalR 客户端已关闭")
    }

    /// Drop the current connection but keep the client reusable (the next
    /// `invoke()` reconnects lazily). Mirrors `transport.py`'s `close()`.
    pub fn close(&self) {
        // Shut down the raw fd first, independent of `state`'s lock, so a
        // thread blocked inside a read on this socket unblocks promptly
        // even if we then have to wait briefly for `state` ourselves.
        if let Some(tcp) = self.raw.lock().unwrap().take() {
            let _ = tcp.shutdown(std::net::Shutdown::Both);
        }
        let mut state = self.state.lock().unwrap();
        self.close_locked(&mut state);
    }

    fn close_locked(&self, state: &mut ConnState) {
        if let Some(tcp) = self.raw.lock().unwrap().take() {
            let _ = tcp.shutdown(std::net::Shutdown::Both);
        }
        if let Some(mut socket) = state.socket.take() {
            let _ = socket.close(None);
        }
        state.record_buffer.clear();
    }

    /// Final close: interrupt a blocked receive and stop reconnecting.
    /// Mirrors `transport.py`'s `shutdown()`.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.turn.shutdown();
        {
            let mut stop = self.keepalive_signal.stop.lock().unwrap();
            *stop = true;
            self.keepalive_signal.cv.notify_all();
        }
        self.close();
    }

    fn headers(&self, token: Option<&str>) -> Vec<(&'static str, String)> {
        let mut headers = vec![
            ("x-id", self.visitor_id.clone()),
            ("Accept", "application/json".to_string()),
            ("User-Agent", "KinNovel/1.0".to_string()),
        ];
        if let Some(t) = token {
            headers.push(("Authorization", format!("Bearer {t}")));
        }
        headers
    }

    fn tls_config(&self) -> Arc<rustls::ClientConfig> {
        stream::tls_config(self.strict_tls)
    }

    fn negotiate(&self, token: Option<&str>, timeout: Duration) -> Result<Value, NetError> {
        let server = self.server();
        let response = http::request(
            &server,
            "/hub/api/negotiate?negotiateVersion=1",
            "POST",
            &self.headers(token),
            b"",
            timeout,
            self.tls_config(),
        )?;
        if response.status >= 500 {
            return Err(NetError::network(format!("Hub 协商失败 ({})", response.status)));
        }
        let body_text = String::from_utf8_lossy(&response.body).to_string();
        if response.status >= 400 {
            let snippet: String = body_text.chars().take(300).collect();
            let message = if snippet.is_empty() {
                format!("Hub 协商失败 ({})", response.status)
            } else {
                snippet
            };
            return Err(NetError::api(message, response.status as i32));
        }
        serde_json::from_str(&body_text)
            .map_err(|_| NetError::protocol("Hub 协商响应不是 JSON"))
    }

    fn connect_locked(&self, state: &mut ConnState) -> Result<(), NetError> {
        if self.is_shutdown() {
            return Err(Self::closed_err());
        }
        self.close_locked(state);
        let token = match self.token_provider.as_ref() {
            Some(provider) => provider()?,
            None => None,
        };
        let negotiation = self.negotiate(token.as_deref(), self.connect_timeout)?;
        let connection_token = negotiation
            .get("connectionToken")
            .and_then(Value::as_str)
            .ok_or_else(|| NetError::protocol("Hub 协商响应缺少 connectionToken"))?
            .to_string();

        let server = self.server();
        let url = ServerUrl::parse(&server)?;
        let mut path = format!("/hub/api?id={}", url_encode(&connection_token));
        if let Some(t) = &token {
            path.push_str("&access_token=");
            path.push_str(&url_encode(t));
        }

        let net_stream =
            NetStream::connect(&url.host, url.port, url.secure, self.connect_timeout, self.tls_config())?;
        net_stream.set_read_timeout(Some(self.connect_timeout)).ok();
        net_stream.set_write_timeout(Some(self.connect_timeout)).ok();
        let raw_handle = net_stream.try_clone_raw()?;

        let host_header = if url.port == default_port(url.secure) {
            url.host.clone()
        } else {
            format!("{}:{}", url.host, url.port)
        };
        let uri = format!("{}://{}{}", url.ws_scheme(), host_header, path);
        let request = tungstenite::http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("Host", host_header)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Origin", "https://www.lightnovel.app")
            .header("User-Agent", "KinNovel/1.0")
            .body(())
            .map_err(|e| NetError::protocol(format!("构造 WebSocket 请求失败: {e}")))?;

        let ws_config = WebSocketConfig {
            max_message_size: Some(MAX_SIGNALR_RECORD_BYTES),
            max_frame_size: Some(MAX_SIGNALR_RECORD_BYTES),
            ..WebSocketConfig::default()
        };
        let (mut socket, _response) =
            tungstenite::client::client_with_config(request, net_stream, Some(ws_config))
                .map_err(|e| match e {
                    tungstenite::HandshakeError::Failure(err) => translate_ws_err(err),
                    tungstenite::HandshakeError::Interrupted(_) => {
                        NetError::network("WebSocket 握手被中断")
                    }
                })?;

        send_record(&mut socket, &json!({"protocol": "json", "version": 1}))
            .map_err(translate_ws_err)?;

        let deadline = Instant::now() + self.connect_timeout;
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                let _ = socket.close(None);
                return Err(NetError::Timeout);
            }
            socket.get_ref().set_read_timeout(Some(deadline - now)).ok();
            let message = match socket.read() {
                Ok(m) => m,
                Err(e) => {
                    let _ = socket.close(None);
                    return Err(translate_ws_err(e));
                }
            };
            match message {
                Message::Text(t) => buf.extend_from_slice(t.as_bytes()),
                Message::Binary(b) => buf.extend_from_slice(&b),
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                Message::Close(frame) => {
                    return Err(NetError::network(format!(
                        "SignalR 握手时连接被关闭: {}",
                        frame.map(|f| f.reason.to_string()).unwrap_or_default()
                    )))
                }
            }
            let mut handshake_done = false;
            while let Some(pos) = buf.iter().position(|&b| b == RECORD_SEPARATOR) {
                let raw: Vec<u8> = buf.drain(..=pos).collect();
                let text = String::from_utf8_lossy(&raw[..raw.len() - 1]).trim().to_string();
                if text.is_empty() {
                    continue;
                }
                let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if let Some(err) = parsed.get("error").and_then(Value::as_str) {
                    let _ = socket.close(None);
                    return Err(NetError::network(format!("SignalR 握手失败: {err}")));
                }
                if parsed.get("type").and_then(Value::as_i64) == Some(7) {
                    let _ = socket.close(None);
                    return Err(NetError::network(format!(
                        "SignalR 握手被拒绝: {}",
                        parsed.get("error").and_then(Value::as_str).unwrap_or("")
                    )));
                }
                if parsed.as_object().map(|o| o.is_empty()).unwrap_or(false) {
                    handshake_done = true;
                    break;
                }
            }
            if handshake_done {
                break;
            }
        }

        *self.raw.lock().unwrap() = Some(raw_handle);
        state.socket = Some(socket);
        state.record_buffer.clear();
        let now = Instant::now();
        state.last_used = Some(now);
        state.last_real_use = Some(now);
        self.start_keepalive();
        Ok(())
    }

    fn ensure_connected_locked(&self, state: &mut ConnState) -> Result<(), NetError> {
        if self.is_shutdown() {
            return Err(Self::closed_err());
        }
        if state.socket.is_some() {
            if let Some(last_used) = state.last_used {
                if last_used.elapsed() > self.idle_reconnect {
                    self.close_locked(state);
                }
            }
        }
        if state.socket.is_none() {
            self.connect_locked(state)?;
        }
        Ok(())
    }

    fn start_keepalive(&self) {
        if self.is_shutdown() {
            return;
        }
        let mut handle = self.keepalive_handle.lock().unwrap();
        if let Some(h) = handle.as_ref() {
            if !h.is_finished() {
                return;
            }
        }
        let Some(strong) = self.self_weak.upgrade() else { return };
        {
            let mut stop = self.keepalive_signal.stop.lock().unwrap();
            *stop = false;
        }
        *handle = Some(std::thread::Builder::new()
            .name("kn-net-signalr-keepalive".into())
            .spawn(move || strong.keepalive_loop())
            .expect("spawn keepalive thread"));
    }

    fn keepalive_loop(&self) {
        loop {
            let timed_out = {
                let stop = self.keepalive_signal.stop.lock().unwrap();
                let (guard, result) = self
                    .keepalive_signal
                    .cv
                    .wait_timeout_while(stop, self.keepalive_interval, |stop| !*stop)
                    .unwrap();
                drop(guard);
                result.timed_out()
            };
            if !timed_out {
                // Woken up because `stop` was set (shutdown or a fresh
                // connect wants a clean restart).
                return;
            }
            if self.is_shutdown() {
                return;
            }
            match self.keepalive_ping() {
                Ok(true) => continue,
                Ok(false) => return,
                Err(_) => {
                    self.close();
                    return;
                }
            }
        }
    }

    /// Send a protocol ping, or close the socket once it has idled too
    /// long. Returns `Ok(false)` once the connection has been closed (the
    /// keepalive thread should exit; the next `invoke()` reconnects).
    fn keepalive_ping(&self) -> Result<bool, NetError> {
        let mut state = self.state.lock().unwrap();
        if self.is_shutdown() || state.socket.is_none() {
            return Ok(false);
        }
        let now = Instant::now();
        if let Some(last_real_use) = state.last_real_use {
            if now.duration_since(last_real_use) >= self.keepalive_idle_limit {
                self.close_locked(&mut state);
                return Ok(false);
            }
        }
        if let Some(last_used) = state.last_used {
            if now.duration_since(last_used) < self.keepalive_interval.mul_f64(0.8) {
                return Ok(true);
            }
        }
        if let Some(socket) = state.socket.as_ref() {
            // A stuck send must not hold `state` for the whole hub timeout.
            socket.get_ref().set_write_timeout(Some(Duration::from_secs(5))).ok();
        }
        let text = format!("{{\"type\":6}}{}", RECORD_SEPARATOR as char);
        match state.socket.as_mut().unwrap().send(Message::Text(text.into())) {
            Ok(()) => {
                state.last_used = Some(Instant::now());
                Ok(true)
            }
            Err(e) => Err(translate_ws_err(e)),
        }
    }

    fn handle_server_message(&self, state: &mut ConnState, message: &Value) {
        if message.get("type").and_then(Value::as_i64) == Some(6) {
            if let Some(socket) = state.socket.as_mut() {
                let text = format!("{{\"type\":6}}{}", RECORD_SEPARATOR as char);
                let _ = socket.send(Message::Text(text.into()));
            }
        }
        // transport.py also stashes `type == 1` server push notifications in
        // a 1-deep diagnostic deque; kn-ui has no realtime-push UI yet, so
        // that's deliberately not ported (see kn-net's crate docs).
    }

    fn decode_response(&self, value: Value) -> Result<Value, NetError> {
        let Value::String(s) = &value else {
            return Ok(value);
        };
        let Ok(raw) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s) else {
            return Ok(value);
        };
        match gunzip_limited(&raw, MAX_GUNZIP_BYTES) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) => Ok(v),
                Err(_) => Ok(value),
            },
            Err(GzipError::TooLarge) => Err(NetError::protocol("gzip 响应超过 8MB 上限")),
            Err(GzipError::Incomplete) => Err(NetError::protocol("gzip 响应不完整")),
            Err(GzipError::InvalidFormat) => Ok(value),
        }
    }

    fn receive_messages(&self, state: &mut ConnState) -> Result<Vec<Value>, NetError> {
        let socket = state.socket.as_mut().ok_or_else(|| NetError::network("未连接"))?;
        let message = socket.read().map_err(translate_ws_err)?;
        match message {
            Message::Text(t) => state.record_buffer.extend_from_slice(t.as_bytes()),
            Message::Binary(b) => state.record_buffer.extend_from_slice(&b),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => return Ok(Vec::new()),
            Message::Close(frame) => {
                return Err(NetError::network(format!(
                    "WebSocket 已被服务器关闭: {}",
                    frame.map(|f| f.reason.to_string()).unwrap_or_default()
                )))
            }
        }
        if state.record_buffer.len() > MAX_SIGNALR_RECORD_BYTES {
            self.close_locked(state);
            return Err(NetError::protocol("SignalR 记录超过 16MB"));
        }
        let mut messages = Vec::new();
        while let Some(pos) = state.record_buffer.iter().position(|&b| b == RECORD_SEPARATOR) {
            let raw: Vec<u8> = state.record_buffer.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&raw[..raw.len() - 1]).trim().to_string();
            if text.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => messages.push(v),
                Err(_) => messages.push(json!({"type": -1, "raw": text})),
            }
        }
        Ok(messages)
    }

    fn invoke_once(&self, invocation: &Value, timeout: Duration) -> Result<Value, NetError> {
        if self.is_shutdown() {
            return Err(Self::closed_err());
        }
        let mut state = self.state.lock().unwrap();
        self.ensure_connected_locked(&mut state)?;
        let text = format!("{}{}", serde_json::to_string(invocation).unwrap(), RECORD_SEPARATOR as char);
        if let Err(e) = state.socket.as_mut().unwrap().send(Message::Text(text.into())) {
            self.close_locked(&mut state);
            return Err(translate_ws_err(e));
        }
        let now = Instant::now();
        state.last_used = Some(now);
        state.last_real_use = Some(now);
        let deadline = now + timeout;
        let target_id = invocation.get("invocationId").and_then(Value::as_str).map(str::to_string);
        loop {
            if self.is_shutdown() {
                return Err(Self::closed_err());
            }
            let now = Instant::now();
            if now >= deadline {
                self.close_locked(&mut state);
                return Err(NetError::Timeout);
            }
            if let Some(socket) = state.socket.as_ref() {
                socket.get_ref().set_read_timeout(Some(deadline - now)).ok();
            }
            let messages = match self.receive_messages(&mut state) {
                Ok(m) => m,
                Err(e) => {
                    self.close_locked(&mut state);
                    return Err(e);
                }
            };
            state.last_used = Some(Instant::now());
            for message in &messages {
                self.handle_server_message(&mut state, message);
                if message.get("type").and_then(Value::as_i64) == Some(7) {
                    let err = message.get("error").and_then(Value::as_str).unwrap_or("服务器关闭连接");
                    let msg = err.to_string();
                    self.close_locked(&mut state);
                    return Err(NetError::network(format!("Hub 被服务器关闭: {msg}")));
                }
                if message.get("invocationId").and_then(Value::as_str) != target_id.as_deref() {
                    continue;
                }
                if message.get("type").and_then(Value::as_i64) == Some(3) {
                    if let Some(error) = message.get("error").and_then(Value::as_str) {
                        let status = if error.to_lowercase().contains("unauthorized") { 401 } else { 500 };
                        return Err(NetError::api(error.to_string(), status));
                    }
                    let envelope = message.get("result").cloned().unwrap_or(Value::Null);
                    if !envelope.is_object() {
                        return self.decode_response(envelope);
                    }
                    let success = envelope_value(&envelope, "success").map(|v| v != Value::Bool(false)).unwrap_or(true);
                    if !success {
                        let text = envelope_value(&envelope, "msg")
                            .and_then(|v| v.as_str().map(str::to_string))
                            .unwrap_or_else(|| "请求失败".to_string());
                        let status = envelope_value(&envelope, "status")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(500) as i32;
                        return Err(NetError::api(text, status));
                    }
                    let response = envelope_value(&envelope, "response").unwrap_or(Value::Null);
                    return self.decode_response(response);
                }
            }
        }
    }

    /// Call a hub method. `retry` controls whether a `Network`/`Protocol`/
    /// `Timeout` failure gets one reconnect-and-retry (mirrors Python:
    /// idempotent methods retry once, `NON_IDEMPOTENT_METHODS` do not); an
    /// `Api` error (the server explicitly rejected the call) is never
    /// retried either way. `priority` feeds the turn scheduler (0 =
    /// interactive, wins over any higher/less-urgent number already
    /// waiting).
    pub fn invoke(
        &self,
        method: &str,
        params: Value,
        use_gzip: bool,
        timeout: Option<Duration>,
        retry: bool,
        priority: i32,
    ) -> Result<Value, NetError> {
        if self.is_shutdown() {
            return Err(Self::closed_err());
        }
        let timeout = timeout.unwrap_or_else(|| self.timeout.mul_f64(2.0).min(Duration::from_secs(25)));
        let invocation_id = random_hex_id();
        let invocation = json!({
            "type": 1,
            "invocationId": invocation_id,
            "target": method,
            "arguments": [params, {"UseGzip": use_gzip}],
        });
        let attempts = if retry { 2 } else { 1 };
        let mut last_error = None;
        for attempt in 0..attempts {
            self.rate_limit.wait();
            if self.is_shutdown() {
                return Err(Self::closed_err());
            }
            if self.turn.acquire(priority).is_err() {
                return Err(Self::closed_err());
            }
            let result = self.invoke_once(&invocation, timeout);
            self.turn.release();
            match result {
                Ok(v) => return Ok(v),
                Err(e @ NetError::Api { .. }) => return Err(e),
                Err(e) => {
                    last_error = Some(e);
                    self.close();
                    if self.is_shutdown() || attempt + 1 >= attempts {
                        return Err(last_error.unwrap());
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        Err(last_error.unwrap_or_else(|| NetError::network(format!("Hub 调用失败: {method}"))))
    }
}

fn envelope_value(envelope: &Value, key: &str) -> Option<Value> {
    let obj = envelope.as_object()?;
    for candidate in [key.to_string(), key.to_lowercase(), key.to_uppercase(), capitalize(key)] {
        if let Some(v) = obj.get(&candidate) {
            return Some(v.clone());
        }
    }
    None
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn send_record(
    socket: &mut tungstenite::WebSocket<NetStream>,
    value: &Value,
) -> Result<(), tungstenite::Error> {
    let text = format!("{}{}", serde_json::to_string(value).unwrap(), RECORD_SEPARATOR as char);
    socket.send(Message::Text(text.into()))
}

fn translate_ws_err(err: tungstenite::Error) -> NetError {
    use tungstenite::Error;
    match err {
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => NetError::Timeout,
            _ => NetError::network(io_err.to_string()),
        },
        Error::ConnectionClosed | Error::AlreadyClosed => NetError::network("WebSocket 连接已关闭"),
        Error::Capacity(_) => NetError::protocol("WebSocket 消息超过上限"),
        Error::Protocol(p) => NetError::protocol(p.to_string()),
        other => NetError::network(other.to_string()),
    }
}

fn default_port(secure: bool) -> u16 {
    if secure { 443 } else { 80 }
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

/// 32 hex characters, good enough as a visitor/invocation id (not a real
/// UUIDv4 -- same tradeoff `rust/spike-net` made: no RNG dependency pulled
/// in just for this).
pub fn random_hex_id() -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let thread_id = format!("{:?}", std::thread::current().id());
    let mut hash: u128 = nanos ^ (counter << 64) ^ (std::process::id() as u128) << 32;
    for b in thread_id.bytes() {
        hash = hash.rotate_left(5) ^ (b as u128);
    }
    format!("{hash:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_hex_id_is_32_lowercase_hex_chars() {
        let id = random_hex_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn random_hex_id_is_unique_across_calls() {
        let a = random_hex_id();
        let b = random_hex_id();
        assert_ne!(a, b);
    }

    #[test]
    fn envelope_value_matches_any_case() {
        let envelope = json!({"Success": true, "msg": "ok"});
        assert_eq!(envelope_value(&envelope, "success"), Some(Value::Bool(true)));
        assert_eq!(envelope_value(&envelope, "Msg"), Some(Value::String("ok".into())));
        assert_eq!(envelope_value(&envelope, "missing"), None);
    }
}
