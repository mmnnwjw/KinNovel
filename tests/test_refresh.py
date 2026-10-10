import os
import sys
import types
import unittest
from collections import OrderedDict
from pathlib import Path
from unittest.mock import MagicMock, patch

from PIL import Image, ImageFont

from kinnovel import progress
from kinnovel.pages import reader
from kinnovel.ui import Canvas, ImageCache, PageContext, Theme


# ---------------------------------------------------------------------------
# Lightweight fakes for the PageContext-level refresh/idle/wake tests.
# These intentionally avoid fonts: the registered "draw" page only draws
# rectangles, so PageContext.render() never touches ctx.fonts.
# ---------------------------------------------------------------------------

class FakeOutput:
    def __init__(self, resolution=(200, 300), protocol=None,
                 reagl_waveform="REAGL", supports_swipe_animation=False):
        self.resolution = resolution
        self.protocol = protocol
        self.reagl_waveform = reagl_waveform
        self.supports_swipe_animation = supports_swipe_animation
        self.calls = []
        self.raise_oserror = False

    def show(self, image, is_flashing=False, **extra):
        if self.raise_oserror:
            raise OSError("fake output failure")
        call = {"is_flashing": is_flashing}
        call.update(extra)
        call["image"] = image.copy()
        self.calls.append(call)

    def set_swipe_direction(self, forward):
        pass

    def set_swipe_animations(self, enabled):
        pass


class FakeScreen:
    def __init__(self, output=None):
        self.output = output or FakeOutput()


class FakeConfig:
    def __init__(self, **overrides):
        self.values = {
            "night_mode": False,
            "page_flash": False,
            "page_turn_animation": False,
            "full_refresh_every": None,
        }
        self.values.update(overrides)

    def get(self, key, default=None):
        return self.values.get(key, default)

    def set(self, key, value, save=True):
        self.values[key] = value


class FakeApp:
    def __init__(self, output=None, config=None):
        self.screen = FakeScreen(output=output)
        self.config = config or FakeConfig()
        self.api = None
        self.images = None
        self.fonts = {}
        self.log_messages = []

    def log(self, message):
        self.log_messages.append(message)


class DrawPage:
    """Fake page module: paints a list of (x0, y0, x1, y1, fill) rectangles."""

    def __init__(self):
        self.boxes = []

    def render(self, ctx, canvas):
        for (x0, y0, x1, y1, fill) in self.boxes:
            canvas.draw.rectangle([x0, y0, x1, y1], fill=fill)


def make_ctx(output=None, config=None):
    app = FakeApp(output=output, config=config)
    ctx = PageContext(app)
    page = DrawPage()
    ctx.register("draw", page)
    ctx.page_name = "draw"
    return ctx, page


class RefreshPlanTests(unittest.TestCase):
    def test_first_show_is_full_refresh_not_flashing(self):
        ctx, _page = make_ctx()
        ctx.show()
        output = ctx.screen.output
        self.assertEqual(len(output.calls), 1)
        call = output.calls[0]
        self.assertNotIn("region", call)
        self.assertFalse(call["is_flashing"])

    def test_identical_second_show_is_skipped(self):
        ctx, _page = make_ctx()
        ctx.show()
        ctx.show()
        self.assertEqual(len(ctx.screen.output.calls), 1)

    def test_small_change_region_contains_changed_area(self):
        ctx, page = make_ctx()
        ctx.show()
        page.boxes = [(10, 10, 30, 30, 0)]
        ctx.show()
        output = ctx.screen.output
        self.assertEqual(len(output.calls), 2)
        call = output.calls[-1]
        self.assertFalse(call["is_flashing"])
        self.assertIn("region", call)
        x, y, w, h = call["region"]
        self.assertLessEqual(x, 10)
        self.assertLessEqual(y, 10)
        self.assertGreaterEqual(x + w, 30)
        self.assertGreaterEqual(y + h, 30)

    def test_ghost_budget_accumulates_then_flashes_and_resets(self):
        ctx, page = make_ctx()
        page.boxes = [(0, 0, 199, 219, 0)]
        ctx.show()
        self.assertAlmostEqual(ctx._ghost_budget, 1.0, places=3)

        flashed = False
        for i in range(20):
            fill = 0 if i % 2 == 0 else 128
            page.boxes = [(0, 0, 199, 219, fill)]
            ctx.show()
            call = ctx.screen.output.calls[-1]
            if call["is_flashing"]:
                flashed = True
                self.assertNotIn("region", call)
                self.assertEqual(ctx._ghost_budget, 0.0)
                break
        self.assertTrue(flashed, "expected accumulated ghost budget to trigger a flash")

    def test_small_updates_never_upgrade_to_flash(self):
        ctx, page = make_ctx()
        ctx.show()
        ctx._ghost_budget = 1000.0  # simulate a budget already far past threshold
        for i in range(10):
            fill = 0 if i % 2 == 0 else 50
            page.boxes = [(5, 5, 15, 15, fill)]
            ctx.show()
            call = ctx.screen.output.calls[-1]
            self.assertFalse(call["is_flashing"])
        # small updates still accumulate budget, they just never act on it
        self.assertGreater(ctx._ghost_budget, 1000.0)

    def test_explicit_region_show_leaves_outside_pixels_stale(self):
        ctx, page = make_ctx()
        page.boxes = [(0, 0, 49, 49, 0), (150, 250, 189, 289, 0)]
        ctx.show()
        page.boxes = [(0, 0, 49, 49, 200), (150, 250, 189, 289, 200)]
        ctx.show(region=(0, 0, 50, 50))
        output = ctx.screen.output
        call = output.calls[-1]
        self.assertFalse(call["is_flashing"])
        self.assertEqual(call["region"], (0, 0, 50, 50))
        frame = ctx._last_frame
        self.assertIsNotNone(frame)
        # inside the explicit region: picks up the new rendered content
        self.assertEqual(frame.getpixel((10, 10)), 200)
        # outside the explicit region: screen physically never changed there
        self.assertEqual(frame.getpixel((160, 260)), 0)

    def test_is_flashing_true_forces_full_refresh_and_resets_budget(self):
        ctx, page = make_ctx()
        ctx.show()
        ctx._ghost_budget = 9.0
        page.boxes = [(5, 5, 9, 9, 5)]
        ctx.show(is_flashing=True, region=(5, 5, 10, 10))
        call = ctx.screen.output.calls[-1]
        self.assertTrue(call["is_flashing"])
        self.assertNotIn("region", call)
        self.assertEqual(ctx._ghost_budget, 0.0)

    def test_invalidate_frame_forces_next_show_full(self):
        ctx, page = make_ctx()
        page.boxes = [(10, 10, 20, 20, 0)]
        ctx.show()
        page.boxes = [(10, 10, 20, 20, 50)]
        ctx.show()
        self.assertIn("region", ctx.screen.output.calls[-1])

        ctx.invalidate_frame()
        self.assertIsNone(ctx._last_frame)

        page.boxes = [(10, 10, 20, 20, 90)]
        ctx.show()
        call = ctx.screen.output.calls[-1]
        self.assertNotIn("region", call)

    def test_turn_kind_with_page_flash_config_forces_flash(self):
        config = FakeConfig(page_flash=True)
        ctx, page = make_ctx(config=config)
        page.boxes = [(0, 0, 199, 219, 0)]
        ctx.show()
        page.boxes = [(0, 0, 199, 219, 128)]
        ctx.show(kind="turn")
        call = ctx.screen.output.calls[-1]
        self.assertTrue(call["is_flashing"])
        self.assertNotIn("region", call)

    def test_waveform_for_turn_mtk_not_flashing(self):
        output = FakeOutput(protocol="mtk", reagl_waveform="REAGL_X")
        ctx, _page = make_ctx(output=output)
        self.assertEqual(ctx._waveform_for("turn", False), "REAGL_X")
        self.assertIsNone(ctx._waveform_for("turn", True))
        self.assertIsNone(ctx._waveform_for("ui", False))

    def test_waveform_for_non_mtk_protocol_is_none(self):
        output = FakeOutput(protocol="other", reagl_waveform="REAGL_X")
        ctx, _page = make_ctx(output=output)
        self.assertIsNone(ctx._waveform_for("turn", False))

    def test_oserror_from_output_show_resets_last_frame(self):
        ctx, page = make_ctx()
        ctx.show()
        self.assertIsNotNone(ctx._last_frame)
        ctx.screen.output.raise_oserror = True
        page.boxes = [(1, 1, 2, 2, 10)]
        ctx.show()  # must swallow OSError internally
        self.assertIsNone(ctx._last_frame)


class IdleTaskTests(unittest.TestCase):
    def test_fifo_order_key_replace_and_cancel(self):
        ctx, _page = make_ctx()
        order = []
        ctx.idle("a", lambda: order.append("a1"))
        ctx.idle("b", lambda: order.append("b"))
        ctx.idle("a", lambda: order.append("a2"))  # same key: replace, move to end
        ctx.idle("c", lambda: order.append("c"))
        ctx.cancel_idle("c")

        self.assertTrue(ctx.run_idle_task())
        self.assertFalse(ctx.run_idle_task())
        self.assertEqual(order, ["b", "a2"])
        # nothing left; calling again is a harmless no-op
        self.assertFalse(ctx.run_idle_task())

    def test_exception_in_task_is_logged_not_raised(self):
        ctx, _page = make_ctx()

        def boom():
            raise ValueError("boom")

        ctx.idle("x", boom)
        result = ctx.run_idle_task()
        self.assertFalse(result)
        self.assertTrue(ctx.app.log_messages)


class NextTimeoutTests(unittest.TestCase):
    def test_zero_when_work_is_pending(self):
        ctx, _page = make_ctx()
        ctx.idle("x", lambda: None)
        self.assertEqual(ctx.next_timeout(), 0.0)
        ctx.cancel_idle("x")

        ctx._refresh_requested = True
        self.assertEqual(ctx.next_timeout(), 0.0)
        ctx._refresh_requested = False

        ctx._ui_queue.put(lambda: None)
        self.assertEqual(ctx.next_timeout(), 0.0)
        ctx._ui_queue.get_nowait()

    def test_default_range_when_idle(self):
        ctx, _page = make_ctx()
        value = ctx.next_timeout()
        self.assertGreaterEqual(value, 0.05)
        self.assertLessEqual(value, 60.1)


class WakePipeTests(unittest.TestCase):
    def test_post_wakes_fd_and_consume_wake_drains_it(self):
        ctx, _page = make_ctx()
        fd = ctx.wake_fd()
        if fd is None:
            self.skipTest("wake pipe unsupported on this platform")

        ctx._ui_loop_running = True
        ctx.post(lambda: None)

        if sys.platform == "win32":
            # select() does not support pipe handles on Windows.
            data = os.read(fd, 10)
            self.assertEqual(data, b"\x00")
        else:
            import select
            ready, _, _ = select.select([fd], [], [], 1.0)
            self.assertIn(fd, ready)
            ctx.consume_wake()
            return

        ctx.consume_wake()
        with self.assertRaises(BlockingIOError):
            os.read(fd, 10)

        # drain the queued callback so it does not leak into other asserts
        ctx.drain_ui_queue()


# ---------------------------------------------------------------------------
# ScreenInput.listen: on_background only fires when truly idle.
# ---------------------------------------------------------------------------

def _install_fake_evdev():
    try:
        import evdev  # noqa: F401
        return
    except ImportError:
        pass
    fake_ecodes = types.SimpleNamespace(
        EV_ABS=3, EV_KEY=1,
        ABS_MT_POSITION_X=53, ABS_MT_POSITION_Y=54,
        ABS_X=0, ABS_Y=1,
        ABS_MT_SLOT=47, ABS_MT_TRACKING_ID=57,
        INPUT_PROP_DIRECT=1,
        BTN_TOUCH=330,
    )
    fake_evdev = types.ModuleType("evdev")
    fake_evdev.InputDevice = object
    fake_evdev.ecodes = fake_ecodes
    fake_evdev.list_devices = lambda: []
    sys.modules["evdev"] = fake_evdev


def _install_screen_package_stub():
    if "screen" in sys.modules:
        return
    src_root = Path(__file__).resolve().parents[1] / "bin" / "src"
    stub = types.ModuleType("screen")
    stub.__path__ = [str(src_root / "screen")]
    sys.modules["screen"] = stub


_install_fake_evdev()
_install_screen_package_stub()

from screen.input.screen import ScreenInput  # noqa: E402


class ScreenInputBackgroundTests(unittest.TestCase):
    def test_on_background_only_runs_when_idle(self):
        screen_input = ScreenInput(screen=None)
        screen_input.device = object()

        pending_calls = {"count": 0}
        background_calls = []

        def on_background():
            background_calls.append(True)
            raise KeyboardInterrupt()  # deterministic way out of listen()'s loop

        def fake_select(rlist, wlist, xlist, timeout):
            if timeout == 0:
                pending_calls["count"] += 1
                if pending_calls["count"] == 1:
                    # first pending check: device still "has pending input"
                    return (list(rlist), [], [])
                return ([], [], [])
            return ([], [], [])

        with patch("select.select", side_effect=fake_select):
            screen_input.listen(on_background=on_background)

        self.assertEqual(background_calls, [True])
        self.assertEqual(pending_calls["count"], 2)


# ---------------------------------------------------------------------------
# Reader: prerender idle task + swipe-no-longer-double-shows.
# ---------------------------------------------------------------------------

FONT_PATH = next((path for path in (
    "C:/Windows/Fonts/simhei.ttf",
    "/usr/java/lib/fonts/STHeitiMedium.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
) if Path(path).exists()), "")


class ReaderOutput:
    def __init__(self):
        self.resolution = (1072, 1448)
        self.calls = []

    def show(self, image, is_flashing=False, **extra):
        call = {"is_flashing": is_flashing}
        call.update(extra)
        self.calls.append(call)

    def set_swipe_direction(self, forward):
        pass

    def set_swipe_animations(self, enabled):
        pass


class ReaderScreen:
    def __init__(self):
        self.output = ReaderOutput()


class ReaderConfig:
    def __init__(self):
        self.values = {
            "night_mode": False,
            "font_size": 48,
            "line_spacing": 1.42,
            "reader_margin": 30,
            "first_line_indent": True,
            "page_flash": False,
            "page_turn_animation": False,
            "strict_tls": False,
            "font_path": FONT_PATH,
            "convert": None,
            "reader_guide_dismissed": True,
        }

    def get(self, key, default=None):
        return self.values.get(key, default)

    def set(self, key, value, save=True):
        self.values[key] = value


class ReaderApi:
    server = "https://api.lightnovel.life"
    user = None


class ReaderApp:
    def __init__(self):
        self.screen = ReaderScreen()
        self.config = ReaderConfig()
        self.api = ReaderApi()
        self.images = ImageCache()
        self.fonts = {
            "hero": ImageFont.truetype(self.config.get("font_path"), 70),
            "title": ImageFont.truetype(self.config.get("font_path"), 44),
            "body": ImageFont.truetype(self.config.get("font_path"), 34),
            "small": ImageFont.truetype(self.config.get("font_path"), 28),
            "tiny": ImageFont.truetype(self.config.get("font_path"), 23),
        }

    def log(self, message):
        pass


class ReaderPrerenderAndSwipeTests(unittest.TestCase):
    def setUp(self):
        if not FONT_PATH:
            self.skipTest("no CJK test font available")
        progress.clear()
        self.app = ReaderApp()
        self.ctx = PageContext(self.app)

        class Catalog:
            @staticmethod
            def render(_ctx, _canvas):
                return None

        self.ctx.register("reader", reader)
        self.ctx.register("catalog", Catalog)

        self.doc = MagicMock()
        self.doc.page_count = 5
        self.doc.pages = [[] for _ in range(5)]
        self.doc.first_anchor_on_page.return_value = ("./p[1]", 0)

        reader.STATE.update({
            "book_id": 1,
            "sort_num": 2,
            "data": {"Chapter": {"Title": "T", "Chapters": ["a", "b"]}},
            "doc": self.doc,
            "page": 1,
            "loading": False,
            "chrome_visible": False,
            "fullscreen_image": None,
            "content_cache": OrderedDict(),
            "fitted_cache": {},
            "image_rects": {},
            "image_retry_rects": {},
            "rects": {},
            "pending_turn": None,
            "pending_turn_upload": False,
            "document_version": 0,
            "image_generation": 0,
            "last_turn_at": 0.0,
            "last_saved": -1,
            "last_durable_saved": -1,
            "last_save_at": 0.0,
        })
        self.ctx.page_name = "reader"
        self.ctx.params = {"book_id": 1, "sort_num": 2}
        reader.STATE["signature"] = reader._signature(self.ctx, 1, 2)

    def tearDown(self):
        reader._READER_CONTEXT = None
        reader._READER_ACTIVE = False

    def _render_once(self):
        image = Image.new("L", (self.ctx.width, self.ctx.height), 255)
        canvas = Canvas(image, self.ctx.fonts, Theme(False))
        reader.render(self.ctx, canvas)

    def test_prerender_fills_next_then_previous_page_cache(self):
        self._render_once()
        self.assertIn("reader-prerender", self.ctx._idle_tasks)

        _, content_height = reader._layout_metrics(self.ctx)
        key_next = reader._content_cache_key(self.ctx, 2, content_height)
        key_prev = reader._content_cache_key(self.ctx, 0, content_height)

        # first idle task: renders page+1 and re-schedules itself
        self.assertTrue(self.ctx.run_idle_task())
        self.assertIn(key_next, reader.STATE["content_cache"])
        self.assertNotIn(key_prev, reader.STATE["content_cache"])

        # second idle task: skips the now-cached page+1, renders page-1,
        # and re-schedules itself again (order still has an untried slot
        # semantically, even though both pages end up cached after this)
        self.assertTrue(self.ctx.run_idle_task())
        self.assertIn(key_prev, reader.STATE["content_cache"])

        # third idle task: both neighbours already cached, nothing to do,
        # no further task gets scheduled
        self.assertFalse(self.ctx.run_idle_task())
        self.assertNotIn("reader-prerender", self.ctx._idle_tasks)

    def test_prerender_noop_if_page_changed_before_running(self):
        self._render_once()
        self.assertIn("reader-prerender", self.ctx._idle_tasks)

        reader.STATE["page"] = 3  # page moved on before the idle task runs

        _, content_height = reader._layout_metrics(self.ctx)
        key_next = reader._content_cache_key(self.ctx, 2, content_height)

        self.ctx.run_idle_task()
        self.assertNotIn(key_next, reader.STATE["content_cache"])

    def test_swipe_left_with_chrome_visible_shows_exactly_once(self):
        reader.STATE["chrome_visible"] = True
        reader.STATE["page"] = 1

        with patch.object(self.ctx, "show", wraps=self.ctx.show) as show_spy:
            reader.handle({"gesture": "left"}, self.ctx)

        self.assertEqual(show_spy.call_count, 1)
        self.assertFalse(reader.STATE["chrome_visible"])
        self.assertEqual(reader.STATE["page"], 2)


if __name__ == "__main__":
    unittest.main()
