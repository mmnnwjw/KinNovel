# 触控解析器
import time
from dataclasses import dataclass

try:
    from evdev import ecodes
except ImportError:
    class _FallbackEcodes:
        EV_KEY = 0x01
        EV_ABS = 0x03
        ABS_X = 0x00
        ABS_Y = 0x01
        ABS_MT_SLOT = 0x2f
        ABS_MT_POSITION_X = 0x35
        ABS_MT_POSITION_Y = 0x36
        ABS_MT_TRACKING_ID = 0x39
        BTN_TOUCH = 0x14a

    ecodes = _FallbackEcodes()


@dataclass
class GestureConfig:
    # 静止按压: 低于 long_press_min_duration_s 的慢按仍按 tap 处理,
    # 避免电子墨水屏上常见的"慢一点的点击"被误判成长按。
    tap_max_move_px: int = 40
    tap_max_duration_s: float = 0.35
    long_press_min_duration_s: float = 0.55
    # 超过这个位移才算滑动, 40-63px 的轻微移动仍按 tap 处理, 不留死区。
    swipe_min_distance_px: int = 64
    scale_x: float = 1.0
    scale_y: float = 1.0


@dataclass
class Gesture:
    kind: str
    slot: int
    tracking_id: int
    start: tuple
    end: tuple
    duration: float
    distance: int
    timestamp: float


class MultiTouchParser:
    """跟踪多点触控 slot 状态,并把 evdev 事件流转换为手势回调.

    用法示例::

        parser = MultiTouchParser(
            on_down=lambda s, t, x, y: print("按下", x, y),
            on_gesture=lambda g: print("手势", g.kind, "起点", g.start, "终点", g.end),
        )
        read_loop(dev, parser.handle_event)
    """
    _SYNTHETIC_TRACKING_ID = -2

    def __init__(self, config=None, on_down=None, on_move=None, on_up=None, on_gesture=None):
        self.config = config or GestureConfig()
        self.on_down_cb = on_down
        self.on_move_cb = on_move
        self.on_up_cb = on_up
        self.on_gesture_cb = on_gesture

        self.slots = {}
        self.current_slot = 0
        self.tracking_to_slot = {}
        self.btn_touch = 0
        self.primary_slot = None

    def reset(self):
        """Discard all in-progress touch and gesture state."""
        self.slots.clear()
        self.current_slot = 0
        self.tracking_to_slot.clear()
        self.btn_touch = 0
        self.primary_slot = None

    def _scale(self, x, y):
        return int(x * self.config.scale_x), int(y * self.config.scale_y)

    @staticmethod
    def _event_time(ev):
        timestamp = getattr(ev, "timestamp", None)
        if callable(timestamp):
            try:
                value = float(timestamp())
                if value > 0:
                    return value
            except Exception:
                pass
        return time.time()

    def handle_event(self, ev):
        now = self._event_time(ev)
        if ev.type == ecodes.EV_ABS:
            code = ev.code
            val = ev.value
            if code == ecodes.ABS_MT_SLOT:
                self.current_slot = val
                if self.current_slot not in self.slots:
                    self.slots[self.current_slot] = {"tracking_id": -1, "x": None, "y": None}
            elif code == ecodes.ABS_MT_TRACKING_ID:
                tid = val
                slot = self.current_slot
                if slot not in self.slots:
                    self.slots[slot] = {"tracking_id": -1, "x": None, "y": None}
                prev_tid = self.slots[slot].get("tracking_id", -1)
                if tid == -1:
                    if slot == self.primary_slot:
                        if prev_tid != -1:
                            self._do_up(slot, prev_tid, now=now)
                        self._clear_all_slots()
                    else:
                        self._clear_slot(slot, prev_tid)
                else:
                    if prev_tid not in (-1, None) and prev_tid != tid:
                        if prev_tid == self._SYNTHETIC_TRACKING_ID:
                            # BTN_TOUCH 先于 MT tracking id,收到真实 id 后升级,
                            # 不把合成触点当成一次已完成的触摸。
                            old = self.slots[slot]
                            self.tracking_to_slot.pop(prev_tid, None)
                            self.slots[slot] = {
                                "tracking_id": tid,
                                "x": old.get("x"),
                                "y": old.get("y"),
                                "start_ts": old.get("start_ts", now),
                                "last_ts": now,
                                "start_pos": old.get("start_pos"),
                                "down_reported": old.get("down_reported", False),
                            }
                        else:
                            # 有些驱动直接换 tracking id 而不发 -1, 先结算旧手势。
                            self._do_up(slot, prev_tid, now=now)
                            self.tracking_to_slot.pop(prev_tid, None)
                            self.slots[slot] = {
                                "tracking_id": tid,
                                "x": None,
                                "y": None,
                                "start_ts": now,
                                "last_ts": now,
                                "start_pos": None,
                            }
                    else:
                        self.slots[slot] = {
                            "tracking_id": tid,
                            "x": None,
                            "y": None,
                            "start_ts": now,
                            "last_ts": now,
                            "start_pos": None,
                        }
                    self.tracking_to_slot[tid] = slot
                    if self.primary_slot is None:
                        self.primary_slot = slot
            elif code in (ecodes.ABS_MT_POSITION_X, ecodes.ABS_X):
                slot = self.current_slot
                self._ensure_slot(slot)
                if code == ecodes.ABS_X and self.btn_touch == 1:
                    self._start_btn_touch_fallback(slot, now)
                x, _ = self._scale(val, 0)
                self._update_pos(slot, x, None, set_x=True, now=now)
            elif code in (ecodes.ABS_MT_POSITION_Y, ecodes.ABS_Y):
                slot = self.current_slot
                self._ensure_slot(slot)
                if code == ecodes.ABS_Y and self.btn_touch == 1:
                    self._start_btn_touch_fallback(slot, now)
                _, y = self._scale(0, val)
                self._update_pos(slot, None, y, set_x=False, now=now)

        elif ev.type == ecodes.EV_KEY:
            if ev.code == ecodes.BTN_TOUCH:
                self.btn_touch = ev.value
                if ev.value == 1:
                    self._begin_btn_touch(now)
                elif ev.value == 0:
                    # 部分驱动用 BTN_TOUCH=0 结束触摸而不发 tracking id -1。
                    self._finish_btn_touch(now)

    def _ensure_slot(self, slot):
        return self.slots.setdefault(
            slot, {"tracking_id": -1, "x": None, "y": None})

    def _clear_slot(self, slot, tracking_id=-1):
        self.slots[slot] = {"tracking_id": -1, "x": None, "y": None}
        if tracking_id != -1:
            self.tracking_to_slot.pop(tracking_id, None)

    def _clear_all_slots(self):
        self.slots.clear()
        self.tracking_to_slot.clear()
        self.primary_slot = None

    def _start_btn_touch_fallback(self, slot, now):
        """给只报 ABS_X/ABS_Y + BTN_TOUCH 的设备补一个合成触点。"""
        if self.primary_slot is not None:
            return
        state = self._ensure_slot(slot)
        if state.get("tracking_id", -1) != -1:
            self.primary_slot = slot
            return
        state.update({
            "tracking_id": self._SYNTHETIC_TRACKING_ID,
            "start_ts": now,
            "last_ts": now,
            "start_pos": None,
        })
        self.tracking_to_slot[self._SYNTHETIC_TRACKING_ID] = slot
        self.primary_slot = slot
        if self._slot_has_coords(slot):
            state["down_reported"] = True
            self._do_down(slot, state["tracking_id"], state["x"], state["y"])

    def _begin_btn_touch(self, now):
        """BTN_TOUCH 到达时,若单轴坐标已先到则立即建立合成触点。"""
        slot = self.current_slot
        state = self._ensure_slot(slot)
        if (state.get("tracking_id", -1) == -1
                and state.get("x") is not None
                and state.get("y") is not None):
            self._start_btn_touch_fallback(slot, now)

    def _finish_btn_touch(self, now):
        slot = self.primary_slot
        if slot is not None:
            state = self.slots.get(slot) or {}
            tid = state.get("tracking_id", -1)
            if tid != -1 and self._slot_has_coords(slot):
                self._do_up(slot, tid, now=now)
        self._clear_all_slots()

    def _update_pos(self, slot, x, y, set_x, now=None):
        # 电源线程在挂起/唤醒时会并发 reset() 清空 slots；setdefault 在 GIL 下
        # 是原子的，避免输入线程抛 KeyError 后触摸彻底失效
        s = self._ensure_slot(slot)
        if set_x:
            s["x"] = x
        else:
            s["y"] = y
        s["last_ts"] = now if now is not None else time.time()
        sp = s.get("start_pos")
        if sp is None:
            s["start_pos"] = (s.get("x"), s.get("y"))
        else:
            sx, sy = sp
            if sx is None and s.get("x") is not None:
                s["start_pos"] = (s["x"], sy)
            elif sy is None and s.get("y") is not None:
                s["start_pos"] = (sx, s["y"])
        if slot != self.primary_slot:
            return
        if self._slot_has_coords(slot) and "down_reported" not in s:
            s["down_reported"] = True
            self._do_down(slot, s["tracking_id"], s["x"], s["y"])
        elif self._slot_has_coords(slot):
            self._do_move(slot, s["tracking_id"], s["x"], s["y"])

    def _slot_has_coords(self, slot):
        s = self.slots.get(slot, {})
        return s.get("x") is not None and s.get("y") is not None and s.get("tracking_id", -1) != -1

    def _do_down(self, slot, tracking_id, x, y):
        if slot == self.primary_slot and self.on_down_cb is not None:
            self.on_down_cb(slot, tracking_id, x, y)

    def _do_move(self, slot, tracking_id, x, y):
        if slot == self.primary_slot and self.on_move_cb is not None:
            self.on_move_cb(slot, tracking_id, x, y)

    def _do_up(self, slot, tracking_id, now=None):
        if slot != self.primary_slot:
            return
        s = self.slots.get(slot, {})
        end_x = s.get("x")
        end_y = s.get("y")
        start_ts = s.get("start_ts")
        start_pos = s.get("start_pos", (end_x, end_y))
        now = now if now is not None else time.time()
        duration = max(0.0, (now - start_ts)) if start_ts else 0.0
        sx, sy = start_pos if start_pos else (None, None)
        ex, ey = end_x, end_y
        if sx is None:
            sx = ex
        if sy is None:
            sy = ey
        if ex is None:
            ex = sx
        if ey is None:
            ey = sy
        if None in (sx, sy, ex, ey):
            sx = sy = ex = ey = 0
        dx = ex - sx
        dy = ey - sy
        dist = (dx * dx + dy * dy) ** 0.5 if dx or dy else 0

        cfg = self.config
        kind = "unknown"
        if dist <= cfg.tap_max_move_px:
            if duration <= cfg.tap_max_duration_s:
                kind = "tap"
            elif duration >= cfg.long_press_min_duration_s:
                kind = "long"
            else:
                # 介于快速点按和长按之间: 按 tap 处理, 消除时间死区。
                kind = "tap"
        elif dist < cfg.swipe_min_distance_px:
            # 轻微移动更可能是没点稳, 按 tap 处理, 消除 40-63px 位移死区。
            kind = "tap"
        else:
            # 滑动方向
            if abs(dx) > abs(dy):
                kind = "left" if dx < 0 else "right"
            else:
                kind = "up" if dy < 0 else "down"

        gesture = Gesture(
            kind=kind,
            slot=slot,
            tracking_id=tracking_id,
            start=(sx, sy),
            end=(ex, ey),
            duration=duration,
            distance=int(dist),
            timestamp=now,
        )
        if self.on_up_cb is not None:
            self.on_up_cb(gesture)
        if self.on_gesture_cb is not None and kind != "unknown":
            self.on_gesture_cb(gesture)
