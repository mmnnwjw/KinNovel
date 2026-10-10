//! 手势识别 (纯逻辑, 主机可测)。
//!
//! 移植自 Python `bin/src/screen/input/parser.py` 的 `MultiTouchParser` + `GestureConfig`, 行为保持一致:
//! - 只跟踪主触点 (第一个按下的 slot); 其它手指忽略。
//! - 位移 <= tap_max_move 且时长 <= tap_max_duration → Tap;
//!   时长 >= long_press_min_duration → Long; 介于两者之间仍算 Tap (无死区)。
//! - 位移 < swipe_min_distance → Tap (轻微移动当作没点稳); 否则按主方向得到四向 Swipe。
//! - 兼容只报 ABS_X/ABS_Y + BTN_TOUCH 的单点设备, 以及不发 tracking_id = -1 直接换 id 的驱动。
//!
//! 新增 (Python 版没有): 主触点坐标齐全时立即发出 `GestureKind::Down`, 供上层做按下反馈;
//! 触摸被取消 (reset) 时不发出任何手势。
//! 输入为已换算到屏幕像素的 `TouchSample` (坐标变换在 input.rs 中按 DeviceInfo 完成)。

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureConfig {
    pub tap_max_move_px: u32,
    pub tap_max_duration_ms: u32,
    pub long_press_min_duration_ms: u32,
    pub swipe_min_distance_px: u32,
}

impl Default for GestureConfig {
    fn default() -> Self {
        // 与 Python 版一致; 像素阈值按 300 ppi 设计, 上层可按 dpi 缩放
        GestureConfig {
            tap_max_move_px: 40,
            tap_max_duration_ms: 350,
            long_press_min_duration_ms: 550,
            swipe_min_distance_px: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureKind {
    /// 手指刚按下 (仅 start 有效)
    Down,
    Tap,
    Long,
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    pub kind: GestureKind,
    pub start: (i32, i32),
    pub end: (i32, i32),
    pub duration_ms: u32,
}

/// 一次 evdev SYN_REPORT 之间累积的原始触摸状态, 已换算为屏幕像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchSample {
    /// slot 切换 (ABS_MT_SLOT)
    Slot(i32),
    /// tracking id (ABS_MT_TRACKING_ID), -1 表示抬起
    TrackingId(i32),
    X(i32),
    Y(i32),
    /// BTN_TOUCH 按下/抬起 (单点设备)
    BtnTouch(bool),
    /// 事件时间戳 (毫秒, 单调)。读取层在每个原始事件之前都喂一次。
    Sync(u64),
    /// SYN_REPORT: 一个完整数据包结束 (按下反馈 Down 在这里发出, 此时坐标才完整)
    Report,
}

/// 合成 tracking id, 给只报 ABS_X/ABS_Y + BTN_TOUCH 的设备补一个虚拟触点。
const SYNTHETIC_TRACKING_ID: i32 = -2;

#[derive(Clone, Debug, Default)]
struct SlotState {
    tracking_id: i32,
    x: Option<i32>,
    y: Option<i32>,
    start_ts: u64,
    last_ts: u64,
    start_pos: Option<(Option<i32>, Option<i32>)>,
    down_reported: bool,
}

impl SlotState {
    fn new_empty() -> Self {
        SlotState {
            tracking_id: -1,
            x: None,
            y: None,
            start_ts: 0,
            last_ts: 0,
            start_pos: None,
            down_reported: false,
        }
    }
    fn has_coords(&self) -> bool {
        self.x.is_some() && self.y.is_some() && self.tracking_id != -1
    }
}

pub struct GestureRecognizer {
    config: GestureConfig,
    slots: HashMap<i32, SlotState>,
    current_slot: i32,
    tracking_to_slot: HashMap<i32, i32>,
    btn_touch: bool,
    primary_slot: Option<i32>,
    /// 每个 slot 最近一次的坐标。MT 协议 B 下内核保留 slot 状态, **数值没变的轴不会重发**:
    /// 第二次点在同一 X (或同一 Y) 时只会收到另一轴。新触点必须从这里继承缺失的轴,
    /// 否则会得到 0 坐标 (实测: 在同一位置连点两次, 第二次被当成点了左上角)。抬起与 reset 都不清除。
    last_pos: HashMap<i32, (Option<i32>, Option<i32>)>,
    /// 最近一次看到的时间戳 (毫秒), BTN_TOUCH 事件没有自带的 Sync 时间戳时使用。
    now_ms: u64,
}

impl GestureRecognizer {
    pub fn new(config: GestureConfig) -> Self {
        GestureRecognizer {
            config,
            slots: HashMap::new(),
            current_slot: 0,
            tracking_to_slot: HashMap::new(),
            btn_touch: false,
            primary_slot: None,
            last_pos: HashMap::new(),
            now_ms: 0,
        }
    }

    /// 设备打开时用内核当前的 slot 坐标 (EVIOCGABS 的 value) 作为初始值, 见 `last_pos`。
    pub fn seed_position(&mut self, slot: i32, x: i32, y: i32) {
        self.last_pos.insert(slot, (Some(x), Some(y)));
    }

    /// 新触点的初始坐标 = 该 slot 最近一次的坐标 (内核不会重发没变的轴)。
    fn inherited(&self, slot: i32) -> (Option<i32>, Option<i32>) {
        self.last_pos.get(&slot).copied().unwrap_or((None, None))
    }

    pub fn reset(&mut self) {
        self.slots.clear();
        self.current_slot = 0;
        self.tracking_to_slot.clear();
        self.btn_touch = false;
        self.primary_slot = None;
    }

    pub fn touch_active(&self) -> bool {
        self.primary_slot.is_some()
    }

    fn ensure_slot(&mut self, slot: i32) {
        self.slots.entry(slot).or_insert_with(SlotState::new_empty);
    }

    fn clear_slot(&mut self, slot: i32, tracking_id: i32) {
        self.slots.insert(slot, SlotState::new_empty());
        if tracking_id != -1 {
            self.tracking_to_slot.remove(&tracking_id);
        }
    }

    fn clear_all_slots(&mut self) {
        self.slots.clear();
        self.tracking_to_slot.clear();
        self.primary_slot = None;
    }

    fn do_down(&self, slot: i32, out: &mut Vec<Gesture>) {
        if Some(slot) != self.primary_slot {
            return;
        }
        if let Some(s) = self.slots.get(&slot) {
            if let (Some(x), Some(y)) = (s.x, s.y) {
                out.push(Gesture {
                    kind: GestureKind::Down,
                    start: (x, y),
                    end: (x, y),
                    duration_ms: 0,
                });
            }
        }
    }

    fn do_up(&mut self, slot: i32, now: u64, out: &mut Vec<Gesture>) {
        if Some(slot) != self.primary_slot {
            return;
        }
        let s = self.slots.get(&slot).cloned().unwrap_or_default();
        let end_x = s.x;
        let end_y = s.y;
        let start_ts = s.start_ts;
        let start_pos = s.start_pos.unwrap_or((end_x, end_y));
        let duration_ms = now.saturating_sub(start_ts);

        let (mut sx, mut sy) = start_pos;
        let (mut ex, mut ey) = (end_x, end_y);
        if sx.is_none() {
            sx = ex;
        }
        if sy.is_none() {
            sy = ey;
        }
        if ex.is_none() {
            ex = sx;
        }
        if ey.is_none() {
            ey = sy;
        }
        let (sx, sy, ex, ey) = match (sx, sy, ex, ey) {
            (Some(sx), Some(sy), Some(ex), Some(ey)) => (sx, sy, ex, ey),
            // 坐标不全: 宁可丢掉这次触摸, 也不要当成点了 (0, 0)
            _ => return,
        };

        let dx = ex - sx;
        let dy = ey - sy;
        let dist_sq = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        let dist = (dist_sq as f64).sqrt();

        let cfg = &self.config;
        let kind: Option<GestureKind> = if dist <= cfg.tap_max_move_px as f64 {
            if duration_ms <= cfg.tap_max_duration_ms as u64 {
                Some(GestureKind::Tap)
            } else if duration_ms >= cfg.long_press_min_duration_ms as u64 {
                Some(GestureKind::Long)
            } else {
                // 介于快速点按和长按之间: 按 tap 处理, 消除时间死区。
                Some(GestureKind::Tap)
            }
        } else if dist < cfg.swipe_min_distance_px as f64 {
            // 轻微移动更可能是没点稳, 按 tap 处理, 消除死区。
            Some(GestureKind::Tap)
        } else if dx.abs() > dy.abs() {
            Some(if dx < 0 {
                GestureKind::SwipeLeft
            } else {
                GestureKind::SwipeRight
            })
        } else {
            Some(if dy < 0 {
                GestureKind::SwipeUp
            } else {
                GestureKind::SwipeDown
            })
        };

        if let Some(kind) = kind {
            out.push(Gesture {
                kind,
                start: (sx, sy),
                end: (ex, ey),
                duration_ms: duration_ms as u32,
            });
        }
    }

    fn update_pos(&mut self, slot: i32, x: Option<i32>, y: Option<i32>, set_x: bool, now: u64, out: &mut Vec<Gesture>) {
        self.ensure_slot(slot);
        {
            let s = self.slots.get_mut(&slot).unwrap();
            if set_x {
                s.x = x;
            } else {
                s.y = y;
            }
            s.last_ts = now;
            // 起点在数据包结束 (Report) 时才确定: 包内 X、Y 分先后到达, 新触点还可能继承了上次的坐标,
            // 在第一个轴到达时就记录起点会把另一轴的旧值当成起点 (实测: 点击被识别成下滑)。
            // 仅对补全缺失的轴保留旧逻辑 (没有 Report 的单元测试输入)。
            if let Some((sx, sy)) = s.start_pos {
                s.start_pos = Some((sx.or(s.x), sy.or(s.y)));
            }
        }
        let _ = out; // Down 在 Report 时发出, 移动不产生离散手势 (与 Python 版一致)
    }

    /// 数据包结束: 主触点坐标齐全且还没报过 Down 时发出 Down。
    fn on_report(&mut self, out: &mut Vec<Gesture>) {
        let Some(slot) = self.primary_slot else { return };
        if let Some(s) = self.slots.get_mut(&slot) {
            if s.start_pos.is_none() && s.tracking_id != -1 && (s.x.is_some() || s.y.is_some()) {
                s.start_pos = Some((s.x, s.y));
            }
        }
        let ready = self.slots.get(&slot).is_some_and(|s| s.has_coords() && !s.down_reported);
        if ready {
            self.slots.get_mut(&slot).unwrap().down_reported = true;
            self.do_down(slot, out);
        }
    }

    /// 给只报 ABS_X/ABS_Y + BTN_TOUCH 的设备补一个合成触点。
    fn start_btn_touch_fallback(&mut self, slot: i32, now: u64, out: &mut Vec<Gesture>) {
        if self.primary_slot.is_some() {
            return;
        }
        self.ensure_slot(slot);
        {
            let s = self.slots.get(&slot).unwrap();
            if s.tracking_id != -1 {
                self.primary_slot = Some(slot);
                return;
            }
        }
        let (ix, iy) = self.inherited(slot);
        {
            let s = self.slots.get_mut(&slot).unwrap();
            s.x = s.x.or(ix);
            s.y = s.y.or(iy);
            s.tracking_id = SYNTHETIC_TRACKING_ID;
            s.start_ts = now;
            s.last_ts = now;
            s.start_pos = None;
        }
        self.tracking_to_slot.insert(SYNTHETIC_TRACKING_ID, slot);
        self.primary_slot = Some(slot);
        let _ = out; // Down 在 Report 时发出
    }

    /// BTN_TOUCH 到达时, 若单轴坐标已先到则立即建立合成触点。
    fn begin_btn_touch(&mut self, now: u64, out: &mut Vec<Gesture>) {
        let slot = self.current_slot;
        self.ensure_slot(slot);
        let (tracking_id, has_x, has_y) = {
            let s = self.slots.get(&slot).unwrap();
            (s.tracking_id, s.x.is_some(), s.y.is_some())
        };
        let (ix, iy) = self.inherited(slot);
        if tracking_id == -1 && (has_x || ix.is_some()) && (has_y || iy.is_some()) {
            self.start_btn_touch_fallback(slot, now, out);
        }
    }

    fn finish_btn_touch(&mut self, now: u64, out: &mut Vec<Gesture>) {
        if let Some(slot) = self.primary_slot {
            let (tid, has_coords) = {
                let s = self.slots.get(&slot).cloned().unwrap_or_default();
                (s.tracking_id, s.has_coords())
            };
            if tid != -1 && has_coords {
                self.do_up(slot, now, out);
            }
        }
        self.clear_all_slots();
    }

    /// 喂一个样本, 识别出的手势追加到 `out`。
    pub fn feed(&mut self, sample: TouchSample, out: &mut Vec<Gesture>) {
        match sample {
            TouchSample::Sync(t) => {
                self.now_ms = t;
            }
            TouchSample::Report => self.on_report(out),
            TouchSample::Slot(val) => {
                self.current_slot = val;
                self.ensure_slot(val);
            }
            TouchSample::TrackingId(tid) => {
                let slot = self.current_slot;
                self.ensure_slot(slot);
                let prev_tid = self.slots.get(&slot).unwrap().tracking_id;
                let now = self.now_ms;
                if tid == -1 {
                    if Some(slot) == self.primary_slot {
                        if prev_tid != -1 {
                            self.do_up(slot, now, out);
                        }
                        self.clear_all_slots();
                    } else {
                        self.clear_slot(slot, prev_tid);
                    }
                } else {
                    if prev_tid != -1 && prev_tid != tid {
                        if prev_tid == SYNTHETIC_TRACKING_ID {
                            // BTN_TOUCH 先于 MT tracking id, 收到真实 id 后升级,
                            // 不把合成触点当成一次已完成的触摸。
                            let old = self.slots.get(&slot).cloned().unwrap();
                            self.tracking_to_slot.remove(&prev_tid);
                            let mut new_state = SlotState::new_empty();
                            new_state.tracking_id = tid;
                            new_state.x = old.x;
                            new_state.y = old.y;
                            new_state.start_ts = old.start_ts;
                            new_state.last_ts = now;
                            new_state.start_pos = old.start_pos;
                            new_state.down_reported = old.down_reported;
                            self.slots.insert(slot, new_state);
                        } else {
                            // 有些驱动直接换 tracking id 而不发 -1, 先结算旧手势。
                            self.do_up(slot, now, out);
                            self.tracking_to_slot.remove(&prev_tid);
                            let mut new_state = SlotState::new_empty();
                            new_state.tracking_id = tid;
                            (new_state.x, new_state.y) = self.inherited(slot);
                            new_state.start_ts = now;
                            new_state.last_ts = now;
                            self.slots.insert(slot, new_state);
                        }
                    } else {
                        let mut new_state = SlotState::new_empty();
                        new_state.tracking_id = tid;
                        (new_state.x, new_state.y) = self.inherited(slot);
                        new_state.start_ts = now;
                        new_state.last_ts = now;
                        self.slots.insert(slot, new_state);
                    }
                    self.tracking_to_slot.insert(tid, slot);
                    if self.primary_slot.is_none() {
                        self.primary_slot = Some(slot);
                    }
                }
            }
            TouchSample::X(val) => {
                let slot = self.current_slot;
                self.ensure_slot(slot);
                self.last_pos.entry(slot).or_insert((None, None)).0 = Some(val);
                let now = self.now_ms;
                if self.btn_touch {
                    self.start_btn_touch_fallback(slot, now, out);
                }
                self.update_pos(slot, Some(val), None, true, now, out);
            }
            TouchSample::Y(val) => {
                let slot = self.current_slot;
                self.ensure_slot(slot);
                self.last_pos.entry(slot).or_insert((None, None)).1 = Some(val);
                let now = self.now_ms;
                if self.btn_touch {
                    self.start_btn_touch_fallback(slot, now, out);
                }
                self.update_pos(slot, None, Some(val), false, now, out);
            }
            TouchSample::BtnTouch(pressed) => {
                self.btn_touch = pressed;
                let now = self.now_ms;
                if pressed {
                    self.begin_btn_touch(now, out);
                } else {
                    // 部分驱动用 BTN_TOUCH=0 结束触摸而不发 tracking id -1。
                    self.finish_btn_touch(now, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(r: &mut GestureRecognizer, x: i32, y: i32, ts: u64, tracking_id: i32, out: &mut Vec<Gesture>) {
        r.feed(TouchSample::Sync(ts), out);
        r.feed(TouchSample::Slot(0), out);
        r.feed(TouchSample::TrackingId(tracking_id), out);
        r.feed(TouchSample::X(x), out);
        r.feed(TouchSample::Y(y), out);
        r.feed(TouchSample::Report, out);
    }

    fn press_slot(r: &mut GestureRecognizer, slot: i32, x: i32, y: i32, ts: u64, tracking_id: i32, out: &mut Vec<Gesture>) {
        r.feed(TouchSample::Sync(ts), out);
        r.feed(TouchSample::Slot(slot), out);
        r.feed(TouchSample::TrackingId(tracking_id), out);
        r.feed(TouchSample::X(x), out);
        r.feed(TouchSample::Y(y), out);
        r.feed(TouchSample::Report, out);
    }

    #[test]
    fn reset_discards_pressed_gesture() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::TrackingId(7), &mut out);
        r.feed(TouchSample::X(100), &mut out);
        r.feed(TouchSample::Y(200), &mut out);
        assert!(r.slots.get(&0).unwrap().tracking_id == 7);
        assert_eq!(r.tracking_to_slot.get(&7), Some(&0));

        r.reset();
        assert!(r.slots.is_empty());
        assert!(r.tracking_to_slot.is_empty());
        assert_eq!(r.current_slot, 0);
        assert!(!r.btn_touch);

        out.clear();
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn position_event_after_reset_does_not_panic() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::TrackingId(7), &mut out);
        r.reset();
        r.feed(TouchSample::X(100), &mut out);
        r.feed(TouchSample::Y(200), &mut out);
        let s = r.slots.get(&0).unwrap();
        assert!(s.x.is_some());
        assert!(s.y.is_some());
    }

    #[test]
    fn slow_press_is_tap_not_long() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1450), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        let gestures: Vec<_> = out.into_iter().filter(|g| g.kind != GestureKind::Down).collect();
        assert_eq!(gestures.len(), 1);
        assert_eq!(gestures[0].kind, GestureKind::Tap);
    }

    #[test]
    fn stationary_long_press_is_long() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1800), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert_eq!(out[0].kind, GestureKind::Long);
    }

    #[test]
    fn slight_drag_is_tap_not_unknown() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::X(150), &mut out);
        r.feed(TouchSample::Sync(1150), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::Tap);
    }

    #[test]
    fn moving_long_press_is_tap() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1200), &mut out);
        r.feed(TouchSample::X(150), &mut out);
        r.feed(TouchSample::Sync(1800), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert_eq!(out[0].kind, GestureKind::Tap);
    }

    #[test]
    fn full_distance_is_swipe() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::X(180), &mut out);
        r.feed(TouchSample::Sync(1200), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert_eq!(out[0].kind, GestureKind::SwipeRight);
    }

    #[test]
    fn btn_touch_release_without_tracking_id_emits_gesture() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1150), &mut out);
        r.feed(TouchSample::BtnTouch(false), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::Tap);
        out.clear();
        r.feed(TouchSample::Sync(1200), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn tracking_id_replacement_finalizes_previous() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::TrackingId(8), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::Tap);
        assert!(!r.tracking_to_slot.contains_key(&7));
    }

    #[test]
    fn two_finger_swipe_only_settles_primary() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press_slot(&mut r, 0, 100, 200, 1000, 7, &mut out);
        press_slot(&mut r, 1, 300, 200, 1000, 8, &mut out);
        out.clear();

        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::Slot(1), &mut out);
        r.feed(TouchSample::X(380), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Sync(1200), &mut out);
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::X(180), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::SwipeRight);
    }

    #[test]
    fn two_finger_tap_only_settles_once() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press_slot(&mut r, 0, 100, 200, 1000, 7, &mut out);
        press_slot(&mut r, 1, 300, 200, 1000, 8, &mut out);
        out.clear();

        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::Slot(1), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Sync(1150), &mut out);
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::Tap);
    }

    #[test]
    fn third_finger_does_not_replace_primary() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press_slot(&mut r, 0, 100, 200, 1000, 7, &mut out);
        press_slot(&mut r, 1, 300, 200, 1000, 8, &mut out);
        press_slot(&mut r, 2, 500, 200, 1000, 9, &mut out);
        assert_eq!(r.primary_slot, Some(0));
        out.clear();

        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::Slot(2), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Sync(1150), &mut out);
        r.feed(TouchSample::Slot(1), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Sync(1200), &mut out);
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);

        assert_eq!(out.len(), 1);
    }

    #[test]
    fn single_axis_btn_touch_fallback_emits_gesture() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        r.feed(TouchSample::Sync(1000), &mut out);
        r.feed(TouchSample::X(100), &mut out);
        r.feed(TouchSample::Y(200), &mut out);
        r.feed(TouchSample::BtnTouch(true), &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::BtnTouch(false), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::Tap);
        assert_eq!(out[0].start, (100, 200));
    }

    #[test]
    fn real_mt_tracking_upgrades_btn_touch_fallback() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        r.feed(TouchSample::Sync(1000), &mut out);
        r.feed(TouchSample::X(100), &mut out);
        r.feed(TouchSample::Y(200), &mut out);
        r.feed(TouchSample::BtnTouch(true), &mut out);
        press_slot(&mut r, 0, 100, 200, 1000, 7, &mut out);
        out.clear();
        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert_eq!(out.len(), 1);
    }

    // --- New (not in Python): Down emission and reset semantics ---

    #[test]
    fn down_emitted_when_primary_touch_gets_coords() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        r.feed(TouchSample::Sync(1000), &mut out);
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::TrackingId(7), &mut out);
        r.feed(TouchSample::X(100), &mut out);
        r.feed(TouchSample::Report, &mut out);
        assert!(out.is_empty(), "no Down until both x and y are known");
        r.feed(TouchSample::Y(200), &mut out);
        assert!(out.is_empty(), "Down waits for the end of the packet");
        r.feed(TouchSample::Report, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, GestureKind::Down);
        assert_eq!(out[0].start, (100, 200));
    }

    #[test]
    fn down_not_emitted_twice_for_same_touch() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        let down_count = out.iter().filter(|g| g.kind == GestureKind::Down).count();
        assert_eq!(down_count, 1);
        out.clear();
        r.feed(TouchSample::Sync(1050), &mut out);
        r.feed(TouchSample::X(105), &mut out);
        assert!(out.is_empty(), "moving does not re-emit Down");
    }

    #[test]
    fn reset_mid_touch_emits_no_gesture_on_subsequent_up() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 100, 200, 1000, 7, &mut out);
        r.reset();
        out.clear();
        // A stray tracking-id -1 after reset must not emit anything or panic.
        r.feed(TouchSample::Slot(0), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert!(out.is_empty());
        assert!(!r.touch_active());
    }

    #[test]
    fn touch_active_reflects_primary_presence() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        assert!(!r.touch_active());
        press(&mut r, 100, 200, 1000, 7, &mut out);
        assert!(r.touch_active());
        out.clear();
        r.feed(TouchSample::Sync(1100), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert!(!r.touch_active());
    }

    /// 回归: 读取层在每个事件前都喂 Sync(事件时间)。上一次触摸的最后一个 SYN 很久以前,
    /// 新的按下也必须从自己的时间开始计时 (曾经所有真机点击都被识别为长按)。
    #[test]
    fn tap_after_long_idle_is_tap() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        let ev = |r: &mut GestureRecognizer, t: u64, s: TouchSample, out: &mut Vec<Gesture>| {
            r.feed(TouchSample::Sync(t), out);
            r.feed(s, out);
        };
        r.feed(TouchSample::Sync(1_000), &mut out); // 很久以前的最后一个 SYN
        let t = 600_000;
        ev(&mut r, t, TouchSample::Slot(0), &mut out);
        ev(&mut r, t, TouchSample::TrackingId(9), &mut out);
        ev(&mut r, t, TouchSample::X(300), &mut out);
        ev(&mut r, t, TouchSample::Y(400), &mut out);
        ev(&mut r, t, TouchSample::BtnTouch(true), &mut out);
        r.feed(TouchSample::Sync(t), &mut out);
        ev(&mut r, t + 80, TouchSample::TrackingId(-1), &mut out);
        ev(&mut r, t + 80, TouchSample::BtnTouch(false), &mut out);
        let taps: Vec<_> = out.iter().filter(|g| g.kind != GestureKind::Down).collect();
        assert_eq!(taps.len(), 1, "{out:?}");
        assert_eq!(taps[0].kind, GestureKind::Tap);
        assert_eq!(taps[0].duration_ms, 80);
    }

    /// 回归 (真机): MT 协议 B 下内核不重发没变的轴。第二次点在同一位置时只有 tracking id,
    /// 必须继承上次的坐标, 而不是报告 (0, 0)。
    #[test]
    fn repeated_tap_at_same_spot_keeps_coordinates() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 1100, 800, 1000, 1, &mut out);
        r.feed(TouchSample::Sync(1080), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Report, &mut out);
        out.clear();
        // 第二次: 坐标没变, 内核只发 tracking id
        r.feed(TouchSample::Sync(3000), &mut out);
        r.feed(TouchSample::TrackingId(2), &mut out);
        r.feed(TouchSample::Report, &mut out);
        r.feed(TouchSample::Sync(3080), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Report, &mut out);
        let kinds: Vec<_> = out.iter().map(|g| (g.kind, g.start)).collect();
        assert_eq!(kinds, vec![(GestureKind::Down, (1100, 800)), (GestureKind::Tap, (1100, 800))]);
        // 只有 Y 变化: X 继承
        out.clear();
        r.feed(TouchSample::Sync(5000), &mut out);
        r.feed(TouchSample::TrackingId(3), &mut out);
        r.feed(TouchSample::Y(300), &mut out);
        r.feed(TouchSample::Report, &mut out);
        r.feed(TouchSample::Sync(5080), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        let taps: Vec<_> = out.iter().filter(|g| g.kind == GestureKind::Tap).map(|g| g.start).collect();
        assert_eq!(taps, vec![(1100, 300)]);
    }

    #[test]
    fn touch_without_any_known_coordinates_emits_nothing() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        r.feed(TouchSample::Sync(1000), &mut out);
        r.feed(TouchSample::TrackingId(1), &mut out);
        r.feed(TouchSample::Report, &mut out);
        r.feed(TouchSample::Sync(1080), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    /// 回归 (真机): 新触点继承了上次的 Y, 本包先到 X 再到 Y; 起点必须是本包的 (X, Y), 不能是 (X, 旧 Y)。
    #[test]
    fn start_point_is_latched_at_report_not_first_axis() {
        let mut r = GestureRecognizer::new(GestureConfig::default());
        let mut out = Vec::new();
        press(&mut r, 335, 464, 1000, 1, &mut out);
        r.feed(TouchSample::Sync(1080), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Report, &mut out);
        out.clear();
        press(&mut r, 600, 616, 3000, 2, &mut out);
        r.feed(TouchSample::Sync(3080), &mut out);
        r.feed(TouchSample::TrackingId(-1), &mut out);
        r.feed(TouchSample::Report, &mut out);
        let kinds: Vec<_> = out.iter().map(|g| (g.kind, g.start)).collect();
        assert_eq!(kinds, vec![(GestureKind::Down, (600, 616)), (GestureKind::Tap, (600, 616))]);
    }
}
