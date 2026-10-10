// Spike: rustls TLS + SignalR JSON hub over WebSocket, synchronous (no async runtime).
//
// Mirrors bin/src/kinnovel/transport.py:
//   1. POST {server}/hub/api/negotiate?negotiateVersion=1  -> connectionToken
//   2. WS connect to {server}/hub/api?id={connectionToken}, Origin header
//   3. send SignalR JSON handshake {"protocol":"json","version":1}\x1e, expect {}
//   4. send an invocation record, read records (0x1e separated) until the
//      matching invocationId comes back; response may be a base64+gzip string.
//
// Chose a plain blocking/synchronous design: this talks to exactly one host,
// one request at a time (same shape as the Python reference's RateLimit +
// single hub socket), so there is nothing an async runtime buys us here. A
// background keepalive thread (not exercised by this spike) is the only
// concurrency the real client needs, and std::thread covers that.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{json, Value};

const SERVER_HOST: &str = "api.lightnovel.life";
const SERVER_PORT: u16 = 443;
const RECORD_SEP: u8 = 0x1e;
const FIXTURE_DIR: &str = "fixtures";

struct Timings {
    connect_tls: Duration,
    negotiate: Duration,
    ws_handshake: Duration,
    invoke: Duration,
    total: Duration,
}

type TlsStream = StreamOwned<ClientConnection, TcpStream>;

fn tls_config() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// TCP connect + TLS handshake to SERVER_HOST:SERVER_PORT. Returns the stream
/// and the combined DNS+TCP+TLS duration (that's the single number the spike
/// report asks for; splitting it further isn't needed here).
fn connect_tls(config: Arc<ClientConfig>) -> std::io::Result<(TlsStream, Duration)> {
    let t0 = Instant::now();
    let server_name = rustls_pki_types::ServerName::try_from(SERVER_HOST)
        .expect("valid DNS name")
        .to_owned();
    let conn = ClientConnection::new(config, server_name)
        .expect("rustls ClientConnection::new");
    // std::net::TcpStream::connect does the DNS resolution + TCP connect.
    let sock = TcpStream::connect((SERVER_HOST, SERVER_PORT))?;
    sock.set_nodelay(true).ok();
    let mut stream = StreamOwned::new(conn, sock);
    // Drive the TLS handshake now (instead of lazily on first app write) so
    // the timing below is just the handshake, not mixed into the negotiate
    // request's first write.
    stream.flush()?; // flush on an empty writer still completes a pending handshake
    let elapsed = t0.elapsed();
    Ok((stream, elapsed))
}

/// Minimal blocking HTTP/1.1 client: one request, Content-Length response body
/// only (negotiate's JSON body is small and ASP.NET Core sends Content-Length,
/// not chunked, for it) with a fallback to read-to-EOF if there's no
/// Content-Length header.
fn http_request(
    stream: &mut TlsStream,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> std::io::Result<(u16, Vec<u8>)> {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {SERVER_HOST}\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    req.push_str("Connection: close\r\n\r\n");
    stream.write_all(req.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;

    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            header_end = pos;
            return finish_http_response(stream, buf, header_end);
        }
    }
    Err(std::io::Error::other("connection closed before headers complete"))
}

fn finish_http_response(
    stream: &mut TlsStream,
    mut buf: Vec<u8>,
    header_end: usize,
) -> std::io::Result<(u16, Vec<u8>)> {
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut content_length: Option<usize> = None;
    for line in lines {
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:").map(|s| s.trim().to_string()) {
            content_length = v.parse().ok();
        }
    }
    let body_start = header_end + 4;
    let mut body: Vec<u8> = buf.split_off(body_start);
    if let Some(total) = content_length {
        let mut tmp = [0u8; 4096];
        while body.len() < total {
            let n = stream.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(total);
    } else {
        // No Content-Length (shouldn't happen for negotiate, but be safe):
        // read to EOF since we sent Connection: close.
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream.read(&mut tmp)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
    }
    Ok((status, body))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn gunzip(raw: &[u8]) -> std::io::Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    let mut decoder = GzDecoder::new(raw);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Decode a SignalR `response` value the same way transport.py's
/// `_decode_response` does: if it's a base64 string, base64-decode, gunzip,
/// then JSON-parse; otherwise return it unchanged.
fn decode_response(value: Value) -> Value {
    let Value::String(s) = &value else {
        return value;
    };
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(s) else {
        return value;
    };
    let Ok(unzipped) = gunzip(&raw) else {
        return value;
    };
    serde_json::from_slice(&unzipped).unwrap_or(value)
}

fn read_record(ws: &mut tungstenite::WebSocket<TlsStream>, buf: &mut Vec<u8>) -> Vec<Value> {
    use tungstenite::Message;
    loop {
        // Drain any complete records already buffered before blocking on recv again.
        if let Some(pos) = buf.iter().position(|b| *b == RECORD_SEP) {
            let raw = buf.drain(..=pos).collect::<Vec<_>>();
            let text = String::from_utf8_lossy(&raw[..raw.len() - 1]).to_string();
            if text.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            return vec![v];
        }
        match ws.read().expect("ws read") {
            Message::Text(t) => buf.extend_from_slice(t.as_bytes()),
            Message::Binary(b) => buf.extend_from_slice(&b),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            Message::Close(_) => panic!("server closed the websocket"),
        }
    }
}

fn send_record(ws: &mut tungstenite::WebSocket<TlsStream>, json: &Value) {
    let mut text = serde_json::to_string(json).unwrap();
    text.push(RECORD_SEP as char);
    ws.send(tungstenite::Message::Text(text.into())).expect("ws send");
}

fn main() {
    let live = std::env::args().any(|a| a == "--live");
    std::fs::create_dir_all(FIXTURE_DIR).ok();

    if live {
        run_live();
    } else {
        run_replay();
    }
}

fn run_live() {
    let visitor_id = uuid_like_hex();
    let config = tls_config();

    // --- 1. negotiate ---
    let (mut stream, connect_dur) = connect_tls(config.clone()).expect("connect+tls");
    let t_negotiate = Instant::now();
    let (status, body) = http_request(
        &mut stream,
        "POST",
        "/hub/api/negotiate?negotiateVersion=1",
        &[
            ("x-id", visitor_id.clone()),
            ("Accept", "application/json".into()),
            ("User-Agent", "spike-net/0.1".into()),
        ],
        b"",
    )
    .expect("negotiate request");
    let negotiate_dur = t_negotiate.elapsed();
    assert_eq!(status, 200, "negotiate HTTP status: {status}");
    std::fs::write(format!("{FIXTURE_DIR}/negotiate.json"), &body).ok();
    let negotiation: Value = serde_json::from_slice(&body).expect("negotiate json");
    let connection_token = negotiation["connectionToken"]
        .as_str()
        .expect("connectionToken in negotiate response")
        .to_string();
    println!("negotiate: {negotiation}");

    // Network etiquette (SPIKE-GUIDE.md): space requests >= 6s apart.
    std::thread::sleep(Duration::from_secs(7));

    // --- 2. WS connect + SignalR JSON handshake ---
    // New TLS connection for the websocket (negotiate's was "Connection: close").
    let (ws_stream, _ws_connect_dur) = connect_tls(config).expect("connect+tls for ws");
    let t_ws = Instant::now();
    let path = format!(
        "/hub/api?id={}",
        urlencode(&connection_token)
    );
    let request = tungstenite::http::Request::builder()
        .method("GET")
        .uri(format!("wss://{SERVER_HOST}{path}"))
        .header("Host", SERVER_HOST)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .header("Origin", "https://www.lightnovel.app")
        .header("User-Agent", "spike-net/0.1")
        .body(())
        .unwrap();
    let (mut ws, _resp) = tungstenite::client(request, ws_stream).expect("ws handshake");

    send_record(&mut ws, &json!({"protocol": "json", "version": 1}));
    let mut buf = Vec::new();
    loop {
        let msgs = read_record(&mut ws, &mut buf);
        let msg = &msgs[0];
        if msg.get("error").is_some() {
            panic!("signalr handshake error: {msg}");
        }
        if msg.as_object().map(|o| o.is_empty()).unwrap_or(false) {
            break; // {} == handshake ack
        }
    }
    let ws_handshake_dur = t_ws.elapsed();
    println!("signalr handshake ok");

    // Network etiquette (SPIKE-GUIDE.md): space requests >= 6s apart.
    std::thread::sleep(Duration::from_secs(7));

    // --- 3. one public invocation: latest book list, page 1, small page size ---
    let invocation_id = uuid_like_hex();
    // GetLatestBookList is the hub method that's usable anonymously (per
    // docs/technical-analysis.md); GetBookList requires auth on the live
    // server ("user is unauthorized" -- confirmed against the real server).
    let invocation = json!({
        "type": 1,
        "invocationId": invocation_id,
        "target": "GetLatestBookList",
        "arguments": [
            {"Page": 1, "Size": 5, "IgnoreJapanese": false, "IgnoreAI": false},
            {"UseGzip": true}
        ]
    });
    let t_invoke = Instant::now();
    send_record(&mut ws, &invocation);
    let result = loop {
        let msgs = read_record(&mut ws, &mut buf);
        let msg = &msgs[0];
        if msg.get("invocationId").and_then(|v| v.as_str()) != Some(invocation_id.as_str()) {
            continue;
        }
        break msg.clone();
    };
    let invoke_dur = t_invoke.elapsed();
    std::fs::write(
        format!("{FIXTURE_DIR}/invoke_getbooklist.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .ok();

    // "total" is the sum of the active work, excluding the >=6s etiquette
    // sleeps between requests (those are a spike-harness artifact, not
    // something the real client would do back-to-back).
    let total_dur = connect_dur + negotiate_dur + ws_handshake_dur + invoke_dur;
    report_result(&result);
    print_timings(&Timings {
        connect_tls: connect_dur,
        negotiate: negotiate_dur,
        ws_handshake: ws_handshake_dur,
        invoke: invoke_dur,
        total: total_dur,
    });
}

fn run_replay() {
    println!("(replay mode -- no network requests; parsing recorded fixtures)");
    let path = format!("{FIXTURE_DIR}/invoke_getbooklist.json");
    let body = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("no fixture at {path} yet -- run with --live first");
            return;
        }
    };
    let result: Value = serde_json::from_slice(&body).expect("fixture json");
    report_result(&result);
}

fn report_result(message: &Value) {
    let envelope = &message["result"];
    let response = if envelope.is_object() {
        decode_response(envelope["response"].clone())
    } else {
        decode_response(envelope.clone())
    };
    let data = response.get("Data").cloned().unwrap_or(response.clone());
    let items = data.as_array().cloned().unwrap_or_default();
    let first_title = items
        .first()
        .and_then(|b| b.get("Title").or_else(|| b.get("title")))
        .and_then(|v| v.as_str())
        .unwrap_or("<none>");
    println!("GetBookList: {} item(s), first title: {first_title}", items.len());
}

fn print_timings(t: &Timings) {
    println!("--- timings ---");
    println!("connect (dns+tcp+tls): {:?}", t.connect_tls);
    println!("negotiate:             {:?}", t.negotiate);
    println!("ws handshake:          {:?}", t.ws_handshake);
    println!("invocation round trip: {:?}", t.invoke);
    println!("total wall time:       {:?}", t.total);
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmHWM:") {
                println!("{line}");
            }
        }
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn uuid_like_hex() -> String {
    // Not a real UUIDv4 (no external rng crate for a spike) -- just 32 hex
    // chars derived from the system time, good enough as a visitor/invocation id.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id() as u128;
    format!("{:032x}", nanos ^ (pid << 64))
}
