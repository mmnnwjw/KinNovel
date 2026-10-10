//! `kn-net`: the KinNovel Rust rewrite's network layer, a from-scratch port
//! of `bin/src/kinnovel/transport.py` + `bin/src/kinnovel/api.py`.
//!
//! Layering (dependency direction top to bottom):
//!
//! - [`client`] -- [`Client`]: session/tokens (`cache/session.json`,
//!   same format as the Python build), response cache + in-flight
//!   de-duplication, and the typed-ish public methods (`get_book_list`,
//!   `get_novel_content`, ...) the UI pages call.
//! - [`transport`] -- [`transport::SignalRClient`]: SignalR-over-WebSocket
//!   (negotiate, JSON protocol handshake, invoke, keepalive/idle-close,
//!   reconnect, priority turns).
//! - [`session`] -- [`session::SessionStore`]: the on-disk session file.
//! - [`rate_limit`], [`turn`]: the sliding-window limiter and the
//!   interactive-vs-prefetch turn scheduler the transport uses.
//! - [`stream`], [`http`], [`gzip`]: TLS/plain socket selection, a minimal
//!   blocking HTTP/1.1 client, and the size-capped gzip decoder.
//! - [`helpers`]: the list/envelope-normalizing free functions `api.py` has
//!   at module scope.
//!
//! Everything here is blocking and plain `std::thread` based (no async
//! runtime): callers are `kn-ui`'s worker-pool threads, never the
//! render/UI thread. `Client` is `Send + Sync` and cheap to `clone()`.

pub mod client;
pub mod error;
pub mod gzip;
pub mod helpers;
pub mod http;
pub mod rate_limit;
pub mod session;
pub mod stream;
pub mod transport;
pub mod turn;

pub use client::{Client, ClientConfig};
pub use error::NetError;
pub use session::SessionStore;
pub use transport::{SignalRClient, SignalRConfig};
