#!/usr/bin/env python3
"""Render README previews from the live account with a ten-second request timer."""

import sys
import time
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))

from kinnovel.api import ApiClient, SessionStore
from kinnovel.config import Config
from kinnovel.pages import book, home, rank, reader
from kinnovel.reader import ReaderDocument, ensure_font
from kinnovel.ui import ImageCache, PageContext
from kinnovel.utils import sha256_text


DELAY_SECONDS = 10
BOOK_ID = 1854
CHAPTER_INDEX = 4
SIZE = (1264, 1680)


class PreviewOutput:
    def __init__(self):
        self.resolution = SIZE
        self.last = None

    def show(self, image, **_kwargs):
        self.last = image.copy()
        return 1


class PreviewScreen:
    def __init__(self):
        self.output = PreviewOutput()


class PreviewConfig:
    def __init__(self, base):
        self._base = base
        self._font_path = _local_font()

    def get(self, key, default=None):
        if key == "font_path":
            return self._font_path
        return self._base.get(key, default)


class PreviewApp:
    def __init__(self, api):
        self.screen = PreviewScreen()
        self.config = PreviewConfig(Config())
        self.api = api
        self.images = ImageCache(maximum=8)
        # 与 app.load_fonts 相同的分辨率缩放逻辑
        scale = max(0.75, min(1.15, min(SIZE[0] / 1072, SIZE[1] / 1448)))
        self.fonts = {
            key: ImageFont.truetype(str(_local_font()), max(12, int(round(size * scale))))
            for key, size in (
                ("hero", 82), ("title", 50), ("body", 38), ("small", 31), ("tiny", 25)
            )
        }

    def stop(self):
        return None

    def log(self, _message):
        return None


def _local_font():
    candidates = [
        Path(r"C:\Windows\Fonts\msyh.ttc"),
        Path(r"C:\Windows\Fonts\simhei.ttf"),
        Path("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
        Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    raise RuntimeError("no CJK font found")


def wait_timer(label):
    print("[timer] %s complete; waiting %ss" % (label, DELAY_SECONDS), flush=True)
    time.sleep(DELAY_SECONDS)


def request(name, operation, rate_limit=True):
    print("[request]", name, flush=True)
    started = time.monotonic()
    result = operation()
    print("[ok] %s in %.2fs" % (name, time.monotonic() - started), flush=True)
    if rate_limit:
        wait_timer(name)
    return result


def load_credentials():
    lines = [
        line.strip()
        for line in (ROOT / "TESTACCOUNT.txt").read_text(
            encoding="utf-8-sig").splitlines()
        if line.strip()
    ]
    if len(lines) < 2 or "@" not in lines[0]:
        raise RuntimeError("TESTACCOUNT.txt is invalid")
    return lines[0], lines[1]


def make_context(api):
    app = PreviewApp(api)
    context = PageContext(app)
    context.register("home", home)
    context.register("rank", rank)
    context.register("book", book)
    context.register("reader", reader)
    return context


def render_home(api):
    context = make_context(api)
    context.page_name = "home"
    context.params = {}
    context.show()
    return context.render()


def render_rank(api, items):
    state = rank.STATE
    state.update({
        "kind": "daily", "items": items, "page": 1, "rects": {},
        "loading": False, "loaded": True, "generation": 0,
    })
    context = make_context(api)
    context.page_name = "rank"
    context.params = {}
    context.show()
    return context.render()


def render_book(api, data):
    state = book.STATE
    state.update({
        "book_id": BOOK_ID, "data": data, "chapter_page": 0,
        "bound": False, "rects": {}, "loading": False, "generation": 0,
    })
    context = make_context(api)
    context.page_name = "book"
    context.params = {"book_id": BOOK_ID}
    context.show()
    return context.render()


def render_reader(api, response, document):
    state = reader.STATE
    state.update({
        "book_id": BOOK_ID,
        "sort_num": int((response.get("Chapter") or {}).get("SortNum") or 1),
        "data": response,
        "doc": document,
        "page": 0,
        "rects": {},
        "loading": False,
        "last_saved": -1,
        "catalog_page": 0,
        "signature": None,
        "image_pending": set(),
        "image_rects": {},
        "fullscreen_image": None,
        "last_turn_at": 0.0,
    })
    context = make_context(api)
    context.page_name = "reader"
    context.params = {"book_id": BOOK_ID, "sort_num": state["sort_num"]}
    context.show()
    return context.render()


def main():
    email, password = load_credentials()
    session_path = ROOT / "build" / "readme-preview-session.json"
    try:
        session_path.unlink()
    except OSError:
        pass
    config = Config()
    api = ApiClient(config, SessionStore(session_path))
    try:
        credentials = request("login", lambda: api._http("/api/user/login", {
            "email": email,
            "password": sha256_text(password),
        }))
        api._store_credentials(credentials)
        api.session.set_many({"TokenUpdatedAt": time.time() + 3600})
        user = request("GetMyInfo", api.get_my_info)
        display_user = dict(user or {})
        display_user["UserName"] = "已登录"
        display_user["Email"] = ""
        api.session.set_many({"User": display_user})

        rank_items = request("GetRank(daily)", lambda: api.get_rank(1))
        book_info = request("GetBookInfo(1854)", lambda: api.get_book_info(BOOK_ID))
        chapters = ((book_info.get("Book") or {}).get("Chapters") or [])
        if len(chapters) <= CHAPTER_INDEX:
            raise RuntimeError("book 1854 has fewer than five chapters")
        chapter = chapters[CHAPTER_INDEX]
        sort_num = int(chapter.get("SortNum") or CHAPTER_INDEX + 1)
        novel = request(
            "GetNovelContent(1854, chapter 5)",
            lambda: api.get_novel_content(BOOK_ID, sort_num),
        )

        cover_url = (book_info.get("Book") or {}).get("Cover") or ""
        if cover_url:
            cache = ImageCache(maximum=4)
            if cache.get(cover_url) is None:
                request("download book cover",
                        lambda: cache.prefetch(cover_url, strict_tls=True))

        chapter_data = novel.get("Chapter") or {}
        font_url = chapter_data.get("Font") or ""
        font_path = ""
        if font_url:
            font_path = request(
                "download chapter font",
                lambda: ensure_font(font_url, api.server, strict_tls=True),
            )
            if font_path:
                print("[font] chapter font ready", flush=True)
            else:
                print("[font] chapter font unavailable; system fallback", flush=True)

        reader_config = PreviewConfig(config)
        document = ReaderDocument(
            chapter_data,
            api.server,
            reader_config.get("font_path"),
            reader_config,
        )
        image = Image.new("L", (8, 8), 255)
        top = max(72, int(SIZE[1] * 0.085))
        document.prepare(
            ImageDraw.Draw(image),
            SIZE[0],
            SIZE[1] - top - 92,
        )
        if document.page_count <= 0:
            raise RuntimeError("reader produced no pages")
        print(
            "[reader] pages=%s body_font=%s fallback_used=%s" % (
                document.page_count,
                bool(getattr(document.body_font, "path", "")),
                not document.font_resolver.custom_font_loaded,
            ),
            flush=True,
        )

        output = ROOT / "docs" / "images" / "readme"
        output.mkdir(parents=True, exist_ok=True)
        pages = {
            "home.png": render_home(api),
            "rank.png": render_rank(api, rank_items),
            "book-1854.png": render_book(api, book_info),
            "reader-1854-chapter-5.png": render_reader(api, novel, document),
        }
        for name, page in pages.items():
            path = output / name
            page.save(path, optimize=True)
            print("[saved] %s %sx%s %s bytes" % (
                path, page.width, page.height, path.stat().st_size), flush=True)
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
