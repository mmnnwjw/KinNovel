import unittest
from pathlib import Path

from kinnovel.config import APP_DIR
from kinnovel.utils import atomic_write, read_json, safe_filename, write_json


class UtilityTests(unittest.TestCase):
    def test_atomic_json_roundtrip(self):
        root = APP_DIR / "build"
        path = root / "test-data.json"
        try:
            data = {"中文": "正文", "count": 2}
            write_json(path, data)
            self.assertEqual(read_json(path), data)
        finally:
            try:
                path.unlink()
            except OSError:
                pass

    def test_safe_filename(self):
        self.assertEqual(safe_filename('a/b:c*?"<>|'), "a_b_c______")


if __name__ == "__main__":
    unittest.main()
