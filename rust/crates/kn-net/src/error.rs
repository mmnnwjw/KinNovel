use std::fmt;

/// Mirrors `bin/src/kinnovel/transport.py`'s `TransportError`/`ApiError`
/// split: network/protocol failures vs. an explicit server-side rejection
/// that carries a status code and should not be retried.
#[derive(Debug, Clone)]
pub enum NetError {
    /// Socket / DNS / TLS / HTTP-framing failure.
    Network(String),
    /// A call did not get a matching response before its deadline.
    Timeout,
    /// The server answered with `success: false` (or an HTTP error status).
    /// Mirrors Python's `ApiError(message, status)`.
    Api { status: i32, message: String },
    /// Login / refresh-token specific failure that is not a generic `Api`
    /// (kept distinct so callers can tell "no credentials" apart from a
    /// transient network error without inspecting a status code).
    Auth(String),
    /// Malformed SignalR/WebSocket framing, unexpected handshake response,
    /// oversized record, etc.
    Protocol(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Network(msg) => write!(f, "网络错误: {msg}"),
            NetError::Timeout => write!(f, "请求超时"),
            NetError::Api { status, message } => write!(f, "{message} (status {status})"),
            NetError::Auth(msg) => write!(f, "认证错误: {msg}"),
            NetError::Protocol(msg) => write!(f, "协议错误: {msg}"),
        }
    }
}

impl std::error::Error for NetError {}

impl NetError {
    pub fn network<S: Into<String>>(msg: S) -> Self {
        NetError::Network(msg.into())
    }

    pub fn protocol<S: Into<String>>(msg: S) -> Self {
        NetError::Protocol(msg.into())
    }

    pub fn api<S: Into<String>>(message: S, status: i32) -> Self {
        NetError::Api { status, message: message.into() }
    }

    /// True for the handful of status codes Python treats as "refresh token
    /// is dead, drop credentials and fall back to anonymous" (api.py
    /// `get_access_token` / `refresh_access_token`: -100 or 404).
    pub fn is_refresh_invalid(&self) -> bool {
        matches!(self, NetError::Api { status, .. } if *status == -100 || *status == 404)
    }

    /// True for the "access token expired" status api.py's `_invoke_direct`
    /// reacts to by refreshing and retrying once.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, NetError::Api { status, .. } if *status == 401)
    }
}

impl From<std::io::Error> for NetError {
    fn from(err: std::io::Error) -> Self {
        NetError::Network(err.to_string())
    }
}
