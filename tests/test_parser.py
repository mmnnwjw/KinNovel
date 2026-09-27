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


if __name__ == "__main__":
    unittest.main()
