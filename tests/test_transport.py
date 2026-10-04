import base64
import gzip
import importlib.util
import io
import json
import sys
import threading
import time
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


def _load_screen_output_module():
    path = (Path(__file__).resolve().parents[1] /
            "bin" / "src" / "screen" / "output" / "__init__.py")
    package = types.ModuleType("_test_screen_output")
    package.__path__ = []
    sys.modules["_test_screen_output"] = package
    framebuffer = types.ModuleType("_test_screen_output.framebuffer")
    framebuffer.EInkDisplay = object
    framebuffer.FLAG = object
    framebuffer.UPDATE = object
    framebuffer.WAVEFORM = object
    sys.modules["_test_screen_output.framebuffer"] = framebuffer
    try:
        spec = importlib.util.spec_from_file_location(
            "_test_screen_output", path
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    except Exception:
        return None


_SCREEN_OUTPUT = _load_screen_output_module()


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

    def test_api_client_marks_non_idempotent_methods(self):
        client = ApiClient.__new__(ApiClient)
        client._flight_lock = threading.Lock()
        client._inflight = {}
        client._cache_lock = threading.Lock()
        client._cache = {}
        hub = mock.Mock()
        hub.invoke.return_value = {"ok": True}
        client.hub = hub

        self.assertEqual(client.invoke("BuyShopItem", {"Key": "x"}), {"ok": True})
        hub.invoke.assert_called_once_with(
            "BuyShopItem", {"Key": "x"}, retry=False, priority=0)

        hub.reset_mock()
        hub.invoke.return_value = {"ok": True}
        self.assertEqual(client.invoke("GetMyInfo", {}), {"ok": True})
        hub.invoke.assert_called_once_with("GetMyInfo", {}, retry=True, priority=0)

    def test_api_401_refreshes_closes_hub_and_retries(self):
        client = ApiClient.__new__(ApiClient)
        client._flight_lock = threading.Lock()
        client._inflight = {}
        client._cache_lock = threading.Lock()
        client._cache = {}
        calls = []

        class Session:
            @staticmethod
            def set_many(values):
                calls.append(("session", values))

        client.session = Session()
        hub = mock.Mock()

        def invoke(method, params, **kwargs):
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

    def test_truncated_gzip_is_rejected(self):
        raw = gzip.compress(b'{"Success": true}')
        with self.assertRaisesRegex(TransportError, "不完整"):
            gunzip_limited(raw[:10])

    def test_non_idempotent_invoke_is_not_retried(self):
        client = SignalRClient("https://example.test")
        client.rate_limit.wait = lambda: None
        calls = []

        def invoke_once(invocation, method, timeout):
            calls.append(method)
            raise TransportError("closed")

        client._invoke_once = invoke_once
        with self.assertRaises(TransportError):
            client.invoke("BuyShopItem", {"Key": "x"}, retry=False)
        self.assertEqual(calls, ["BuyShopItem"])

    def test_priority_scheduler_prefers_interactive(self):
        client = SignalRClient("https://example.test")
        client._turn_active = False
        client._turn_waiting = {1: 1}
        self.assertTrue(client._can_acquire_locked(0))
        client._turn_waiting = {0: 1}
        self.assertFalse(client._can_acquire_locked(1))

    def test_keepalive_ping_sends_type6(self):
        sent = []

        class Socket:
            @staticmethod
            def send_text(text):
                sent.append(text)

        client = SignalRClient("https://example.test")
        client._socket = Socket()
        client._last_used = 0.0
        self.assertTrue(client._keepalive_ping())
        self.assertTrue(sent and sent[0].startswith('{"type":6}'))

    def test_shutdown_interrupts_and_disables_reconnect(self):
        class Socket:
            def __init__(self):
                self.shutdown_called = False
                self.closed = False

            def shutdown(self):
                self.shutdown_called = True

            def close(self):
                self.closed = True

        client = SignalRClient("https://example.test")
        socket_ = Socket()
        client._socket = socket_
        client.shutdown()
        self.assertTrue(socket_.shutdown_called)
        self.assertTrue(socket_.closed)
        self.assertIsNone(client._socket)
        with self.assertRaises(TransportError):
            client.invoke("GetMyInfo")

    def test_shutdown_unblocks_inflight_receive(self):
        class BlockingSocket:
            def __init__(self):
                self.started = threading.Event()
                self.stop = threading.Event()
                self.shutdown_called = False

            @staticmethod
            def send_text(_text):
                return None

            @staticmethod
            def settimeout(_seconds):
                return None

            def receive(self):
                self.started.set()
                self.stop.wait(5)
                raise OSError("connection closed")

            def shutdown(self):
                self.shutdown_called = True
                self.stop.set()

            def close(self):
                self.stop.set()

        client = SignalRClient("https://example.test")
        socket_ = BlockingSocket()
        client._socket = socket_
        client._ensure_connected_locked = lambda: socket_

        def worker():
            try:
                client.invoke("GetMyInfo")
            except TransportError:
                pass

        thread = threading.Thread(target=worker)
        thread.start()
        self.assertTrue(socket_.started.wait(5))
        started = time.monotonic()
        client.shutdown()
        thread.join(5)
        self.assertLess(time.monotonic() - started, 2.0)
        self.assertFalse(thread.is_alive())
        self.assertTrue(socket_.shutdown_called)

    def test_type7_close_raises_with_reason(self):
        class Socket:
            @staticmethod
            def send_text(_text):
                return None

            @staticmethod
            def settimeout(_seconds):
                return None

        client = SignalRClient("https://example.test")
        client._socket = Socket()
        client._ensure_connected_locked = lambda: client._socket
        client._receive_messages = lambda _socket: [
            {"type": 7, "error": "bye"}]
        invocation = {"type": 1, "invocationId": "abc",
                      "target": "GetMyInfo", "arguments": [{}, {}]}
        with self.assertRaisesRegex(TransportError, "bye"):
            client._invoke_once(invocation, "GetMyInfo", 1)

    def test_api_invoke_coalesces_identical_reads(self):
        client = ApiClient.__new__(ApiClient)
        client._flight_lock = threading.Lock()
        client._inflight = {}
        client._cache_lock = threading.Lock()
        client._cache = {}
        started = threading.Event()
        release = threading.Event()
        calls = []

        class Hub:
            @staticmethod
            def invoke(method, params, retry=True, priority=0):
                calls.append(method)
                started.set()
                release.wait(5)
                return {"Id": 7}

        client.hub = Hub()
        results = []

        def worker():
            results.append(client.invoke("GetBookInfo", {"Id": 1}))

        threads = [threading.Thread(target=worker) for _ in range(2)]
        threads[0].start()
        self.assertTrue(started.wait(5))
        threads[1].start()
        time.sleep(0.1)
        release.set()
        for thread in threads:
            thread.join(5)
        self.assertEqual(results, [{"Id": 7}, {"Id": 7}])
        self.assertEqual(calls, ["GetBookInfo"])

    def test_access_token_only_downgrades_on_invalid_refresh(self):
        class Session:
            def __init__(self):
                self.cleared = False

            @staticmethod
            def get(key, default=None):
                return "refresh" if key == "RefreshToken" else default

            def clear_credentials(self):
                self.cleared = True

        client = ApiClient.__new__(ApiClient)
        client.session = Session()

        def invalid():
            raise ApiError("invalid refresh", 401)

        client.refresh_access_token = invalid
        self.assertIsNone(client.get_access_token())
        self.assertTrue(client.session.cleared)

        client.session.cleared = False

        def transient():
            raise TransportError("network down")

        client.refresh_access_token = transient
        with self.assertRaises(TransportError):
            client.get_access_token()
        self.assertFalse(client.session.cleared)

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

            @staticmethod
            def settimeout(_seconds):
                return None

        client = SignalRClient("https://example.test")
        client._socket = Socket()
        client._ensure_connected_locked = lambda: client._socket
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
    @unittest.skipIf(_SCREEN_OUTPUT is None, "screen output module unavailable")
    def test_screen_output_forwards_swipe_animation(self):
        output = _SCREEN_OUTPUT.ScreenOutput(None)
        output.display = mock.Mock()
        output.display.supports_swipe_animation = True

        self.assertTrue(output.supports_swipe_animation)
        output.set_swipe_animations(True)
        output.set_swipe_direction(True)

        output.display.set_swipe_animations.assert_called_once_with(True)
        output.display.set_swipe_direction.assert_called_once_with(True)

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

    def test_mtk_swipe_animation_sets_flag_and_resets(self):
        module = _FRAMEBUFFER
        display = module.EInkDisplay.__new__(module.EInkDisplay)
        display.update_data_cls = module.MxcfbUpdateDataMtk
        display.ioctls = {"send_update": 1}
        display.W = module.WAVEFORM_MTK
        display.FLAG = module.FLAG_MTK
        display.width = 1000
        display.height = 1000
        display.alignment = 8
        display.temp = 25
        display.is_reagl = False
        display.night_mode = False
        display.flash_invalid_waveforms = (
            module.WAVEFORM_MTK.AUTO,
            module.WAVEFORM_MTK.DU,
            module.WAVEFORM_MTK.A2,
            module.WAVEFORM_MTK.DU4,
        )
        display.reagl_waveform = module.WAVEFORM_MTK.REAGL
        display.wait_for_submission_before = False
        display.wait_for_completion = False
        display._pending_marker = None
        display._marker = 0
        display.supports_swipe_animation = True
        display.swipe_steps = 12
        display._swipe_animation = True
        display._swipe_direction = module.SWIPE_MTK.LEFT
        captured = {}

        def send_update(x, y, w, h, waveform, update_mode, flags, marker,
                        swipe_direction=None):
            captured.update({
                "flags": flags,
                "waveform": waveform,
                "swipe_direction": swipe_direction,
                "marker": marker,
            })
            return True

        display._send_update = send_update
        display._get_next_marker = lambda: 7
        marker = display.mxc_update(
            0, 0, 100, 100, False, module.WAVEFORM_MTK.GC16
        )

        self.assertEqual(marker, 7)
        self.assertTrue(captured["flags"] & module.FLAG_MTK.ENABLE_SWIPE)
        self.assertEqual(captured["swipe_direction"], module.SWIPE_MTK.LEFT)
        self.assertFalse(display._swipe_animation)

    def test_mtk_swipe_animation_is_skipped_for_tiny_region(self):
        module = _FRAMEBUFFER
        display = module.EInkDisplay.__new__(module.EInkDisplay)
        display.update_data_cls = module.MxcfbUpdateDataMtk
        display.ioctls = {"send_update": 1}
        display.W = module.WAVEFORM_MTK
        display.FLAG = module.FLAG_MTK
        display.width = 1000
        display.height = 1000
        display.alignment = 8
        display.temp = 25
        display.is_reagl = False
        display.night_mode = False
        display.flash_invalid_waveforms = (
            module.WAVEFORM_MTK.AUTO,
            module.WAVEFORM_MTK.DU,
            module.WAVEFORM_MTK.A2,
            module.WAVEFORM_MTK.DU4,
        )
        display.reagl_waveform = module.WAVEFORM_MTK.REAGL
        display.wait_for_submission_before = False
        display.wait_for_completion = False
        display._pending_marker = None
        display._marker = 0
        display.supports_swipe_animation = True
        display.swipe_steps = 12
        display._swipe_animation = True
        display._swipe_direction = module.SWIPE_MTK.LEFT
        captured = {}
        display._send_update = (
            lambda x, y, w, h, waveform, update_mode, flags, marker,
            swipe_direction=None: captured.update({
                "flags": flags,
                "swipe_direction": swipe_direction,
            }) or True
        )
        display._get_next_marker = lambda: 8

        display.mxc_update(0, 0, 8, 8, False, module.WAVEFORM_MTK.GC16)

        self.assertFalse(captured["flags"] & module.FLAG_MTK.ENABLE_SWIPE)
        self.assertIsNone(captured["swipe_direction"])
        self.assertFalse(display._swipe_animation)

    def test_rex_and_zelda_struct_sizes_and_ioctls(self):
        import ctypes
        module = _FRAMEBUFFER
        self.assertEqual(ctypes.sizeof(module.MxcfbUpdateDataRex), 80)
        self.assertEqual(ctypes.sizeof(module.MxcfbUpdateDataZelda), 88)

        rex_ioctls = module._rex_ioctls()
        zelda_ioctls = module._zelda_ioctls()
        # 0x4050462E = _IOW('F', 0x2E, 80)
        self.assertEqual(rex_ioctls["send_update"], 0x4050462E)
        # 0x4058462E = _IOW('F', 0x2E, 88)
        self.assertEqual(zelda_ioctls["send_update"], 0x4058462E)

    def test_rex_and_zelda_protocol_init(self):
        module = _FRAMEBUFFER
        display_cls = module.EInkDisplay

        with mock.patch.object(display_cls, "_read_screeninfo"), \
                mock.patch.object(display_cls, "_init_epdc"), \
                mock.patch.object(module.os, "open", return_value=100), \
                mock.patch.object(module.os, "close"):
            display_rex = display_cls("/dev/fb0", protocol="rex")
            self.assertIs(display_rex.update_data_cls, module.MxcfbUpdateDataRex)
            self.assertEqual(display_rex.ioctls["send_update"], 0x4050462E)
            self.assertFalse(display_rex.supports_swipe_animation)

            display_zelda = display_cls("/dev/fb0", protocol="zelda")
            self.assertIs(display_zelda.update_data_cls, module.MxcfbUpdateDataZelda)
            self.assertEqual(display_zelda.ioctls["send_update"], 0x4058462E)
            self.assertFalse(display_zelda.supports_swipe_animation)


if __name__ == "__main__":
    unittest.main()
