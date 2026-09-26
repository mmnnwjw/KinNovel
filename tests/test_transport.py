import base64
import gzip
import importlib.util
import io
import json
import sys
import types
import unittest
from pathlib import Path
from unittest import mock

from kinnovel.api import ApiClient
from kinnovel.transport import (
    ApiError,
    SignalRClient,
    TransportError,
    WebSocketConnection,
    gunzip_limited,
)


def _load_framebuffer_module():
    try:
        __import__("fcntl")
    except ImportError:
        sys.modules["fcntl"] = types.ModuleType("fcntl")
    path = (Path(__file__).resolve().parents[1] /
            "bin" / "src" / "screen" / "output" / "framebuffer.py")
    try:
        spec = importlib.util.spec_from_file_location("_test_framebuffer", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    except Exception:
        return None


_FRAMEBUFFER = _load_framebuffer_module()


class TransportTests(unittest.TestCase):
    def test_gzip_base64_envelope_decodes(self):
        client = SignalRClient("https://example.test")
        payload = {"Data": [{"Id": 1, "Title": "测试"}], "TotalPages": 1}
        encoded = base64.b64encode(gzip.compress(json.dumps(
            payload, ensure_ascii=False).encode("utf-8"))).decode("ascii")
        self.assertEqual(client._decode_response(encoded), payload)

    def test_websocket_buffer_is_consumed_before_socket(self):
        class Socket:
            def __init__(self):
                self.reads = 0

            def recv(self, count):
                self.reads += 1
                return b"cd"[:count]

        socket_ = WebSocketConnection("ws://example.test")
        socket_.sock = Socket()
        socket_._buffer.extend(b"ab")
        self.assertEqual(socket_._recv_exact(3), b"abc")
        self.assertEqual(socket_.sock.reads, 1)

    def test_websocket_fragment_accumulation_limit_closes(self):
        class Socket:
            def __init__(self):
                self.closed = False

            def sendall(self, _data):
                return None

            def close(self):
                self.closed = True

        socket_ = WebSocketConnection("ws://example.test")
        raw_socket = Socket()
        socket_.sock = raw_socket
        stream = io.BytesIO(
            bytes([0x01, 3]) + b"abc" + bytes([0x80, 3]) + b"def")
        socket_._recv_exact = stream.read

        with mock.patch("kinnovel.transport.MAX_WEBSOCKET_MESSAGE_BYTES", 5):
            with self.assertRaisesRegex(TransportError, "WebSocket 消息超过"):
                socket_.receive()
        self.assertTrue(raw_socket.closed)
        self.assertIsNone(socket_.sock)

    def test_transport_error_reconnects_once(self):
        client = SignalRClient("https://example.test")
        client.rate_limit.wait = lambda: None
        calls = []

        def invoke_once(invocation, method, timeout):
            calls.append(method)
            if len(calls) == 1:
                from kinnovel.transport import TransportError
                raise TransportError("closed")
            return {"ok": True}

        client._invoke_once = invoke_once
        self.assertEqual(client.invoke("GetOnlineInfo"), {"ok": True})
        self.assertEqual(len(calls), 2)

    def test_api_error_is_not_retried(self):
        client = SignalRClient("https://example.test")
        client.rate_limit.wait = lambda: None
        calls = []

        def invoke_once(invocation, method, timeout):
            calls.append(method)
            raise ApiError("user is unauthorized", 401)

        client._invoke_once = invoke_once
        with self.assertRaises(ApiError):
            client.invoke("GetMyInfo")
        self.assertEqual(len(calls), 1)

    def test_api_401_refreshes_closes_hub_and_retries(self):
        client = ApiClient.__new__(ApiClient)
        calls = []

        class Session:
            @staticmethod
            def set_many(values):
                calls.append(("session", values))

        client.session = Session()
        hub = mock.Mock()

        def invoke(method, params):
            calls.append("invoke")
            if sum(item == "invoke" for item in calls) == 1:
                raise ApiError("unauthorized", 401)
            return {"ok": True}

        def refresh():
            calls.append("refresh")
            return "new-token"

        hub.invoke.side_effect = invoke
        hub.close.side_effect = lambda: calls.append("close")
        client.hub = hub
        client.refresh_access_token = refresh

        self.assertEqual(client.invoke("GetMyInfo", {"Id": 1}), {"ok": True})
        self.assertEqual(calls, [
            "invoke",
            ("session", {"Token": "", "TokenUpdatedAt": 0}),
            "refresh",
            "close",
            "invoke",
        ])
        self.assertEqual(hub.invoke.call_count, 2)
        self.assertEqual(hub.close.call_count, 1)

    def test_record_split_across_ws_messages(self):
        client = SignalRClient("https://example.test")

        class Socket:
            def __init__(self, chunks):
                self.chunks = chunks

            def receive(self):
                return 1, self.chunks.pop(0)

        socket_ = Socket([b'{"type":6'])
        self.assertEqual(client._receive_messages(socket_), [])
        socket_.chunks.append(b"}\x1e")
        self.assertEqual(client._receive_messages(socket_), [{"type": 6}])

    def test_record_buffer_limit_closes_connection(self):
        client = SignalRClient("https://example.test")

        class Socket:
            def __init__(self):
                self.closed = False

            @staticmethod
            def receive():
                return 1, b"abcdef"

            def close(self):
                self.closed = True

        socket_ = Socket()
        client._socket = socket_
        with mock.patch("kinnovel.transport.MAX_SIGNALR_RECORD_BYTES", 5):
            with self.assertRaisesRegex(TransportError, "SignalR 记录超过"):
                client._receive_messages(socket_)
        self.assertTrue(socket_.closed)
        self.assertIsNone(client._socket)
        self.assertEqual(len(client._record_buffer), 0)

    def test_gunzip_limited(self):
        raw = gzip.compress(b"A" * 1024)
        with self.assertRaises(TransportError):
            gunzip_limited(raw, limit=100)
        self.assertEqual(gunzip_limited(raw, limit=2048), b"A" * 1024)

    def test_invoke_once_matches_invocation_id(self):
        class Socket:
            @staticmethod
            def send_text(_text):
                return None

        client = SignalRClient("https://example.test")
        client._socket = Socket()
        client._ensure_connected_locked = lambda: None
        invocation = {
            "type": 1,
            "invocationId": "abc",
            "target": "GetMyInfo",
            "arguments": [{}, {"UseGzip": True}],
        }
        client._receive_messages = lambda _socket: [{
            "type": 3,
            "invocationId": "abc",
            "result": {"success": True, "response": {"Id": 1}},
        }]
        self.assertEqual(client._invoke_once(invocation, "GetMyInfo", 1), {"Id": 1})


@unittest.skipIf(_FRAMEBUFFER is None, "framebuffer module unavailable")
class FramebufferInitializationTests(unittest.TestCase):
    def test_screeninfo_ioctl_failure_raises_before_mmap(self):
        display = _FRAMEBUFFER.EInkDisplay.__new__(_FRAMEBUFFER.EInkDisplay)
        display.fd = 123
        display._ioctl = mock.Mock(return_value=None)

        with mock.patch.object(_FRAMEBUFFER.mmap, "mmap") as mmap_:
            with self.assertRaisesRegex(OSError, "VSCREENINFO"):
                display._read_screeninfo()
        mmap_.assert_not_called()

    def test_init_closes_fd_and_mmap_on_failure(self):
        display_cls = _FRAMEBUFFER.EInkDisplay
        fake_mem = mock.Mock()

        def fail_after_mmap(display):
            display.mem = fake_mem
            raise OSError("screeninfo invalid")

        with mock.patch.object(display_cls, "_read_screeninfo", fail_after_mmap), \
                mock.patch.object(_FRAMEBUFFER.os, "open", return_value=123) as open_, \
                mock.patch.object(_FRAMEBUFFER.os, "close") as close_:
            with self.assertRaisesRegex(OSError, "screeninfo invalid"):
                display_cls("/dev/fb0", protocol="mtk")

        open_.assert_called_once_with("/dev/fb0", _FRAMEBUFFER.os.O_RDWR)
        fake_mem.close.assert_called_once_with()
        close_.assert_called_once_with(123)


if __name__ == "__main__":
    unittest.main()
