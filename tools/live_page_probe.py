#!/usr/bin/env python3
"""用真实 API 数据离线渲染 阅读历史/最近/排行 页面,验证空值不再导致闪退。

请求频率由 ApiClient 内置的 9/5.5s 限流器保护,不会打印账号或 token。
"""

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))
sys.path.insert(0, str(ROOT / "bin" / "vendor"))

from PIL import Image, ImageDraw, ImageFont  # noqa: E402

from kinnovel.api import ApiClient  # noqa: E402
from kinnovel.config import Config  # noqa: E402
from kinnovel.pages import browse, history, rank  # noqa: E402
from kinnovel.ui import Canvas, PageContext, Theme  # noqa: E402


SIZE = (1236, 1648)


class Output:
    resolution = SIZE
    last = None

    def show(self, image, **_kwargs):
        self.last = image
        return 1


class Screen:
    def __init__(self):
        self.output = Output()


class App:
    def __init__(self, config, api):
        self.screen = Screen()
        self.config = config
        self.api = api
        self.images = None
        self.fonts = {}
        self.power = None

    def log(self, message):
        print("[app] %s" % message)


def load_fonts(config):
    path = config.get("font_path") or "/usr/java/lib/fonts/STHeitiMedium.ttf"
    fonts = {}
    for key, size in (("hero", 70), ("title", 44), ("body", 34),
                      ("small", 28), ("tiny", 23)):
        fonts[key] = ImageFont.truetype(path, size)
    return fonts


def render_page(context, module, name):
    image = Image.new("L", SIZE, 255)
    canvas = Canvas(image, context.fonts, Theme(False))
    module.render(context, canvas)
    return image


def counts(items):
    valid = sum(1 for item in items if isinstance(item, dict))
    return len(items), len(items) - valid


def main():
    config = Config()
    email = str(config.get("account_email") or "").strip()
    password = str(config.get("account_password") or "")
    if not email or not password:
        print("config.json 未配置账号")
        return 2
    api = ApiClient(config)
    api.login(email, password)

    app = App(config, api)
    app.fonts = load_fonts(config)
    context = PageContext(app)
    context.register("history", history)
    context.register("browse", browse)
    context.register("rank", rank)

    # 1. 阅读历史
    raw = api.get_read_history()
    novel = raw.get("Novel") if isinstance(raw, dict) else []
    print("[history] raw=%s novel=%s invalid=%s" % (
        type(raw).__name__, len(novel or []),
        sum(1 for item in (novel or []) if not isinstance(item, int))))
    ids = [int(item) for item in (novel or []) if isinstance(item, int)][:24]
    if ids:
        print("[history] sample ids=%s" % ids[:5])
        raw_books = api.hub.invoke("GetBookListByIds", {"Ids": ids})
        if isinstance(raw_books, dict):
            raw_data = raw_books.get("Data")
            kinds = {}
            if isinstance(raw_data, list):
                for entry in raw_data:
                    kinds[type(entry).__name__] = kinds.get(type(entry).__name__, 0) + 1
            print("[history] raw keys=%s Data=%s kinds=%s" % (
                sorted(raw_books.keys()),
                len(raw_data) if isinstance(raw_data, list) else raw_data,
                kinds))
        else:
            print("[history] raw type=%s len=%s" % (
                type(raw_books).__name__,
                len(raw_books) if isinstance(raw_books, list) else "n/a"))
        try:
            first = api.get_book_info(ids[0])
            print("[history] first id title=%r" % (
                ((first or {}).get("Book") or {}).get("Title"),))
        except Exception as exc:  # noqa: BLE001
            print("[history] first id lookup failed: %s" % exc)
        books = api.get_book_list_by_ids(ids, "Novel")
        items = books.get("Data") if isinstance(books, dict) else books
        print("[history] books total/invalid=%s" % (counts(items or []),))
        history.STATE.update({"items": items or [], "loading": False,
                              "loaded": True, "error": "", "page": 0})
    else:
        history.STATE.update({"items": [None, "bad"], "loading": False,
                              "loaded": True, "error": "", "page": 0})
    context.page_name = "history"
    render_page(context, history, "history")
    print("[history] 离线渲染 OK")

    # 2. 最近/分类
    listing = api.get_book_list(page=1, size=8)
    items = listing.get("Data") if isinstance(listing, dict) else []
    print("[browse] items total/invalid=%s" % (counts(items or []),))
    browse.STATE.update({"items": items or [], "page": 1, "total_pages": 1,
                         "loading": False, "loaded": True})
    context.page_name = "browse"
    render_page(context, browse, "browse")
    print("[browse] 离线渲染 OK")

    # 3. 排行榜
    ranked = api.get_rank(1)
    if isinstance(ranked, dict):
        items = ranked.get("Data")
    else:
        items = ranked
    print("[rank] items total/invalid=%s" % (counts(items or []),))
    rank.STATE.update({"items": items or [], "page": 1, "loading": False,
                       "loaded": True})
    context.page_name = "rank"
    render_page(context, rank, "rank")
    print("[rank] 离线渲染 OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
