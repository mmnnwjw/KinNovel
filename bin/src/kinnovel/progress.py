"""Session-scoped reading progress cache.

Progress is uploaded to the server only when the reader is left. The book
detail page must show "continue reading" immediately after returning, so the
reader records its latest local position here on every page turn. The cache is
cleared when the app exits; the next run reads the server position again.
"""

import threading
import time


_LOCK = threading.RLock()
_SESSION = {}


def record(book_id, sort_num, page, path="", offset=0, page_count=None):
    try:
        book_id = int(book_id)
        sort_num = int(sort_num)
    except (TypeError, ValueError):
        return
    with _LOCK:
        _SESSION[book_id] = {
            "book_id": book_id,
            "sort_num": sort_num,
            "page": max(0, int(page or 0)),
            "path": str(path or ""),
            "offset": max(0, int(offset or 0)),
            "page_count": int(page_count) if page_count else None,
            "updated_at": time.monotonic(),
        }


def get(book_id):
    try:
        key = int(book_id)
    except (TypeError, ValueError):
        return None
    with _LOCK:
        value = _SESSION.get(key)
        return dict(value) if value else None


def clear(book_id=None):
    with _LOCK:
        if book_id is None:
            _SESSION.clear()
            return
        try:
            key = int(book_id)
        except (TypeError, ValueError):
            return
        _SESSION.pop(key, None)
