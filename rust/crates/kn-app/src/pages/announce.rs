//! 公告: 列表 (分页) + 详情 (正文分页, 带"评论"入口)。对照 Python `announcements.py`。
//! 都是压栈页面: 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::comments::CommentsPage;
use crate::api::{self, Announcement, AnnouncementDetail};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const ROW_BASE: u32 = 1000;

enum Status<T> {
    Loading,
    Offline,
    Error(String),
    Ready(T),
}

fn online(cx: &Cx<KinNovel>) -> bool {
    cx.app.net().is_some() || api::fake_mode()
}

// ---------------------------------------------------------------------------
// 列表
// ---------------------------------------------------------------------------

struct ListLoaded(i64, Result<Vec<Announcement>, String>, i64, i64);

pub struct AnnouncementsPage {
    status: Status<()>,
    items: Vec<Announcement>,
    page: i64,
    total_pages: i64,
}

impl AnnouncementsPage {
    pub fn new() -> Self {
        AnnouncementsPage { status: Status::Loading, items: Vec::new(), page: 1, total_pages: 1 }
    }
}

fn per_page(cx: &Cx<KinNovel>) -> i64 {
    let m = cx.app.metrics;
    let top = m.header_h() as i32;
    let bottom = m.pager_h() as i32;
    let area = cx.height as i32 - top - bottom;
    (area / m.row_h() as i32).max(1) as i64
}

impl AnnouncementsPage {
    fn load(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if !online(cx) {
            self.status = Status::Offline;
            return;
        }
        self.status = Status::Loading;
        self.page = page;
        let net = cx.app.net();
        let size = per_page(cx);
        cx.spawn(move || {
            let r = api::load_announcement_list(net.as_ref(), page, size);
            match r {
                Ok(p) => ListLoaded(page, Ok(p.items), p.page, p.total_pages),
                Err(e) => ListLoaded(page, Err(e), page, page),
            }
        });
    }
}

impl Page<KinNovel> for AnnouncementsPage {
    fn id(&self) -> PageId {
        PageId(300)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx, 1);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        if let Ok(l) = msg.downcast::<ListLoaded>() {
            if l.0 == self.page {
                match l.1 {
                    Ok(items) => {
                        self.items = items;
                        self.page = l.2;
                        self.total_pages = l.3;
                        self.status = Status::Ready(());
                    }
                    Err(e) => self.status = Status::Error(e),
                }
                cx.request_redraw(RefreshHint::Ui);
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "公告", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);
        match &self.status {
            Status::Loading => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                return;
            }
            Status::Offline => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY)));
                return;
            }
            Status::Error(e) => {
                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                return;
            }
            Status::Ready(()) => {}
        }
        if self.items.is_empty() {
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无公告", None);
            return;
        }
        let row_h = m.row_h();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        for (row, item) in self.items.iter().enumerate() {
            let r = Rect::new(0, bar.bottom() + row as i32 * row_h as i32, cx.width, row_h);
            if r.bottom() > list_bottom {
                break;
            }
            widgets::list_row(&mut ink, frame, &theme, &m, r, &item.title, "", &item.created_at);
            cx.hits.add(HitId(ROW_BASE + row as u32), r);
        }
        if self.total_pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (self.page - 1).max(0) as usize, self.total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        match hit {
            HIT_BACK => Transition::Back,
            HIT_RETRY => {
                self.load(cx, self.page);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_PREV => {
                if self.page > 1 {
                    self.load(cx, self.page - 1);
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            HIT_NEXT => {
                if self.page < self.total_pages {
                    self.load(cx, self.page + 1);
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            HitId(id) if id >= ROW_BASE => match self.items.get((id - ROW_BASE) as usize) {
                Some(item) => Transition::Push(Box::new(AnnouncementDetailPage::new(item.id))),
                None => Transition::None,
            },
            _ => Transition::None,
        }
    }
}

// ---------------------------------------------------------------------------
// 详情
// ---------------------------------------------------------------------------

const DET_HIT_BACK: HitId = HitId(1);
const DET_HIT_PREV: HitId = HitId(2);
const DET_HIT_NEXT: HitId = HitId(3);
const DET_HIT_RETRY: HitId = HitId(4);
const DET_HIT_COMMENTS: HitId = HitId(5);

struct DetailLoaded(Result<AnnouncementDetail, String>);

pub struct AnnouncementDetailPage {
    id: i64,
    status: Status<AnnouncementDetail>,
    /// 正文已折行的全部文本行 (按当前屏宽计算一次)。
    lines: Vec<String>,
    page: usize,
}

impl AnnouncementDetailPage {
    pub fn new(id: i64) -> Self {
        AnnouncementDetailPage { id, status: Status::Loading, lines: Vec::new(), page: 0 }
    }

    fn load(&mut self, cx: &mut Cx<KinNovel>) {
        if !online(cx) {
            self.status = Status::Offline;
            return;
        }
        self.status = Status::Loading;
        let net = cx.app.net();
        let id = self.id;
        cx.spawn(move || DetailLoaded(api::load_announcement_detail(net.as_ref(), id)));
    }

    fn body_rows_per_page(&self, cx: &Cx<KinNovel>) -> usize {
        let m = cx.app.metrics;
        let top = m.header_h() as i32 + m.touch as i32 + m.margin as i32 / 2;
        let bottom = m.pager_h() as i32;
        let area = cx.height as i32 - top - bottom;
        let line_h = (m.small * 1.5) as i32;
        (area / line_h.max(1)).max(1) as usize
    }
}

impl Page<KinNovel> for AnnouncementDetailPage {
    fn id(&self) -> PageId {
        PageId(301)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        if let Ok(l) = msg.downcast::<DetailLoaded>() {
            self.status = match l.0 {
                Ok(detail) => Status::Ready(detail),
                Err(e) => Status::Error(e),
            };
            self.lines.clear();
            self.page = 0;
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "公告详情", &super::status_text(), true, false);
        cx.hits.add(DET_HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);

        let detail = match &self.status {
            Status::Loading => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                return;
            }
            Status::Offline => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", DET_HIT_RETRY)));
                return;
            }
            Status::Error(e) => {
                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", DET_HIT_RETRY)));
                return;
            }
            Status::Ready(d) => d,
        };

        let side = m.margin as i32;
        let title_w = (cx.width - 2 * m.margin) as i32 - (m.small as i32 * 6);
        let title = ink.fit(&format!("{} · {}", detail.title, detail.date), m.small, title_w as f32);
        ink.text(frame, side, bar.bottom() + m.margin as i32 / 4, &title, m.small, theme.muted);

        let comments_rect = Rect::new(cx.width as i32 - side - (m.small * 5.0) as i32, bar.bottom(), (m.small * 5.0) as u32, m.touch);
        widgets::button(&mut ink, frame, &theme, &m, comments_rect, "评论", ButtonStyle::Secondary);
        cx.hits.add(DET_HIT_COMMENTS, comments_rect).rounded(m.radius);

        if self.lines.is_empty() && !detail.paragraphs.is_empty() {
            let body_w = (cx.width - 2 * m.margin) as f32;
            for (i, p) in detail.paragraphs.iter().enumerate() {
                self.lines.extend(wrap_all(p, &ink, m.small, body_w));
                if i + 1 < detail.paragraphs.len() {
                    self.lines.push(String::new());
                }
            }
        }

        let body_top = bar.bottom() + m.touch as i32 + m.margin as i32 / 2;
        let body_bottom = cx.height as i32 - m.pager_h() as i32;
        let line_h = (m.small * 1.5) as i32;
        let per_page = ((body_bottom - body_top) / line_h.max(1)).max(1) as usize;
        let pages = self.lines.len().div_ceil(per_page).max(1);
        self.page = self.page.min(pages - 1);
        let mut y = body_top;
        for line in self.lines.iter().skip(self.page * per_page).take(per_page) {
            if !line.is_empty() {
                ink.text(frame, side, y, line, m.small, theme.foreground);
            }
            y += line_h;
        }
        if pages > 1 {
            let pr = Rect::new(0, body_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, DET_HIT_PREV, DET_HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match g.kind {
            GestureKind::Tap => {
                let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
                match hit {
                    DET_HIT_BACK => Transition::Back,
                    DET_HIT_RETRY => {
                        self.load(cx);
                        cx.request_redraw(RefreshHint::Ui);
                        Transition::None
                    }
                    DET_HIT_PREV => {
                        self.turn(cx, -1);
                        Transition::None
                    }
                    DET_HIT_NEXT => {
                        self.turn(cx, 1);
                        Transition::None
                    }
                    DET_HIT_COMMENTS => Transition::Push(Box::new(CommentsPage::new("Announcement", self.id))),
                    _ => Transition::None,
                }
            }
            GestureKind::SwipeUp | GestureKind::SwipeLeft => {
                self.turn(cx, 1);
                Transition::None
            }
            GestureKind::SwipeDown | GestureKind::SwipeRight => {
                self.turn(cx, -1);
                Transition::None
            }
            _ => Transition::None,
        }
    }
}

impl AnnouncementDetailPage {
    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) {
        let per_page = self.body_rows_per_page(cx);
        let pages = self.lines.len().div_ceil(per_page.max(1)).max(1);
        let next = (self.page as i64 + delta).clamp(0, pages as i64 - 1) as usize;
        if next != self.page {
            self.page = next;
            cx.request_redraw(RefreshHint::Ui);
        }
    }
}

/// 极简折行 (同 `book.rs::wrap_plain`, 不做 kinsoku, 不限制行数)。
pub(crate) fn wrap_all(text: &str, ink: &Ink, size: f32, max_width: f32) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let mut candidate = current.clone();
        candidate.push(ch);
        if ink.width(&candidate, size) > max_width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
