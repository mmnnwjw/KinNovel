import unittest
import subprocess
import sys
import threading
from pathlib import Path
from unittest import mock

from PIL import Image

from kinnovel.config import APP_DIR
from kinnovel import utils
from kinnovel.utils import atomic_write, read_json


class UtilityTests(unittest.TestCase):
    def test_atomic_json_roundtrip(self):
        root = APP_DIR / "build"
        path = root / "test-data.json"
        try:
            data = {"中文": "正文", "count": 2}
            atomic_write(path, __import__("json").dumps(data, ensure_ascii=False))
            self.assertEqual(read_json(path), data)
        finally:
            try:
                path.unlink()
            except OSError:
                pass

    def test_atomic_write_is_serialized_per_path(self):
        path = APP_DIR / "build" / "atomic-concurrent.bin"
        contents = [
            bytes([index]) * 65536
            for index in range(8)
        ]
        barrier = threading.Barrier(len(contents))
        errors = []

        def worker(payload):
            try:
                barrier.wait(timeout=5)
                for _ in range(20):
                    atomic_write(path, payload)
            except BaseException as exc:
                errors.append(exc)

        try:
            threads = [
                threading.Thread(target=worker, args=(payload,))
                for payload in contents
            ]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(timeout=20)
            self.assertFalse(any(thread.is_alive() for thread in threads))
            self.assertEqual(errors, [])
            self.assertIn(path.read_bytes(), contents)
        finally:
            try:
                path.unlink()
            except OSError:
                pass
            for temp in path.parent.glob(path.name + "*.tmp"):
                try:
                    temp.unlink()
                except OSError:
                    pass

    def test_atomic_write_removes_temp_file_on_error(self):
        path = APP_DIR / "build" / "atomic-cleanup.bin"
        with mock.patch.object(
                utils.os, "replace",
                side_effect=OSError("replace failed")):
            with self.assertRaises(OSError):
                atomic_write(path, b"payload")
        self.assertFalse(path.exists())
        self.assertEqual(
            list(path.parent.glob(path.name + "*.tmp")),
            [],
        )

    def test_image_worker_isolated_conversion(self):
        source = APP_DIR / "build" / "worker-source.png"
        output = APP_DIR / "build" / "worker-output.png"
        try:
            output.unlink()
        except OSError:
            pass
        Image.new("RGB", (12, 8), (200, 40, 10)).save(source)
        worker = APP_DIR / "bin" / "image_worker.py"
        completed = subprocess.run(
            [sys.executable, str(worker), source.as_uri(), str(output), "1"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=20,
            check=False,
        )
        self.assertEqual(completed.returncode, 0)
        converted = Image.open(output)
        converted.load()
        self.assertEqual(converted.mode, "L")
        self.assertEqual(converted.size, (12, 8))
        for path in (source, output):
            try:
                path.unlink()
            except OSError:
                pass

if __name__ == "__main__":
    unittest.main()
