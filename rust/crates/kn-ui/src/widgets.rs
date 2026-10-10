//! 基础控件绘制 (立即模式): 文字、按钮、顶栏。只负责画, 命中区域由调用方登记到 `cx.hits`。
//!
//! 尺寸约定按 300 ppi 设计 (KPW5), 由 `Metrics` 按实际 dpi/分辨率缩放;
//! 触控目标不小于 ~9 mm (≈ 106 px @300ppi), 与 Python 0.8.0 widgets.py 的下限一致。

use kn_render::{Bitmap, FontId, FontStore, GlyphCache, Rect, TextStyle};

use crate::hits::{HitId, Hits};
use crate::theme::Theme;

/// 与屏幕相关的尺寸 (按 dpi 缩放后的像素)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub scale: f32,
    /// 正文 UI 字号
    pub body: f32,
    pub small: f32,
    pub tiny: f32,
    pub title: f32,
    pub hero: f32,
    /// 最小触控尺寸
    pub touch: u32,
    pub margin: u32,
    pub radius: u32,
}

impl Metrics {
    /// 以 KPW5 (1236x1648, 300 ppi) 为基准按宽高缩放, 上限 1.15 (Scribe 不再放大)。
    /// 下限 0.45: 600x800 (167 ppi, 入门款/KT) 要 0.485、758x1024 (PW1/PW2) 要 0.61 才放得下,
    /// 物理尺寸与 KPW5 接近 (宽度都是 ~3.6–4.1 英寸)。Python 版的 0.75 下限会让版面溢出屏幕。
    pub fn for_screen(width: u32, height: u32) -> Self {
        let scale = (width as f32 / 1236.0).min(height as f32 / 1648.0).clamp(0.45, 1.15);
        let px = |v: f32| (v * scale).round();
        Metrics {
            scale,
            body: px(40.0),
            small: px(33.0),
            tiny: px(27.0),
            title: px(50.0),
            hero: px(90.0),
            touch: px(106.0) as u32,
            margin: px(48.0) as u32,
            radius: px(12.0) as u32,
        }
    }
}

/// 绘制文字所需的资源 (从 Cx 借出)。
pub struct Ink<'a> {
    pub fonts: &'a FontStore,
    pub glyphs: &'a mut GlyphCache,
    pub chain: &'a [FontId],
}

impl<'a> Ink<'a> {
    pub fn style(&self, size: f32) -> TextStyle {
        TextStyle { fonts: self.chain.to_vec(), size_px: size }
    }

    pub fn width(&self, text: &str, size: f32) -> f32 {
        self.fonts.measure(text, &self.style(size))
    }

    /// 左对齐, 以 `top` 为文字行顶部。返回宽度。
    pub fn text(&mut self, frame: &mut Bitmap, x: i32, top: i32, text: &str, size: f32, color: u8) -> f32 {
        let style = self.style(size);
        let m = self.fonts.line_metrics(&style);
        self.glyphs.draw_text(self.fonts, frame, x as f32, top as f32 + m.ascent, text, &style, color)
    }

    /// 在矩形内水平垂直居中 (超宽时截断加省略号)。
    pub fn text_centered(&mut self, frame: &mut Bitmap, rect: Rect, text: &str, size: f32, color: u8) {
        let fitted = self.fit(text, size, rect.w as f32);
        let style = self.style(size);
        let m = self.fonts.line_metrics(&style);
        let w = self.fonts.measure(&fitted, &style);
        let x = rect.x as f32 + (rect.w as f32 - w) / 2.0;
        let line_h = m.ascent + m.descent;
        let baseline = rect.y as f32 + (rect.h as f32 - line_h) / 2.0 + m.ascent;
        self.glyphs.draw_text(self.fonts, frame, x.round(), baseline.round(), &fitted, &style, color);
    }

    /// 截断到 max_width 以内, 超出时末尾加 "…"。
    pub fn fit(&self, text: &str, size: f32, max_width: f32) -> String {
        if self.width(text, size) <= max_width {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate: String = chars.iter().collect::<String>() + "…";
            if self.width(&candidate, size) <= max_width {
                return candidate;
            }
        }
        "…".to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    /// 黑底白字 (主要操作)
    Primary,
    /// 白底黑框 (次要操作)
    Secondary,
    /// 不可用 (灰字)
    Disabled,
}

pub fn button(ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, rect: Rect, label: &str, style: ButtonStyle) {
    let (fill, text) = match style {
        ButtonStyle::Primary => (theme.foreground, theme.background),
        ButtonStyle::Secondary => (theme.background, theme.foreground),
        ButtonStyle::Disabled => (theme.background, theme.muted),
    };
    frame.rounded_rect(rect, m.radius, Some(fill), Some(theme.foreground), 2);
    ink.text_centered(frame, rect.inflate(-8), label, m.small, text);
}

/// 顶栏右端的图标按钮 (命中区域 = 顶栏右端 h×h 的方块, 由调用方登记)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderIcon {
    Home,
    Search,
}

/// 顶栏: 返回箭头 (可选)、标题、右侧状态文字 (时间 · 电量)、主页图标 (可选)。返回顶栏矩形。
pub fn header(ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, title: &str, status: &str, back: bool, home: bool) -> Rect {
    header_with_icon(ink, frame, theme, m, title, status, back, home.then_some(HeaderIcon::Home))
}

/// [`header`] 的一般形式: 右端可以是主页或搜索图标。
#[allow(clippy::too_many_arguments)]
pub fn header_with_icon(ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, title: &str, status: &str, back: bool, icon_kind: Option<HeaderIcon>) -> Rect {
    let h = (m.touch as f32 * 1.1) as u32;
    let bar = Rect::new(0, 0, frame.width(), h);
    frame.fill_rect(bar, theme.light);
    let cy = h as f32 / 2.0;
    let icon = (h as f32 * 0.22).max(14.0);
    let line = (4.0 * m.scale).max(3.0);
    if back {
        let cx = h as f32 / 2.0;
        frame.line(cx + icon, cy, cx - icon, cy, line, theme.foreground);
        frame.line(cx - icon, cy, cx, cy - icon, line, theme.foreground);
        frame.line(cx - icon, cy, cx, cy + icon, line, theme.foreground);
    }
    let mut right = frame.width() as f32;
    if icon_kind == Some(HeaderIcon::Search) {
        // 放大镜: 圆 + 右下手柄
        let cx = frame.width() as f32 - h as f32 / 2.0 - icon * 0.25;
        let cy = cy - icon * 0.25;
        let r = icon * 0.72;
        let steps = 28;
        for i in 0..steps {
            let a0 = i as f32 / steps as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / steps as f32 * std::f32::consts::TAU;
            frame.line(cx + r * a0.cos(), cy + r * a0.sin(), cx + r * a1.cos(), cy + r * a1.sin(), line, theme.foreground);
        }
        let d = r * std::f32::consts::FRAC_1_SQRT_2;
        frame.line(cx + d, cy + d, cx + d + icon * 0.6, cy + d + icon * 0.6, line * 1.4, theme.foreground);
        right -= h as f32;
    }
    if icon_kind == Some(HeaderIcon::Home) {
        let cx = frame.width() as f32 - h as f32 / 2.0;
        let s = icon * 0.9;
        frame.line(cx - s, cy, cx, cy - s, line, theme.foreground);
        frame.line(cx, cy - s, cx + s, cy, line, theme.foreground);
        frame.line(cx - s + 3.0, cy, cx - s + 3.0, cy + s, line, theme.foreground);
        frame.line(cx + s - 3.0, cy, cx + s - 3.0, cy + s, line, theme.foreground);
        frame.line(cx - s + 3.0, cy + s, cx + s - 3.0, cy + s, line, theme.foreground);
        right -= h as f32;
    }
    if !status.is_empty() {
        let w = ink.width(status, m.tiny);
        right -= w + m.margin as f32 / 2.0;
        let st = TextStyle { fonts: ink.chain.to_vec(), size_px: m.tiny };
        let lm = ink.fonts.line_metrics(&st);
        let top = (h as f32 - (lm.ascent + lm.descent)) / 2.0;
        ink.text(frame, right as i32, top as i32, status, m.tiny, theme.foreground);
    }
    let side = h as i32 + m.margin as i32;
    let title_rect = Rect::new(side, 0, (right as i32 - side).max(0) as u32, h);
    ink.text_centered(frame, title_rect, title, m.title, theme.foreground);
    bar
}

// ---------------------------------------------------------------------------
// 1.0 组件 (规范见 rust/UI-DESIGN.md)。都只负责画和登记命中区域, 不保存状态。
// ---------------------------------------------------------------------------

impl Metrics {
    /// 列表行高 (无封面)
    pub fn row_h(&self) -> u32 {
        (self.touch as f32 * 1.35) as u32
    }

    /// 带封面的列表行高
    pub fn row_h_cover(&self) -> u32 {
        (self.touch as f32 * 1.9) as u32
    }

    pub fn header_h(&self) -> u32 {
        (self.touch as f32 * 1.1) as u32
    }

    pub fn tab_bar_h(&self) -> u32 {
        (self.touch as f32 * 1.15) as u32
    }

    pub fn pager_h(&self) -> u32 {
        self.touch
    }
}

/// 底部标签栏。标签的命中 id 为 first_id + 序号。返回标签栏矩形。
#[allow(clippy::too_many_arguments)]
pub fn tab_bar(ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, tabs: &[&str], active: usize, first_id: u32) -> Rect {
    let h = m.tab_bar_h();
    let bar = Rect::new(0, frame.height() as i32 - h as i32, frame.width(), h);
    frame.fill_rect(bar, theme.background);
    frame.fill_rect(Rect::new(0, bar.y, bar.w, 2), theme.foreground);
    let n = tabs.len().max(1) as i32;
    let pad = (m.margin / 4) as i32;
    let cell_w = bar.w as i32 / n;
    for (i, label) in tabs.iter().enumerate() {
        let cell = Rect::new(i as i32 * cell_w, bar.y + 2, cell_w as u32, h - 2);
        if i == active {
            let pill = Rect::new(cell.x + pad, cell.y + pad, (cell_w - 2 * pad) as u32, h - 2 - 2 * pad as u32);
            frame.rounded_rect(pill, m.radius, Some(theme.foreground), None, 0);
            ink.text_centered(frame, pill, label, m.body, theme.background);
        } else {
            ink.text_centered(frame, cell, label, m.body, theme.foreground);
        }
        hits.add(HitId(first_id + i as u32), cell);
    }
    bar
}

/// 列表行: 标题 (body, 单行) / 副标题 (small, muted) / 右侧 meta (small, muted), 底部内缩分隔线。
/// 命中区域由调用方登记 (整行)。
#[allow(clippy::too_many_arguments)]
pub fn list_row(ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, rect: Rect, title: &str, subtitle: &str, meta: &str) {
    let side = m.margin as i32;
    let mut right = rect.right() - side;
    if !meta.is_empty() {
        let w = ink.width(meta, m.small).ceil() as i32;
        let lm = (m.small * 1.25) as i32;
        let y = if subtitle.is_empty() { rect.y + (rect.h as i32 - lm) / 2 } else { rect.y + (rect.h as f32 * 0.16) as i32 };
        ink.text(frame, right - w, y, meta, m.small, theme.muted);
        right -= w + side / 2;
    }
    let width = (right - rect.x - side).max(0) as f32;
    let title_h = (m.body * 1.25) as i32;
    let sub_h = (m.small * 1.25) as i32;
    let block = if subtitle.is_empty() { title_h } else { title_h + sub_h + (m.small * 0.3) as i32 };
    let top = rect.y + (rect.h as i32 - block) / 2;
    let title = ink.fit(title, m.body, width);
    ink.text(frame, rect.x + side, top, &title, m.body, theme.foreground);
    if !subtitle.is_empty() {
        let subtitle = ink.fit(subtitle, m.small, (rect.right() - side - rect.x - side) as f32);
        ink.text(frame, rect.x + side, top + title_h + (m.small * 0.3) as i32, &subtitle, m.small, theme.muted);
    }
    frame.fill_rect(Rect::new(rect.x + side, rect.bottom() - 1, (rect.w as i32 - 2 * side).max(0) as u32, 1), theme.mid);
}

/// 带封面的列表行里封面占的矩形 (纵横比 ~0.72, 贴左对齐, 上下留 margin/2 的边)。
/// 调用方用它的宽高向封面缓存要图, 再把结果传给 [`list_row_cover`]。
pub fn cover_slot(m: &Metrics, rect: Rect) -> Rect {
    let side = m.margin as i32;
    let pad = side / 2;
    let cover_h = (rect.h as i32 - 2 * pad).max(1);
    let cover_w = ((cover_h as f32) * 0.72) as i32;
    Rect::new(rect.x + side, rect.y + pad, cover_w.max(1) as u32, cover_h as u32)
}

/// 列表行 (带封面): 封面居左 (见 [`cover_slot`]), 标题 (body) / 副标题 (small muted) /
/// meta (small muted) 居右, 底部内缩分隔线。用 `Metrics::row_h_cover`。
/// `cover` 为 `None` 时画一个细描边圆角占位框 (不画内容, 与 UI-DESIGN 的约定一致)。
#[allow(clippy::too_many_arguments)]
pub fn list_row_cover(ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, rect: Rect, cover: Option<&Bitmap>, title: &str, subtitle: &str, meta: &str) {
    let side = m.margin as i32;
    let pad = side / 2;
    let cover_rect = cover_slot(m, rect);
    match cover {
        Some(img) => {
            let dx = cover_rect.x + (cover_rect.w as i32 - img.width() as i32) / 2;
            let dy = cover_rect.y + (cover_rect.h as i32 - img.height() as i32) / 2;
            frame.blit(img, img.bounds(), dx, dy);
        }
        None => frame.rounded_rect(cover_rect, (m.radius / 2).max(4), None, Some(theme.mid), 2),
    }
    let text_x = cover_rect.right() + pad;
    let text_w = (rect.right() - side - text_x).max(0) as f32;
    let title_h = (m.body * 1.25) as i32;
    let sub_h = (m.small * 1.25) as i32;
    let mut y = rect.y + pad;
    let fitted_title = ink.fit(title, m.body, text_w);
    ink.text(frame, text_x, y, &fitted_title, m.body, theme.foreground);
    y += title_h + (m.small * 0.3) as i32;
    if !subtitle.is_empty() {
        let fitted = ink.fit(subtitle, m.small, text_w);
        ink.text(frame, text_x, y, &fitted, m.small, theme.muted);
        y += sub_h + (m.small * 0.2) as i32;
    }
    if !meta.is_empty() {
        let fitted = ink.fit(meta, m.small, text_w);
        ink.text(frame, text_x, y, &fitted, m.small, theme.muted);
    }
    frame.fill_rect(Rect::new(rect.x + side, rect.bottom() - 1, (rect.w as i32 - 2 * side).max(0) as u32, 1), theme.mid);
}

/// 分页条: 上一页 · n / m · 下一页。到头的按钮置灰且不可点。
#[allow(clippy::too_many_arguments)]
pub fn pager(ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, rect: Rect, page: usize, pages: usize, prev_id: HitId, next_id: HitId) {
    let pages = pages.max(1);
    let side = m.margin as i32;
    let bw = ((rect.w as i32 - 2 * side) / 3).max(0) as u32;
    let prev = Rect::new(rect.x + side, rect.y, bw, rect.h);
    let next = Rect::new(rect.right() - side - bw as i32, rect.y, bw, rect.h);
    let has_prev = page > 0;
    let has_next = page + 1 < pages;
    ink.text_centered(frame, prev, "上一页", m.small, if has_prev { theme.foreground } else { theme.mid });
    ink.text_centered(frame, next, "下一页", m.small, if has_next { theme.foreground } else { theme.mid });
    let label = format!("{} / {}", page + 1, pages);
    ink.text_centered(frame, Rect::new(prev.right(), rect.y, (next.x - prev.right()).max(0) as u32, rect.h), &label, m.small, theme.muted);
    hits.add(prev_id, prev).enabled(has_prev);
    hits.add(next_id, next).enabled(has_next);
}

/// 分段选择: 等宽按钮, 当前项为 Primary。命中 id 为 first_id + 序号。
#[allow(clippy::too_many_arguments)]
pub fn segmented(ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, rect: Rect, options: &[&str], active: usize, first_id: u32) {
    let n = options.len().max(1) as i32;
    let gap = (m.margin / 3) as i32;
    let w = ((rect.w as i32 - gap * (n - 1)) / n).max(0) as u32;
    for (i, label) in options.iter().enumerate() {
        let r = Rect::new(rect.x + i as i32 * (w as i32 + gap), rect.y, w, rect.h);
        let style = if i == active { ButtonStyle::Primary } else { ButtonStyle::Secondary };
        button(ink, frame, theme, m, r, label, style);
        hits.add(HitId(first_id + i as u32), r);
    }
}

/// 卡片底框 (圆角 + 2 px 描边)。
pub fn card(frame: &mut Bitmap, theme: &Theme, m: &Metrics, rect: Rect) {
    frame.rounded_rect(rect, m.radius, Some(theme.background), Some(theme.foreground), 2);
}

/// 模态对话框: 居中卡片 (80% 宽), 标题 + 若干行文字 + 底部按钮行。
/// 先登记一个覆盖全屏、无反馈的命中区域 (id = u32::MAX) 吞掉框外点击, 再登记按钮。返回对话框矩形。
#[allow(clippy::too_many_arguments)]
pub fn dialog(ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, title: &str, lines: &[&str], buttons: &[(&str, HitId, ButtonStyle)]) -> Rect {
    let (fw, fh) = (frame.width(), frame.height());
    let w = (fw as f32 * 0.8) as u32;
    let pad = m.margin as i32;
    let inner = w as i32 - 2 * pad;
    let line_h = (m.body * 1.5) as i32;
    let title_h = if title.is_empty() { 0 } else { (m.title * 1.5) as i32 };
    let btn_h = m.touch as i32;
    let h = pad + title_h + line_h * lines.len() as i32 + pad / 2 + btn_h + pad;
    let rect = Rect::new((fw - w) as i32 / 2, (fh as i32 - h) / 2, w, h as u32);
    hits.add(HitId(u32::MAX), frame.bounds()).no_feedback();
    frame.rounded_rect(rect, m.radius, Some(theme.background), Some(theme.foreground), 3);
    let mut y = rect.y + pad;
    if !title.is_empty() {
        let t = ink.fit(title, m.title, inner as f32);
        ink.text_centered(frame, Rect::new(rect.x + pad, y, inner as u32, title_h as u32), &t, m.title, theme.foreground);
        y += title_h;
    }
    for line in lines {
        let t = ink.fit(line, m.body, inner as f32);
        ink.text_centered(frame, Rect::new(rect.x + pad, y, inner as u32, line_h as u32), &t, m.body, theme.foreground);
        y += line_h;
    }
    y += pad / 2;
    let n = buttons.len().max(1) as i32;
    let gap = pad / 2;
    let bw = ((inner - gap * (n - 1)) / n).max(0) as u32;
    for (i, (label, id, style)) in buttons.iter().enumerate() {
        let r = Rect::new(rect.x + pad + i as i32 * (bw as i32 + gap), y, bw, btn_h as u32);
        button(ink, frame, theme, m, r, label, *style);
        hits.add(*id, r);
    }
    rect
}

/// 提示条: 反色圆角条, 水平居中, 底边在 bottom。
pub fn toast(ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, text: &str, bottom: i32) -> Rect {
    let max_w = frame.width() - 2 * m.margin;
    let text = ink.fit(text, m.small, (max_w - 2 * m.margin) as f32);
    let w = (ink.width(&text, m.small).ceil() as u32 + 2 * m.margin).min(max_w);
    let h = (m.small * 2.2) as u32;
    let rect = Rect::new((frame.width() - w) as i32 / 2, bottom - h as i32, w, h);
    frame.rounded_rect(rect, h / 2, Some(theme.foreground), None, 0);
    ink.text_centered(frame, rect, &text, m.small, theme.background);
    rect
}

/// 居中的状态文字 (加载中 / 空 / 出错), 可带一个主按钮。
#[allow(clippy::too_many_arguments)]
pub fn state_message(ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, area: Rect, text: &str, action: Option<(&str, HitId)>) {
    let line_h = (m.body * 1.6) as i32;
    let lines: Vec<&str> = text.lines().collect();
    let btn_h = if action.is_some() { m.touch as i32 + m.margin as i32 } else { 0 };
    let total = line_h * lines.len() as i32 + btn_h;
    let mut y = area.y + (area.h as i32 - total) / 2;
    for line in lines {
        let t = ink.fit(line, m.body, (area.w - 2 * m.margin) as f32);
        ink.text_centered(frame, Rect::new(area.x, y, area.w, line_h as u32), &t, m.body, theme.muted);
        y += line_h;
    }
    if let Some((label, id)) = action {
        let w = (area.w as f32 * 0.45) as u32;
        let r = Rect::new(area.x + (area.w - w) as i32 / 2, y + m.margin as i32 / 2, w, m.touch);
        button(ink, frame, theme, m, r, label, ButtonStyle::Primary);
        hits.add(id, r);
    }
}
