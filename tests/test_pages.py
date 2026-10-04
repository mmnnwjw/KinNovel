import time
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from PIL import Image, ImageFont

from kinnovel.pages import account, announcements, book, browse, history, home, rank, reader, series, settings, shelf
from kinnovel.ui import Canvas, ImageCache, PageContext, Theme, height_bucket


FONT_PATH = next((path for path in (
    "C:/Windows/Fonts/simhei.ttf",
    "/usr/java/lib/fonts/STHeitiMedium.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
) if Path(path).exists()), "")


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
            "page_turn_animation": True,
            "strict_tls": False,
            "font_path": FONT_PATH,
            "convert": None,
            "ignore_japanese": False,
            "ignore_ai": False,
            "prefetch_chapters": False,
            "prefetch_reading_target": False,
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

    def set(self, key, value, save=True):
        self.values[key] = value


class Api:
    server = "https://api.lightnovel.life"
    user = {"Id": 1, "UserName": "测试用户", "Growth": {}}

    def save_read_position(self, book_id, chapter_id, xpath):
        return None


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
        if not FONT_PATH:
            self.skipTest("no CJK test font available")
        self.context = PageContext(App())
        for name, module in {
            "home": home,
            "browse": browse,
            "rank": rank,
            "book": book,
            "series": series,
            "history": history,
            "reader": reader,
            "shelf": shelf,
            "account": account,
            "settings": settings,
            "announcements": announcements,
        }.items():
            self.context.register(name, module)
        # 默认关闭阅读指引弹窗，避免干扰其它用例；指引用例自行打开
        self.context.config.values["reader_guide_dismissed"] = True

    def _prime_reader(self, **params):
        class Catalog:
            @staticmethod
            def render(_context, _canvas):
                return None

        self.context.register("catalog", Catalog)
        document = MagicMock()
        document.page_count = 10
        document.pages = [[] for _ in range(10)]
        document.first_anchor_on_page.return_value = ("./p[1]", 0)
        book_id = 1
        sort_num = 2
        reader.STATE.update({
            "book_id": book_id,
            "sort_num": sort_num,
            "data": {"Chapter": {"Title": "测试", "Chapters": ["一", "二"]}},
            "doc": document,
            "page": 5,
            "signature": reader._signature(self.context, book_id, sort_num),
            "loading": False,
        })
        self.context.page_name = "reader"
        self.context.params = {
            "book_id": book_id,
            "sort_num": sort_num,
            **params,
        }
        reader.enter(self.context)

    def test_reader_fresh_intent_is_consumed_before_navigation_round_trip(self):
        self._prime_reader(fresh=True)
        self.assertEqual(reader.STATE["page"], 0)
        self.assertNotIn("fresh", self.context.params)

        reader.STATE["page"] = 5
        self.context.navigate("catalog", book_id=1, sort_num=2)
        self.context.back()
        self.assertEqual(reader.STATE["page"], 5)
        self.assertNotIn("fresh", self.context.params)

    def test_reader_at_last_intent_is_consumed_before_navigation_round_trip(self):
        self._prime_reader(at_last=True)
        self.assertEqual(reader.STATE["page"], 9)
        self.assertNotIn("at_last", self.context.params)

        reader.STATE["page"] = 5
        self.context.navigate("catalog", book_id=1, sort_num=2)
        self.context.back()
        self.assertEqual(reader.STATE["page"], 5)
        self.assertNotIn("at_last", self.context.params)

    def test_book_prefetch_reading_target_requires_setting(self):
        class ImmediateContext:
            config = self.context.config
            api = self.context.api

            @staticmethod
            def run_async(_owner, operation, on_success=None, on_error=None):
                result = operation()
                if on_success:
                    on_success(result)

        book.STATE["book_id"] = 1
        info = {
            "Book": {
                "Chapters": [
                    {"Id": 10, "SortNum": 1},
                    {"Id": 11, "SortNum": 10},
                ],
            },
            "ReadPosition": {"ChapterId": 11},
        }
        context = ImmediateContext()
        with patch("kinnovel.pages.reader.prefetch_chapter") as prefetch:
            book._prefetch_reading_target(context, info)
            prefetch.assert_not_called()
            context.config.values["prefetch_reading_target"] = True
            book._prefetch_reading_target(context, info)
        prefetch.assert_called_once_with(context, 1, 10)
        context.config.values["prefetch_reading_target"] = False

    def test_book_series_button_opens_series_page(self):
        book.STATE.update({
            "book_id": 1,
            "data": {
                "Book": {
                    "Id": 1,
                    "Title": "第一卷",
                    "Chapters": [],
                },
                "SeriesTitle": "测试系列",
                "Series": [
                    {"Id": 1, "Title": "第一卷", "Cover": ""},
                    {"Id": 2, "Title": "第二卷", "Cover": ""},
                ],
            },
            "chapter_page": 0,
            "bound": False,
        })
        self.context.page_name = "book"
        self.context.render()
        rect = book.STATE["rects"][("series", 0)]
        calls = []
        self.context.navigate = lambda name, **params: calls.append((name, params))
        self.context.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        })
        self.assertEqual(calls[0][0], "series")
        self.assertEqual(calls[0][1]["title"], "测试系列")
        self.assertEqual(len(calls[0][1]["books"]), 2)

    def test_book_series_button_renders_on_small_screen(self):
        self.context.screen.output.resolution = (758, 1024)
        book.STATE.update({
            "book_id": 1,
            "data": {
                "Book": {
                    "Id": 1,
                    "Title": "测试系列第一卷",
                    "Author": "作者",
                    "Introduction": "简介",
                    "Chapters": [],
                },
                "SeriesTitle": "测试系列",
                "Series": [
                    {"Id": 1, "Title": "第一卷", "Cover": ""},
                    {"Id": 2, "Title": "第二卷", "Cover": ""},
                ],
            },
            "chapter_page": 0,
            "bound": False,
        })
        self.context.page_name = "book"
        image = self.context.render()
        self.assertEqual(image.size, (758, 1024))
        self.assertIn(("series", 0), book.STATE["rects"])

    def test_series_page_opens_other_book(self):
        series.STATE.update({
            "title": "测试系列",
            "series_name": "测试系列",
            "items": [
                {"Id": 1, "Title": "第一卷", "Cover": ""},
                {"Id": 2, "Title": "第二卷", "Cover": ""},
            ],
            "current_id": 1,
            "page": 0,
            "rects": {},
            "loading": False,
            "loaded": True,
        })
        self.context.page_name = "series"
        self.context.params = {}
        self.context.render()
        rect = series.STATE["rects"][("item", 1)]
        calls = []
        self.context.navigate = lambda name, **params: calls.append((name, params))
        self.context.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        })
        self.assertEqual(calls, [("book", {"book_id": 2})])

    def test_series_page_does_not_prefetch_covers(self):
        self.context.params = {
            "title": "测试系列",
            "series_name": "测试系列",
            "current_id": 1,
            "books": [
                {"Id": 1, "Title": "第一卷", "Cover": "cover-1"},
                {"Id": 2, "Title": "第二卷", "Cover": "cover-2"},
            ],
        }
        with patch.object(self.context, "run_async") as run_async:
            series.enter(self.context)
        run_async.assert_not_called()

    def test_reader_configures_native_swipe_animation(self):
        class Output:
            supports_swipe_animation = True

            def __init__(self):
                self.calls = []

            def set_swipe_direction(self, left):
                self.calls.append(("direction", left))

            def set_swipe_animations(self, enabled):
                self.calls.append(("enabled", enabled))

        class Context:
            config = self.context.config
            screen = type("Screen", (), {"output": Output()})()

        reader._configure_swipe(Context(), 1)
        self.assertEqual(Context.screen.output.calls, [
            ("direction", True),
            ("enabled", True),
        ])

    def test_reader_top_tap_toggles_chrome(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        with patch.object(self.context, "show") as show:
            reader.handle({
                "gesture": "tap",
                "x-pixel": self.context.width // 2,
                "y-pixel": 8,
            }, self.context)
        self.assertFalse(reader.STATE["chrome_visible"])
        show.assert_called_once()

    def test_reader_chrome_toggle_keeps_pagination(self):
        class ImmediateContext:
            config = self.context.config
            api = self.context.api
            width = self.context.width
            height = self.context.height
            show = MagicMock()

            @staticmethod
            def run_async(_owner, operation, on_success=None, on_error=None):
                raise AssertionError("切换控件层不应触发重新排版")

        chapter = {
            "Title": "测试",
            "Font": None,
            "Chapters": ["测试"],
            "Content": "<p>" + ("测试正文。" * 400) + "</p>",
        }
        reader.STATE.update({
            "book_id": 1,
            "sort_num": 1,
            "data": {"Chapter": chapter},
            "chrome_visible": True,
            "layout_generation": 0,
        })
        context = ImmediateContext()
        old_document = reader._prepare_document(context, chapter)
        signature = reader._signature(context, 1, 1)
        reader.STATE.update({
            "doc": old_document,
            "page": 1,
            "signature": signature,
            "last_saved": -1,
        })

        reader._set_chrome_visible(context, False)

        self.assertFalse(reader.STATE["chrome_visible"])
        self.assertIs(reader.STATE["doc"], old_document)
        self.assertEqual(reader.STATE["page"], 1)
        self.assertEqual(reader.STATE["signature"], signature)
        context.show.assert_called_once()

    def test_reader_enters_in_compact_mode(self):
        self._prime_reader()
        self.assertFalse(reader.STATE["chrome_visible"])
        self.context.render()
        self.assertEqual(self.context._header_state["left"], "")

    def test_reader_middle_tap_hides_chrome_without_turning_page(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        reader.STATE["page"] = 5
        reader.handle({
            "gesture": "tap",
            "x-pixel": self.context.width // 2,
            "y-pixel": self.context.height // 2,
        }, self.context)
        self.assertFalse(reader.STATE["chrome_visible"])
        self.assertEqual(reader.STATE["page"], 5)

    def test_catalog_jump_drops_stale_reader_from_stack(self):
        self._prime_reader()
        self.context.navigate("catalog", book_id=1, sort_num=2)
        self.assertEqual(self.context.stack[-1][0], "reader")
        reader.STATE["rects"] = {("catalog", 1): (0, 0, 200, 60)}
        calls = []
        self.context.replace = lambda name, **params: calls.append((name, params))

        reader.handle_catalog(
            {"gesture": "tap", "x-pixel": 20, "y-pixel": 20}, self.context)

        self.assertEqual(calls[0][0], "reader")
        self.assertEqual(calls[0][1]["sort_num"], 2)
        self.assertEqual([name for name, _ in self.context.stack], [])

    def test_catalog_blank_row_is_not_clickable(self):
        self._prime_reader()
        reader.STATE["rects"] = {("catalog", 9): (0, 0, 200, 60)}
        calls = []
        self.context.replace = lambda name, **params: calls.append((name, params))
        reader.handle_catalog(
            {"gesture": "tap", "x-pixel": 20, "y-pixel": 20}, self.context)
        self.assertEqual(calls, [])

    def test_paged_lists_request_only_what_they_render(self):
        cases = [
            (announcements._load, announcements._layout, "get_announcement_list"),
            (account._load_notifications, account._notification_layout,
             "get_notifications"),
        ]
        for loader, layout, method in cases:
            with self.subTest(method=method):
                captured = {}
                sizes = []
                setattr(self.context.api, method,
                        lambda page, size: sizes.append(size) or {"Data": []})
                with patch.object(
                    self.context, "run_async",
                    lambda _owner, operation, _s=None, _e=None:
                        captured.setdefault("operation", operation),
                ):
                    loader(self.context)
                captured["operation"]()
                _top, _row_height, per_page = layout(self.context)
                # 拉取条数必须等于可渲染行数，否则每页尾部条目永远翻不到
                self.assertEqual(sizes, [per_page])
                self.assertLess(per_page, 16)

    def test_about_page_reports_current_version(self):
        from kinnovel import VERSION
        self.assertEqual(settings.ABOUT_LINES[0], "KinNovel " + VERSION)

    def test_reader_image_tap_opens_and_closes_preview(self):
        self._prime_reader()
        reader.STATE["image_rects"] = {
            ("./p[2]", 40): (100, 400, 200, 150, "https://img/1.jpg"),
        }
        reader.handle({
            "gesture": "tap", "x-pixel": 150, "y-pixel": 450,
        }, self.context)
        self.assertEqual(reader.STATE["fullscreen_image"], "https://img/1.jpg")

        reader.handle({
            "gesture": "tap", "x-pixel": 10, "y-pixel": 10,
        }, self.context)
        self.assertIsNone(reader.STATE["fullscreen_image"])

    def test_reader_image_tap_behind_chrome_dismisses_chrome_first(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        reader.STATE["image_rects"] = {
            ("./p[2]", 40): (100, 400, 200, 150, "https://img/1.jpg"),
        }
        reader.handle({
            "gesture": "tap", "x-pixel": 150, "y-pixel": 450,
        }, self.context)
        self.assertFalse(reader.STATE["chrome_visible"])
        self.assertIsNone(reader.STATE["fullscreen_image"])

    def test_reader_guide_shown_until_dismissed(self):
        self.context.config.values["reader_guide_dismissed"] = False
        self.context.previous_page = "book"
        self._prime_reader()
        self.assertIsNotNone(self.context.modal)
        self.assertIn("不再提示", self.context.modal["buttons"])

        self.context.modal["actions"]["不再提示"]()
        self.context.modal = None
        self.assertTrue(self.context.config.get("reader_guide_dismissed"))

        self._prime_reader()
        self.assertIsNone(self.context.modal)

    def test_reader_guide_skipped_on_chapter_turn(self):
        self.context.config.values["reader_guide_dismissed"] = False
        self.context.previous_page = "reader"
        self._prime_reader()
        self.assertIsNone(self.context.modal)

    def test_browse_enter_resets_to_first_page(self):
        self.context.api.get_book_list = lambda **_kw: {
            "Data": [], "Page": 1, "TotalPages": 3}
        self.context.api.get_book_categories = lambda _kind: []
        browse.STATE.update({"loaded": True, "page": 3, "categories": []})

        browse.enter(self.context)

        self.assertEqual(browse.STATE["page"], 1)

    def test_reader_swipe_down_from_top_restores_chrome(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        with patch.object(self.context, "show") as show:
            reader.handle({
                "gesture": "down",
                "start": {"y-ratio": 0.04},
                "end": {"y-ratio": 0.20},
                "distance": 240,
            }, self.context)
        self.assertTrue(reader.STATE["chrome_visible"])
        show.assert_called_once()

    def test_page_context_forwards_down_gesture(self):
        class DownPage:
            @staticmethod
            def render(_context, _canvas):
                return None

            @staticmethod
            def handle(data, _context):
                return data.get("gesture")

        self.context.register("down", DownPage)
        self.context.page_name = "down"
        self.assertEqual(
            self.context.handle({
                "gesture": "down",
                "start": {"y-ratio": 0.04},
                "end": {"y-ratio": 0.20},
            }),
            "down",
        )

    def test_reader_bottom_previous_chapter_button_works(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        self.context.render()
        rect = reader.STATE["rects"][("prev", 0)]
        calls = []
        self.context.replace = lambda name, **params: calls.append((name, params))
        reader.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        }, self.context)
        self.assertEqual(calls[0][0], "reader")
        self.assertEqual(calls[0][1]["sort_num"], 1)
        self.assertTrue(calls[0][1]["at_last"])

    def test_reader_bottom_catalog_button_works(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        self.context.render()
        rect = reader.STATE["rects"][("catalog", 0)]
        calls = []
        self.context.navigate = lambda name, **params: calls.append((name, params))
        reader.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        }, self.context)
        self.assertEqual(calls[0][0], "catalog")

    def test_reader_compact_header_uses_page_background(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        image = self.context.render()
        self.assertEqual(image.getpixel((5, 5)), 255)

    def test_reader_expanded_header_restores_actions(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        self.context.render()
        self.assertEqual(
            self.context._header_state,
            {"height": max(72, int(self.context.height * 0.085)),
             "left": "返回", "right": "主页"},
        )

    def test_reader_turn_page_does_not_upload_progress(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        reader.STATE["page"] = 2
        with patch.object(self.context.api, "save_read_position") as save_pos:
            reader._turn(self.context, 1)
            self.assertEqual(reader.STATE["page"], 3)
            save_pos.assert_not_called()

    def test_reader_header_back_and_home_upload_progress(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        reader.STATE["last_turn_at"] = 0.0
        self.context.render()
        reader.STATE["page"] = 4
        with patch.object(self.context.api, "save_read_position") as save_pos:
            self.context.handle({
                "gesture": "tap",
                "x-pixel": 50,
                "y-pixel": 20,
            })
            time.sleep(0.05)
            self.assertTrue(save_pos.called)

    def test_reader_tap_sides_turn_pages(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        reader.STATE["page"] = 5
        reader.handle({
            "gesture": "tap",
            "x-pixel": 10,
            "y-pixel": self.context.height // 2,
        }, self.context)
        self.assertEqual(reader.STATE["page"], 4)
        reader.handle({
            "gesture": "tap",
            "x-pixel": self.context.width - 10,
            "y-pixel": self.context.height // 2,
        }, self.context)
        self.assertEqual(reader.STATE["page"], 5)

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

    def test_browse_second_page_rows_map_to_current_page_items(self):
        browse.STATE.update({
            "items": [{"Id": 11, "Title": "第二页书"}],
            "page": 2,
            "total_pages": 2,
            "loading": False,
            "loaded": True,
            "categories": [],
            "category": 0,
        })
        self.context.page_name = "browse"
        self.context.params = {}
        self.context.render()
        calls = []
        self.context.navigate = lambda name, **params: calls.append((name, params))
        rect = browse.STATE["rects"][("item", 0)]
        self.context.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        })
        self.assertEqual(calls, [("book", {"book_id": 11})])

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

    def test_reader_image_tap_opens_preview(self):
        reader.STATE["fullscreen_image"] = None
        reader.STATE["chrome_visible"] = False
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

    def test_reader_image_hit_rect_matches_fitted_image(self):
        class Doc:
            page_count = 1
            pages = [[{
                "type": "image", "url": "illu", "x": 34, "y": 0,
                "width": 1004, "height": 600, "path": "p",
            }]]

        reader.STATE.update({
            "data": {"Chapter": {"Title": "测试"}},
            "doc": Doc(),
            "page": 0,
            "fullscreen_image": None,
        })
        self.context.images._memory["illu"] = Image.new("L", (400, 200), 255)
        self.context.page_name = "reader"
        self.context.params = {}
        self.context.render()
        rect = reader.STATE["image_rects"][("p", 0)]
        self.assertEqual(rect[2], 1004)
        self.assertEqual(rect[3], 502)

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

    def test_history_render_survives_null_entries(self):
        history.STATE.update({
            "items": [None, {"Title": "正常", "UserName": "作者"}, "bad"],
            "page": 0,
            "loading": False,
            "loaded": True,
            "error": "",
        })
        self.context.register("history", history)
        self.context.page_name = "history"
        self.context.render()  # 不应抛出 AttributeError

    def test_show_survives_broken_page_render(self):
        class Broken:
            @staticmethod
            def render(context, canvas):
                raise RuntimeError("boom")

            @staticmethod
            def handle(data, context):
                return None

        self.context.register("broken", Broken)
        self.context.page_name = "broken"
        self.context.show()  # 必须降级到错误页而不是让异常冒泡

    def test_browse_render_survives_null_entries(self):
        browse.STATE.update({
            "items": [None, {"Title": "书", "UserName": "作者"}],
            "page": 1,
            "total_pages": 1,
            "loading": False,
            "loaded": True,
        })
        self.context.register("browse", browse)
        self.context.page_name = "browse"
        self.context.render()

    def test_back_marks_page_as_returning(self):
        seen = []

        class Probe:
            @staticmethod
            def enter(context):
                seen.append(context.returning)

            @staticmethod
            def render(_context, _canvas):
                return None

            @staticmethod
            def handle(_data, _context):
                return None

        self.context.register("listprobe", Probe)
        self.context.register("detailprobe", Probe)
        self.context.page_name = "listprobe"
        self.context.navigate("detailprobe")
        self.assertIs(seen[-1], False)
        self.context.back()
        self.assertIs(seen[-1], True)

    def test_list_pages_reset_only_on_fresh_entry(self):
        browse.STATE["categories"] = [{"Name": "全部", "Id": 1}]
        browse.STATE["page"] = 3
        self.context._returning = False
        with patch.object(browse, "_load") as load:
            browse.enter(self.context)
        load.assert_called_once_with(self.context, 1)
        self.context._returning = True
        with patch.object(browse, "_load") as load:
            browse.enter(self.context)
        load.assert_called_once_with(self.context, 3)
        self.context._returning = False

        with patch.object(rank, "_load") as load:
            rank.enter(self.context)
        load.assert_called_once_with(self.context, reset_page=True)
        self.context._returning = True
        with patch.object(rank, "_load") as load:
            rank.enter(self.context)
        load.assert_called_once_with(self.context, reset_page=False)
        self.context._returning = False

        with patch.object(history, "_load") as load:
            history.enter(self.context)
        load.assert_called_once_with(self.context, reset_page=True)
        self.context._returning = True
        with patch.object(history, "_load") as load:
            history.enter(self.context)
        load.assert_called_once_with(self.context, reset_page=False)
        self.context._returning = False

        with patch.object(shelf, "_load") as load:
            shelf.enter(self.context)
        load.assert_called_once_with(self.context, reset_page=True)
        self.context._returning = True
        with patch.object(shelf, "_load") as load:
            shelf.enter(self.context)
        load.assert_called_once_with(self.context, reset_page=False)
        self.context._returning = False

    def test_history_buttons_stay_inside_screen(self):
        history.STATE.update({
            "items": [{"Id": 1, "Title": "书", "UserName": "作者"}],
            "page": 0,
            "error": "",
            "loaded": True,
            "loading": False,
        })
        canvas = Canvas(Image.new("L", (1072, 1448), 255),
                        self.context.fonts, Theme(False))
        history.render(self.context, canvas)
        for key in (("prev", 0), ("count", 0), ("next", 0),
                    ("retry", 0), ("clear", 0)):
            rx, ry, width, height = history.STATE["rects"][key]
            self.assertGreaterEqual(rx, 0)
            self.assertLessEqual(rx + width, canvas.width)
            self.assertLessEqual(ry + height, canvas.height)

    def test_popup_long_text_stays_inside_screen(self):
        canvas = Canvas(Image.new("L", (1072, 1448), 255),
                        self.context.fonts, Theme(False))
        rects = canvas.popup(["很长的错误信息" * 60], ["确定"])
        _label, (bx, by, width, height) = rects[0]
        self.assertGreaterEqual(bx, 0)
        self.assertGreaterEqual(by, 0)
        self.assertLessEqual(bx + width, canvas.width)
        self.assertLessEqual(by + height, canvas.height)

    def test_cover_reuses_nearest_cached_bucket(self):
        with tempfile.TemporaryDirectory() as tmp, patch(
                "kinnovel.ui.CACHE_DIR", Path(tmp)):
            cache = ImageCache(workers=1)
            try:
                path = cache._path("https://example.test/cover", 512)
                path.parent.mkdir(parents=True, exist_ok=True)
                Image.new("L", (64, 64), 128).save(path, "JPEG")
                result = cache.cover("https://example.test/cover", 100, 140)
                self.assertIsNotNone(result)
                self.assertEqual(result.size, (100, 140))
            finally:
                cache.close()


if __name__ == "__main__":
    unittest.main()
