//! 刷新调度: 差分 + 波形选择 + 残影预算。纯逻辑, 主机可测。
//!
//! 规则移植自 Python 0.8.0 `PageContext._refresh_plan / _waveform_for` (bin/src/kinnovel/ui.py), 并扩展:
//! - `Flash`: 强制整屏 GC16 闪刷, 预算清零。
//! - 差分为空 → 不刷新 (返回 None)。
//! - 变化矩形四周外扩 4 px; 面积 >= 95% 屏视为整屏。
//! - 大面积 (>= 50% 屏) 且 (hint == Turn 且 policy.flash_every_turn) 或 预算 >= 阈值 → 闪刷。
//!   阈值: 翻页用 policy.full_refresh_every; 其它界面最多 `UI_FLASH_EVERY` 屏 (列表/菜单的残影比正文明显)。
//! - `Clean`: 只对变化区域闪刷 (关闭弹窗等, 深色按钮在局部刷新下残留严重)。
//! - 波形: Turn → Reagl (设备支持时, 否则 Gl16); Ui → Gc16; Fast → Du; Feedback → A2; Flash/Clean → Gc16 + flash。
//! - 预算累计: 非闪刷时 += 面积占比, Turn 且使用 Reagl 时 × 0.25; 局部闪刷扣掉对应面积。
//! - Turn 跳过差分直接整屏 (几乎全屏都变, 省掉 diff 开销)。
//! - 首帧或 `invalidate()` 之后: 整屏刷新 (不闪, 除非 hint 为 Flash)。

use kn_platform::{RefreshRequest, Waveform};
use kn_render::{Bitmap, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshHint {
    /// 普通界面变化
    Ui,
    /// 阅读翻页
    Turn,
    /// 菜单/弹层等追求速度的变化
    Fast,
    /// 变化区域闪刷 (关闭弹窗: 原弹窗区域彻底清掉)
    Clean,
    /// 按下反馈 (由运行时内部使用)
    Feedback,
    /// 强制闪刷 (唤醒恢复、用户手动全刷)
    Flash,
}

impl RefreshHint {
    /// 合并多次重绘请求时的优先级: Flash > Turn > Clean > Ui > Fast > Feedback。
    pub fn strength(self) -> u8 {
        match self {
            RefreshHint::Flash => 5,
            RefreshHint::Turn => 4,
            RefreshHint::Clean => 3,
            RefreshHint::Ui => 2,
            RefreshHint::Fast => 1,
            RefreshHint::Feedback => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefreshPolicy {
    /// 局部刷新累计多少屏后闪刷一次 (config: full_refresh_every, 默认 6)
    pub full_refresh_every: f32,
    /// 每次翻页都闪刷 (config: page_flash)
    pub flash_every_turn: bool,
    /// 设备支持 REAGL
    pub supports_reagl: bool,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        RefreshPolicy { full_refresh_every: 6.0, flash_every_turn: false, supports_reagl: false }
    }
}

/// 变化矩形外扩像素, 防止抗锯齿边缘残留。
const REGION_PAD: i32 = 4;
/// 变化面积达到整屏这个比例才算"大面积", 只有大面积刷新会被升级为闪刷。
const LARGE_RATIO: f32 = 0.5;
/// 达到这个比例直接按整屏处理。
const FULL_RATIO: f32 = 0.95;
/// REAGL 翻页自带残影抑制, 计入预算时打折。
const REAGL_TURN_WEIGHT: f32 = 0.25;
/// 阅读页以外: 局部刷新累计多少屏后, 下一次大面积刷新升级为闪刷 (配置值更小时用配置值)。
const UI_FLASH_EVERY: f32 = 3.0;

pub struct RefreshScheduler {
    /// 屏幕上当前内容 (仅 screen_valid 时可信)
    screen: Bitmap,
    screen_valid: bool,
    budget: f32,
    policy: RefreshPolicy,
}

impl RefreshScheduler {
    pub fn new(width: u32, height: u32, policy: RefreshPolicy) -> Self {
        RefreshScheduler { screen: Bitmap::new(width, height, 255), screen_valid: false, budget: 0.0, policy }
    }

    pub fn set_policy(&mut self, policy: RefreshPolicy) {
        self.policy = policy;
    }

    /// 屏幕被外部改写过; 下一次刷新走整屏。
    pub fn invalidate(&mut self) {
        self.screen_valid = false;
    }

    fn full(&self) -> Rect {
        self.screen.bounds()
    }

    fn threshold(&self, hint: RefreshHint) -> f32 {
        let configured = if self.policy.full_refresh_every > 0.0 { self.policy.full_refresh_every } else { 6.0 };
        if hint == RefreshHint::Turn {
            configured
        } else {
            configured.min(UI_FLASH_EVERY)
        }
    }

    fn waveform(&self, hint: RefreshHint) -> Waveform {
        match hint {
            RefreshHint::Turn if self.policy.supports_reagl => Waveform::Reagl,
            RefreshHint::Turn => Waveform::Gl16,
            RefreshHint::Ui | RefreshHint::Flash | RefreshHint::Clean => Waveform::Gc16,
            RefreshHint::Fast => Waveform::Du,
            RefreshHint::Feedback => Waveform::A2,
        }
    }

    fn flash_request(&self) -> RefreshRequest {
        RefreshRequest { rect: self.full(), waveform: Waveform::Gc16, flash: true, swipe: None }
    }

    /// 计算本次需要提交的刷新; None 表示无需刷新。不修改内部状态, 由 `commit` 落实。
    pub fn plan(&self, frame: &Bitmap, hint: RefreshHint) -> Option<RefreshRequest> {
        let full = self.full();
        if hint == RefreshHint::Flash {
            return Some(self.flash_request());
        }
        let same_size = frame.width() == self.screen.width() && frame.height() == self.screen.height();
        let rect = if !self.screen_valid || !same_size || hint == RefreshHint::Turn {
            full
        } else {
            let bbox = frame.diff_bbox(&self.screen)?;
            bbox.inflate(REGION_PAD).intersect(&full)?
        };
        let ratio = rect.area() as f32 / full.area().max(1) as f32;
        let rect = if ratio >= FULL_RATIO { full } else { rect };
        if ratio >= LARGE_RATIO && hint != RefreshHint::Feedback {
            let turn_flash = hint == RefreshHint::Turn && self.policy.flash_every_turn;
            if turn_flash || self.budget >= self.threshold(hint) {
                return Some(self.flash_request());
            }
        }
        if hint == RefreshHint::Clean {
            return Some(if rect == full { self.flash_request() } else { RefreshRequest { rect, waveform: Waveform::Gc16, flash: true, swipe: None } });
        }
        Some(RefreshRequest { rect, waveform: self.waveform(hint), flash: false, swipe: None })
    }

    /// 刷新已成功提交: 更新屏幕帧副本 (只拷贝 req.rect 区域) 与残影预算。
    pub fn commit(&mut self, frame: &Bitmap, req: &RefreshRequest, hint: RefreshHint) {
        let full = self.full();
        if req.rect == full || !self.screen_valid {
            self.screen.blit(frame, full, 0, 0);
            self.screen_valid = true;
        } else {
            self.screen.blit(frame, req.rect, req.rect.x, req.rect.y);
        }
        let mut weight = req.rect.area() as f32 / full.area().max(1) as f32;
        if req.flash {
            // 整屏闪刷清零; 局部闪刷只清掉那一块
            self.budget = if req.rect == full { 0.0 } else { (self.budget - weight).max(0.0) };
            return;
        }
        if hint == RefreshHint::Turn && req.waveform == Waveform::Reagl {
            weight *= REAGL_TURN_WEIGHT;
        }
        self.budget += weight;
    }

    /// 屏幕上当前内容 (按下反馈需要在其上反相)。
    pub fn screen(&self) -> &Bitmap {
        &self.screen
    }

    pub fn ghost_budget(&self) -> f32 {
        self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 100;
    const H: u32 = 200;

    fn sched(policy: RefreshPolicy) -> RefreshScheduler {
        RefreshScheduler::new(W, H, policy)
    }

    fn reagl() -> RefreshPolicy {
        RefreshPolicy { supports_reagl: true, ..RefreshPolicy::default() }
    }

    fn present(s: &mut RefreshScheduler, f: &Bitmap, hint: RefreshHint) -> Option<RefreshRequest> {
        let req = s.plan(f, hint)?;
        s.commit(f, &req, hint);
        Some(req)
    }

    #[test]
    fn first_frame_is_full_without_flash() {
        let mut s = sched(RefreshPolicy::default());
        let req = present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Ui).unwrap();
        assert_eq!(req.rect, Rect::new(0, 0, W, H));
        assert!(!req.flash);
        assert_eq!(req.waveform, Waveform::Gc16);
    }

    #[test]
    fn identical_frame_skips_refresh() {
        let mut s = sched(RefreshPolicy::default());
        let f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        assert!(s.plan(&f, RefreshHint::Ui).is_none());
    }

    #[test]
    fn small_change_refreshes_padded_bbox() {
        let mut s = sched(RefreshPolicy::default());
        let mut f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(10, 20, 5, 6), 0);
        let req = s.plan(&f, RefreshHint::Ui).unwrap();
        assert_eq!(req.rect, Rect::new(6, 16, 13, 14));
        assert!(!req.flash);
    }

    #[test]
    fn budget_upgrades_next_large_update_to_flash() {
        let mut s = sched(RefreshPolicy { full_refresh_every: 2.0, ..RefreshPolicy::default() });
        let mut flashes = 0;
        for i in 0..6u8 {
            let f = Bitmap::new(W, H, i.wrapping_mul(40).wrapping_add(10));
            if present(&mut s, &f, RefreshHint::Ui).unwrap().flash {
                flashes += 1;
                assert_eq!(s.ghost_budget(), 0.0);
            }
        }
        // 1.0 + 1.0 达到阈值 → 第 3 帧闪; 第 6 帧再闪
        assert_eq!(flashes, 2);
    }

    #[test]
    fn small_updates_never_flash() {
        let mut s = sched(RefreshPolicy { full_refresh_every: 0.5, ..RefreshPolicy::default() });
        let mut f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        for i in 0..20u32 {
            f.fill_rect(Rect::new(0, (i * 5) as i32, 10, 5), (i * 10) as u8);
            assert!(!present(&mut s, &f, RefreshHint::Ui).unwrap().flash);
        }
    }

    #[test]
    fn reagl_turns_count_quarter() {
        let mut s = sched(reagl());
        present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Flash);
        let req = present(&mut s, &Bitmap::new(W, H, 0), RefreshHint::Turn).unwrap();
        assert_eq!(req.waveform, Waveform::Reagl);
        assert!((s.ghost_budget() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn turn_without_reagl_uses_gl16_full_weight() {
        let mut s = sched(RefreshPolicy::default());
        present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Flash);
        let req = present(&mut s, &Bitmap::new(W, H, 0), RefreshHint::Turn).unwrap();
        assert_eq!(req.waveform, Waveform::Gl16);
        assert!((s.ghost_budget() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn flash_every_turn_policy() {
        let mut s = sched(RefreshPolicy { flash_every_turn: true, ..reagl() });
        present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Ui);
        assert!(present(&mut s, &Bitmap::new(W, H, 0), RefreshHint::Turn).unwrap().flash);
    }

    #[test]
    fn invalidate_forces_full_refresh() {
        let mut s = sched(RefreshPolicy::default());
        let f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        s.invalidate();
        assert_eq!(s.plan(&f, RefreshHint::Ui).unwrap().rect, Rect::new(0, 0, W, H));
    }

    #[test]
    fn partial_commit_tracks_only_region() {
        let mut s = sched(RefreshPolicy::default());
        present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Ui);
        let mut f = Bitmap::new(W, H, 255);
        f.fill_rect(Rect::new(0, 0, 10, 10), 0);
        f.fill_rect(Rect::new(90, 190, 10, 10), 0);
        let req = RefreshRequest { rect: Rect::new(0, 0, 10, 10), waveform: Waveform::A2, flash: false, swipe: None };
        s.commit(&f, &req, RefreshHint::Feedback);
        assert_eq!(s.screen().get(0, 0), 0);
        assert_eq!(s.screen().get(95, 195), 255);
    }

    #[test]
    fn feedback_uses_a2_and_never_flashes() {
        let mut s = sched(RefreshPolicy { full_refresh_every: 0.1, ..RefreshPolicy::default() });
        present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Ui);
        present(&mut s, &Bitmap::new(W, H, 0), RefreshHint::Ui);
        let req = s.plan(&Bitmap::new(W, H, 128), RefreshHint::Feedback).unwrap();
        assert_eq!(req.waveform, Waveform::A2);
        assert!(!req.flash);
    }

    #[test]
    fn ui_flashes_sooner_than_reader() {
        // 配置 6 屏: 翻页 (Gl16) 要攒 6 屏, 其它界面 3 屏就闪
        let mut s = sched(RefreshPolicy::default());
        let mut flashed_at = None;
        for i in 0..6u8 {
            let f = Bitmap::new(W, H, i.wrapping_mul(40).wrapping_add(10));
            if present(&mut s, &f, RefreshHint::Ui).unwrap().flash {
                flashed_at = Some(i);
                break;
            }
        }
        assert_eq!(flashed_at, Some(3));
        let mut s = sched(RefreshPolicy::default());
        for i in 0..6u8 {
            let f = Bitmap::new(W, H, i.wrapping_mul(40).wrapping_add(10));
            assert!(!present(&mut s, &f, RefreshHint::Turn).unwrap().flash, "turn {i}");
        }
    }

    #[test]
    fn clean_flashes_only_changed_region() {
        let mut s = sched(RefreshPolicy::default());
        let mut f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(0, 0, W, 100), 0);
        present(&mut s, &f, RefreshHint::Ui);
        let before = s.ghost_budget();
        f.fill_rect(Rect::new(10, 20, 30, 40), 255);
        let req = present(&mut s, &f, RefreshHint::Clean).unwrap();
        assert!(req.flash);
        assert_eq!(req.rect, Rect::new(6, 16, 38, 48));
        assert!(s.ghost_budget() < before && s.ghost_budget() > 0.0);
    }

    #[test]
    fn hint_strength_order() {
        assert!(RefreshHint::Flash.strength() > RefreshHint::Turn.strength());
        assert!(RefreshHint::Turn.strength() > RefreshHint::Clean.strength());
        assert!(RefreshHint::Clean.strength() > RefreshHint::Ui.strength());
        assert!(RefreshHint::Ui.strength() > RefreshHint::Fast.strength());
    }
}
