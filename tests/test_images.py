import http.server
import io
import os
import threading
import time
import unittest

from PIL import Image

from kinnovel.ui import (
    ImageCache,
    _ssl_context,
    scaled_image_url,
    system_image_size,
    with_image_height,
)


class _Handler(http.server.BaseHTTPRequestHandler):
    payload = b""
    hits = 0

    def do_GET(self):  # noqa: N802
        type(self).hits += 1
        self.send_response(200)
        self.send_header("Content-Type", "image/png")
        self.send_header("Content-Length", str(len(self.payload)))
        self.end_headers()
        self.wfile.write(self.payload)

    def log_message(self, *_args):
        return


class ImageCacheTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        buffer = io.BytesIO()
        Image.new("RGB", (600, 400), (10, 120, 200)).save(buffer, "PNG")
        _Handler.payload = buffer.getvalue()
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = "http://127.0.0.1:%d" % cls.server.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def setUp(self):
        _Handler.hits = 0
        self.cache = ImageCache(maximum=4, max_bytes=10 * 1024 * 1024,
                                fitted_max_bytes=1024 * 1024, workers=2)

    def _wait(self, url, height, timeout=15.0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.cache.is_cached(url, height):
                return True
            time.sleep(0.05)
        return False

    def test_size_variant_keys_are_distinct(self):
        url = self.base + "/a.png?size=16x12&placeholder=x"
        self.assertNotEqual(self.cache._path(url, 512), self.cache._path(url, 1024))

    def test_legacy_file_is_not_reported_as_size_variant(self):
        url = self.base + "/legacy.png"
        legacy = self.cache._path(url)
        legacy.parent.mkdir(parents=True, exist_ok=True)
        Image.new("L", (20, 20), 0).save(legacy, "JPEG")
        self.assertIsNotNone(self.cache.get(url, 512))
        self.assertFalse(self.cache.is_cached(url, 512))

    def test_scaled_url_only_rewrites_system_images(self):
        url = self.base + "/a.png?size=16x12"
        self.assertEqual(system_image_size(url), (16, 12))
        self.assertIn("height=512", scaled_image_url(url, 512))
        plain = self.base + "/a.png"
        self.assertEqual(scaled_image_url(plain, 512), plain)

    def test_with_image_height_replaces_existing_value(self):
        result = with_image_height(self.base + "/a.png?size=16x12&height=99", 512)
        self.assertIn("height=512", result)
        self.assertNotIn("height=99", result)

    def test_download_downscales_and_caches(self):
        url = self.base + "/cover.png?size=600x400"
        self.assertTrue(self.cache.prefetch(url, height=256))
        self.assertTrue(self._wait(url, 256))
        image = self.cache.get(url, 256)
        self.assertIsNotNone(image)
        self.assertEqual(image.mode, "L")
        self.assertLessEqual(max(image.size), 256)
        self.assertGreater(max(image.size), 200)
        self.assertEqual(_Handler.hits, 1)
        # 第二次必须命中缓存，不再产生请求
        self.cache.prefetch(url, height=256)
        time.sleep(0.3)
        self.assertEqual(_Handler.hits, 1)

    def test_inflight_requests_are_deduplicated(self):
        url = self.base + "/dup.png?size=600x400"
        self.assertTrue(self.cache.prefetch(url, height=256))
        self.assertTrue(self.cache.prefetch(url, height=256))
        self.assertTrue(self._wait(url, 256))
        self.assertEqual(_Handler.hits, 1)

    def test_failure_backoff_prevents_retry_storm(self):
        url = "http://127.0.0.1:1/missing.png?size=600x400"
        self.assertTrue(self.cache.prefetch(url, height=256))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if not self.cache.prefetch(url, height=256):
                break
            time.sleep(0.05)
        else:
            self.fail("failure backoff never engaged")
        # 退避期内必须拒绝再次排队
        self.assertFalse(self.cache.prefetch(url, height=256))

    def test_corrupt_cache_file_is_removed(self):
        url = self.base + "/bad.png?size=600x400"
        path = self.cache._path(url, 256)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"not an image")
        self.assertIsNone(self.cache.get(url, 256))
        self.assertFalse(path.exists())

    def test_visible_priority_jumps_ahead_of_background(self):
        cache = ImageCache(workers=1)
        order = []
        gate = threading.Event()

        def fake_download(url, key, height, strict_tls, priority=0):
            order.append(url)
            if len(order) == 1:
                gate.wait(5)

        cache._download = fake_download
        cache.prefetch("bg?size=1x1", height=256, priority=6)
        time.sleep(0.2)  # 让 worker 先拿起后台任务
        cache.prefetch("mid?size=1x1", height=256, priority=3)
        cache.prefetch("visible?size=1x1", height=256, priority=0)
        gate.set()
        deadline = time.monotonic() + 5
        while len(order) < 3 and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual(order, ["bg?size=1x1", "visible?size=1x1",
                                 "mid?size=1x1"])

    def test_visible_image_is_retried_after_failure(self):
        results = []
        calls = {"count": 0}
        original = self.cache._download_image

        def flaky(url, height, strict_tls):
            calls["count"] += 1
            if calls["count"] == 1:
                raise OSError("transient")
            return Image.new("L", (10, 10), 255)

        self.cache._download_image = flaky
        self.cache._save = lambda path, image: None
        url = "http://127.0.0.1:1/retry.png?size=600x400"
        self.assertTrue(self.cache.prefetch(
            url, height=256, priority=0, callback=results.append))
        deadline = time.monotonic() + 8
        while not results and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertEqual(results, [True])
        self.assertGreaterEqual(calls["count"], 2)
        self.cache._download_image = original

    def test_ssl_context_is_reused_per_strict_tls(self):
        first = _ssl_context(False)
        second = _ssl_context(False)
        strict = _ssl_context(True)
        self.assertIs(first, second)
        self.assertIsNot(first, strict)
        self.assertFalse(first.check_hostname)
        self.assertTrue(strict.check_hostname)


if __name__ == "__main__":
    unittest.main()
