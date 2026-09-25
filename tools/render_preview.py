#!/usr/bin/env python3
"""Render page previews on a desktop without touching /dev/fb0."""

import os
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))

from PIL import Image, ImageDraw, ImageFont

from kinnovel.pages import browse, home, reader
from kinnovel.reader import ReaderDocument
from kinnovel.ui import ImageCache, PageContext


class FakeOutput:
    def __init__(self, width, height):
        self.resolution = (width, height)
        self.last = None

    def show(self, image, **_kwargs):
        self.last = image.copy()
        return 1


class FakeScreen:
    def __init__(self, width=1264, height=1680):
        self.output = FakeOutput(width, height)


class FakeConfig:
    def __init__(self):
        self.values = {
            "night_mode": False,
            "font_size": 34,
            "line_spacing": 1.42,
            "reader_margin": 34,
            "first_line_indent": True,
            "page_flash": False,
            "strict_tls": False,
            "font_path": str(_font_path()),
        }

    def get(self, key, default=None):
        return self.values.get(key, default)


class FakeApi:
    server = "https://api.lightnovel.life"
    user = None


class FakeApp:
    def __init__(self):
        self.screen = FakeScreen()
        self.config = FakeConfig()
        self.api = FakeApi()
        self.images = ImageCache()
        sizes = {"hero": 82, "title": 50, "body": 38, "small": 31, "tiny": 25}
        self.fonts = {key: ImageFont.truetype(str(_font_path()), size)
                      for key, size in sizes.items()}
        self.stopped = False

    def stop(self):
        self.stopped = True


def _font_path():
    candidates = [
        Path(r"C:\Windows\Fonts\msyh.ttc"),
        Path(r"C:\Windows\Fonts\simhei.ttf"),
        Path("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
        Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
    ]
    for path in candidates:
        if path.exists():
            return path
    return Path("")


def render_home():
    app = FakeApp()
    context = PageContext(app)
    context.register("home", home)
    context.home()
    return app.screen.output.last


def render_browse():
    app = FakeApp()
    context = PageContext(app)
    context.register("browse", browse)
    browse.STATE.update({
        "items": [
            {"Id": 1, "Title": "測試小說：長標題排版是否穩定", "UserName": "作者"},
            {"Id": 2, "Title": "第二本測試書", "UserName": "作者"},
            {"Id": 3, "Title": "第三本測試書", "UserName": "作者"},
        ],
        "page": 1,
        "total_pages": 12,
        "loaded": True,
        "loading": False,
        "categories": [],
        "category": 0,
    })
    context.page_name = "browse"
    context.params = {}
    context.show()
    return app.screen.output.last


def render_reader():
    app = FakeApp()
    context = PageContext(app)
    chapter = {
        "BookId": 1,
        "BookName": "测试小说",
        "Id": 1,
        "Title": "第一章 字体与排版",
        "SortNum": 1,
        "Chapters": ["第一章 字体与排版", "第二章 分页测试"],
        "Content": (
            "<h1>第一章 字体与排版</h1>"
            "<p>这是用于验证 Kindle 灰阶渲染、中文断行、首行缩进和自动分页的测试文本。"
            "LightNovelShelf 的正文会配合章节专用字体加载，正文文本必须使用该字体测量和绘制。</p>"
            "<p>“字体混淆”本质上通常不是字符加密，而是将正文码位映射到字体 cmap 中对应汉字字形。"
            "只要使用章节 Font 渲染，页面显示就正确。</p>"
            "<p>第二段继续填充内容，用于确认每页行数、页间距以及底部导航不会相互覆盖。"
            "在本机使用微软雅黑代替 Kindle 系统字体，不改变排版算法。</p>"
        ),
    }
    document = ReaderDocument(chapter, app.api.server, app.config.get("font_path"), app.config)
    image = Image.new("L", (8, 8), 255)
    document.prepare(ImageDraw.Draw(image), app.screen.output.resolution[0],
                     app.screen.output.resolution[1] - 104)
    reader.STATE.update({
        "book_id": 1,
        "sort_num": 1,
        "data": {"Chapter": chapter, "ReadPosition": None},
        "doc": document,
        "page": 0,
        "loading": False,
    })
    context.register("reader", reader)
    context.page_name = "reader"
    context.params = {"book_id": 1, "sort_num": 1}
    context.show()
    return app.screen.output.last


def main():
    output = ROOT / "build" / "previews"
    output.mkdir(parents=True, exist_ok=True)
    pages = {
        "home": render_home(),
        "browse": render_browse(),
        "reader": render_reader(),
    }
    for name, image in pages.items():
        path = output / (name + ".png")
        image.save(path)
        print(path)


if __name__ == "__main__":
    main()
