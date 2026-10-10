//! A minimal fake LightNovelShelf server for offline tests: handles the
//! plain-HTTP `negotiate`/`refresh_token` endpoints and the SignalR
//! WebSocket hub (protocol handshake + invocation/response framing), all
//! over plain `ws://`/`http://` on 127.0.0.1 -- no TLS, no real network.
//!
//! Test cases configure behavior through `FakeServerConfig`'s closures
//! rather than this file growing a special case per test.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::Message;

const RECORD_SEPARATOR: u8 = 0x1e;

/// What the hub should do in response to one received invocation.
#[allow(dead_code)] // `Failure`/`NoReply` are available for tests that need them, even if none currently do.
pub enum Action {
    /// Send back a successful envelope wrapping `data` as the `Response`.
    Success(Value),
    /// Send back an envelope with `success: false`.
    Failure { message: String, status: i64 },
    /// Send back a raw `{"error": ...}` SignalR completion (maps to
    /// `NetError::Api` with status 401 if the message contains
    /// "unauthorized", 500 otherwise -- mirrors the real server's style).
    RawError(String),
    /// Don't respond at all (the client's invoke should time out).
    NoReply,
    /// Close the TCP connection immediately, simulating a dropped socket.
    DropConnection,
}

pub type InvokeHandler = Arc<dyn Fn(&str, &Value) -> Action + Send + Sync>;
pub type RefreshHandler = Arc<dyn Fn(&Value) -> (u16, Value) + Send + Sync>;

pub struct FakeServerConfig {
    pub invoke_handler: InvokeHandler,
    pub refresh_handler: Option<RefreshHandler>,
    /// If true, the WS hub never ACKs the SignalR protocol handshake
    /// (used to make `connect` fail/time out).
    pub reject_handshake: bool,
}

impl Default for FakeServerConfig {
    fn default() -> Self {
        FakeServerConfig {
            invoke_handler: Arc::new(|_, _| Action::Success(json!({"ok": true}))),
            refresh_handler: None,
            reject_handshake: false,
        }
    }
}

pub struct FakeServer {
    pub addr: String,
    pub ws_connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    pub fn start(config: FakeServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake server");
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let ws_connections = Arc::new(AtomicUsize::new(0));
        let config = Arc::new(config);

        let stop_clone = stop.clone();
        let ws_connections_clone = ws_connections.clone();
        std::thread::spawn(move || {
            while !stop_clone.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // The listener is non-blocking (so this accept loop
                        // can also check `stop`); accepted sockets inherit
                        // that on some platforms (observed on Windows), so
                        // force each connection back to blocking I/O.
                        stream.set_nonblocking(false).ok();
                        let config = config.clone();
                        let ws_connections = ws_connections_clone.clone();
                        std::thread::spawn(move || {
                            handle_connection(stream, config, ws_connections);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        FakeServer { addr: format!("127.0.0.1:{}", addr.port()), ws_connections, stop }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn connection_count(&self) -> usize {
        self.ws_connections.load(Ordering::SeqCst)
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn handle_connection(mut stream: TcpStream, config: Arc<FakeServerConfig>, ws_connections: Arc<AtomicUsize>) {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buf = Vec::new();
    let header_end = match read_until(&mut stream, &mut buf, b"\r\n\r\n") {
        Some(pos) => pos,
        None => return,
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("").to_string();
    let mut content_length = 0usize;
    let mut is_upgrade = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("upgrade:") && lower.contains("websocket") {
            is_upgrade = true;
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    if is_upgrade {
        handle_ws(stream, buf, header_end, config, ws_connections);
        return;
    }

    let body_start = header_end + 4;
    let mut body: Vec<u8> = buf.split_off(body_start.min(buf.len()));
    while body.len() < content_length {
        let mut tmp = [0u8; 4096];
        match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
        }
    }

    if path.starts_with("/hub/api/negotiate") {
        let token = format!("{:032x}", rand_u128());
        let body = json!({
            "connectionId": "fake",
            "connectionToken": token,
            "negotiateVersion": 1,
            "availableTransports": [{"transport": "WebSockets", "transferFormats": ["Text", "Binary"]}],
        });
        write_json_response(&mut stream, 200, &body);
    } else if path.starts_with("/api/user/refresh_token") && method == "POST" {
        let request_value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if let Some(handler) = &config.refresh_handler {
            let (status, response) = handler(&request_value);
            write_json_response(&mut stream, status, &response);
        } else {
            write_json_response(&mut stream, 200, &json!({"Success": true, "Response": "new-token"}));
        }
    } else {
        write_json_response(&mut stream, 404, &json!({"Success": false, "Msg": "not found"}));
    }
}

fn handle_ws(stream: TcpStream, prefix: Vec<u8>, header_end: usize, config: Arc<FakeServerConfig>, ws_connections: Arc<AtomicUsize>) {
    // `tungstenite::accept` wants to read the request itself; replay what
    // we already buffered (headers up to and including the blank line)
    // through a tiny `Read` adapter, then fall through to the live socket.
    let already_read = prefix[..header_end + 4].to_vec();
    let replay = ReplayStream { prefix: already_read, pos: 0, inner: stream };
    let mut socket = match tungstenite::accept(replay) {
        Ok(s) => s,
        Err(_) => return,
    };
    ws_connections.fetch_add(1, Ordering::SeqCst);
    socket.get_mut().inner.set_read_timeout(Some(Duration::from_secs(10))).ok();

    if config.reject_handshake {
        // Never ACK the SignalR protocol handshake -- the client's connect
        // should time out waiting for `{}`.
        loop {
            match socket.read() {
                Ok(_) => continue,
                Err(_) => return,
            }
        }
    }

    // SignalR JSON protocol handshake: first client record is
    // `{"protocol":"json","version":1}`; ack with an empty record.
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let message = match socket.read() {
            Ok(m) => m,
            Err(_) => return,
        };
        match message {
            Message::Text(t) => buf.extend_from_slice(t.as_bytes()),
            Message::Binary(b) => buf.extend_from_slice(&b),
            Message::Close(_) => return,
            _ => continue,
        }
        if let Some(pos) = buf.iter().position(|&b| b == RECORD_SEPARATOR) {
            buf.drain(..=pos);
            break;
        }
    }
    if send_record(&mut socket, &json!({})).is_err() {
        return;
    }

    loop {
        let message = match socket.read() {
            Ok(m) => m,
            Err(_) => return,
        };
        match message {
            Message::Text(t) => buf.extend_from_slice(t.as_bytes()),
            Message::Binary(b) => buf.extend_from_slice(&b),
            Message::Close(_) => return,
            _ => continue,
        }
        while let Some(pos) = buf.iter().position(|&b| b == RECORD_SEPARATOR) {
            let raw: Vec<u8> = buf.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&raw[..raw.len() - 1]).trim().to_string();
            if text.is_empty() {
                continue;
            }
            let invocation: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if invocation.get("type").and_then(Value::as_i64) == Some(6) {
                // Client keepalive ping: SignalR pings don't get a
                // response in the real protocol (they're fire-and-forget
                // in both directions); nothing to do.
                continue;
            }
            let invocation_id = invocation.get("invocationId").and_then(Value::as_str).unwrap_or("").to_string();
            let method = invocation.get("target").and_then(Value::as_str).unwrap_or("").to_string();
            let action = (config.invoke_handler)(&method, &invocation);
            let response = match action {
                Action::Success(data) => {
                    json!({"type": 3, "invocationId": invocation_id, "result": {"success": true, "response": data}})
                }
                Action::Failure { message, status } => {
                    json!({"type": 3, "invocationId": invocation_id, "result": {"success": false, "msg": message, "status": status}})
                }
                Action::RawError(message) => {
                    json!({"type": 3, "invocationId": invocation_id, "error": message})
                }
                Action::NoReply => continue,
                Action::DropConnection => return,
            };
            if send_record(&mut socket, &response).is_err() {
                return;
            }
        }
    }
}

fn send_record(socket: &mut tungstenite::WebSocket<ReplayStream>, value: &Value) -> Result<(), tungstenite::Error> {
    let text = format!("{}{}", serde_json::to_string(value).unwrap(), RECORD_SEPARATOR as char);
    socket.send(Message::Text(text.into()))
}

/// `tungstenite::accept` needs an owned `Read + Write` stream, but we've
/// already consumed the request line/headers into `buf` while sniffing for
/// `Upgrade: websocket`. This adapter replays those bytes first, then reads
/// through to the live socket, so `accept`'s own header parser sees the
/// exact same bytes it would have seen reading the socket directly.
struct ReplayStream {
    prefix: Vec<u8>,
    pos: usize,
    inner: TcpStream,
}

impl Read for ReplayStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos < self.prefix.len() {
            let n = (self.prefix.len() - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.prefix[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        self.inner.read(buf)
    }
}

impl Write for ReplayStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn read_until(stream: &mut TcpStream, buf: &mut Vec<u8>, needle: &[u8]) -> Option<usize> {
    let mut tmp = [0u8; 4096];
    loop {
        if let Some(pos) = buf.windows(needle.len()).position(|w| w == needle) {
            return Some(pos);
        }
        match stream.read(&mut tmp) {
            Ok(0) => return None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => return None,
        }
        if buf.len() > 1 << 20 {
            return None;
        }
    }
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: &Value) {
    let text = serde_json::to_vec(body).unwrap();
    let status_text = match status {
        200 => "200 OK",
        400 => "400 Bad Request",
        401 => "401 Unauthorized",
        404 => "404 Not Found",
        429 => "429 Too Many Requests",
        500 => "500 Internal Server Error",
        other => return write_raw_status(stream, other),
    };
    let header = format!(
        "HTTP/1.1 {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&text);
    let _ = stream.flush();
}

fn write_raw_status(stream: &mut TcpStream, status: u16) {
    let header = format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(header.as_bytes());
}

fn rand_u128() -> u128 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    nanos ^ ((std::process::id() as u128) << 64)
}

