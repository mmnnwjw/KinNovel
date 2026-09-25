import base64
import gzip
import json
import os
import socket
import ssl
import struct
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from collections import deque


class TransportError(RuntimeError):
    pass


class ApiError(TransportError):
    def __init__(self, message="请求失败", status=500):
        super().__init__(message)
        self.status = status


class RateLimit:
    def __init__(self, maximum=9, window_seconds=5.5):
        self.maximum = max(1, int(maximum))
        self.window = max(0.1, float(window_seconds))
        self._times = deque()
        self._condition = threading.Condition()

    def wait(self):
        with self._condition:
            while True:
                now = time.monotonic()
                while self._times and now - self._times[0] >= self.window:
                    self._times.popleft()
                if len(self._times) < self.maximum:
                    self._times.append(now)
                    return
                delay = self.window - (now - self._times[0]) + 0.02
                self._condition.wait(max(0.02, delay))


class WebSocketConnection:
    """Minimal RFC 6455 client used to avoid a Kindle-side websocket dependency."""

    def __init__(self, url, headers=None, timeout=30, ssl_context=None):
        self.url = url
        self.headers = dict(headers or {})
        self.timeout = timeout
        self.ssl_context = ssl_context
        self.sock = None
        self._send_lock = threading.Lock()
        self._closed = False
        self._buffer = bytearray()

    def connect(self):
        parsed = urllib.parse.urlsplit(self.url)
        secure = parsed.scheme == "wss"
        host = parsed.hostname
        if not host:
            raise TransportError("WebSocket URL 缺少主机")
        port = parsed.port or (443 if secure else 80)
        raw = socket.create_connection((host, port), timeout=self.timeout)
        if secure:
            context = self.ssl_context
            if context is None:
                context = ssl.create_default_context()
                try:
                    context.check_hostname = True
                    context.verify_mode = ssl.CERT_REQUIRED
                except ssl.SSLError as exc:
                    raise TransportError("TLS 初始化失败: %s" % exc)
            raw = context.wrap_socket(raw, server_hostname=host)
        raw.settimeout(self.timeout)
        self.sock = raw
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        path = parsed.path or "/"
        if parsed.query:
            path += "?" + parsed.query
        headers = {
            "Host": parsed.netloc,
            "Upgrade": "websocket",
            "Connection": "Upgrade",
            "Sec-WebSocket-Key": key,
            "Sec-WebSocket-Version": "13",
            "User-Agent": "KinNovel/0.1",
        }
        headers.update(self.headers)
        request = "GET %s HTTP/1.1\r\n" % path
        request += "".join("%s: %s\r\n" % item for item in headers.items())
        request += "\r\n"
        raw.sendall(request.encode("ascii", "replace"))
        response = b""
        while b"\r\n\r\n" not in response:
            chunk = raw.recv(4096)
            if not chunk:
                raise TransportError("WebSocket 握手连接被关闭")
            response += chunk
            if len(response) > 65536:
                raise TransportError("WebSocket 握手响应过大")
        header_end = response.find(b"\r\n\r\n")
        if not response.startswith(b"HTTP/1.1 101") and not response.startswith(b"HTTP/1.0 101"):
            first_line = response.split(b"\r\n", 1)[0].decode("latin1", "replace")
            raise TransportError("WebSocket 握手失败: " + first_line)
        if header_end >= 0:
            self._buffer.extend(response[header_end + 4:])
        return self

    def _recv_exact(self, count):
        chunks = []
        if self._buffer:
            take = min(count, len(self._buffer))
            chunks.append(bytes(self._buffer[:take]))
            del self._buffer[:take]
        remaining = count - sum(len(chunk) for chunk in chunks)
        while remaining > 0:
            chunk = self.sock.recv(remaining)
            if not chunk:
                raise TransportError("WebSocket 连接已关闭")
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    def _send_frame(self, opcode, payload=b""):
        if self.sock is None or self._closed:
            raise TransportError("WebSocket 未连接")
        payload = bytes(payload)
        mask = os.urandom(4)
        length = len(payload)
        header = bytearray([0x80 | opcode])
        if length < 126:
            header.append(0x80 | length)
        elif length <= 0xFFFF:
            header.append(0x80 | 126)
            header.extend(struct.pack("!H", length))
        else:
            header.append(0x80 | 127)
            header.extend(struct.pack("!Q", length))
        header.extend(mask)
        masked = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        with self._send_lock:
            self.sock.sendall(bytes(header) + masked)

    def send_text(self, text):
        self._send_frame(0x1, text.encode("utf-8"))

    def send_pong(self, data):
        self._send_frame(0xA, data)

    def receive(self):
        fragments = bytearray()
        message_opcode = None
        while True:
            head = self._recv_exact(2)
            first, second = head[0], head[1]
            fin = bool(first & 0x80)
            opcode = first & 0x0F
            masked = bool(second & 0x80)
            length = second & 0x7F
            if length == 126:
                length = struct.unpack("!H", self._recv_exact(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self._recv_exact(8))[0]
            if length > 32 * 1024 * 1024:
                raise TransportError("WebSocket 消息超过 32MB")
            mask = self._recv_exact(4) if masked else None
            payload = self._recv_exact(length) if length else b""
            if mask:
                payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
            if opcode == 0x8:
                self._closed = True
                raise TransportError("WebSocket 已被服务器关闭")
            if opcode == 0x9:
                self.send_pong(payload)
                continue
            if opcode == 0xA:
                continue
            if opcode in (0x1, 0x2):
                message_opcode = opcode
                fragments = bytearray(payload)
            elif opcode == 0x0 and message_opcode is not None:
                fragments.extend(payload)
            else:
                continue
            if fin:
                return message_opcode, bytes(fragments)

    def close(self):
        if self.sock is None:
            return
        try:
            self._send_frame(0x8, b"\x03\xe8")
        except Exception:
            pass
        try:
            self.sock.close()
        except OSError:
            pass
        self._closed = True
        self.sock = None


class SignalRClient:
    """ASP.NET Core SignalR JSON client with gzip ApiEnvelope support."""

    RECORD_SEPARATOR = "\x1e"

    def __init__(self, server, token_provider=None, strict_tls=False,
                 request_limit=9, request_window=5.5, timeout=30,
                 visitor_id=None):
        self.server = server.rstrip("/")
        self.token_provider = token_provider
        self.strict_tls = bool(strict_tls)
        self.timeout = timeout
        self.rate_limit = RateLimit(request_limit, request_window)
        self._lock = threading.RLock()
        self._socket = None
        self._visitor_id = visitor_id or uuid.uuid4().hex
        self.notifications = deque(maxlen=100)
        self.last_error = ""

    def set_server(self, server):
        with self._lock:
            self._close_locked()
            self.server = server.rstrip("/")

    def _ssl_context(self):
        context = ssl.create_default_context()
        if not self.strict_tls:
            context.check_hostname = False
            context.verify_mode = ssl.CERT_NONE
        return context

    def _headers(self, token=None, json_body=False):
        headers = {
            "x-id": self._visitor_id,
            "Accept": "application/json",
            "User-Agent": "KinNovel/0.1",
        }
        if token:
            headers["Authorization"] = "Bearer " + token
        if json_body:
            headers["Content-Type"] = "application/json"
        return headers

    def _negotiate(self, token=None):
        url = self.server + "/hub/api/negotiate?negotiateVersion=1"
        request = urllib.request.Request(url, data=b"", method="POST",
                                         headers=self._headers(token))
        try:
            with urllib.request.urlopen(request, timeout=self.timeout,
                                        context=self._ssl_context()) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", "replace")
            raise ApiError(body[:300] or ("Hub 协商失败 (%s)" % exc.code), exc.code)
        except OSError as exc:
            raise TransportError("Hub 协商网络错误: %s" % exc)

    def _close_locked(self):
        if self._socket is not None:
            self._socket.close()
            self._socket = None

    def _connect_locked(self):
        self._close_locked()
        token = self.token_provider() if self.token_provider else None
        negotiation = self._negotiate(token)
        connection_token = negotiation.get("connectionToken")
        if not connection_token:
            raise TransportError("Hub 协商响应缺少 connectionToken")
        parsed = urllib.parse.urlsplit(self.server)
        scheme = "wss" if parsed.scheme == "https" else "ws"
        url = "%s://%s/hub/api?id=%s" % (
            scheme,
            parsed.netloc,
            urllib.parse.quote(connection_token, safe=""),
        )
        if token:
            url += "&access_token=" + urllib.parse.quote(token, safe="")
        socket_ = WebSocketConnection(
            url,
            headers={"Origin": "https://www.lightnovel.app"},
            timeout=self.timeout,
            ssl_context=self._ssl_context(),
        ).connect()
        socket_.send_text('{"protocol":"json","version":1}' + self.RECORD_SEPARATOR)
        deadline = time.monotonic() + self.timeout
        while time.monotonic() < deadline:
            messages = self._receive_messages(socket_)
            if any(message == {} for message in messages):
                self._socket = socket_
                return
        socket_.close()
        raise TransportError("SignalR 握手超时")

    def _ensure_connected_locked(self):
        if self._socket is None:
            self._connect_locked()

    def _receive_messages(self, socket_):
        _, data = socket_.receive()
        text = data.decode("utf-8", "replace")
        messages = []
        for raw in text.split(self.RECORD_SEPARATOR):
            raw = raw.strip()
            if not raw:
                continue
            try:
                messages.append(json.loads(raw))
            except ValueError:
                messages.append({"type": -1, "raw": raw})
        return messages

    def _handle_server_message(self, message):
        if message.get("type") == 1:
            self.notifications.append(message)
        elif message.get("type") == 6:
            if self._socket is not None:
                self._socket.send_text('{"type":6}' + self.RECORD_SEPARATOR)

    @staticmethod
    def _envelope_value(envelope, name, default=None):
        if not isinstance(envelope, dict):
            return default
        for key in (name, name.lower(), name.upper(), name[:1].upper() + name[1:]):
            if key in envelope:
                return envelope[key]
        return default

    def _decode_response(self, value):
        if not isinstance(value, str):
            return value
        try:
            raw = base64.b64decode(value, validate=True)
        except (ValueError, TypeError):
            return value
        try:
            raw = gzip.decompress(raw)
        except (OSError, EOFError):
            return value
        try:
            return json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, ValueError):
            return raw

    def _invoke_once(self, invocation, method, timeout):
        with self._lock:
            self._ensure_connected_locked()
            self._socket.send_text(json.dumps(invocation, ensure_ascii=False, separators=(",", ":"))
                                   + self.RECORD_SEPARATOR)
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                try:
                    messages = self._receive_messages(self._socket)
                except (OSError, TransportError) as exc:
                    self.last_error = str(exc)
                    self._close_locked()
                    raise TransportError(str(exc))
                for message in messages:
                    self._handle_server_message(message)
                    if message.get("invocationId") != invocation.get("invocationId"):
                        continue
                    if message.get("type") == 3 and message.get("error"):
                        error = str(message.get("error"))
                        if "unauthorized" in error.lower():
                            raise ApiError(error, 401)
                        raise ApiError(error, 500)
                    if message.get("type") == 3:
                        envelope = message.get("result")
                        if not isinstance(envelope, dict):
                            return self._decode_response(envelope)
                        success = self._envelope_value(envelope, "success", True)
                        if not success:
                            raise ApiError(
                                str(self._envelope_value(envelope, "msg", "请求失败")),
                                int(self._envelope_value(envelope, "status", 500) or 500),
                            )
                        return self._decode_response(self._envelope_value(envelope, "response"))
            raise TransportError("Hub 调用超时: " + method)

    def invoke(self, method, params=None, use_gzip=True, timeout=None):
        timeout = timeout or self.timeout * 2
        self.rate_limit.wait()
        invocation_id = uuid.uuid4().hex
        invocation = {
            "type": 1,
            "invocationId": invocation_id,
            "target": method,
            "arguments": [params or {}, {"UseGzip": bool(use_gzip)}],
        }
        last_error = None
        for attempt in range(2):
            try:
                return self._invoke_once(invocation, method, timeout)
            except TransportError as exc:
                last_error = exc
                with self._lock:
                    self._close_locked()
                if attempt == 0:
                    time.sleep(1.0)
                    continue
                raise
        raise last_error or TransportError("Hub 调用失败: " + method)

    def close(self):
        with self._lock:
            self._close_locked()
