import unittest
from pathlib import Path

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

if __name__ == "__main__":
    unittest.main()
