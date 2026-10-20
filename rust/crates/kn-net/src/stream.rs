use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::verify_tls12_signature;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme, StreamOwned};

use crate::error::NetError;

/// Accepts any certificate chain. Used when `strict_tls` is false, mirroring
/// Python's `ssl.CERT_NONE` / `check_hostname = False` (api.py `_ssl_context`,
/// transport.py `SignalRClient._ssl_context`). The Kindle's clock/cert store
/// can be stale, so the app has always allowed opting out of verification.
#[derive(Debug)]
struct NoVerify(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// 每种设置只建一次 `ClientConfig`: rustls 的会话缓存 (默认内存里 256 个主机,
/// TLS 1.3 票据与 TLS 1.2 会话 ID/票据) 挂在它上面, 共用同一个才能复用会话,
/// 省掉完整握手和证书链验证。调用方都只读 (`ClientConnection::new`), 不改设置。
pub fn tls_config(strict: bool) -> Arc<ClientConfig> {
    static STRICT: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    static LOOSE: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    let cell = if strict { &STRICT } else { &LOOSE };
    cell.get_or_init(|| build_tls_config(strict)).clone()
}

fn build_tls_config(strict: bool) -> Arc<ClientConfig> {
    if strict {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    } else {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(NoVerify(provider.clone()));
        Arc::new(
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("default TLS protocol versions")
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth(),
        )
    }
}

/// Either a plain TCP socket (`http://`/`ws://`, used for the fake test
/// server) or a TLS-wrapped one (`https://`/`wss://`, the real server). The
/// URL scheme alone decides which: see `connect()`.
pub enum NetStream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl NetStream {
    pub fn connect(
        host: &str,
        port: u16,
        secure: bool,
        connect_timeout: Duration,
        tls: Arc<ClientConfig>,
    ) -> Result<Self, NetError> {
        let mut last_err = None;
        let mut stream = None;
        for candidate in crate::dns::resolve(host, port).map_err(|e| NetError::network(e.to_string()))? {
            match TcpStream::connect_timeout(&candidate, connect_timeout) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        if stream.is_none() {
            crate::dns::forget(host);
        }
        let tcp = stream.ok_or_else(|| {
            NetError::network(
                last_err.map(|e| e.to_string()).unwrap_or_else(|| "连接失败".into()),
            )
        })?;
        tcp.set_nodelay(true).ok();
        if !secure {
            return Ok(NetStream::Plain(tcp));
        }
        let server_name = ServerName::try_from(host.to_string())
            .map_err(|_| NetError::protocol("无效的服务器主机名"))?;
        let conn = ClientConnection::new(tls, server_name)
            .map_err(|e| NetError::network(format!("TLS 初始化失败: {e}")))?;
        let mut owned = StreamOwned::new(conn, tcp);
        owned.flush().map_err(|e| NetError::network(format!("TLS 握手失败: {e}")))?;
        Ok(NetStream::Tls(Box::new(owned)))
    }

    pub fn try_clone_raw(&self) -> io::Result<TcpStream> {
        match self {
            NetStream::Plain(s) => s.try_clone(),
            NetStream::Tls(s) => s.get_ref().try_clone(),
        }
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            NetStream::Plain(s) => s.set_read_timeout(timeout),
            NetStream::Tls(s) => s.get_ref().set_read_timeout(timeout),
        }
    }

    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            NetStream::Plain(s) => s.set_write_timeout(timeout),
            NetStream::Tls(s) => s.get_ref().set_write_timeout(timeout),
        }
    }
}

impl Read for NetStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            NetStream::Plain(s) => s.read(buf),
            NetStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for NetStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            NetStream::Plain(s) => s.write(buf),
            NetStream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            NetStream::Plain(s) => s.flush(),
            NetStream::Tls(s) => s.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_config_is_shared_per_mode() {
        assert!(Arc::ptr_eq(&tls_config(true), &tls_config(true)));
        assert!(Arc::ptr_eq(&tls_config(false), &tls_config(false)));
        assert!(!Arc::ptr_eq(&tls_config(true), &tls_config(false)));
    }
}
