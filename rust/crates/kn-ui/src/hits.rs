//! 命中区域: 渲染时登记, 事件分发时查询。取代 Python 各页面手写的 STATE["rects"] + 循环判断。

use kn_render::{Point, Rect};

/// 页面自定义的命中标识 (例如 枚举值 as u32, 或 行号)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HitId(pub u32);

#[derive(Clone, Copy, Debug)]
struct Hit {
    id: HitId,
    rect: Rect,
    /// 按下时是否做反相反馈
    feedback: bool,
    /// 是否接受长按
    long_press: bool,
    enabled: bool,
}

#[derive(Default)]
pub struct Hits {
    items: Vec<Hit>,
}

impl Hits {
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// 登记可点区域 (默认: 有按下反馈, 不接受长按, 启用)。后登记的在上层, 查询时优先。
    pub fn add(&mut self, id: HitId, rect: Rect) -> &mut Self {
        self.items.push(Hit { id, rect, feedback: true, long_press: false, enabled: true });
        self
    }

    /// 修改最近一次 add 的属性。
    pub fn no_feedback(&mut self) -> &mut Self {
        if let Some(h) = self.items.last_mut() {
            h.feedback = false;
        }
        self
    }

    pub fn long_press(&mut self) -> &mut Self {
        if let Some(h) = self.items.last_mut() {
            h.long_press = true;
        }
        self
    }

    pub fn enabled(&mut self, enabled: bool) -> &mut Self {
        if let Some(h) = self.items.last_mut() {
            h.enabled = enabled;
        }
        self
    }

    /// 点所在的最上层启用区域。
    pub fn at(&self, p: Point) -> Option<HitId> {
        self.items.iter().rev().find(|h| h.enabled && h.rect.contains(p)).map(|h| h.id)
    }

    /// 需要按下反馈时返回其矩形。
    pub fn feedback_rect(&self, p: Point) -> Option<Rect> {
        self.items
            .iter()
            .rev()
            .find(|h| h.enabled && h.rect.contains(p))
            .filter(|h| h.feedback)
            .map(|h| h.rect)
    }

    pub fn accepts_long(&self, p: Point) -> Option<HitId> {
        self.items
            .iter()
            .rev()
            .find(|h| h.enabled && h.rect.contains(p))
            .filter(|h| h.long_press)
            .map(|h| h.id)
    }

    pub fn rect_of(&self, id: HitId) -> Option<Rect> {
        self.items.iter().rev().find(|h| h.id == id).map(|h| h.rect)
    }
}
