import unittest

from PIL import ImageFont

from kinnovel.pages import account, announcements, book, browse, history, home, rank, reader, search, series, settings, shelf
from kinnovel.ui import ImageCache, PageContext
from page import keyboard


class Output:
    def __init__(self):
        self.resolution = (1072, 1448)

    def show(self, image, **_kwargs):
        return None


class Screen:
    def __init__(self):
        self.output = Output()


class Config:
    def __init__(self):
        self.values = {
            "night_mode": False,
            "font_size": 34,
            "line_spacing": 1.42,
            "reader_margin": 30,
            "first_line_indent": True,
            "page_flash": False,
            "strict_tls": False,
            "font_path": "C:/Windows/Fonts/simhei.ttf",
            "convert": None,
            "ignore_japanese": False,
            "ignore_ai": False,
        }

    def get(self, key, default=None):
        return self.values.get(key, default)


class Api:
    server = "https://api.lightnovel.life"
    user = {"Id": 1, "UserName": "测试用户", "Growth": {}}


class App:
    def __init__(self):
        self.screen = Screen()
        self.config = Config()
        self.api = Api()
        self.images = ImageCache()
        self.fonts = {
            "hero": ImageFont.truetype(self.config.get("font_path"), 70),
            "title": ImageFont.truetype(self.config.get("font_path"), 44),
            "body": ImageFont.truetype(self.config.get("font_path"), 34),
            "small": ImageFont.truetype(self.config.get("font_path"), 28),
            "tiny": ImageFont.truetype(self.config.get("font_path"), 23),
            96: ImageFont.truetype(self.config.get("font_path"), 96),
            48: ImageFont.truetype(self.config.get("font_path"), 48),
            36: ImageFont.truetype(self.config.get("font_path"), 36),
            28: ImageFont.truetype(self.config.get("font_path"), 28),
        }


class PageSmokeTests(unittest.TestCase):
    def setUp(self):
        self.context = PageContext(App())
        for name, module in {
            "home": home,
            "browse": browse,
            "rank": rank,
            "search": search,
            "book": book,
            "history": history,
            "reader": reader,
            "series": series,
            "shelf": shelf,
            "account": account,
            "settings": settings,
            "announcements": announcements,
        }.items():
            self.context.register(name, module)

    def test_core_pages_render_at_paperwhite_resolution(self):
        book.STATE["data"] = {
            "Book": {
                "Id": 1,
                "Type": "Novel",
                "Title": "测试书",
                "Author": "作者",
                "Introduction": "简介文本",
                "Chapters": [{"Id": 10, "SortNum": 1, "Title": "第一章"}],
            },
            "ReadPosition": None,
        }
        browse.STATE.update({"items": [{"Id": 1, "Title": "测试书"}], "loaded": True})
        for page in ("home", "browse", "rank", "search", "book", "history",
                     "reader", "series", "shelf", "account", "settings",
                     "announcements"):
            self.context.page_name = page
            image = self.context.render()
            self.assertEqual(image.size, (1072, 1448), page)

    def test_modal_render_populates_hit_rects(self):
        self.context.page_name = "home"
        self.context.confirm("确认操作", lambda: None)
        self.context.render()
        self.assertEqual(len(self.context.modal["rects"]), 2)

    def test_keyboard_renders_with_device_font_map(self):
        app = App()
        keyboard.start(app.screen, app.fonts, capabilities=("en", "numsym"),
                       hint="测试", owner="home")
        image = keyboard.render(app.screen, app.fonts)
        self.assertEqual(image.size, (1072, 1448))

    def test_header_left_uses_back_stack(self):
        self.context.stack = [("home", {})]
        self.context.page_name = "browse"
        self.context.render()
        self.context.handle({"gesture": "tap", "x-pixel": 10, "y-pixel": 10})
        self.assertEqual(self.context.page_name, "home")

    def test_long_gesture_reaches_page(self):
        class LongPage:
            @staticmethod
            def render(context, canvas):
                canvas.header("长按")

            @staticmethod
            def handle(data, context):
                return data.get("gesture")

        self.context.register("long", LongPage)
        self.context.page_name = "long"
        self.context.render()
        result = self.context.handle({"gesture": "long", "x-pixel": 10, "y-pixel": 200})
        self.assertEqual(result, "long")


if __name__ == "__main__":
    unittest.main()
