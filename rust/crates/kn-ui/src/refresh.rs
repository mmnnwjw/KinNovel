//! 刷新调度: 差分 + 波形选择 + 残影预算。纯逻辑, 主机可测。
//!
//! 规则移植自 Python 0.8.0 `PageContext._refresh_plan / _waveform_for` (bin/src/kinnovel/ui.py),
//! 并参照 KOReader `UIManager` (`_refresh` / `setDirty` 的刷新模式与脏区合并) 扩展:
//! - `plan` 返回按顺序提交的若干刷新; 空表示无需刷新。
//! - 多脏区 (KOReader 每次重绘可有多个刷新区域, 只合并相交的): 差分按纵向间隔 (>= 32 行未变) 拆成
//!   最多 4 块, 各自外扩 4 px 分别刷新; 总面积 >= 95% 屏视为整屏。顶部按钮与底部标签同时变化时不再整屏刷。
//! - `Flash`: 强制整屏 GC16 闪刷, 预算清零。
//! - 大面积 (变化总面积 >= 50% 屏) 且预算 >= 阈值 → 整屏闪刷 (KOReader: partial 累计 FULL_REFRESH_COUNT 次升级)。
//!   阈值: 翻页用 policy.full_refresh_every; 其它界面最多 `UI_FLASH_EVERY` 屏 (列表/菜单的残影比正文明显)。
//!   翻页且 policy.flash_every_turn → 每页闪刷。
//! - `Clean`: 只对各变化区域闪刷 (关闭弹窗; KOReader 关闭 ButtonDialog 等用 "flashui")。
//! - 波形: Turn → Reagl (设备支持时, 否则 Gl16); Ui → Gc16; Fast → Du; Flash/Clean → Gc16 + flash;
//!   Feedback (按下反相) → 小块 (< 2% 屏, 键盘按键) A2, 其余 DU (KOReader: 键盘 "a2", 其它高亮 "fast")。
//! - 定点清残影 (仅 Ui/Fast): 实心深色块 (选中的标签/按钮、反相的行) 在局部刷新下变回背景色最容易留残影。
//!   按 16 px 格子统计 "由深变成背景色" 的像素, 过半的格子连成块, 块够大 (>= 0.6% 屏, 键盘按键以下不算)
//!   就在该区域刷新之后对块单独闪刷; 块占该区域一半以上时区域本身改为闪刷。
//!   小于 2% 屏的按下反馈区域内的还原不算 (键盘逐键反相不闪)。夜间模式方向相反 (`set_background`)。
//! - 预算累计: 非闪刷时 += 面积占比, Turn 且使用 Reagl 时 × 0.25; 局部闪刷扣掉对应面积。
//! - Turn 跳过差分直接整屏 (几乎全屏都变, 省掉 diff 开销)。
//! - 首帧或 `invalidate()` 之后: 整屏刷新 (不闪, 除非 hint 为 Flash 或预算已满)。

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
/// 相隔至少这么多行未变化, 差分就拆成两个区域。
const REGION_GAP: u32 = 32;
/// 一次重绘最多几个差分区域 (EPDC 同时进行的更新数有限, 再多就合并间隔最小的)。
const MAX_REGIONS: usize = 4;
/// 变化面积达到整屏这个比例才算"大面积", 只有大面积刷新会被升级为闪刷。
const LARGE_RATIO: f32 = 0.5;
/// 达到这个比例直接按整屏处理。
const FULL_RATIO: f32 = 0.95;
/// REAGL 翻页自带残影抑制, 计入预算时打折。
const REAGL_TURN_WEIGHT: f32 = 0.25;
/// 阅读页以外: 局部刷新累计多少屏后, 下一次大面积刷新升级为闪刷 (配置值更小时用配置值)。
const UI_FLASH_EVERY: f32 = 3.0;
/// 定点清残影的格子边长 (px)。
const CLEAR_TILE: u32 = 16;
/// 格子里至少这么多像素由深色变成背景色, 才算 "实心深色块被清掉"。正文/列表文字远达不到。
const CLEAR_TILE_FILL: u32 = CLEAR_TILE * CLEAR_TILE / 2;
/// 清掉的实心块至少占整屏这个比例才单独闪刷 (KPW5: 键盘按键 ~0.45%, 筛选按钮 ~0.75%, 底部标签 ~1.3%)。
const CLEAR_MIN_RATIO: f32 = 0.006;
/// 按下反馈区域小于整屏这个比例时, 其中的反相还原不触发闪刷 (键盘按键、小按钮)。
const FEEDBACK_SMALL_RATIO: f32 = 0.02;
/// 清掉的块占主刷新区域这个比例以上: 主刷新直接改成区域闪刷, 省一次刷新。
const CLEAR_TAKEOVER_RATIO: f32 = 0.5;
/// 单独闪刷的块最多几个, 再多就合并成一个包围盒。
const CLEAR_MAX_RECTS: usize = 4;
/// 深/浅的判定阈值 (L8)。
const DARK_MAX: u8 = 96;
const LIGHT_MIN: u8 = 160;

pub struct RefreshScheduler {
    /// 屏幕上当前内容 (仅 screen_valid 时可信)
    screen: Bitmap,
    screen_valid: bool,
    budget: f32,
    policy: RefreshPolicy,
    /// 主题背景是否为浅色 (夜间模式为深色: 残影来自浅色块变黑)
    light_background: bool,
    /// 最近一次按下反馈的区域, 直到下一次非反馈刷新提交
    feedback: Option<Rect>,
}

impl RefreshScheduler {
    pub fn new(width: u32, height: u32, policy: RefreshPolicy) -> Self {
        RefreshScheduler {
            screen: Bitmap::new(width, height, 255),
            screen_valid: false,
            budget: 0.0,
            policy,
            light_background: true,
            feedback: None,
        }
    }

    pub fn set_policy(&mut self, policy: RefreshPolicy) {
        self.policy = policy;
    }

    /// 主题背景色 (定点清残影按 "变成背景色" 判断方向)。
    pub fn set_background(&mut self, background: u8) {
        self.light_background = background >= 128;
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
        if ratio >= LARGE_RATIO && self.budget >= self.threshold(hint) {
            return vec![self.flash_request()];
        }
        if hint == RefreshHint::Clean {
            return regions.into_iter().map(|r| Self::request(r, Waveform::Gc16, true)).collect();
        }
        // Ui / Fast: 定点清残影
        let ignore = self.feedback.filter(|r| (r.area() as f32) < full_area * FEEDBACK_SMALL_RATIO);
        let min_area = (full_area * CLEAR_MIN_RATIO) as u64;
        let mut out = Vec::with_capacity(regions.len());
        let mut blocks = Vec::new();
        for r in regions {
            let found = cleared_blocks(&self.screen, frame, r, self.light_background, ignore, min_area);
            let cleared: u64 = found.iter().map(|b| b.area()).sum();
            if cleared as f32 >= r.area() as f32 * CLEAR_TAKEOVER_RATIO {
                out.push(Self::request(r, Waveform::Gc16, true));
            } else {
                out.push(Self::request(r, self.waveform(hint), false));
                blocks.extend(found);
            }
        }
        if blocks.len() > CLEAR_MAX_RECTS {
            let union = blocks.iter().skip(1).fold(blocks[0], |acc, b| acc.union(b));
            blocks = vec![union];
        }
        out.extend(
            blocks
                .into_iter()
                .filter_map(|b| b.inflate(REGION_PAD).intersect(&full))
                .map(|b| Self::request(b, Waveform::Gc16, true)),
        );
        out
    }

    /// 整屏刷新 (首帧、invalidate 之后、翻页、变化接近整屏)。
    fn plan_full(&self, hint: RefreshHint) -> RefreshRequest {
        let full = self.full();
        if hint != RefreshHint::Feedback {
            let turn_flash = hint == RefreshHint::Turn && self.policy.flash_every_turn;
            if turn_flash || hint == RefreshHint::Clean || self.budget >= self.threshold(hint) {
                return self.flash_request();
            }
        }
        let waveform = if hint == RefreshHint::Feedback { self.feedback_waveform(full) } else { self.waveform(hint) };
        Self::request(full, waveform, false)
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

    /// 刷新已成功提交: 更新屏幕帧副本 (只拷贝 req.rect 区域) 与残影预算。
    pub fn commit(&mut self, frame: &Bitmap, req: &RefreshRequest, hint: RefreshHint) {
        let full = self.full();
        if req.rect == full || !self.screen_valid {
            self.screen.blit(frame, full, 0, 0);
            self.screen_valid = true;
        } else {
            self.screen.blit(frame, req.rect, req.rect.x, req.rect.y);
        }
        if hint == RefreshHint::Feedback {
            self.feedback = Some(self.feedback.map_or(req.rect, |r| r.union(&req.rect)));
        } else {
            self.feedback = None;
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

/// 在 `rect` 内找 "实心深色块被清成背景色" 的连通块 (见模块说明), 返回各块包围盒 (已裁到 `rect`)。
fn cleared_blocks(old: &Bitmap, new: &Bitmap, rect: Rect, light_bg: bool, ignore: Option<Rect>, min_area: u64) -> Vec<Rect> {
    let t = CLEAR_TILE as i32;
    let (tx0, ty0) = (rect.x / t, rect.y / t);
    let (tx1, ty1) = ((rect.right() + t - 1) / t, (rect.bottom() + t - 1) / t);
    let (cols, rows) = ((tx1 - tx0) as usize, (ty1 - ty0) as usize);
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let mut counts = vec![0u32; cols * rows];
    // 逐行把命中 (0/1) 累加到按列的 u8 累加器 (整行一个长循环, 便于 NEON 向量化), 每满一行格子再按格汇总。
    // 逐格切片计数在 KPW5 上让整屏计划多花 ~15 ms。
    let (x0, x1) = (rect.x as usize, rect.right() as usize);
    let mut acc = vec![0u8; x1 - x0];
    let flush = |acc: &mut [u8], tile_row: usize, counts: &mut [u32]| {
        for (c, count) in counts[tile_row * cols..][..cols].iter_mut().enumerate() {
            let from = (((tx0 + c as i32) * t) as usize).max(x0) - x0;
            let to = (((tx0 + c as i32 + 1) * t) as usize).min(x1) - x0;
            *count += acc[from..to].iter().map(|&v| v as u32).sum::<u32>();
        }
        acc.fill(0);
    };
    let flip = if light_bg { 0u8 } else { 255u8 };
    for y in rect.y..rect.bottom() {
        let a = &old.row(y as u32)[x0..x1];
        let b = &new.row(y as u32)[x0..x1];
        // 夜间模式取反后同一判断: 浅色块变黑
        for ((v, &o), &n) in acc.iter_mut().zip(a).zip(b) {
            *v += (((o ^ flip) <= DARK_MAX) & ((n ^ flip) >= LIGHT_MIN)) as u8;
        }
        if (y + 1) % t == 0 || y + 1 == rect.bottom() {
            flush(&mut acc, (y / t - ty0) as usize, &mut counts);
        }
    }
    let mut solid: Vec<bool> = counts.iter().map(|&n| n >= CLEAR_TILE_FILL).collect();
    if let Some(ig) = ignore {
        for (i, s) in solid.iter_mut().enumerate() {
            let cx = (tx0 + (i % cols) as i32) * t + t / 2;
            let cy = (ty0 + (i / cols) as i32) * t + t / 2;
            if ig.contains(kn_render::Point { x: cx, y: cy }) {
                *s = false;
            }
        }
    }
    // 8 邻域连通块
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..solid.len() {
        if !solid[start] {
            continue;
        }
        solid[start] = false;
        stack.push(start);
        let (mut c0, mut c1, mut r0, mut r1, mut n) = (cols, 0, rows, 0, 0u64);
        while let Some(i) = stack.pop() {
            let (c, r) = (i % cols, i / cols);
            (c0, c1, r0, r1) = (c0.min(c), c1.max(c), r0.min(r), r1.max(r));
            n += 1;
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    let (nc, nr) = (c as i32 + dc, r as i32 + dr);
                    if nc < 0 || nr < 0 || nc >= cols as i32 || nr >= rows as i32 {
                        continue;
                    }
                    let j = nr as usize * cols + nc as usize;
                    if solid[j] {
                        solid[j] = false;
                        stack.push(j);
                    }
                }
            }
        }
        if n * (t * t) as u64 >= min_area {
            let block = Rect::new(
                (tx0 + c0 as i32) * t,
                (ty0 + r0 as i32) * t,
                ((c1 - c0 + 1) as i32 * t) as u32,
                ((r1 - r0 + 1) as i32 * t) as u32,
            );
            out.extend(block.intersect(&rect));
        }
    }
    out
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

    // 定点清残影与多脏区用接近真实比例的屏幕 (0.6% = 1440 px, 约 6 个格子)
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
        let (mut s, mut f) = big();
        f.fill_rect(Rect::new(10, 10, 20, 10), 0);
        f.fill_rect(Rect::new(300, 500, 20, 10), 0);
        let reqs = s.plan(&f, RefreshHint::Ui);
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].rect, Rect::new(6, 6, 28, 18));
        assert_eq!(reqs[1].rect, Rect::new(296, 496, 28, 18));
        assert!(reqs.iter().all(|r| !r.flash && r.waveform == Waveform::Gc16));
    }

    #[test]
    fn cleared_dark_block_gets_its_own_flash() {
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
        let flashes: Vec<_> = reqs.iter().filter(|r| r.flash).collect();
        assert_eq!(flashes.len(), 1, "{reqs:?}");
        assert!(flashes[0].rect.contains(kn_render::Point { x: 48, y: 552 }));
        assert!(!flashes[0].rect.contains(kn_render::Point { x: 250, y: 552 }));
        assert!(reqs.iter().any(|r| !r.flash && r.rect.contains(kn_render::Point { x: 100, y: 200 })));
        // 闪刷排在普通刷新之后
        assert!(reqs.last().unwrap().flash);
    }

    #[test]
    fn region_mostly_cleared_flashes_itself() {
        // 关掉带深色按钮的小弹层 (Ui 提示): 区域本身改闪刷, 不再额外刷一次
        let (mut s, mut f) = big();
        f.fill_rect(Rect::new(100, 100, 160, 96), 0);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(100, 100, 160, 96), 255);
        let reqs = s.plan(&f, RefreshHint::Ui);
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].flash && reqs[0].waveform == Waveform::Gc16);
    }

    #[test]
    fn text_and_small_blocks_do_not_flash() {
        let (mut s, mut f) = big();
        // 按键大小的黑块 (约 0.4% 屏) 与文字线条
        f.fill_rect(Rect::new(16, 16, 32, 32), 0);
        for y in (100..300).step_by(12) {
            f.fill_rect(Rect::new(20, y, 300, 3), 0);
        }
        present(&mut s, &f, RefreshHint::Ui);
        let blank = Bitmap::new(BW, BH, 255);
        assert!(s.plan(&blank, RefreshHint::Ui).iter().all(|r| !r.flash));
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
        // 较大的反馈区域 (列表行) 还原 → 闪刷
        let (mut s, f) = big();
        let row = Rect::new(0, 200, BW, 80);
        let mut pressed = f.clone();
        pressed.invert_rect(row);
        present(&mut s, &pressed, RefreshHint::Feedback);
        assert!(s.plan(&f, RefreshHint::Ui).iter().any(|r| r.flash));
    }

    #[test]
    fn night_mode_detects_light_blocks() {
        let mut s = RefreshScheduler::new(BW, BH, RefreshPolicy::default());
        s.set_background(0);
        let mut f = Bitmap::new(BW, BH, 0);
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(100, 100, 160, 96), 255);
        assert!(s.plan(&f, RefreshHint::Ui).iter().all(|r| !r.flash));
        present(&mut s, &f, RefreshHint::Ui);
        f.fill_rect(Rect::new(100, 100, 160, 96), 0);
        assert!(s.plan(&f, RefreshHint::Ui).iter().any(|r| r.flash));
    }

    #[test]
    fn hint_strength_order() {
        assert!(RefreshHint::Flash.strength() > RefreshHint::Turn.strength());
        assert!(RefreshHint::Turn.strength() > RefreshHint::Clean.strength());
        assert!(RefreshHint::Clean.strength() > RefreshHint::Ui.strength());
        assert!(RefreshHint::Ui.strength() > RefreshHint::Fast.strength());
    }
}
