import base64
import contextlib
import datetime
import hashlib
import ipaddress
import json
import shutil
import ssl
import struct
import subprocess
import sys
import tempfile
import threading
import unittest
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from kinnovel.api import ApiClient
from kinnovel.transport import ApiError, RateLimit, SignalRClient, TransportError


RECORD_SEPARATOR = "\x1e"


class _Config:
    def __init__(self, **values):
        self._values = values

    def get(self, key, default=None):
        return self._values.get(key, default)


class _QuietThreadingHTTPServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.server_errors = []

    def handle_error(self, _request, client_address):
        self.server_errors.append((client_address, repr(sys.exc_info()[1])))


class _ApiTestServer(_QuietThreadingHTTPServer):
    def __init__(self, address, handler):
        super().__init__(address, handler)
        self.requests = []
        self.retry_attempts = 0


class _TlsApiTestServer(_ApiTestServer):
    def __init__(self, address, handler, certfile, keyfile):
        super().__init__(address, handler)
        self.ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.ssl_context.load_cert_chain(str(certfile), str(keyfile))

    def get_request(self):
        raw_socket, address = self.socket.accept()
        try:
            return (
                self.ssl_context.wrap_socket(raw_socket, server_side=True),
                address,
            )
        except Exception:
            raw_socket.close()
            raise


class _ApiRequestHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, _format, *args):
        return

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        self.server.requests.append({
            "method": self.command,
            "path": self.path,
            "headers": {
                key.lower(): value for key, value in self.headers.items()
            },
            "body": body,
        })

        if self.path == "/error":
            self._write_json(400, {
                "Success": False,
                "Msg": "bad request",
                "Status": 422,
            })
            return

        if self.path == "/app-error":
            self._write_json(200, {
                "Success": False,
                "Msg": "application error",
                "Status": 409,
            })
            return

        if self.path == "/retry":
            self.server.retry_attempts += 1
            if self.server.retry_attempts == 1:
                self._write_json(
                    429,
                    {"Success": False, "Msg": "slow down", "Status": 429},
                    {"Retry-After": "1.5"},
                )
                return
            self._write_json(200, {
                "Success": True,
                "Response": {"attempt": self.server.retry_attempts},
            })
            return

        self._write_json(200, {
            "Success": True,
            "Response": {"ok": True},
        })

    def _write_json(self, status, payload, extra_headers=None):
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        for key, value in (extra_headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()
        self.close_connection = True


def _xor_mask(payload, mask):
    cycle = (mask * (len(payload) // 4 + 1))[:len(payload)]
    merged = int.from_bytes(payload, "big") ^ int.from_bytes(cycle, "big")
    return merged.to_bytes(len(payload), "big")


def _read_exact(stream, count):
    chunks = []
    remaining = count
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise EOFError("socket closed")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def _read_websocket_frame(stream):
    first, second = _read_exact(stream, 2)
    opcode = first & 0x0F
    masked = bool(second & 0x80)
    length = second & 0x7F
    if length == 126:
        length = struct.unpack("!H", _read_exact(stream, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", _read_exact(stream, 8))[0]
    mask = _read_exact(stream, 4) if masked else None
    payload = _read_exact(stream, length) if length else b""
    if mask:
        payload = _xor_mask(payload, mask)
    return opcode, payload


def _write_websocket_frame(stream, opcode, payload=b""):
    payload = bytes(payload)
    header = bytearray([0x80 | opcode])
    if len(payload) < 126:
        header.append(len(payload))
    elif len(payload) <= 0xFFFF:
        header.append(126)
        header.extend(struct.pack("!H", len(payload)))
    else:
        header.append(127)
        header.extend(struct.pack("!Q", len(payload)))
    stream.write(bytes(header) + payload)
    stream.flush()


class _SignalRTestServer(_QuietThreadingHTTPServer):
    def __init__(self, address, handler):
        super().__init__(address, handler)
        self.observed = {}


class _SignalRRequestHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, _format, *args):
        return

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        parsed = urllib.parse.urlsplit(self.path)
        if parsed.path != "/hub/api/negotiate":
            self._write_json(404, {"error": "not found"})
            return
        self.server.observed["negotiate"] = {
            "path": self.path,
            "authorization": self.headers.get("Authorization"),
            "body": body,
        }
        self._write_json(200, {
            "negotiateVersion": 1,
            "connectionToken": "integration-token",
            "availableTransports": [],
        })

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        if (parsed.path != "/hub/api"
                or self.headers.get("Upgrade", "").lower() != "websocket"):
            self._write_json(404, {"error": "not found"})
            return

        key = self.headers.get("Sec-WebSocket-Key") or ""
        accept = base64.b64encode(hashlib.sha1(
            (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")
        ).digest()).decode("ascii")
        self.server.observed["upgrade"] = {
            "path": self.path,
            "upgrade": self.headers.get("Upgrade"),
            "connection": self.headers.get("Connection"),
            "origin": self.headers.get("Origin"),
        }
        self.send_response_only(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        self.wfile.flush()

        self.connection.settimeout(3.0)
        try:
            opcode, payload = _read_websocket_frame(self.rfile)
            if opcode != 0x1:
                raise AssertionError("client handshake was not a text frame")
            self.server.observed["handshake"] = payload.decode("utf-8")

            # Split the SignalR record separator across WebSocket frames so the
            # client has to buffer and join records from separate messages.
            _write_websocket_frame(self.wfile, 0x1, b"{}")
            _write_websocket_frame(self.wfile, 0x1, RECORD_SEPARATOR.encode("ascii"))
            self.server.observed["handshake_frames"] = 2

            opcode, payload = _read_websocket_frame(self.rfile)
            if opcode != 0x1:
                raise AssertionError("client invocation was not a text frame")
            raw_invocation = payload.decode("utf-8")
            self.server.observed["invocation_raw"] = raw_invocation
            invocation = json.loads(raw_invocation.rstrip(RECORD_SEPARATOR))
            self.server.observed["invocation"] = invocation

            result = {
                "type": 3,
                "invocationId": invocation.get("invocationId"),
                "result": {
                    "Success": True,
                    "Response": {
                        "ok": True,
                        "method": invocation.get("target"),
                        "params": invocation.get("arguments", [{}])[0],
                    },
                },
            }
            records = (
                json.dumps(result, separators=(",", ":"))
                + RECORD_SEPARATOR
                + json.dumps({"type": 6}, separators=(",", ":"))
                + RECORD_SEPARATOR
            )
            _write_websocket_frame(self.wfile, 0x1, records.encode("utf-8"))
            self.server.observed["response_records"] = 2

            # Wait for the client's close frame so shutdown is exercised too.
            try:
                _read_websocket_frame(self.rfile)
            except (EOFError, OSError):
                pass
        except Exception as exc:
            self.server.observed["error"] = repr(exc)
        finally:
            self.close_connection = True

    def _write_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()
        self.close_connection = True


@contextlib.contextmanager
def _running_server(server):
    thread = threading.Thread(
        target=server.serve_forever,
        kwargs={"poll_interval": 0.02},
        daemon=True,
        name="kinnovel-network-test-server",
    )
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()
        thread.join(5)


def _server_url(server, scheme="http"):
    host, port = server.server_address[:2]
    return "%s://%s:%s" % (scheme, host, port)


def _new_api_client(server, strict_tls=False, scheme="http"):
    client = ApiClient.__new__(ApiClient)
    client.server = _server_url(server, scheme=scheme)
    client.config = _Config(strict_tls=strict_tls)
    client.hub = SimpleNamespace(
        rate_limit=RateLimit(maximum=1000, window_seconds=0.01),
        _visitor_id="integration-visitor",
    )
    client._ssl_context_obj = None
    return client


class NetworkIntegrationTests(unittest.TestCase):
    def test_http_post_json_authorization_and_error_json(self):
        server = _ApiTestServer(("127.0.0.1", 0), _ApiRequestHandler)
        with _running_server(server):
            self.assertEqual(server.server_address[0], "127.0.0.1")
            client = _new_api_client(server)

            result = client._http(
                "/ok", {"Title": "integration"}, token="token-1"
            )

            self.assertEqual(result, {"ok": True})
            self.assertEqual(len(server.requests), 1)
            request = server.requests[0]
            self.assertEqual(request["method"], "POST")
            self.assertEqual(request["path"], "/ok")
            self.assertEqual(
                json.loads(request["body"].decode("utf-8")),
                {"Title": "integration"},
            )
            self.assertEqual(
                request["headers"]["authorization"], "Bearer token-1"
            )
            self.assertEqual(
                request["headers"]["content-type"], "application/json"
            )
            self.assertEqual(
                request["headers"]["x-id"], "integration-visitor"
            )

            with self.assertRaises(ApiError) as caught:
                client._http("/error", {"bad": True})
            self.assertEqual(str(caught.exception), "bad request")
            self.assertEqual(caught.exception.status, 422)

            with self.assertRaises(ApiError) as caught:
                client._http("/app-error", {"bad": True})
            self.assertEqual(str(caught.exception), "application error")
            self.assertEqual(caught.exception.status, 409)

    def test_http_429_retries_and_uses_retry_after(self):
        server = _ApiTestServer(("127.0.0.1", 0), _ApiRequestHandler)
        with _running_server(server):
            client = _new_api_client(server)
            with mock.patch("kinnovel.api.time.sleep") as sleep_:
                result = client._http("/retry", {"attempt": 1})

            self.assertEqual(result, {"attempt": 2})
            self.assertEqual(server.retry_attempts, 2)
            self.assertEqual(len(server.requests), 2)
            sleep_.assert_called_once_with(1.5)

    def test_signalr_websocket_handshake_and_invoke_roundtrip(self):
        server = _SignalRTestServer(("127.0.0.1", 0), _SignalRRequestHandler)
        with _running_server(server):
            self.assertEqual(server.server_address[0], "127.0.0.1")
            client = SignalRClient(
                _server_url(server),
                token_provider=lambda: "token-1",
                strict_tls=False,
                request_limit=1000,
                request_window=0.01,
                timeout=3,
                visitor_id="integration-visitor",
            )
            try:
                result = client.invoke(
                    "GetMyInfo",
                    {"Id": 7},
                    use_gzip=False,
                    timeout=3,
                    retry=False,
                )
            finally:
                client.shutdown()

        self.assertEqual(result["ok"], True)
        self.assertEqual(result["method"], "GetMyInfo")
        self.assertEqual(result["params"], {"Id": 7})
        self.assertNotIn("error", server.observed)
        self.assertEqual(
            server.observed["negotiate"]["authorization"], "Bearer token-1"
        )
        self.assertEqual(server.observed["upgrade"]["upgrade"], "websocket")
        self.assertEqual(server.observed["upgrade"]["connection"], "Upgrade")
        self.assertIn("id=integration-token", server.observed["upgrade"]["path"])
        self.assertIn("access_token=token-1", server.observed["upgrade"]["path"])
        self.assertEqual(
            server.observed["handshake"],
            '{"protocol":"json","version":1}' + RECORD_SEPARATOR,
        )
        self.assertEqual(server.observed["handshake_frames"], 2)
        invocation = server.observed["invocation"]
        self.assertEqual(invocation["type"], 1)
        self.assertEqual(invocation["target"], "GetMyInfo")
        self.assertEqual(
            invocation["arguments"], [{"Id": 7}, {"UseGzip": False}]
        )
        self.assertTrue(
            server.observed["invocation_raw"].endswith(RECORD_SEPARATOR)
        )
        self.assertEqual(server.observed["response_records"], 2)


def _write_certificate_with_cryptography(directory):
    try:
        from cryptography import x509
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import rsa
        from cryptography.x509.oid import NameOID
    except ImportError:
        return None

    cert_path = Path(directory) / "localhost-cert.pem"
    key_path = Path(directory) / "localhost-key.pem"
    now = datetime.datetime.now(datetime.timezone.utc)
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "localhost")])
    certificate = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + datetime.timedelta(days=1))
        .add_extension(
            x509.SubjectAlternativeName([
                x509.DNSName("localhost"),
                x509.IPAddress(ipaddress.IPv4Address("127.0.0.1")),
            ]),
            critical=False,
        )
        .sign(key, hashes.SHA256())
    )
    cert_path.write_bytes(certificate.public_bytes(serialization.Encoding.PEM))
    key_path.write_bytes(key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.TraditionalOpenSSL,
        serialization.NoEncryption(),
    ))
    return cert_path, key_path


def _write_certificate_with_openssl(directory):
    openssl = shutil.which("openssl")
    if not openssl:
        raise unittest.SkipTest("openssl or cryptography unavailable")
    cert_path = Path(directory) / "localhost-cert.pem"
    key_path = Path(directory) / "localhost-key.pem"
    command = [
        openssl,
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "1",
        "-keyout",
        str(key_path),
        "-out",
        str(cert_path),
        "-subj",
        "/CN=localhost",
        "-addext",
        "subjectAltName=DNS:localhost,IP:127.0.0.1",
    ]
    result = subprocess.run(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
    )
    if result.returncode != 0:
        raise unittest.SkipTest("unable to generate self-signed certificate")
    return cert_path, key_path


def _create_self_signed_certificate(directory):
    try:
        certificate = _write_certificate_with_cryptography(directory)
        if certificate is not None:
            return certificate
    except Exception:
        pass
    return _write_certificate_with_openssl(directory)


class TlsNetworkIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tempdir = tempfile.TemporaryDirectory()
        try:
            cls.certfile, cls.keyfile = _create_self_signed_certificate(
                cls.tempdir.name
            )
        except Exception:
            cls.tempdir.cleanup()
            raise

    @classmethod
    def tearDownClass(cls):
        cls.tempdir.cleanup()

    def test_api_http_strict_tls_fails_and_relaxed_tls_succeeds(self):
        server = _TlsApiTestServer(
            ("127.0.0.1", 0),
            _ApiRequestHandler,
            self.certfile,
            self.keyfile,
        )
        with _running_server(server):
            strict = _new_api_client(server, strict_tls=True, scheme="https")
            with self.assertRaises(TransportError):
                strict._http("/ok", {"encrypted": True})

            relaxed = _new_api_client(server, strict_tls=False, scheme="https")
            result = relaxed._http("/ok", {"encrypted": True})

        self.assertEqual(result, {"ok": True})
        self.assertEqual(len(server.requests), 1)


if __name__ == "__main__":
    unittest.main()
