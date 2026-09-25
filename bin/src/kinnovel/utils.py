import hashlib
import json
import os
import shutil
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path


def atomic_write(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    mode = "wb" if isinstance(data, (bytes, bytearray)) else "w"
    kwargs = {} if "b" in mode else {"encoding": "utf-8"}
    with temp.open(mode, **kwargs) as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temp, path)


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
