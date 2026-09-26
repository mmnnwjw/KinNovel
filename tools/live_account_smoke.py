#!/usr/bin/env python3
"""Low-rate authenticated smoke test using TESTACCOUNT.txt.

Every network step is separated by ten seconds. The script intentionally keeps
list sizes small and never prints credentials or tokens.
"""

import sys
import time
from pathlib import Path

from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))

from kinnovel.api import ApiClient, SessionStore
from kinnovel.config import Config
from kinnovel.reader import ReaderDocument, ensure_font
from kinnovel.utils import sha256_text


DELAY_SECONDS = 10


class SmokeConfig:
    def __init__(self, base):
        self._base = base
        self._values = {
            "font_size": 34,
            "line_spacing": 1.42,
            "reader_margin": 34,
            "first_line_indent": True,
            "strict_tls": True,
        }

    def get(self, key, default=None):
        if key == "api_server":
            return self._base.get("api_server", default)
        if key == "font_path":
            return self._base.get("font_path", default)
        return self._values.get(key, self._base.get(key, default))


def credentials():
    path = ROOT / "TESTACCOUNT.txt"
    lines = [line.strip() for line in path.read_text(encoding="utf-8-sig").splitlines()
             if line.strip()]
    if len(lines) < 2 or "@" not in lines[0]:
        raise RuntimeError("TESTACCOUNT.txt must contain email on line 1 and password on line 2")
    return lines[0], lines[1]


def wait_timer(label):
    print("[timer] %s complete; waiting %ss" % (label, DELAY_SECONDS), flush=True)
    time.sleep(DELAY_SECONDS)


def call(name, operation):
    print("[request] " + name, flush=True)
    started = time.monotonic()
    try:
        result = operation()
    except Exception as exc:
        print("[fail] %s: %s: %s" % (name, type(exc).__name__, exc), flush=True)
        raise
    elapsed = time.monotonic() - started
    print("[ok] %s in %.2fs" % (name, elapsed), flush=True)
    wait_timer(name)
    return result


def ascii_preview(value, length=80):
    return ascii(str(value or "")[:length])


def main():
    email, password = credentials()
    session_path = ROOT / "build" / "live-session.json"
    try:
        session_path.unlink()
    except OSError:
        pass

    base_config = Config()
    config = SmokeConfig(base_config)
    api = ApiClient(config, SessionStore(session_path))
    created_preview = ROOT / "build" / "live-reader.png"

    try:
        credentials_response = call(
            "POST /api/user/login",
            lambda: api._http("/api/user/login", {
                "email": email,
                "password": sha256_text(password),
            }),
        )
        api._store_credentials(credentials_response)
        # Keep the authenticated WebSocket alive without hidden refresh requests.
        api.session.set_many({"TokenUpdatedAt": time.time() + 3600})

        current_user = call("GetMyInfo", lambda: api.get_my_info())
        api.session.set_many({"User": current_user})
        print("[summary] GetMyInfo user_id=%s name=%s" % (
            current_user.get("Id"), ascii_preview(current_user.get("UserName"), 30)), flush=True)

        shelf = call("GetBookShelf", api.get_book_shelf)
        shelf_count = len(shelf.get("data") or [])
        print("[summary] shelf items=%s" % shelf_count, flush=True)

        history = call("GetReadHistory", api.get_read_history)
        print("[summary] history novel=%s comic=%s" % (
            len(history.get("Novel") or []), len(history.get("Comic") or [])), flush=True)

        latest = call(
            "GetBookList",
            lambda: api.get_book_list(page=1, size=2),
        )
        latest_items = latest.get("Data") or []
        print("[summary] latest returned=%s total=%s" % (
            len(latest_items), latest.get("Total")), flush=True)
        if not latest_items:
            raise RuntimeError("GetBookList returned no books")
        book_id = int(latest_items[0].get("Id"))
        print("[summary] selected book_id=%s title=%s" % (
            book_id, ascii_preview(latest_items[0].get("Title"), 50)), flush=True)

        book_info = call("GetBookInfo", lambda: api.get_book_info(book_id))
        book = book_info.get("Book") or {}
        chapters = book.get("Chapters") or []
        print("[summary] book chapters=%s type=%s" % (
            len(chapters), book.get("Type")), flush=True)
        if not chapters:
            raise RuntimeError("selected book has no chapters")
        # Chapter 1 is often only a cover page. Use the next chapter to exercise
        # real text and the chapter-specific font without requesting a batch.
        selected_index = min(1, len(chapters) - 1)
        first_chapter = chapters[selected_index]
        sort_num = int(first_chapter.get("SortNum") or (selected_index + 1))

        novel = call(
            "GetNovelContent",
            lambda: api.get_novel_content(book_id, sort_num),
        )
        chapter = novel.get("Chapter") or {}
        content = chapter.get("Content") or ""
        font_url = chapter.get("Font")
        print("[summary] chapter id=%s chars=%s font=%s sort=%s" % (
            chapter.get("Id"), len(content), bool(font_url), sort_num), flush=True)
        if len(content) > 2_000_000:
            raise RuntimeError("chapter is unexpectedly large; stopping before font/rendering test")

        font_path = ""
        if font_url:
            font_path = call(
                "download chapter font",
                lambda: ensure_font(font_url, api.server, strict_tls=True),
            )
            if font_path:
                path = Path(font_path)
                print("[summary] font cached=%s bytes=%s suffix=%s" % (
                    path.exists(), path.stat().st_size if path.exists() else 0,
                    path.suffix), flush=True)
            else:
                print("[summary] font download failed", flush=True)
        else:
            print("[summary] chapter has no Font field", flush=True)

        document = ReaderDocument(chapter, api.server, config.get("font_path"), config)
        started = time.monotonic()
        document.prepare(ImageDraw.Draw(Image.new("L", (8, 8), 255)), 1072, 1233)
        elapsed = time.monotonic() - started
        print("[summary] reader pages=%s layout=%.3fs body_font=%s" % (
            document.page_count, elapsed,
            bool(getattr(document.body_font, "path", ""))), flush=True)

        image = Image.new("L", (1072, 1448), 255)
        draw = ImageDraw.Draw(image)
        for item in document.pages[0]:
            if item["type"] == "text":
                draw.text((item["x"], item["y"] + 120), item["text"],
                          font=item["font"], fill=0)
        created_preview.parent.mkdir(parents=True, exist_ok=True)
        image.save(created_preview)
        print("[summary] preview=%s" % created_preview, flush=True)

        announcements = call(
            "GetAnnouncementList",
            lambda: api.get_announcement_list(page=1, size=2),
        )
        print("[summary] announcements returned=%s" % len(
            announcements.get("Data") or []), flush=True)

        notifications = call(
            "GetNotifications",
            lambda: api.get_notifications(page=1, size=2),
        )
        print("[summary] notifications returned=%s unread_user_count=%s" % (
            len(notifications.get("Data") or []),
            current_user.get("UnreadNotificationCount")), flush=True)

        print("[done] authenticated smoke test completed", flush=True)
    finally:
        try:
            api.hub.close()
        except Exception:
            pass
        try:
            session_path.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    main()
