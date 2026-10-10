import unittest

from kinnovel import widgets


class HitTestTests(unittest.TestCase):
    def test_returns_matching_key(self):
        rects = {"a": (0, 0, 10, 10), "b": (20, 20, 10, 10)}
        self.assertEqual(widgets.hit_test(rects, 5, 5), "a")
        self.assertEqual(widgets.hit_test(rects, 25, 25), "b")

    def test_returns_none_when_no_match(self):
        rects = {"a": (0, 0, 10, 10)}
        self.assertIsNone(widgets.hit_test(rects, 50, 50))

    def test_boundaries_are_half_open(self):
        rects = {"a": (0, 0, 10, 10)}
        self.assertEqual(widgets.hit_test(rects, 0, 0), "a")
        self.assertIsNone(widgets.hit_test(rects, 10, 10))


class FakeContext:
    def __init__(self):
        self.calls = []

    def run_async(self, owner_page, fetch, on_success=None, on_error=None,
                  refresh=True, sticky=False):
        self.calls.append((owner_page, sticky))
        try:
            result = fetch()
        except Exception as exc:
            if on_error:
                on_error(exc)
            return
        if on_success:
            on_success(result)


class ListLoaderTests(unittest.TestCase):
    def setUp(self):
        self.state = {"items": []}
        self.loader = widgets.ListLoader(self.state)

    def test_initializes_defaults(self):
        self.assertEqual(self.state["generation"], 0)
        self.assertFalse(self.state["loading"])
        self.assertFalse(self.state["loaded"])
        self.assertEqual(self.state["error"], "")

    def test_success_marks_loaded_and_calls_on_done(self):
        ctx = FakeContext()
        captured = []
        self.loader.start(ctx, "rank", lambda: [1, 2, 3], on_done=captured.append)
        self.assertEqual(captured, [[1, 2, 3]])
        self.assertFalse(self.state["loading"])
        self.assertTrue(self.state["loaded"])
        self.assertEqual(self.state["error"], "")

    def test_error_marks_error_and_calls_on_error(self):
        ctx = FakeContext()
        captured = []

        def fetch():
            raise RuntimeError("offline")

        self.loader.start(ctx, "rank", fetch, on_error=lambda exc: captured.append(str(exc)))
        self.assertEqual(captured, ["offline"])
        self.assertEqual(self.state["error"], "offline")
        self.assertFalse(self.state["loading"])

    def test_stale_result_is_rejected(self):
        # 模拟异步请求乱序返回: 第一次请求的回调在第二次请求发起之后才执行,
        # 此时 generation 已经不匹配, on_done 不应该被调用。
        pending = []

        class DeferredContext:
            def run_async(self, owner_page, fetch, on_success=None,
                          on_error=None, refresh=True, sticky=False):
                pending.append((fetch, on_success, on_error))

        ctx = DeferredContext()
        done_calls = []
        self.loader.start(ctx, "rank", lambda: "first", on_done=done_calls.append)
        self.loader.start(ctx, "rank", lambda: "second", on_done=done_calls.append)

        # 先执行"第一次"请求遗留的回调(模拟慢请求后到达)
        first_fetch, first_success, _ = pending[0]
        first_success(first_fetch())
        self.assertEqual(done_calls, [])

        second_fetch, second_success, _ = pending[1]
        second_success(second_fetch())
        self.assertEqual(done_calls, ["second"])

    def test_is_current_reflects_latest_generation(self):
        ctx = FakeContext()
        generation = self.loader.start(ctx, "rank", lambda: 1)
        self.assertTrue(self.loader.is_current(generation))
        self.loader.start(ctx, "rank", lambda: 2)
        self.assertFalse(self.loader.is_current(generation))


class Theme:
    def __init__(self):
        self.foreground = 0
        self.background = 255
        self.inverse_fg = 255
        self.inverse_bg = 0
        self.muted = 105
        self.mid = 170


class FakeDraw:
    def __init__(self):
        self.calls = []

    def textbbox(self, _xy, text, font=None):
        return (0, 0, len(str(text)) * 6, 12)

    def textlength(self, text, font=None):
        return len(str(text)) * 6

    def text(self, *args, **kwargs):
        self.calls.append(("text", args, kwargs))

    def rounded_rectangle(self, *args, **kwargs):
        self.calls.append(("rounded_rectangle", args, kwargs))


class FakeCanvas:
    """最小 Canvas 替身, 只提供 pager_bar/row_card/error_state 用到的方法。"""

    def __init__(self, width=1072, height=1448):
        self.width = width
        self.height = height
        self.theme = Theme()
        self.draw = FakeDraw()
        self.fonts = {"tiny": None, "small": None, "body": None}

    def button(self, rect, label, active=True, font=None):
        self.draw.calls.append(("button", rect, label, active))

    def centered_text(self, value, font, cx, cy, fill=None):
        self.draw.calls.append(("centered_text", value, cx, cy))

    def fit_text(self, text, font, max_width):
        text = str(text or "")
        return text if len(text) * 6 <= max_width else text[:max_width // 6] + "…"

    def wrap(self, text, font, max_width):
        text = str(text or "")
        chunk = max(1, max_width // 6)
        return [text[i:i + chunk] for i in range(0, len(text), chunk)] or [""]

    def text(self, *args, **kwargs):
        self.draw.calls.append(("text", args, kwargs))


class FakeFonts(dict):
    def __getitem__(self, key):
        return type("Font", (), {"size": 14})()


class FakeCtx:
    def __init__(self, height=1448):
        self.height = height
        self.fonts = FakeFonts()


class PagerBarTests(unittest.TestCase):
    def test_registers_only_prev_and_next(self):
        canvas = FakeCanvas()
        ctx = FakeCtx()
        rects = {}
        widgets.pager_bar(canvas, ctx, (40, 1300, 992, 88), True, False, "2/5", rects)
        self.assertIn(("prev", 0), rects)
        self.assertIn(("next", 0), rects)
        self.assertNotIn(("count", 0), rects)
        self.assertEqual(len(rects), 2)

    def test_disabled_states_tracked_on_button_calls(self):
        canvas = FakeCanvas()
        ctx = FakeCtx()
        rects = {}
        widgets.pager_bar(canvas, ctx, (40, 1300, 992, 88), False, True, "1/5", rects)
        button_calls = [call for call in canvas.draw.calls if call[0] == "button"]
        prev_call = next(call for call in button_calls if call[2] == "上一页")
        next_call = next(call for call in button_calls if call[2] == "下一页")
        self.assertFalse(prev_call[3])
        self.assertTrue(next_call[3])

    def test_rect_heights_match_requested_pager_height(self):
        canvas = FakeCanvas()
        ctx = FakeCtx()
        rects = {}
        prev_rect, next_rect = widgets.pager_bar(
            canvas, ctx, (40, 1300, 992, 88), True, True, "3/5", rects)
        self.assertEqual(prev_rect[3], 88)
        self.assertEqual(next_rect[3], 88)


class LayoutHelperTests(unittest.TestCase):
    def test_pager_height_has_floor(self):
        ctx = FakeCtx(height=200)
        self.assertGreaterEqual(widgets.pager_height(ctx), 88)

    def test_list_row_height_has_floor(self):
        ctx = FakeCtx(height=200)
        self.assertGreaterEqual(widgets.list_row_height(ctx), 96)

    def test_list_row_height_scales_with_screen(self):
        small = widgets.list_row_height(FakeCtx(height=800))
        large = widgets.list_row_height(FakeCtx(height=3000))
        self.assertGreater(large, small)


if __name__ == "__main__":
    unittest.main()
