import datetime
import tempfile
import threading
import time
import traceback
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from PIL import Image, ImageFont

from kinnovel import progress
import kinnovel.pages.reader as reader_page
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
        progress.clear()
        book.STATE.update({
            "book_id": 0,
            "data": None,
            "chapter_page": 0,
            "bound": None,
            "rects": {},
            "loading": False,
            "loaded": False,
            "error": "",
            "generation": 0,
        })
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

    def test_book_detail_uses_session_progress(self):
        data = {
            "Book": {"Chapters": [
                {"Id": 1, "SortNum": 1},
                {"Id": 10, "SortNum": 10},
            ]},
            "ReadPosition": {},
        }
        progress.record(1, 10, 3, "./p[9]", 12)
        self.assertTrue(book._has_resume(data, 1))
        self.assertEqual(book._resume_sort_num(data, 1), 10)

    def test_book_resume_prefers_further_chapter(self):
        data = {
            "Book": {"Chapters": [
                {"Id": 1, "SortNum": 1},
                {"Id": 10, "SortNum": 10},
            ]},
            "ReadPosition": {"ChapterId": 10},
        }
        progress.record(1, 1, 0)
        self.assertEqual(book._resume_sort_num(data, 1), 10)

    def test_book_has_no_resume_without_progress(self):
        data = {
            "Book": {"Chapters": [{"Id": 1, "SortNum": 1}]},
            "ReadPosition": {},
        }
        self.assertFalse(book._has_resume(data, 1))

    def test_book_read_button_uses_session_progress_sort(self):
        book.STATE.update({
            "book_id": 1,
            "data": {
                "Book": {"Chapters": [
                    {"Id": 1, "SortNum": 1},
                    {"Id": 10, "SortNum": 10},
                ]},
                "ReadPosition": {},
            },
            "rects": {("read", 0): (0, 0, 100, 50)},
        })
        progress.record(1, 10, 3)
        calls = []
        self.context.navigate = lambda name, **params: calls.append((name, params))
        book.handle({"gesture": "tap", "x-pixel": 10, "y-pixel": 10},
                    self.context)
        self.assertEqual(calls[0][0], "reader")
        self.assertEqual(calls[0][1]["sort_num"], 10)

    def test_book_enter_force_loads_after_reader(self):
        self.context.previous_page = "reader"
        with patch.object(book, "_load") as load:
            book.enter(self.context)
        load.assert_called_once_with(self.context, force=True)

    def test_reader_save_progress_records_session(self):
        self._prime_reader()
        reader.STATE["page"] = 5
        reader.STATE["last_saved"] = -1
        reader._save_progress(self.context)
        session = progress.get(1)
        self.assertIsNotNone(session)
        self.assertEqual(session["sort_num"], 2)
        self.assertEqual(session["page"], 5)

    def test_reader_turn_page_keeps_memory_progress_without_disk_write(self):
        self._prime_reader()
        reader.STATE["page"] = 2
        reader.STATE["last_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_durable_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_save_at"] = time.monotonic()
        with patch.object(reader_page, "atomic_write") as atomic:
            reader._turn(self.context, 1)
        self.assertEqual(reader.STATE["page"], 3)
        atomic.assert_not_called()
        self.assertEqual(progress.get(1)["page"], 3)

    def test_reader_leave_forces_durable_write_and_upload(self):
        self._prime_reader()
        reader.STATE["last_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_durable_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_save_at"] = time.monotonic()
        with patch.object(reader_page, "atomic_write") as atomic, patch.object(
                self.context.api, "save_read_position") as save_pos:
            reader.leave(self.context)
            deadline = time.monotonic() + 1
            while not save_pos.called and time.monotonic() < deadline:
                time.sleep(0.01)
        self.assertTrue(atomic.called)
        self.assertTrue(save_pos.called)

    def test_reader_suspend_flushes_progress(self):
        self._prime_reader()
        reader.STATE["last_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_durable_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_save_at"] = time.monotonic()
        with patch.object(reader_page, "atomic_write") as atomic:
            reader.handle_suspend(self.context)
        self.assertTrue(atomic.called)

    def test_reader_chapter_change_flushes_progress(self):
        self._prime_reader()
        reader.STATE["last_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_durable_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_save_at"] = time.monotonic()
        calls = []
        self.context.replace = lambda name, **params: calls.append(
            (name, params))
        with patch.object(reader_page, "atomic_write") as atomic:
            reader._change_chapter(self.context, -1)
        self.assertTrue(atomic.called)
        self.assertEqual(calls[0][0], "reader")

    def test_navigation_calls_reader_leave(self):
        self._prime_reader()
        with patch.object(reader, "leave") as leave:
            self.context.navigate("home")
        leave.assert_called_once_with(self.context)

    def test_reader_exit_flushes_the_last_position(self):
        self._prime_reader()
        reader.STATE["last_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_durable_saved_path"] = (1, 2, 2, "./p[1]", 0)
        reader.STATE["last_save_at"] = time.monotonic()
        with patch.object(reader_page, "atomic_write") as atomic:
            reader.handle_exit()
        self.assertTrue(atomic.called)

    def test_settings_cache_size_is_cached_for_sixty_seconds(self):
        settings.STATE["cache_text"] = "1.0 MB"
        settings.STATE["cache_loaded_at"] = time.monotonic()
        settings.STATE["cache_loading"] = False
        calls = []
        with patch.object(settings, "cache_size",
                          side_effect=lambda _path: calls.append(1) or 1024):
            settings.enter(self.context)
        self.assertEqual(calls, [])
        self.assertEqual(settings._cache_text(), "1.0 MB")

    def test_settings_logout_clears_credentials(self):
        self.context.api.session = MagicMock()
        self.context.api.hub = MagicMock()
        settings._logout(self.context)
        self.context.api.session.clear_credentials.assert_called_once_with()
        self.context.api.hub.close.assert_called_once_with()

    def test_sign_refresh_runs_async_and_detects_today_signed(self):
        self.assertTrue(account._is_today_signed(
            {"Growth": {"TodaySigned": True}}))
        user = {"Growth": {"LastSignAt": time.strftime("%Y-%m-%dT%H:%M:%S")}}
        self.assertTrue(account._is_today_signed(user))
        calls = []
        self.context.api.sign_in = lambda: calls.append("sign") or {"Reward": 5}
        self.context.api.refresh_user = lambda: calls.append("refresh")
        with patch.object(
                self.context, "run_async",
                lambda _owner, operation, success=None, error=None, **kw:
                    success(operation())):
            account._sign_in(self.context)
        self.assertEqual(calls, ["sign", "refresh"])
        self.assertTrue(account.STATE["today_signed"])

    def test_session_progress_can_be_cleared(self):
        progress.record(1, 2, 3)
        self.assertIsNotNone(progress.get(1))
        progress.clear()
        self.assertIsNone(progress.get(1))

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

    def test_book_error_state_clears_old_data_and_retries(self):
        self.context.page_name = "book"
        self.context.params = {"book_id": 7}
        self.context.api.get_book_info = MagicMock(side_effect=[
            RuntimeError("offline"),
            {"Book": {"Id": 7, "Title": "恢复", "Chapters": []}},
        ])
        self.context.api.get_book_shelf = MagicMock(return_value={"data": []})
        book.STATE.update({
            "book_id": 7,
            "data": {"Book": {"Id": 7, "Title": "旧数据", "Chapters": []}},
            "chapter_page": 0,
            "bound": True,
            "rects": {},
            "loading": False,
            "loaded": True,
            "error": "",
            "generation": 0,
        })

        def immediate(_owner, operation, on_success=None, on_error=None,
                      **_kwargs):
            try:
                result = operation()
            except Exception as exc:
                if on_error:
                    on_error(exc)
                return
            if on_success:
                on_success(result)

        with patch.object(self.context, "run_async", side_effect=immediate):
            book._load(self.context, force=True)
        self.assertIsNone(book.STATE["data"])
        self.assertEqual(book.STATE["error"], "offline")
        self.assertTrue(book.STATE["loaded"])
        self.assertFalse(book.STATE["loading"])
        self.assertIn(("retry", 0), book.STATE["rects"])

        rect = book.STATE["rects"][("retry", 0)]
        book.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        }, self.context)
        self.assertEqual(self.context.api.get_book_info.call_count, 2)
        self.assertEqual(book.STATE["data"]["Book"]["Title"], "恢复")
        self.assertEqual(book.STATE["error"], "")
        self.assertFalse(book.STATE["loading"])

    def test_shelf_error_state_clears_old_data_and_retries_current_path(self):
        self.context.page_name = "shelf"
        items = [
            {
                "id": "folder",
                "type": "FOLDER",
                "index": -1,
                "parents": [],
                "title": "文件夹",
            },
        ] + [
            {
                "id": value,
                "type": "NOVEL",
                "index": value,
                "parents": ["folder"],
            }
            for value in range(1, 31)
        ]
        calls = []

        def get_shelf():
            calls.append("shelf")
            if len(calls) == 1:
                raise RuntimeError("offline")
            return {"data": [dict(item) for item in items]}

        self.context.api.get_book_shelf = MagicMock(side_effect=get_shelf)
        self.context.api.get_book_list_by_ids_chunked = MagicMock(
            side_effect=lambda ids, _book_type=None, chunk_size=24: [
                {"Id": value, "Title": "书%s" % value} for value in ids
            ]
        )
        shelf.STATE.update({
            "items": [{"id": 99, "type": "NOVEL"}],
            "visible": [{"id": 99, "type": "NOVEL"}],
            "books": {99: {"Id": 99, "Title": "旧书"}},
            "path": ["folder"],
            "page": 1,
            "rects": {},
            "loading": False,
            "loaded": True,
            "error": "",
            "retry_reset_page": True,
            "generation": 0,
        })

        def immediate(_owner, operation, on_success=None, on_error=None,
                      **_kwargs):
            try:
                result = operation()
            except Exception as exc:
                if on_error:
                    on_error(exc)
                return
            if on_success:
                on_success(result)

        with patch.object(self.context, "run_async", side_effect=immediate):
            shelf._load(self.context, reset_page=False)
        self.assertEqual(shelf.STATE["items"], [])
        self.assertEqual(shelf.STATE["visible"], [])
        self.assertEqual(shelf.STATE["books"], {})
        self.assertEqual(shelf.STATE["path"], ["folder"])
        self.assertEqual(shelf.STATE["error"], "offline")
        self.assertFalse(shelf.STATE["retry_reset_page"])
        self.assertIn(("retry", 0), shelf.STATE["rects"])

        rect = shelf.STATE["rects"][("retry", 0)]
        shelf.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        }, self.context)
        self.assertEqual(calls, ["shelf", "shelf"])
        self.assertEqual(shelf.STATE["error"], "")
        self.assertEqual(shelf.STATE["page"], 1)
        self.assertEqual(shelf.STATE["path"], ["folder"])
        self.assertEqual(len(shelf.STATE["visible"]), 30)
        self.assertEqual(shelf.STATE["books"][15]["Title"], "书15")

    def test_series_error_state_clears_old_data_and_retries_current_page(self):
        self.context.page_name = "series"
        calls = []

        def get_books_by_series(_name, page=1, size=24, **kwargs):
            calls.append(page)
            if len(calls) == 1:
                raise RuntimeError("offline")
            return {
                "Data": [{"Id": 31, "Title": "恢复卷"}],
                "Page": page,
                "TotalPages": 5,
            }

        self.context.api.get_books_by_series = MagicMock(
            side_effect=get_books_by_series
        )
        series.STATE.update({
            "title": "测试系列",
            "series_name": "测试系列",
            "items": [{"Id": 1, "Title": "旧卷"}],
            "current_id": 1,
            "page": 2,
            "total_pages": 5,
            "server_paged": True,
            "rects": {},
            "loading": False,
            "loaded": True,
            "error": "",
            "generation": 0,
        })

        def immediate(_owner, operation, on_success=None, on_error=None,
                      **_kwargs):
            try:
                result = operation()
            except Exception as exc:
                if on_error:
                    on_error(exc)
                return
            if on_success:
                on_success(result)

        with patch.object(self.context, "run_async", side_effect=immediate):
            series._load(self.context, page=2)
        self.assertEqual(series.STATE["items"], [])
        self.assertEqual(series.STATE["total_pages"], 1)
        self.assertEqual(series.STATE["page"], 2)
        self.assertEqual(series.STATE["error"], "offline")
        self.assertIn(("retry", 0), series.STATE["rects"])

        rect = series.STATE["rects"][("retry", 0)]
        series.handle({
            "gesture": "tap",
            "x-pixel": rect[0] + 4,
            "y-pixel": rect[1] + 4,
        }, self.context)
        self.assertEqual(calls, [3, 3])
        self.assertEqual(series.STATE["error"], "")
        self.assertEqual(series.STATE["page"], 2)
        self.assertEqual(series.STATE["items"][0]["Title"], "恢复卷")

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

    def test_retry_icon_is_smooth_and_open_at_top_right(self):
        image = Image.new("L", (240, 240), 255)
        Canvas(image, {}, Theme(False)).retry_icon(120, 120, size=200)
        values = set(image.getdata())
        # 超采样 + LANCZOS 必须留下中间灰阶, 否则就是 1-bit 锯齿
        self.assertTrue(any(10 < value < 245 for value in values))
        # 圆弧覆盖底部(12,21): 应为前景
        self.assertLess(image.getpixel((120, 195)), 60)
        # SVG 缺口在右上象限(3 点钟到 12 点钟之间): 应为背景
        self.assertGreater(image.getpixel((194, 107)), 200)

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

    def test_reader_long_press_turns_page(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        reader.STATE["page"] = 5
        reader.handle({
            "gesture": "long",
            "x-pixel": 10,
            "y-pixel": self.context.height // 2,
        }, self.context)
        self.assertEqual(reader.STATE["page"], 4)

    def test_reader_swipe_left_right_turn_pages(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        reader.STATE["page"] = 5
        reader.handle({"gesture": "left"}, self.context)
        self.assertEqual(reader.STATE["page"], 6)
        reader.handle({"gesture": "right"}, self.context)
        self.assertEqual(reader.STATE["page"], 5)

    def test_reader_middle_tap_shows_chrome_in_compact(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = False
        reader.STATE["page"] = 5
        with patch.object(self.context, "show") as show:
            reader.handle({
                "gesture": "tap",
                "x-pixel": self.context.width // 2,
                "y-pixel": self.context.height // 2,
            }, self.context)
        self.assertTrue(reader.STATE["chrome_visible"])
        self.assertEqual(reader.STATE["page"], 5)
        show.assert_called_once()

    def test_reader_chrome_side_tap_turns_page(self):
        self._prime_reader()
        reader.STATE["chrome_visible"] = True
        reader.STATE["page"] = 5
        reader.handle({
            "gesture": "tap",
            "x-pixel": 10,
            "y-pixel": self.context.height // 2,
        }, self.context)
        self.assertEqual(reader.STATE["page"], 4)
        self.assertTrue(reader.STATE["chrome_visible"])

    def test_reader_pending_turn_when_document_missing(self):
        self._prime_reader()
        reader.STATE["doc"] = None
        reader.STATE["pending_turn"] = None
        with patch.object(self.context, "toast") as toast:
            reader.handle({
                "gesture": "tap",
                "x-pixel": 10,
                "y-pixel": self.context.height // 2,
            }, self.context)
        self.assertEqual(reader.STATE["pending_turn"], -1)
        toast.assert_called_once()

    def test_reader_guide_mentions_new_gestures(self):
        self.assertIn("左右滑动：翻页", reader._GUIDE_LINES)
        self.assertIn("点击中间：显示控件", reader._GUIDE_LINES)

    def test_page_context_forwards_left_right(self):
        class SwipePage:
            @staticmethod
            def render(_context, _canvas):
                return None

            @staticmethod
            def handle(data, _context):
                return data.get("gesture")

        self.context.register("swipe", SwipePage)
        self.context.page_name = "swipe"
        self.assertEqual(self.context.handle({"gesture": "left"}), "left")
        self.assertEqual(self.context.handle({"gesture": "right"}), "right")

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

    def test_page_context_posts_show_from_worker_thread(self):
        self.context.page_name = "home"
        self.context._ui_loop_running = True
        self.context._ui_thread = threading.current_thread()
        calls = []
        self.context.screen.output.show = lambda *a, **k: calls.append(1)
        worker = threading.Thread(target=self.context.show, daemon=True)
        worker.start()
        worker.join(2)
        self.assertEqual(calls, [])
        self.context.drain_ui_queue()
        self.assertEqual(calls, [1])
        self.context._ui_loop_running = False

    def test_clock_minute_change_requests_redraw(self):
        self.context.app.power = type("Power", (), {"is_sleeping": False})()
        self.context.page_name = "home"
        self.context._ui_thread = threading.current_thread()
        self.context._ui_loop_running = True
        old = datetime.datetime.now() - datetime.timedelta(minutes=5)
        self.context._last_minute = old.strftime("%Y%m%d%H%M")
        self.context._check_clock_tick()
        self.assertTrue(self.context._refresh_requested)
        self.context._ui_loop_running = False

    def test_concurrent_render_does_not_deadlock(self):
        self.context.page_name = "home"
        errors = []

        def worker():
            for _ in range(5):
                try:
                    self.context.render()
                except Exception:
                    errors.append(traceback.format_exc())

        threads = [threading.Thread(target=worker) for _ in range(2)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(5)
        self.assertEqual(errors, [])

    def test_browse_uses_six_items_per_page(self):
        browse.STATE.update({"page": 1, "total_pages": 2, "items": []})
        self.context.page_name = "browse"
        self.context.render()
        item_rects = [key for key in browse.STATE["rects"] if key[0] == "item"]
        # 页码指示器是纯文字, 不再是可点击按钮(修复"看起来像按钮却点不动"),
        # 所以这里只断言 prev/next 两个热区。
        nav_rects = [key for key in browse.STATE["rects"] if key[0] in ("prev", "next")]
        self.assertGreater(len(item_rects), 0)
        self.assertEqual(len(nav_rects), 2)

    def test_rank_has_bottom_pager(self):
        rank.STATE.update({"items": [{"Id": 1, "Title": "A"}], "page": 1,
                           "total_pages": 2, "loaded": True})
        self.context.page_name = "rank"
        self.context.render()
        self.assertIn(("prev", 0), rank.STATE["rects"])
        # 页码指示器现在是纯文字(修复"看起来像按钮却点不动"的问题),
        # 不再注册可点击热区，所以这里不再断言 ("count", 0)。
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
            "accepted": [
                {"Id": index, "Title": "Page 1 item %s" % index}
                for index in range(13)
            ],
            "next_server_page": 2,
            "server_total_pages": 3,
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

    def test_browse_fills_sparse_filtered_pages(self):
        calls = []

        def get_page(**kwargs):
            page = kwargs["page"]
            calls.append(page)
            return {
                "Page": page,
                "TotalPages": 7,
                "Data": [
                    {"Id": page * 10 + row, "Title": "Book %s-%s" % (page, row)}
                    for row in range(2)
                ],
            }

        self.context.api.get_book_list = get_page
        browse.STATE.update({
            "categories": [],
            "category": 0,
            "order": "latest",
        })
        self.context.page_name = "browse"
        browse._load(self.context, 1, reset=True)
        deadline = time.monotonic() + 2
        while len(browse.STATE["items"]) != 13 and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(calls, [1, 2, 3, 4, 5, 6, 7])
        self.assertEqual(browse.STATE["items"][0]["Title"], "Book 1-0")
        self.assertEqual(browse.STATE["items"][-1]["Title"], "Book 7-0")
        self.assertEqual(browse.STATE["total_pages"], 2)

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
        # 图片已加载时不应出现重试按钮
        self.assertEqual(reader.STATE["image_retry_rects"], {})

    def test_reader_missing_image_registers_retry_button(self):
        class Doc:
            page_count = 1
            pages = [[{
                "type": "image", "url": "missing-illu", "x": 34, "y": 0,
                "width": 1004, "height": 600, "path": "p",
            }]]

        reader.STATE.update({
            "data": {"Chapter": {"Title": "测试"}},
            "doc": Doc(),
            "page": 0,
            "fullscreen_image": None,
            "chrome_visible": False,
            "image_retry_rects": {},
        })
        self.context.page_name = "reader"
        self.context.params = {}
        with patch.object(self.context.images, "prefetch",
                          return_value=True) as prefetch:
            self.context.render()
        rects = reader.STATE["image_retry_rects"]
        self.assertIn(("p", 0), rects)
        x, _y, width, height, url, _target = rects[("p", 0)]
        self.assertEqual(url, "missing-illu")
        self.assertEqual(width, height)
        self.assertEqual(x + width,
                         34 + 1004 - reader._RETRY_BUTTON_MARGIN)
        self.assertTrue(any(call.kwargs.get("priority") == 0
                            for call in prefetch.call_args_list))

    def test_reader_retry_button_uses_top_priority_manual_request(self):
        class Doc:
            page_count = 1
            pages = [[{
                "type": "image", "url": "missing-illu", "x": 34, "y": 0,
                "width": 1004, "height": 600, "path": "p",
            }]]

        reader.STATE.update({
            "data": {"Chapter": {"Title": "测试"}},
            "doc": Doc(),
            "page": 0,
            "fullscreen_image": None,
            "chrome_visible": False,
            "image_retry_rects": {},
        })
        self.context.page_name = "reader"
        self.context.params = {}
        with patch.object(self.context.images, "prefetch", return_value=True):
            self.context.render()
        x, y, width, height, _url, _target = (
            reader.STATE["image_retry_rects"][("p", 0)])
        calls = []

        def fake_prefetch(url, strict_tls=False, height=None, callback=None,
                          priority=0, retry=False, force=False,
                          manual_callback=None):
            calls.append({
                "url": url, "height": height, "priority": priority,
                "force": force, "manual": manual_callback,
            })
            if callable(manual_callback):
                manual_callback(True)
            return True

        with patch.object(self.context, "show"), \
                patch.object(self.context.images, "prefetch",
                             side_effect=fake_prefetch):
            reader.handle({
                "gesture": "tap",
                "x-pixel": x + width // 2,
                "y-pixel": y + height // 2,
            }, self.context)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["url"], "missing-illu")
        self.assertEqual(calls[0]["priority"], -1)
        self.assertTrue(calls[0]["force"])
        self.assertTrue(callable(calls[0]["manual"]))
        # 点重试按钮不能误触全屏预览
        self.assertIsNone(reader.STATE["fullscreen_image"])

    def test_reader_manual_retry_failure_only_manual_pops_message(self):
        messages = []
        self.context.page_name = "reader"

        def fake_prefetch(url, strict_tls=False, height=None, callback=None,
                          priority=0, retry=False, force=False,
                          manual_callback=None):
            if callable(manual_callback):
                manual_callback(False)
            return True

        with patch.object(self.context, "toast"), \
                patch.object(self.context, "show"), \
                patch.object(self.context, "message",
                             side_effect=lambda lines, **kwargs:
                             messages.append(lines)), \
                patch.object(self.context.images, "prefetch",
                             side_effect=fake_prefetch):
            reader._manual_retry(self.context, "u", 512)
        self.assertTrue(messages)
        self.assertIsNone(reader.STATE.get("manual_retry_url"))
        # 后台/自动失败（ok=False）不弹窗
        messages.clear()
        reader._image_ready(self.context, "u", False)
        self.assertEqual(messages, [])

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
        load.assert_called_once_with(self.context, 1, reset=True)
        self.context._returning = True
        with patch.object(browse, "_load") as load:
            browse.enter(self.context)
        load.assert_called_once_with(self.context, 3, reset=False)
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
        # 页码指示器是纯文字, 不再是可点击按钮(修复"看起来像按钮却点不动"),
        # 所以这里不再断言 ("count", 0) 这个热区。
        for key in (("prev", 0), ("next", 0),
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


class ShelfApi:
    """书架的同步 API 替身, 只覆盖本批次测试用到的接口。"""

    def __init__(self, items):
        self.items = [dict(item) for item in items]
        self.requests = []

    def get_book_shelf(self):
        return {"data": [dict(item) for item in self.items]}

    def save_book_shelf(self, items):
        self.items = list(items)
        return None

    def get_book_list_by_ids_chunked(self, ids, book_type=None, chunk_size=24):
        self.requests.append(list(ids))
        return [{"Id": value, "Title": "书%s" % value} for value in ids]


class ImmediateContext:
    def __init__(self, api, height=1448, width=1072, returning=False, params=None):
        self.api = api
        self.height = height
        self.width = width
        self.config = Config()
        self.returning = returning
        self.params = params or {}
        self.messages = []
        self.show_count = 0

    def run_async(self, owner, operation, on_success=None, on_error=None,
                  refresh=True, sticky=False):
        try:
            result = operation()
        except Exception as exc:
            if on_error:
                on_error(exc)
            return
        if on_success:
            on_success(result)

    def message(self, lines):
        self.messages.append(lines)

    def show(self):
        self.show_count += 1


class ShelfHistorySeriesTests(unittest.TestCase):
    def setUp(self):
        shelf.STATE.update({
            "items": [], "visible": [], "books": {}, "path": [],
            "page": 0, "rects": {}, "loading": False, "loaded": False,
            "error": "", "generation": 0,
        })
        history.STATE.update({
            "items": [], "history_ids": [], "page": 0, "rects": {},
            "loading": False, "loaded": False, "error": "", "generation": 0,
        })
        series.STATE.update({
            "title": "系列", "series_name": "系列", "items": [],
            "current_id": 0, "page": 0, "total_pages": 1,
            "server_paged": False, "rects": {}, "loading": False,
            "loaded": False, "error": "", "generation": 0,
        })

    def test_shelf_book_id_skips_folders_and_damaged(self):
        self.assertEqual(shelf.shelf_book_id({"id": "7"}), 7)
        self.assertIsNone(shelf.shelf_book_id({"id": "abc", "type": "FOLDER"}))
        self.assertIsNone(shelf.shelf_book_id({"id": "x"}))
        self.assertIsNone(shelf.shelf_book_id({"type": "NOVEL"}))
        self.assertIsNone(shelf.shelf_book_id(None))
        self.assertTrue(shelf.is_folder({"type": "folder", "id": "abc"}))

    def test_set_shelf_preserves_comic_and_folder(self):
        api = ShelfApi([
            {"id": 1, "type": "NOVEL"},
            {"id": 2, "type": "COMIC"},
            {"id": "abc", "type": "FOLDER"},
        ])
        ctx = SimpleNamespace(api=api)
        book._set_shelf(ctx, 3, "NOVEL", bound=False)
        self.assertEqual([item["id"] for item in api.items], [3, 1, 2, "abc"])
        book._set_shelf(ctx, 1, "NOVEL", bound=True)
        self.assertEqual([item["id"] for item in api.items], [3, 2, "abc"])

    def test_set_shelf_accepts_uppercase_data_key(self):
        api = ShelfApi([{"id": 1, "type": "NOVEL"}])
        api.get_book_shelf = lambda: {"Data": [dict(item) for item in api.items]}
        ctx = SimpleNamespace(api=api)
        book._set_shelf(ctx, 2, "NOVEL", bound=False)
        self.assertEqual([item["id"] for item in api.items], [2, 1])

    def test_shelf_remove_book_keeps_folder_comic_and_damaged(self):
        shelf.STATE["items"] = [
            {"id": 1, "type": "NOVEL"},
            {"id": 2, "type": "COMIC"},
            {"id": "abc", "type": "FOLDER"},
            {"id": "broken", "type": "NOVEL"},
        ]
        saved = {}

        class Api:
            def save_book_shelf(self, items):
                saved["items"] = items

        class Ctx:
            api = Api()

            @staticmethod
            def run_async(owner, operation, on_success=None, on_error=None,
                          refresh=True, sticky=False):
                on_success(operation())

            @staticmethod
            def message(lines):
                return None

        with patch.object(shelf, "_load"):
            shelf._remove_book(Ctx(), 1)
        self.assertEqual(
            [item.get("id") for item in saved["items"]], [2, "abc", "broken"]
        )

    def test_shelf_delete_folder_does_not_int_coerce(self):
        shelf.STATE["items"] = [
            {"id": 1, "type": "NOVEL", "parents": ["abc"]},
            {"id": "abc", "type": "FOLDER", "parents": []},
            {"id": 2, "type": "COMIC", "parents": []},
        ]
        saved = {}

        class Api:
            def save_book_shelf(self, items):
                saved["items"] = items

        class Ctx:
            api = Api()

            @staticmethod
            def run_async(owner, operation, on_success=None, on_error=None,
                          refresh=True, sticky=False):
                on_success(operation())

            @staticmethod
            def message(lines):
                return None

        with patch.object(shelf, "_load"):
            shelf._delete_folder(Ctx(), "abc")
        self.assertEqual([item.get("id") for item in saved["items"]], [1, 2])
        self.assertEqual(saved["items"][0]["parents"], [])

    def test_shelf_pagination_reaches_last_page(self):
        items = [
            {"id": value, "type": "NOVEL", "index": value, "parents": []}
            for value in range(1, 61)
        ]
        api = ShelfApi(list(reversed(items)))
        ctx = ImmediateContext(api)
        shelf._load(ctx)
        _top, _row_height, per_page = shelf._layout(ctx)
        self.assertEqual(per_page, 14)
        self.assertEqual(
            [item["id"] for item in shelf.STATE["visible"][:3]], [1, 2, 3]
        )
        self.assertEqual(len(api.requests[0]), per_page)
        self.assertTrue(all(len(chunk) <= 24 for chunk in api.requests))
        last = (60 + per_page - 1) // per_page - 1
        shelf.STATE["page"] = last
        shelf._load_page(ctx)
        self.assertIn(60, shelf.STATE["books"])
        self.assertEqual(shelf.STATE["books"][60]["Title"], "书60")
        self.assertEqual(len(api.requests[-1]), 4)

    def test_history_pagination_reaches_last_page(self):
        class HistoryApi:
            def __init__(self):
                self.requests = []

            @staticmethod
            def get_read_history():
                return {"Novel": list(range(1, 61))}

            def get_book_list_by_ids_chunked(self, ids, book_type=None,
                                             chunk_size=24):
                self.requests.append(list(ids))
                return [{"Id": value, "Title": "历史%s" % value} for value in ids]

        api = HistoryApi()
        ctx = ImmediateContext(api)
        history._load(ctx)
        self.assertEqual(history.STATE["history_ids"], list(range(1, 61)))
        rows = history._layout(ctx)[2]
        pages = (60 + rows - 1) // rows
        self.assertGreater(pages, 1)
        self.assertTrue(all(len(chunk) <= 24 for chunk in api.requests))
        history.STATE["page"] = pages - 1
        history._load(ctx, reset_page=False)
        self.assertEqual(history.STATE["page"], pages - 1)
        self.assertEqual(
            history.STATE["items"][0]["Title"],
            "历史%s" % ((pages - 1) * rows + 1),
        )

    def test_series_server_pagination_uses_total_pages(self):
        class SeriesApi:
            def __init__(self):
                self.calls = []

            def get_books_by_series(self, name, page=1, size=24, order="latest",
                                    ignore_japanese=False, ignore_ai=False):
                self.calls.append((page, size))
                start = (page - 1) * size
                data = [
                    {"Id": value, "Title": "卷%s" % value}
                    for value in range(start + 1, min(start + size, 60) + 1)
                ]
                return {
                    "Data": data, "Page": page,
                    "TotalPages": (60 + size - 1) // size,
                }

        api = SeriesApi()
        ctx = ImmediateContext(api)
        series._load(ctx, page=0)
        rows = series._layout(ctx)[2]
        self.assertEqual(api.calls[0], (1, rows))
        total = series.STATE["total_pages"]
        self.assertEqual(total, (60 + rows - 1) // rows)
        series._load(ctx, page=total - 1)
        self.assertEqual(api.calls[-1], (total, rows))
        self.assertEqual(series.STATE["page"], total - 1)
        ids = [item["Id"] for item in series._page_items(ctx)]
        self.assertEqual(ids[-1], 60)

    def test_series_enter_keeps_page_when_returning(self):
        captured = {}

        class Api:
            @staticmethod
            def get_books_by_series(name, page=1, size=24, **kwargs):
                captured["page"] = page
                captured["size"] = size
                return {
                    "Data": [{"Id": 7, "Title": "卷"}],
                    "Page": page, "TotalPages": 5,
                }

        ctx = ImmediateContext(Api(), returning=True, params={
            "title": "系列", "series_name": "系列", "current_id": 3,
            "books": [{"Id": 3, "Title": "当前"}],
        })
        series.STATE["page"] = 2
        series.enter(ctx)
        self.assertEqual(captured["page"], 3)
        self.assertEqual(series.STATE["page"], 2)

    def test_series_local_page_survives_return(self):
        api = MagicMock()
        ctx = ImmediateContext(api, returning=True, params={
            "title": "系列", "series_name": "系列", "current_id": 1,
            "books": [{"Id": 1}, {"Id": 2}, {"Id": 3}],
        })
        series.STATE["page"] = 1
        series.enter(ctx)
        self.assertEqual(series.STATE["page"], 1)
        api.get_books_by_series.assert_not_called()


if __name__ == "__main__":
    unittest.main()
