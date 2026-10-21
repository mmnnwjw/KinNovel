//! 刷新调度: 差分 + 波形选择 + 残影预算。纯逻辑, 主机可测。
//!
//! - 流畅 UI (默认): 按像素差分拆区, 不因面积预算或深色块消失自动闪刷。
//!   黑白 MTK 用 AUTO, 其它设备沿用 GC16; 按下反馈仍为小块 A2 / 大块 DU。
//! - 经典 UI: 沿用 v1.6.4 的 GC16、面积预算和深色块定点清理。
//! - 阅读 Turn: 整屏 REAGL / GL16, 保留按面积累计的阅读预算和每页闪刷设置。
//!   UI / 按下反馈不增加阅读预算; 无动画 REAGL 翻页按 0.25 屏计。
//! - Flash: 显式整屏清理; Clean / plan_with_clean: 显式局部清理, 不被 Turn 覆盖。
//! - 每次成功提交后更新副本和预算; 驱动 FULL 配对不等于清理闪刷。

use kn_platform::{RefreshRequest, Waveform};
use kn_render::{Bitmap, Rect};

mod classic;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiRefreshMode {
    #[default]
    Smooth,
    Classic,
}

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
    /// 普通 UI 的设备波形, 黑白 MTK 用 AUTO, 其余保守回退 GC16
    pub ui_waveform: Waveform,
    pub ui_mode: UiRefreshMode,
    /// 设备支持 REAGL
    pub supports_reagl: bool,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        RefreshPolicy { full_refresh_every: 6.0, flash_every_turn: false, ui_waveform: Waveform::Gc16, ui_mode: UiRefreshMode::Smooth, supports_reagl: false }
    }
}

/// 变化矩形外扩像素, 防止抗锯齿边缘残留。
const REGION_PAD: i32 = 4;
/// 相隔至少这么多行未变化, 差分就拆成两个区域。
const REGION_GAP: u32 = 32;
/// 一次重绘最多几个差分区域 (EPDC 同时进行的更新数有限, 再多就合并间隔最小的)。
const MAX_REGIONS: usize = 4;
/// 达到这个比例直接按整屏处理。
const FULL_RATIO: f32 = 0.95;
/// REAGL 翻页自带残影抑制, 计入预算时打折。
const REAGL_TURN_WEIGHT: f32 = 0.25;
/// 按下反馈小于整屏这个比例时用 A2, 其余用 DU。
const FEEDBACK_SMALL_RATIO: f32 = 0.02;

pub struct RefreshScheduler {
    /// 屏幕上当前内容 (仅 screen_valid 时可信)
    screen: Bitmap,
    screen_valid: bool,
    budget: f32,
    policy: RefreshPolicy,
    /// 经典 UI 的面积预算独立记录, 不让模式切换改变阅读清理间隔。
    ui_budget: f32,
    light_background: bool,
    feedback: Option<Rect>,
}

impl RefreshScheduler {
    pub fn new(width: u32, height: u32, policy: RefreshPolicy) -> Self {
        RefreshScheduler {
            screen: Bitmap::new(width, height, 255),
            screen_valid: false,
            budget: 0.0,
            policy,
            ui_budget: 0.0,
            light_background: true,
            feedback: None,
        }
    }

    pub fn set_policy(&mut self, policy: RefreshPolicy) {
        self.set_ui_mode(policy.ui_mode);
        self.policy = policy;
    }

    pub fn set_ui_mode(&mut self, mode: UiRefreshMode) {
        if mode != self.policy.ui_mode {
            self.policy.ui_mode = mode;
            self.ui_budget = 0.0;
            self.feedback = None;
        }
    }

    pub fn set_background(&mut self, background: u8) {
        self.light_background = background >= 128;
    }

    /// 屏幕上现在就是 `screen` (从帧缓冲读回的): 之后按它做差分。
    pub fn adopt(&mut self, screen: &Bitmap) {
        if screen.width() == self.screen.width() && screen.height() == self.screen.height() {
            self.screen.blit(screen, screen.bounds(), 0, 0);
            self.screen_valid = true;
            self.feedback = None;
        }
    }

    /// 屏幕被外部改写过; 下一次刷新走整屏。
    pub fn invalidate(&mut self) {
        self.screen_valid = false;
        self.feedback = None;
    }

    /// 逻辑画面尺寸变了 (横竖屏切换): 之后按新尺寸规划, 下一次刷新走整屏。
    pub fn resize(&mut self, width: u32, height: u32) {
        self.screen = Bitmap::new(width, height, 255);
        self.screen_valid = false;
        self.feedback = None;
    }

    fn full(&self) -> Rect {
        self.screen.bounds()
    }

    fn threshold(&self) -> f32 {
        if self.policy.full_refresh_every > 0.0 { self.policy.full_refresh_every } else { 6.0 }
    }

    fn waveform(&self, hint: RefreshHint) -> Waveform {
        match hint {
            RefreshHint::Turn if self.policy.supports_reagl => Waveform::Reagl,
            RefreshHint::Turn => Waveform::Gl16,
            RefreshHint::Ui if self.policy.ui_mode == UiRefreshMode::Classic => Waveform::Gc16,
            RefreshHint::Ui => self.policy.ui_waveform,
            RefreshHint::Flash | RefreshHint::Clean => Waveform::Gc16,
            RefreshHint::Fast => Waveform::Du,
            RefreshHint::Feedback => Waveform::A2,
        }
    }

    fn flash_request(&self) -> RefreshRequest {
        RefreshRequest { rect: self.full(), waveform: Waveform::Gc16, flash: true, swipe: None }
    }

    fn request(rect: Rect, waveform: Waveform, flash: bool) -> RefreshRequest {
        RefreshRequest { rect, waveform, flash, swipe: None }
    }

    /// 计算本次需要提交的刷新, 按顺序逐个提交并 `commit`; 空表示无需刷新。不修改内部状态。
    pub fn plan(&self, frame: &Bitmap, hint: RefreshHint) -> Vec<RefreshRequest> {
        let full = self.full();
        if hint == RefreshHint::Flash {
            return vec![self.flash_request()];
        }
        let same_size = frame.width() == self.screen.width() && frame.height() == self.screen.height();
        if !self.screen_valid || !same_size || hint == RefreshHint::Turn {
            return vec![self.plan_full(hint)];
        }
        let regions: Vec<Rect> = frame
            .diff_regions(&self.screen, REGION_GAP, MAX_REGIONS)
            .into_iter()
            .filter_map(|r| r.inflate(REGION_PAD).intersect(&full))
            .collect();
        if regions.is_empty() {
            return Vec::new();
        }
        let full_area = full.area().max(1) as f32;
        let ratio = regions.iter().map(|r| r.area()).sum::<u64>() as f32 / full_area;
        if ratio >= FULL_RATIO {
            return vec![self.plan_full(hint)];
        }
        if hint == RefreshHint::Feedback {
            return regions.into_iter().map(|r| Self::request(r, self.feedback_waveform(r), false)).collect();
        }
        if self.policy.ui_mode == UiRefreshMode::Classic && ratio >= 0.5 && self.classic_budget_full() {
            return vec![self.flash_request()];
        }
        if hint == RefreshHint::Clean {
            return regions.into_iter().map(|r| Self::request(r, Waveform::Gc16, true)).collect();
        }
        if self.policy.ui_mode == UiRefreshMode::Classic {
            return classic::plan(&self.screen, frame, regions, self.waveform(hint), self.light_background, self.feedback);
        }
        regions.into_iter().map(|r| Self::request(r, self.waveform(hint), false)).collect()
    }

    /// 显式清理与差分独立: 相同像素仍需清掉物理残影。普通翻页不能吞掉局部清理。
    pub fn plan_with_clean(&self, frame: &Bitmap, hint: RefreshHint, clean: &[Rect]) -> Vec<RefreshRequest> {
        let mut plan = self.plan(frame, hint);
        if plan.iter().any(|r| r.flash && r.rect == self.full()) {
            return plan;
        }
        let mut regions = Vec::<Rect>::new();
        for rect in clean.iter().filter_map(|r| r.inflate(REGION_PAD).intersect(&self.full())).filter(|r| !r.is_empty()) {
            merge_clean(&mut regions, rect);
        }
        // 正常只有弹窗或菜单的几块区域; 超过上限时优先合并增加面积最少的一对。
        while regions.len() > MAX_REGIONS {
            let (i, j) = (0..regions.len())
                .flat_map(|i| (i + 1..regions.len()).map(move |j| (i, j)))
                .min_by_key(|&(i, j)| regions[i].union(&regions[j]).area().saturating_sub(regions[i].area() + regions[j].area()))
                .unwrap();
            let r = regions.remove(j);
            let merged = regions.remove(i).union(&r);
            merge_clean(&mut regions, merged);
        }
        for rect in &regions {
            plan = plan.into_iter()
                .flat_map(|req| subtract(req.rect, *rect).into_iter().map(move |r| RefreshRequest { rect: r, ..req }))
                .collect();
        }
        plan.extend(regions.into_iter().map(|r| Self::request(r, Waveform::Gc16, true)));
        plan
    }

    /// 整屏刷新 (首帧、invalidate 之后、翻页、变化接近整屏)。
    fn plan_full(&self, hint: RefreshHint) -> RefreshRequest {
        let full = self.full();
        if hint != RefreshHint::Feedback {
            let turn_flash = hint == RefreshHint::Turn && self.policy.flash_every_turn;
            let budget_full = if hint == RefreshHint::Turn { self.budget >= self.threshold() } else { self.classic_budget_full() };
            if turn_flash || hint == RefreshHint::Clean || budget_full {
                return self.flash_request();
            }
        }
        let waveform = if hint == RefreshHint::Feedback { self.feedback_waveform(full) } else { self.waveform(hint) };
        Self::request(full, waveform, false)
    }

    fn classic_budget_full(&self) -> bool {
        self.policy.ui_mode == UiRefreshMode::Classic && self.ui_budget >= classic::threshold(self.threshold(), self.light_background)
    }

    /// 按下反馈: 键盘按键这类小块用 A2 (最快), 较大的 (列表行、含灰阶封面) 用 DU ——
    /// KOReader 也只在键盘上用 a2, 其余高亮用 "fast" (DU): A2 只适合黑白之间切换, 灰阶内容反相后残影重。
    fn feedback_waveform(&self, rect: Rect) -> Waveform {
        if (rect.area() as f32) < self.full().area() as f32 * FEEDBACK_SMALL_RATIO {
            Waveform::A2
        } else {
            Waveform::Du
        }
    }

    /// 按下反馈: 直接在屏幕副本上把 `rect` (圆角 `radius`) 反相, 返回只刷这一块的请求
    /// (用 `screen()` 作帧提交)。结果与 "拷一份屏幕、反相、`plan(Feedback)`、`commit`" 相同, 但不拷整屏、不做整屏差分。
    /// 屏幕副本不可信 (首帧前、invalidate 之后) 或区域在屏幕外时返回 None。
    pub fn press(&mut self, rect: Rect, radius: u32) -> Option<RefreshRequest> {
        let full = self.full();
        if !self.screen_valid {
            return None;
        }
        let changed = rect.intersect(&full).filter(|r| !r.is_empty())?;
        self.screen.invert_rounded(rect, radius);
        let padded = changed.inflate(REGION_PAD).intersect(&full)?;
        let full_area = full.area().max(1) as f32;
        let rect = if padded.area() as f32 / full_area >= FULL_RATIO { full } else { padded };
        let req = Self::request(rect, self.feedback_waveform(rect), false);
        self.account(&req, RefreshHint::Feedback);
        Some(req)
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
        self.account(req, hint);
    }

    /// 提交后的阅读预算记账, 普通 UI 与反馈不消耗阅读预算。
    fn account(&mut self, req: &RefreshRequest, hint: RefreshHint) {
        let full = self.full();
        let mut weight = req.rect.area() as f32 / full.area().max(1) as f32;
        if self.policy.ui_mode == UiRefreshMode::Classic {
            if hint == RefreshHint::Feedback {
                self.feedback = Some(self.feedback.map_or(req.rect, |r| r.union(&req.rect)));
            } else {
                self.feedback = None;
            }
            if req.flash {
                self.ui_budget = if req.rect == full { 0.0 } else { (self.ui_budget - weight).max(0.0) };
            } else {
                let ui_weight = if hint == RefreshHint::Turn && req.waveform == Waveform::Reagl { weight * REAGL_TURN_WEIGHT } else { weight };
                self.ui_budget += ui_weight;
            }
        }
        if req.flash {
            // 整屏闪刷清零; 局部闪刷只清掉那一块
            self.budget = if req.rect == full { 0.0 } else { (self.budget - weight).max(0.0) };
            return;
        }
        if hint != RefreshHint::Turn {
            return;
        }
        if req.waveform == Waveform::Reagl && req.swipe.is_none() {
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

fn merge_clean(regions: &mut Vec<Rect>, mut rect: Rect) {
    let mut i = 0;
    while i < regions.len() {
        let r = regions[i];
        if rect.x <= r.right() && r.x <= rect.right() && rect.y <= r.bottom() && r.y <= rect.bottom() {
            rect = rect.union(&regions.remove(i));
            i = 0;
        } else {
            i += 1;
        }
    }
    regions.push(rect);
}

/// 从普通更新中去掉将由清理更新覆盖的部分, 避免先整屏翻页再刷弹窗。
fn subtract(rect: Rect, clean: Rect) -> Vec<Rect> {
    let Some(i) = rect.intersect(&clean).filter(|r| !r.is_empty()) else { return vec![rect] };
    [
        Rect::new(rect.x, rect.y, rect.w, (i.y - rect.y) as u32),
        Rect::new(rect.x, i.bottom(), rect.w, (rect.bottom() - i.bottom()) as u32),
        Rect::new(rect.x, i.y, (i.x - rect.x) as u32, i.h),
        Rect::new(i.right(), i.y, (rect.right() - i.right()) as u32, i.h),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
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

    /// 按顺序提交 plan 的全部刷新, 返回第一个。
    fn present(s: &mut RefreshScheduler, f: &Bitmap, hint: RefreshHint) -> Option<RefreshRequest> {
        let reqs = s.plan(f, hint);
        for req in &reqs {
            s.commit(f, req, hint);
        }
        reqs.into_iter().next()
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
        assert!(s.plan(&f, RefreshHint::Ui).is_empty());
    }

    #[test]
    fn explicit_clean_survives_identical_pixels_and_turn() {
        let (mut s, f) = big();
        let clean = [Rect::new(20, 40, 70, 60), Rect::new(290, 480, 60, 60)];
        let reqs = s.plan_with_clean(&f, RefreshHint::Ui, &clean);
        assert_eq!(reqs.len(), 2);
        assert!(reqs.iter().all(|r| r.flash));
        let reqs = s.plan_with_clean(&f, RefreshHint::Turn, &clean);
        assert_eq!(reqs.iter().filter(|r| r.flash).count(), 2);
        assert!(reqs.iter().all(|r| r.rect != f.bounds()));
        // 普通翻页与清理区域不重复提交; 所有区域仍覆盖全屏。
        assert_eq!(reqs.iter().map(|r| r.rect.area()).sum::<u64>(), f.bounds().area());
        for req in &reqs { s.commit(&f, req, RefreshHint::Turn); }
        assert!(s.ghost_budget() > 0.0);
        let flash = s.plan_with_clean(&f, RefreshHint::Flash, &clean);
        assert_eq!(flash.len(), 1);
        assert!(flash[0].flash && flash[0].rect == f.bounds());
    }

    #[test]
    fn animated_turn_does_not_get_reagl_discount() {
        let mut s = sched(reagl());
        let f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Flash);
        let mut req = s.plan(&f, RefreshHint::Turn)[0];
        req.swipe = Some(kn_platform::SwipeDir::Left);
        s.commit(&f, &req, RefreshHint::Turn);
        assert_eq!(s.ghost_budget(), 1.0);
    }

    #[test]
    fn switching_ui_modes_changes_cleanup_without_resetting_reading_budget() {
        let mut s = RefreshScheduler::new(BW, BH, RefreshPolicy { ui_waveform: Waveform::Auto, ..reagl() });
        let mut f = Bitmap::new(BW, BH, 255);
        present(&mut s, &f, RefreshHint::Turn);
        let reading_budget = s.ghost_budget();
        s.set_ui_mode(UiRefreshMode::Classic);
        assert_eq!(s.ghost_budget(), reading_budget);
        f.fill_rect(Rect::new(100, 100, 160, 96), 0);
        assert_eq!(present(&mut s, &f, RefreshHint::Ui).unwrap().waveform, Waveform::Gc16);
        assert_eq!(s.ghost_budget(), reading_budget);
        f.fill_rect(Rect::new(100, 100, 160, 96), 255);
        assert!(s.plan(&f, RefreshHint::Ui).iter().any(|r| r.flash));
        s.set_ui_mode(UiRefreshMode::Smooth);
        assert!(s.plan(&f, RefreshHint::Ui).iter().all(|r| !r.flash && r.waveform == Waveform::Auto));
        s.set_ui_mode(UiRefreshMode::Classic);
        for gray in [110, 120, 130] {
            assert!(!present(&mut s, &Bitmap::new(BW, BH, gray), RefreshHint::Ui).unwrap().flash);
        }
        assert!(present(&mut s, &Bitmap::new(BW, BH, 140), RefreshHint::Ui).unwrap().flash);
    }

    #[test]
    fn small_change_refreshes_padded_bbox() {
        let mut s = sched(RefreshPolicy::default());
        let mut f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(10, 20, 5, 6), 0);
        let req = s.plan(&f, RefreshHint::Ui)[0];
        assert_eq!(req.rect, Rect::new(6, 16, 13, 14));
        assert!(!req.flash);
    }

    #[test]
    fn budget_upgrades_next_large_update_to_flash() {
        let mut s = sched(RefreshPolicy { full_refresh_every: 2.0, ..RefreshPolicy::default() });
        let mut flashes = 0;
        for i in 0..6u8 {
            let f = Bitmap::new(W, H, i.wrapping_mul(40).wrapping_add(10));
            if present(&mut s, &f, RefreshHint::Turn).unwrap().flash {
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
        assert_eq!(s.plan(&f, RefreshHint::Ui)[0].rect, Rect::new(0, 0, W, H));
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
    fn feedback_never_flashes_and_picks_waveform_by_size() {
        let mut s = sched(RefreshPolicy { full_refresh_every: 0.1, ..RefreshPolicy::default() });
        present(&mut s, &Bitmap::new(W, H, 255), RefreshHint::Ui);
        present(&mut s, &Bitmap::new(W, H, 0), RefreshHint::Ui);
        // 整屏反相 (大块): DU
        let req = s.plan(&Bitmap::new(W, H, 128), RefreshHint::Feedback)[0];
        assert_eq!(req.waveform, Waveform::Du);
        assert!(!req.flash);
        // 小块 (< 2% 屏, 键盘按键): A2
        let mut f = Bitmap::new(W, H, 0);
        f.invert_rect(Rect::new(10, 10, 5, 5));
        let req = s.plan(&f, RefreshHint::Feedback)[0];
        assert_eq!(req.waveform, Waveform::A2);
        assert!(!req.flash);
    }

    #[test]
    fn ui_and_feedback_do_not_consume_or_trigger_reading_budget() {
        let mut s = sched(RefreshPolicy { ui_waveform: Waveform::Auto, ..reagl() });
        for i in 0..30u8 {
            let f = Bitmap::new(W, H, i.wrapping_mul(40));
            let req = present(&mut s, &f, RefreshHint::Ui).unwrap();
            assert!(!req.flash);
            assert_eq!(req.waveform, Waveform::Auto);
            s.press(Rect::new(0, 0, W, 80), 0);
        }
        assert_eq!(s.ghost_budget(), 0.0);
        let f = Bitmap::new(W, H, 128);
        assert_eq!(present(&mut s, &f, RefreshHint::Turn).unwrap().waveform, Waveform::Reagl);
        assert_eq!(s.ghost_budget(), 0.25);
        s.budget = 6.0;
        assert!(!present(&mut s, &Bitmap::new(W, H, 40), RefreshHint::Ui).unwrap().flash);
        assert_eq!(s.ghost_budget(), 6.0);
        assert!(present(&mut s, &f, RefreshHint::Turn).unwrap().flash);
    }

    #[test]
    fn clean_flashes_only_changed_region() {
        let mut s = sched(RefreshPolicy::default());
        let mut f = Bitmap::new(W, H, 255);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(0, 0, W, 100), 0);
        present(&mut s, &f, RefreshHint::Turn);
        let before = s.ghost_budget();
        f.fill_rect(Rect::new(10, 20, 30, 40), 255);
        let req = present(&mut s, &f, RefreshHint::Clean).unwrap();
        assert!(req.flash);
        assert_eq!(req.rect, Rect::new(6, 16, 38, 48));
        assert!(s.ghost_budget() < before && s.ghost_budget() > 0.0);
    }

    // 标签、弹窗和多脏区使用接近实际比例的屏幕
    const BW: u32 = 400;
    const BH: u32 = 600;

    fn big() -> (RefreshScheduler, Bitmap) {
        let mut s = RefreshScheduler::new(BW, BH, RefreshPolicy::default());
        let f = Bitmap::new(BW, BH, 255);
        present(&mut s, &f, RefreshHint::Ui);
        (s, f)
    }

    #[test]
    fn distant_changes_refresh_as_separate_regions() {
        let (s, mut f) = big();
        f.fill_rect(Rect::new(10, 10, 20, 10), 0);
        f.fill_rect(Rect::new(300, 500, 20, 10), 0);
        let reqs = s.plan(&f, RefreshHint::Ui);
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].rect, Rect::new(6, 6, 28, 18));
        assert_eq!(reqs[1].rect, Rect::new(296, 496, 28, 18));
        assert!(reqs.iter().all(|r| !r.flash && r.waveform == Waveform::Gc16));
    }

    #[test]
    fn dark_tab_restore_does_not_add_a_flash() {
        // 选中的标签 (实心黑块) 移走, 同时下方大片内容变化
        let (mut s, mut f) = big();
        f.fill_rect(Rect::new(0, 520, 96, 64), 0);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(0, 520, 96, 64), 255);
        f.fill_rect(Rect::new(200, 520, 96, 64), 0);
        for y in (40..480).step_by(20) {
            f.fill_rect(Rect::new(20, y, 300, 2), 0);
        }
        let reqs = s.plan(&f, RefreshHint::Ui);
        assert!(reqs.iter().all(|r| !r.flash), "{reqs:?}");
    }

    #[test]
    fn small_feedback_restore_does_not_flash() {
        // 键盘按键: 反相 (A2) 后还原, 反馈区域 < 2% 屏 → 不闪
        let (mut s, f) = big();
        let key = Rect::new(32, 32, 48, 48);
        let mut pressed = f.clone();
        pressed.invert_rect(key);
        present(&mut s, &pressed, RefreshHint::Feedback);
        assert!(s.plan(&f, RefreshHint::Ui).iter().all(|r| !r.flash));
        // 较大的反馈区域 (列表行) 还原也不自动闪刷
        let (mut s, f) = big();
        let row = Rect::new(0, 200, BW, 80);
        let mut pressed = f.clone();
        pressed.invert_rect(row);
        present(&mut s, &pressed, RefreshHint::Feedback);
        assert!(s.plan(&f, RefreshHint::Ui).iter().all(|r| !r.flash));
    }

    #[test]
    fn press_needs_a_valid_screen_copy() {
        let mut s = RefreshScheduler::new(BW, BH, RefreshPolicy::default());
        assert!(s.press(Rect::new(10, 10, 20, 20), 0).is_none());
        present(&mut s, &Bitmap::new(BW, BH, 255), RefreshHint::Ui);
        assert!(s.press(Rect::new(BW as i32 + 5, 10, 20, 20), 0).is_none());
        let req = s.press(Rect::new(10, 10, 20, 20), 0).unwrap();
        assert_eq!(req.waveform, Waveform::A2);
        assert_eq!(req.rect, Rect::new(6, 6, 28, 28));
        assert_eq!(s.screen().get(15, 15), 0);
        assert_eq!(s.screen().get(40, 40), 255);
    }

}
