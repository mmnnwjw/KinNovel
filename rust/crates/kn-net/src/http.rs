use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;

use crate::error::NetError;
use crate::stream::NetStream;

/// Parsed `scheme://host[:port]` pieces of a server URL (`config.py`'s
/// `api_server`, e.g. `https://api.lightnovel.life`).
pub struct ServerUrl {
    pub secure: bool,
    pub host: String,
    pub port: u16,
}

impl ServerUrl {
    pub fn parse(server: &str) -> Result<Self, NetError> {
        let server = server.trim_end_matches('/');
        let (secure, rest) = if let Some(rest) = server.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = server.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(NetError::protocol(format!("不支持的服务器地址: {server}")));
        };
        let (host_port, _path) = rest.split_once('/').unwrap_or((rest, ""));
        let (host, port) = match host_port.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().unwrap_or(if secure { 443 } else { 80 })),
            None => (host_port.to_string(), if secure { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err(NetError::protocol("服务器地址缺少主机名"));
        }
        Ok(ServerUrl { secure, host, port })
    }

    pub fn ws_scheme(&self) -> &'static str {
        if self.secure { "wss" } else { "ws" }
    }

    /// `Host` 头: 非默认端口时带上端口 (RFC 7230 §5.4)。
    pub fn host_header(&self) -> String {
        if self.port == if self.secure { 443 } else { 80 } {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Minimal blocking HTTP/1.1 client: one request per connection
/// (`Connection: close`), `Content-Length` or chunked response bodies (the
/// server switched login/refresh to chunked in 2026-10), used for the
/// handful of plain-REST calls api.py makes outside the SignalR hub.
pub fn request(
    server: &str,
    path: &str,
    method: &str,
    headers: &[(&str, String)],
    body: &[u8],
    timeout: Duration,
    tls: Arc<ClientConfig>,
) -> Result<HttpResponse, NetError> {
    let url = ServerUrl::parse(server)?;
    let mut stream = NetStream::connect(&url.host, url.port, url.secure, timeout, tls)?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();

    let mut request_text = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", url.host_header());
    for (k, v) in headers {
        request_text.push_str(&format!("{k}: {v}\r\n"));
    }
    request_text.push_str(&format!("Content-Length: {}\r\n", body.len()));
    request_text.push_str("Connection: close\r\n\r\n");
    stream.write_all(request_text.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;

    let (status, headers, body) = read_response(&mut stream, 8 * 1024 * 1024, &mut |_, _| true)?;
    Ok(HttpResponse { status, body, headers })
}

/// 下载任意 http(s) URL 的完整内容 (字体、封面、插图)。
/// 跟随最多 3 次重定向; 支持 Content-Length 与 chunked; 超过 `limit` 字节立即失败; 非 200 报错。
pub fn download(url: &str, limit: usize, timeout: Duration, tls: Arc<ClientConfig>) -> Result<Vec<u8>, NetError> {
    download_with(url, limit, timeout, tls, &mut |_, _| true)
}

/// 同 [`download`], 每收到数据调用 `progress(已收到的正文字节, 总长 [知道时])`; 它返回 false 时放弃 (例如太慢)。
pub fn download_with(url: &str, limit: usize, timeout: Duration, tls: Arc<ClientConfig>, progress: &mut dyn FnMut(usize, Option<usize>) -> bool) -> Result<Vec<u8>, NetError> {
    let mut url = url.to_string();
    for _ in 0..4 {
        let (server, path) = split_url(&url)?;
        let parsed = ServerUrl::parse(&server)?;
        let mut stream = NetStream::connect(&parsed.host, parsed.port, parsed.secure, timeout, tls.clone())?;
        stream.set_read_timeout(Some(timeout)).ok();
        stream.set_write_timeout(Some(timeout)).ok();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: KinNovel/1.0\r\nAccept: */*\r\nConnection: close\r\n\r\n",
            parsed.host_header()
        );
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        let (status, headers, body) = read_response(&mut stream, limit, progress)?;
        let header = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        match status {
            200 => return Ok(body),
            301 | 302 | 303 | 307 | 308 => {
                let location = header("location").ok_or_else(|| NetError::protocol("重定向缺少 Location"))?;
                url = resolve_location(&server, &path, location.trim());
            }
            _ => return Err(NetError::api(format!("下载失败 (HTTP {status})"), status as i32)),
        }
    }
    Err(NetError::protocol("重定向次数过多"))
}

/// 按当前地址 (`server` = "scheme://host[:port]", `path` = "/a/b?q") 解析重定向的
/// `Location` (RFC 3986 §5.2): 绝对地址、`//host/..`、`/..`、相对路径 (含 `./` `../`)、`?q`。
fn resolve_location(server: &str, path: &str, location: &str) -> String {
    let lower = location.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return location.to_string();
    }
    if let Some(rest) = location.strip_prefix("//") {
        let scheme = server.split("://").next().unwrap_or("https");
        return format!("{scheme}://{rest}");
    }
    let location = location.split('#').next().unwrap_or("");
    let base_path = path.split(['?', '#']).next().unwrap_or("/");
    if location.is_empty() {
        return format!("{server}{path}");
    }
    if location.starts_with('?') {
        return format!("{server}{base_path}{location}");
    }
    let (loc_path, query) = match location.find('?') {
        Some(i) => location.split_at(i),
        None => (location, ""),
    };
    let merged = if loc_path.starts_with('/') {
        loc_path.to_string()
    } else {
        let dir = &base_path[..base_path.rfind('/').map(|i| i + 1).unwrap_or(0)];
        format!("{}{loc_path}", if dir.is_empty() { "/" } else { dir })
    };
    format!("{server}{}{query}", remove_dot_segments(&merged))
}

/// RFC 3986 §5.2.4, 输入以 `/` 开头。
fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match *seg {
            "." => {
                if last {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

/// "https://host[:port]/path?q" → ("https://host[:port]", "/path?q")
fn split_url(url: &str) -> Result<(String, String), NetError> {
    let scheme_end = url.find("://").ok_or_else(|| NetError::protocol(format!("不支持的地址: {url}")))? + 3;
    match url[scheme_end..].find('/') {
        Some(i) => Ok((url[..scheme_end + i].to_string(), url[scheme_end + i..].to_string())),
        None => Ok((url.to_string(), "/".to_string())),
    }
}

/// 读响应: (状态码, 头, 正文)。正文按 chunked / Content-Length / 读到关闭 三种方式, 都受 limit 约束。
fn read_response(stream: &mut NetStream, limit: usize, progress: &mut dyn FnMut(usize, Option<usize>) -> bool) -> Result<(u16, Vec<(String, String)>, Vec<u8>), NetError> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(NetError::network("连接在响应头读完前关闭"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return Err(NetError::protocol("HTTP 响应头过大"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next().unwrap_or("").split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let get = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let chunked = get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let content_length: Option<usize> = get("content-length").and_then(|v| v.parse().ok());
    if content_length.is_some_and(|n| n > limit) {
        return Err(NetError::protocol("下载内容超过上限"));
    }
    let mut raw = buf.split_off(header_end + 4);
    let too_big = || NetError::protocol("下载内容超过上限");
    // 读一块正文并报告进度 (已收到的原始字节数, chunked 时含块头, 只用于显示和测速)
    let mut received = raw.len();
    let mut read_body = |stream: &mut NetStream, tmp: &mut [u8]| -> Result<usize, NetError> {
        let n = stream.read(tmp)?;
        received += n;
        if !progress(received, content_length) {
            return Err(NetError::network("下载已取消"));
        }
        Ok(n)
    };
    if chunked {
        // 逐块解析; 原始数据不够时继续读
        let mut body = Vec::new();
        let mut pos = 0usize;
        loop {
            let line_end = loop {
                if let Some(i) = find_subslice(&raw[pos..], b"\r\n") {
                    break pos + i;
                }
                let n = read_body(stream, &mut tmp)?;
                if n == 0 {
                    return Err(NetError::network("chunked 响应提前结束"));
                }
                raw.extend_from_slice(&tmp[..n]);
            };
            let size_text = String::from_utf8_lossy(&raw[pos..line_end]).to_string();
            let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| NetError::protocol("chunked 长度无效"))?;
            pos = line_end + 2;
            if size == 0 {
                break;
            }
            if body.len() + size > limit {
                return Err(too_big());
            }
            while raw.len() < pos + size + 2 {
                let n = read_body(stream, &mut tmp)?;
                if n == 0 {
                    return Err(NetError::network("chunked 响应提前结束"));
                }
                raw.extend_from_slice(&tmp[..n]);
            }
            body.extend_from_slice(&raw[pos..pos + size]);
            // 用过的原始数据马上丢掉, 峰值内存不再是正文的两倍
            raw.drain(..pos + size + 2);
            pos = 0;
        }
        return Ok((status, headers, body));
    }
    loop {
        if let Some(total) = content_length {
            if raw.len() >= total {
                raw.truncate(total);
                break;
            }
        }
        let n = read_body(stream, &mut tmp)?;
        if n == 0 {
            if content_length.is_some_and(|total| raw.len() < total) {
                return Err(NetError::network("响应正文不完整"));
            }
            break;
        }
        raw.extend_from_slice(&tmp[..n]);
        if raw.len() > limit {
            return Err(too_big());
        }
    }
    Ok((status, headers, raw))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_default_port() {
        let url = ServerUrl::parse("https://api.lightnovel.life").unwrap();
        assert!(url.secure);
        assert_eq!(url.host, "api.lightnovel.life");
        assert_eq!(url.port, 443);
    }

    #[test]
    fn parse_http_with_explicit_port() {
        let url = ServerUrl::parse("http://127.0.0.1:8080/").unwrap();
        assert!(!url.secure);
        assert_eq!(url.host, "127.0.0.1");
        assert_eq!(url.port, 8080);
    }

    /// 本地一次性 HTTP 服务器: 读完请求后原样写出 `response`, 返回 URL。
    fn serve_once(response: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            // Read the whole request head: closing with unread bytes makes Windows send RST.
            let (mut req, mut buf) = (Vec::new(), [0u8; 4096]);
            while find_subslice(&req, b"\r\n\r\n").is_none() {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            s.write_all(&response).unwrap();
        });
        format!("http://127.0.0.1:{port}/img/a.jpg?size=10x10&height=256")
    }

    /// 依次接受 `responses.len()` 个连接, 第 i 个回 `responses[i]` (其中的 `{port}` 换成端口);
    /// 收到的请求头经 channel 送回。
    fn serve_seq(responses: Vec<String>) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for response in responses {
                let (mut s, _) = listener.accept().unwrap();
                let (mut req, mut buf) = (Vec::new(), [0u8; 4096]);
                while find_subslice(&req, b"\r\n\r\n").is_none() {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&req).to_string());
                s.write_all(response.replace("{port}", &port.to_string()).as_bytes()).unwrap();
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }

    #[test]
    fn host_header_carries_non_default_port() {
        let (server, rx) = serve_seq(vec!["HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".into()]);
        assert_eq!(download(&format!("{server}/x"), 100, Duration::from_secs(5), tls()).unwrap(), b"ok");
        let port = server.rsplit(':').next().unwrap();
        assert!(rx.recv().unwrap().contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")));
        assert_eq!(ServerUrl::parse("https://api.lightnovel.life").unwrap().host_header(), "api.lightnovel.life");
        assert_eq!(ServerUrl::parse("http://h:80").unwrap().host_header(), "h");
        assert_eq!(ServerUrl::parse("https://h:8443").unwrap().host_header(), "h:8443");
    }

    #[test]
    fn redirects_resolve_against_the_current_url() {
        let redirect = |loc: &str| format!("HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\n\r\n");
        let (server, rx) = serve_seq(vec![
            redirect("c.jpg?v=2"),
            redirect("../up/d.jpg"),
            redirect("/root/e.jpg"),
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone".into(),
        ]);
        let body = download(&format!("{server}/img/a/b.jpg?x=1"), 100, Duration::from_secs(5), tls()).unwrap();
        assert_eq!(body, b"done");
        let lines: Vec<String> = rx.iter().take(4).map(|r| r.lines().next().unwrap().to_string()).collect();
        assert_eq!(lines, [
            "GET /img/a/b.jpg?x=1 HTTP/1.1",
            "GET /img/a/c.jpg?v=2 HTTP/1.1",
            "GET /img/up/d.jpg HTTP/1.1",
            "GET /root/e.jpg HTTP/1.1",
        ]);
    }

    #[test]
    fn resolve_location_forms() {
        let s = "https://img.example:8443";
        assert_eq!(resolve_location(s, "/a/b?q", "http://other/x"), "http://other/x");
        assert_eq!(resolve_location(s, "/a/b?q", "HTTPS://Other/x"), "HTTPS://Other/x");
        assert_eq!(resolve_location(s, "/a/b?q", "//cdn.example/y.jpg"), "https://cdn.example/y.jpg");
        assert_eq!(resolve_location("http://h", "/a/b", "//cdn/y"), "http://cdn/y");
        assert_eq!(resolve_location(s, "/a/b?q", "/z"), "https://img.example:8443/z");
        assert_eq!(resolve_location(s, "/a/b?q", "c"), "https://img.example:8443/a/c");
        assert_eq!(resolve_location(s, "/a/b?q", "./c/"), "https://img.example:8443/a/c/");
        assert_eq!(resolve_location(s, "/a/b?q", "../../../c"), "https://img.example:8443/c");
        assert_eq!(resolve_location(s, "/a/b?q", ".."), "https://img.example:8443/");
        assert_eq!(resolve_location(s, "/a/b?q", "?r=1"), "https://img.example:8443/a/b?r=1");
        assert_eq!(resolve_location(s, "/a/b?q", "c#frag"), "https://img.example:8443/a/c");
        assert_eq!(resolve_location(s, "/", "c?x=/y"), "https://img.example:8443/c?x=/y");
    }

    /// 很多小块 + 块头在读缓冲边界上: 解码结果不变 (每块之后丢掉原始数据)。
    #[test]
    fn download_many_chunks() {
        let mut expected = Vec::new();
        let mut raw = String::from("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
        for i in 0..3000 {
            let chunk: String = std::iter::repeat((b'a' + (i % 26) as u8) as char).take(1 + i % 37).collect();
            raw.push_str(&format!("{:x}\r\n{chunk}\r\n", chunk.len()));
            expected.extend_from_slice(chunk.as_bytes());
        }
        raw.push_str("0\r\n\r\n");
        let (server, _rx) = serve_seq(vec![raw]);
        let body = download(&format!("{server}/big"), 1 << 20, Duration::from_secs(5), tls()).unwrap();
        assert_eq!(body, expected);
    }

    fn tls() -> Arc<ClientConfig> {
        crate::stream::tls_config(true)
    }

    #[test]
    fn download_content_length() {
        let url = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".to_vec());
        assert_eq!(download(&url, 100, Duration::from_secs(5), tls()).unwrap(), b"hello");
    }

    #[test]
    fn download_chunked() {
        let body = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n7;ext=1\r\n, world\r\n0\r\n\r\n";
        let url = serve_once(body.to_vec());
        assert_eq!(download(&url, 100, Duration::from_secs(5), tls()).unwrap(), b"hello, world");
    }

    #[test]
    fn download_enforces_limit_and_status() {
        let url = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\n".to_vec());
        assert!(download(&url, 10, Duration::from_secs(5), tls()).is_err());
        let url = serve_once(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec());
        assert!(matches!(download(&url, 10, Duration::from_secs(5), tls()), Err(NetError::Api { .. })));
    }

    /// The server answers login / refresh_token chunked since 2026-10.
    #[test]
    fn request_decodes_chunked_json() {
        let json = br#"{"Response":"t","Status":200,"Success":true,"Msg":""}"#;
        let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", json.len()).into_bytes();
        raw.extend_from_slice(json);
        raw.extend_from_slice(b"\r\n0\r\n\r\n");
        let (server, _) = split_url(&serve_once(raw)).unwrap();
        let resp = request(&server, "/api/user/refresh_token", "GET", &[], b"", Duration::from_secs(5), tls()).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, json);
    }

    #[test]
    fn split_url_keeps_query() {
        assert_eq!(
            split_url("https://img.lightnovel.life/a/b.jpg?x=1&y=2").unwrap(),
            ("https://img.lightnovel.life".to_string(), "/a/b.jpg?x=1&y=2".to_string())
        );
        assert_eq!(split_url("http://h:8080").unwrap(), ("http://h:8080".to_string(), "/".to_string()));
    }
}
