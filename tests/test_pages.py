import time
import unittest

from PIL import Image, ImageFont

from kinnovel.pages import account, announcements, book, browse, history, home, rank, reader, settings, shelf
from kinnovel.ui import ImageCache, PageContext


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
            "font_size": 48,
            "line_spacing": 1.42,
            "reader_margin": 30,
            "first_line_indent": True,
            "page_flash": False,
            "strict_tls": False,
            "font_path": "C:/Windows/Fonts/simhei.ttf",
            "convert": None,
            "ignore_japanese": False,
            "ignore_ai": False,
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

    def log(self, _message):
        return None


class PageSmokeTests(unittest.TestCase):
    def setUp(self):
        self.context = PageContext(App())
        for name, module in {
            "home": home,
            "browse": browse,
            "rank": rank,
            "book": book,
            "history": history,
            "reader": reader,
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
        for page in ("home", "browse", "rank", "book", "history",
                     "reader", "shelf", "account", "settings",
                     "announcements"):
            self.context.page_name = page
            image = self.context.render()
            self.assertEqual(image.size, (1072, 1448), page)

    def test_modal_render_populates_hit_rects(self):
        self.context.page_name = "home"
        self.context.confirm("确认操作", lambda: None)
        self.context.render()
        self.assertEqual(len(self.context.modal["rects"]), 2)

    def test_settings_stepper_rects_are_visible_and_clickable(self):
        self.context.page_name = "settings"
        self.context.render()
        for key in (("font_down", 0), ("font_up", 0),
                    ("spacing_down", 0), ("spacing_up", 0)):
            self.assertIn(key, settings.STATE["rects"])

    def test_browse_uses_six_items_per_page(self):
        browse.STATE.update({"page": 1, "total_pages": 2, "items": []})
        self.context.page_name = "browse"
        self.context.render()
        item_rects = [key for key in browse.STATE["rects"] if key[0] == "item"]
        nav_rects = [key for key in browse.STATE["rects"] if key[0] in ("prev", "count", "next")]
        self.assertGreater(len(item_rects), 0)
        self.assertEqual(len(nav_rects), 3)

    def test_rank_has_bottom_pager(self):
        rank.STATE.update({"items": [{"Id": 1, "Title": "A"}], "page": 1,
                           "total_pages": 2, "loaded": True})
        self.context.page_name = "rank"
        self.context.render()
        self.assertIn(("prev", 0), rank.STATE["rects"])
        self.assertIn(("count", 0), rank.STATE["rects"])
        self.assertIn(("next", 0), rank.STATE["rects"])
        self.assertIsNotNone(self.context._header_state)

    def test_browse_next_page_replaces_items(self):
        calls = []
        self.context.api.get_book_list = lambda **kwargs: (
            calls.append(kwargs) or {
                "Page": kwargs["page"],
                "TotalPages": 3,
                "Data": [{"Id": kwargs["page"], "Title": "Page %s" % kwargs["page"]}],
            }
        )
        browse.STATE.update({
            "items": [{"Id": 1, "Title": "Page 1"}],
            "page": 1,
            "total_pages": 3,
            "loading": False,
            "loaded": True,
            "categories": [],
            "category": 0,
        })
        self.context.page_name = "browse"
        self.context.params = {}
        self.context.render()
        next_rect = browse.STATE["rects"][("next", 0)]
        self.context.handle({
            "gesture": "tap",
            "x-pixel": next_rect[0] + 4,
            "y-pixel": next_rect[1] + 4,
        })
        deadline = time.monotonic() + 1
        while not calls and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(calls[0]["page"], 2)
        deadline = time.monotonic() + 1
        while browse.STATE["page"] != 2 and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(browse.STATE["page"], 2)
        self.assertEqual(browse.STATE["items"][0]["Title"], "Page 2")

    def test_home_order_can_hide_and_reorder(self):
        self.context.config.values["home_order"] = {
            "rank": 0,
            "shelf": 1,
            "browse": -1,
            "history": -1,
            "account": -1,
            "settings": -1,
            "about": -1,
            "exit": -1,
            "announcements": -1,
            "notifications": -1,
            "shop": -1,
        }
        self.assertEqual(home._items(self.context), [
            ("rank", "排行榜"),
            ("shelf", "书架"),
        ])

    def test_previous_chapter_starts_at_last_page(self):
        reader.STATE["data"] = {"Chapter": {"Chapters": ["一", "二", "三"]}}
        reader.STATE["sort_num"] = 2

        class ReplaceContext:
            def __init__(self):
                self.call = None

            def replace(self, page, **params):
                self.call = (page, params)

            def toast(self, _message):
                pass

        replace_context = ReplaceContext()
        reader._change_chapter(replace_context, -1, at_last=True)
        self.assertEqual(replace_context.call[1]["sort_num"], 1)
        self.assertTrue(replace_context.call[1]["at_last"])

    def test_reader_image_preview_toggles(self):
        reader.STATE["fullscreen_image"] = None
        reader.STATE["image_rects"] = {
            ("p", 0): (100, 100, 200, 200, "image-url"),
        }
        self.context.images._memory["image-url"] = Image.new("L", (200, 200), 255)
        self.context.page_name = "reader"
        self.context.params = {}
        self.context.handle({
            "gesture": "tap", "x-pixel": 150, "y-pixel": 150,
        })
        self.assertEqual(reader.STATE["fullscreen_image"], "image-url")
        self.context.handle({
            "gesture": "tap", "x-pixel": 150, "y-pixel": 150,
        })
        self.assertIsNone(reader.STATE["fullscreen_image"])

    def test_header_left_uses_back_stack(self):
        self.context.stack = [("home", {})]
        self.context.page_name = "browse"
        self.context.render()
        self.context.handle({"gesture": "tap", "x-pixel": 10, "y-pixel": 10})
        self.assertEqual(self.context.page_name, "home")

    def test_back_is_debounced(self):
        self.context.stack = [("home", {})]
        self.context.page_name = "browse"
        self.context._last_back_at = time.monotonic()
        self.context.back()
        self.assertEqual(self.context.page_name, "browse")

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
