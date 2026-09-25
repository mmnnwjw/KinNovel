import json
import os
import threading
from pathlib import Path


BIN_DIR = Path(__file__).resolve().parents[2]
APP_DIR = BIN_DIR.parent
CONFIG_PATH = BIN_DIR / "config.json"
CACHE_DIR = APP_DIR / "cache"
LOG_DIR = APP_DIR / "logs"

DEFAULTS = {
    "api_server": "https://api.lightnovel.life",
    "account_email": "",
    "account_password": "",
    "screen_protocol": "auto",
    "framebuffer": "/dev/fb0",
    "font_path": "/usr/java/lib/fonts/STHeitiMedium.ttf",
    "font_size": 48,
    "line_spacing": 1.42,
    "reader_margin": 34,
    "page_flash": False,
    "night_mode": False,
    "justify": False,
    "first_line_indent": True,
    "convert": None,
    "ignore_japanese": False,
    "ignore_ai": False,
    "request_limit": 9,
    "request_window_ms": 5500,
    "cache_limit_mb": 192,
    "strict_tls": True,
    "check_update": True,
    "home_order": {
        "shelf": 0,
        "history": 1,
        "rank": 2,
        "browse": 3,
        "account": 4,
        "settings": 5,
        "about": 6,
        "exit": 7,
        "announcements": -1,
        "notifications": -1,
        "shop": -1,
    },
}


class Config:
    """Small thread-safe JSON configuration store."""

    def __init__(self, path=CONFIG_PATH):
        self.path = Path(path)
        self._lock = threading.RLock()
        self._data = dict(DEFAULTS)
        self.load()

    def load(self):
        with self._lock:
            try:
                with self.path.open("r", encoding="utf-8") as handle:
                    data = json.load(handle)
                if isinstance(data, dict):
                    for key, value in data.items():
                        self._data[key] = value
            except (OSError, ValueError):
                pass
            changed = False
            for key, value in DEFAULTS.items():
                if key not in self._data:
                    self._data[key] = value
                    changed = True
                elif isinstance(value, dict) and isinstance(self._data.get(key), dict):
                    for child_key, child_value in value.items():
                        if child_key not in self._data[key]:
                            self._data[key][child_key] = child_value
                            changed = True
            if changed:
                self.save()
            return dict(self._data)

    def save(self):
        with self._lock:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            temp = self.path.with_suffix(self.path.suffix + ".tmp")
            with temp.open("w", encoding="utf-8") as handle:
                json.dump(self._data, handle, ensure_ascii=False, indent=2)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temp, self.path)

    def get(self, key, default=None):
        with self._lock:
            return self._data.get(key, DEFAULTS.get(key, default))

    def set(self, key, value, save=True):
        with self._lock:
            self._data[key] = value
            if save:
                self.save()

    def update(self, values, save=True):
        with self._lock:
            self._data.update(values)
            if save:
                self.save()

    def as_dict(self):
        with self._lock:
            return dict(self._data)


def ensure_directories():
    for path in (CACHE_DIR, LOG_DIR, CACHE_DIR / "covers", CACHE_DIR / "fonts",
                 CACHE_DIR / "images", CACHE_DIR / "content"):
        path.mkdir(parents=True, exist_ok=True)
