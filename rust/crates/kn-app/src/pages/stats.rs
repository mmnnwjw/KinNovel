//! 「我的 → 阅读统计」: 今天 / 近 7 天 / 累计、连续阅读天数、近 14 天柱状图、读得最多的书。数据见 [`crate::stats`]。

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, Transition};

use crate::stats::{self, duration_text, Stats};
use crate::store;
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const CHART_DAYS: i64 = 14;
const TOP_BOOKS: usize = 5;

#[derive(Default)]
pub struct StatsPage {
    stats: Stats,
}

/// 从今天往前第 `back` 天的 (日期键, 日)
fn day_back(now: i64, back: i64) -> (String, u32) {
    let t = now - back * 86_400;
    (stats::day_key(t), stats::local_date(t).2)
}

impl Page<KinNovel> for StatsPage {
    fn id(&self) -> PageId {
        PageId(136)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        self.stats = stats::snapshot(&cx.app.paths);
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "阅读统计", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let side = m.margin as i32;
        let inner_w = cx.width as i32 - 2 * side;
        let now = store::unix_now();
        let s = &self.stats;
        let day = |back: i64| s.days.get(&day_back(now, back).0).copied().unwrap_or((0, 0));
        let today = day(0);
        let week = (0..7).map(day).fold((0, 0), |a, d| (a.0 + d.0, a.1 + d.1));
        let total = s.days.values().fold((0, 0), |a, d| (a.0 + d.0, a.1 + d.1));

        // 三张数字卡片
        let mut y = bar.bottom() + side / 2;
        let gap = side / 2;
        let card_w = (inner_w - 2 * gap) / 3;
        let card_h = (m.title * 1.4 + m.small * 1.5 * 2.0) as i32 + side;
        for (i, (label, (secs, pages))) in [("今天", today), ("近 7 天", week), ("累计", total)].into_iter().enumerate() {
            let r = Rect::new(side + i as i32 * (card_w + gap), y, card_w as u32, card_h as u32);
            widgets::card(frame, &theme, &m, r);
            let mut ty = r.y + side / 2;
            ink.text_centered(frame, Rect::new(r.x, ty, r.w, (m.small * 1.4) as u32), label, m.small, theme.muted);
            ty += (m.small * 1.5) as i32;
            let time = ink.fit(&duration_text(secs), m.title, (r.w as i32 - side / 2) as f32);
            ink.text_centered(frame, Rect::new(r.x, ty, r.w, (m.title * 1.4) as u32), &time, m.title, theme.foreground);
            ty += (m.title * 1.4) as i32;
            ink.text_centered(frame, Rect::new(r.x, ty, r.w, (m.small * 1.4) as u32), &format!("{pages} 页"), m.small, theme.muted);
        }
        y += card_h + side / 2;
        let streak = s.streak(now);
        let streak_text = if streak > 0 { format!("已连续阅读 {streak} 天") } else { "今天还没有阅读".to_string() };
        ink.text(frame, side, y, &streak_text, m.body, theme.foreground);
        y += (m.body * 1.8) as i32;

        // 近 14 天柱状图 (分钟)
        ink.text(frame, side, y, "近 14 天", m.small, theme.muted);
        y += (m.small * 1.6) as i32;
        let chart_h = (cx.height as f32 * 0.16) as i32;
        let label_h = (m.tiny * 1.5) as i32;
        let max = (0..CHART_DAYS).map(|b| day(b).0).max().unwrap_or(0).max(60);
        let col_w = inner_w / CHART_DAYS as i32;
        let bar_w = (col_w as f32 * 0.6) as i32;
        let base = y + chart_h;
        frame.fill_rect(Rect::new(side, base, inner_w as u32, 2), theme.foreground);
        for i in 0..CHART_DAYS {
            let back = CHART_DAYS - 1 - i;
            let (secs, _) = day(back);
            let h = (secs as f32 / max as f32 * (chart_h - 4) as f32).round() as i32;
            let x = side + i as i32 * col_w + (col_w - bar_w) / 2;
            if h > 0 {
                let color = if back == 0 { theme.foreground } else { theme.muted };
                frame.fill_rect(Rect::new(x, base - h, bar_w as u32, h as u32), color);
            }
            let (_, d) = day_back(now, back);
            ink.text_centered(frame, Rect::new(side + i as i32 * col_w, base + 4, col_w as u32, label_h as u32), &d.to_string(), m.tiny, if back == 0 { theme.foreground } else { theme.muted });
        }
        let peak = format!("最多 {}", duration_text(max));
        let pw = ink.width(&peak, m.tiny).ceil() as i32;
        ink.text(frame, side + inner_w - pw, y - (m.small * 1.6) as i32, &peak, m.tiny, theme.muted);
        y = base + label_h + side;

        // 读得最多的书
        ink.text(frame, side, y, "读得最多", m.small, theme.muted);
        y += (m.small * 1.6) as i32;
        frame.fill_rect(Rect::new(side, y, inner_w as u32, 2), theme.foreground);
        let mut books: Vec<_> = s.books.values().filter(|b| b.secs > 0).collect();
        books.sort_by(|a, b| b.secs.cmp(&a.secs));
        if books.is_empty() {
            let area = Rect::new(0, y, cx.width, (cx.height as i32 - y).max(0) as u32);
            ink.text_centered(frame, Rect::new(area.x, area.y, area.w, (m.body * 3.0) as u32), "还没有阅读记录", m.body, theme.muted);
            return;
        }
        let row_h = m.row_h() as i32;
        for b in books.into_iter().take(TOP_BOOKS) {
            if y + row_h > cx.height as i32 {
                break;
            }
            let name = if b.name.is_empty() { "未命名" } else { b.name.as_str() };
            let unit = if b.comic { "漫画 · " } else { "" };
            let sub = format!("{unit}{} 页 · 最近 {}", b.pages, super::relative_time(b.last));
            widgets::list_row(&mut ink, frame, &theme, &m, Rect::new(0, y, cx.width, row_h as u32), name, &sub, &duration_text(b.secs));
            y += row_h;
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match (g.kind, cx.hits.at(Point { x: g.start.0, y: g.start.1 })) {
            (GestureKind::Tap, Some(HIT_BACK)) | (GestureKind::SwipeRight, _) => Transition::Back,
            _ => Transition::None,
        }
    }
}
