import unittest
import subprocess
import sys
from pathlib import Path

from PIL import Image

from kinnovel.config import APP_DIR
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
