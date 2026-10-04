import importlib.util
import unittest
from pathlib import Path
from types import SimpleNamespace


MODULE_PATH = (
    Path(__file__).resolve().parents[1]
    / "bin"
    / "src"
    / "screen"
    / "input"
    / "parser.py"
)
SPEC = importlib.util.spec_from_file_location("screen_input_parser", MODULE_PATH)
PARSER_MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PARSER_MODULE)

MultiTouchParser = PARSER_MODULE.MultiTouchParser
ecodes = PARSER_MODULE.ecodes


def event(event_type, code, value):
    return SimpleNamespace(type=event_type, code=code, value=value)


def timed_event(event_type, code, value, timestamp):
    return SimpleNamespace(
        type=event_type, code=code, value=value,
        timestamp=lambda: timestamp)


class TestMultiTouchParser(unittest.TestCase):
    def test_reset_discards_pressed_gesture(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)

        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_SLOT, 0))
        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, 7))
        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_POSITION_X, 100))
        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_POSITION_Y, 200))
        self.assertEqual(parser.slots[0]["tracking_id"], 7)
        self.assertEqual(parser.tracking_to_slot[7], 0)

        parser.reset()

        self.assertEqual(parser.slots, {})
        self.assertEqual(parser.tracking_to_slot, {})
        self.assertEqual(parser.current_slot, 0)
        self.assertEqual(parser.btn_touch, 0)

        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1))

        self.assertEqual(gestures, [])


    def test_position_event_after_reset_does_not_raise(self):
        parser = MultiTouchParser(on_gesture=lambda _gesture: None)
        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_SLOT, 0))
        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, 7))

        # 电源线程在两个坐标事件之间清空触摸状态（挂起/唤醒）
        parser.reset()

        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_POSITION_X, 100))
        parser.handle_event(event(ecodes.EV_ABS, ecodes.ABS_MT_POSITION_Y, 200))

        self.assertIn(0, parser.slots)
        self.assertIsNotNone(parser.slots[0]["x"])
        self.assertIsNotNone(parser.slots[0]["y"])

    def _press(self, parser, payload, ts=1.0, tracking_id=7):
        parser.handle_event(timed_event(ecodes.EV_ABS, ecodes.ABS_MT_SLOT, 0, ts))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, tracking_id, ts))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_POSITION_X, payload[0], ts))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_POSITION_Y, payload[1], ts))

    def test_slow_press_is_tap_not_long(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1, 1.45))
        self.assertEqual(len(gestures), 1)
        self.assertEqual(gestures[0].kind, "tap")

    def test_stationary_long_press_is_long(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1, 1.80))
        self.assertEqual(gestures[0].kind, "long")

    def test_slight_drag_is_tap_not_unknown(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_POSITION_X, 150, 1.10))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1, 1.15))
        self.assertEqual(len(gestures), 1)
        self.assertEqual(gestures[0].kind, "tap")

    def test_moving_long_press_is_tap(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_POSITION_X, 150, 1.20))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1, 1.80))
        self.assertEqual(gestures[0].kind, "tap")

    def test_full_distance_is_swipe(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_POSITION_X, 180, 1.10))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1, 1.20))
        self.assertEqual(gestures[0].kind, "right")

    def test_btn_touch_release_without_tracking_id_emits_gesture(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_KEY, ecodes.BTN_TOUCH, 0, 1.15))
        self.assertEqual(len(gestures), 1)
        self.assertEqual(gestures[0].kind, "tap")
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, -1, 1.20))
        self.assertEqual(len(gestures), 1)

    def test_tracking_id_replacement_finalizes_previous(self):
        gestures = []
        parser = MultiTouchParser(on_gesture=gestures.append)
        self._press(parser, (100, 200))
        parser.handle_event(timed_event(
            ecodes.EV_ABS, ecodes.ABS_MT_TRACKING_ID, 8, 1.10))
        self.assertEqual(len(gestures), 1)
        self.assertEqual(gestures[0].tracking_id, 7)
        self.assertNotIn(7, parser.tracking_to_slot)


if __name__ == "__main__":
    unittest.main()
