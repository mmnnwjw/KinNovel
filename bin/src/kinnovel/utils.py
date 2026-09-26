import glob
import hashlib
import json
import os
import shutil
import tempfile
import threading
import time
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path


_ATOMIC_WRITE_LOCKS = {}
_ATOMIC_WRITE_LOCKS_GUARD = threading.Lock()


def _atomic_write_lock(path):
    key = os.path.abspath(os.fspath(path))
    with _ATOMIC_WRITE_LOCKS_GUARD:
        lock = _ATOMIC_WRITE_LOCKS.get(key)
        if lock is None:
            lock = threading.Lock()
            _ATOMIC_WRITE_LOCKS[key] = lock
        return lock


def atomic_write(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    mode = "wb" if isinstance(data, (bytes, bytearray)) else "w"
    kwargs = {} if "b" in mode else {"encoding": "utf-8"}
    with _atomic_write_lock(path):
        descriptor = None
        temp = None
        try:
            descriptor, temp_name = tempfile.mkstemp(
                dir=path.parent, prefix=path.name, suffix=".tmp")
            temp = Path(temp_name)
            handle = os.fdopen(descriptor, mode, **kwargs)
            descriptor = None
            with handle:
                handle.write(data)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temp, path)
            temp = None
        except BaseException:
            if descriptor is not None:
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            if temp is not None:
                try:
                    temp.unlink()
                except OSError:
                    pass
            raise


def read_json(path, default=None):
    try:
        with Path(path).open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError):
        return default


def sha256_text(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def stable_cache_name(value, suffix=""):
    digest = hashlib.sha256(value.encode("utf-8")).hexdigest()
    return digest + suffix


def absolute_url(base, value):
    if not value:
        return ""
    if value.startswith(("http://", "https://")):
        return value
    return urllib.parse.urljoin(base.rstrip("/") + "/", value.lstrip("/"))


def format_time(value):
    if not value:
        return "未知"
    text = str(value)
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        return parsed.astimezone().strftime("%Y-%m-%d %H:%M")
    except ValueError:
        return text[:16].replace("T", " ")


def cache_size(path):
    total = 0
    path = Path(path)
    if not path.exists():
        return 0
    for item in path.rglob("*"):
        try:
            if item.is_file():
                total += item.stat().st_size
        except OSError:
            pass
    return total


def prune_cache(path, limit_bytes):
    path = Path(path)
    if not path.exists():
        return 0
    files = []
    total = 0
    for item in path.rglob("*"):
        try:
            if item.is_file():
                stat = item.stat()
                files.append((stat.st_mtime, stat.st_size, item))
                total += stat.st_size
        except OSError:
            pass
    if total <= limit_bytes:
        return 0
    files.sort()
    removed = 0
    for _, size, item in files:
        if total <= limit_bytes:
            break
        try:
            item.unlink()
            removed += size
            total -= size
        except OSError:
            pass
    for directory in sorted((p for p in path.rglob("*") if p.is_dir()), reverse=True):
        try:
            directory.rmdir()
        except OSError:
            pass
    return removed


def clear_cache(path):
    path = Path(path)
    if not path.exists():
        return
    for item in path.iterdir():
        try:
            if item.is_dir() and not item.is_symlink():
                shutil.rmtree(item, ignore_errors=True)
            else:
                item.unlink()
        except OSError:
            pass


_BATTERY_CACHE = {"at": 0.0, "value": None}


def battery_level():
    now = time.monotonic()
    if now - _BATTERY_CACHE["at"] < 60:
        return _BATTERY_CACHE["value"]
    value = None
    for path in sorted(glob.glob("/sys/class/power_supply/*/capacity")):
        try:
            level = int(Path(path).read_text(encoding="ascii").strip())
        except (OSError, ValueError):
            continue
        if 0 <= level <= 100:
            value = level
            break
    _BATTERY_CACHE["at"] = now
    _BATTERY_CACHE["value"] = value
    return value
