use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
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

pub fn tls_config(strict: bool) -> Arc<ClientConfig> {
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
        let addr = format!("{host}:{port}");
        let mut last_err = None;
        let mut stream = None;
        for candidate in std::net::ToSocketAddrs::to_socket_addrs(&addr)
            .map_err(|e| NetError::network(e.to_string()))?
        {
            match TcpStream::connect_timeout(&candidate, connect_timeout) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
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
