import base64
import gzip
import json
import unittest

from kinnovel.api import ApiClient
from kinnovel.transport import (
    ApiError,
    SignalRClient,
    TransportError,
    WebSocketConnection,
    gunzip_limited,
)


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


if __name__ == "__main__":
    unittest.main()
