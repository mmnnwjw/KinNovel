//! 历史: 阅读历史 (云端, 对照 Python `history.py`)。只取当前页的书籍元数据, 翻页时按需补。

use std::any::Any;
use std::collections::HashMap;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::book::BookDetailPage;
use super::Tab;
use crate::api::{self, BookItem};
use crate::covers::CoverCache;
use crate::KinNovel;

const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const ROW_BASE: u32 = 1000;

enum Status {
    Loading,
    /// 离线且不是 fixture 模式
    Offline,
    /// 在线但未登录
    NeedLogin,
    Error(String),
    Ready,
}

struct IdsLoaded(Result<Vec<i64>, String>);
struct BooksLoaded {
    page: usize,
    result: Result<Vec<BookItem>, String>,
}

pub struct HistoryPage {
    status: Status,
    ids: Vec<i64>,
    pages: HashMap<usize, Vec<BookItem>>,
    page: usize,
    covers: CoverCache,
}

impl Default for HistoryPage {
    fn default() -> Self {
        HistoryPage { status: Status::Loading, ids: Vec::new(), pages: HashMap::new(), page: 0, covers: CoverCache::default() }
    }
}

fn per_page(cx: &Cx<KinNovel>) -> usize {
    let m = cx.app.metrics;
    let top = m.header_h() as i32 + m.margin as i32 / 2;
    let bottom = m.tab_bar_h() as i32 + m.pager_h() as i32;
    let area = cx.height as i32 - top - bottom;
    (area / m.row_h_cover() as i32).max(1) as usize
}

fn online(cx: &Cx<KinNovel>) -> bool {
    cx.app.net().is_some() || api::fake_mode()
}

fn logged_in(cx: &Cx<KinNovel>) -> bool {
    cx.app.signed_in()
}

impl HistoryPage {
    fn start_load(&mut self, cx: &mut Cx<KinNovel>) {
        if !online(cx) {
            self.status = Status::Offline;
            return;
        }
        if !logged_in(cx) {
            self.status = Status::NeedLogin;
            return;
        }
        self.status = Status::Loading;
        let net = cx.app.net();
        cx.spawn(move || IdsLoaded(api::load_history_ids(net.as_ref())));
    }

    fn load_page(&mut self, cx: &mut Cx<KinNovel>, page: usize) {
        if self.pages.contains_key(&page) {
            return;
        }
        let per = per_page(cx);
        let start = page * per;
        let slice: Vec<i64> = self.ids.iter().skip(start).take(per).copied().collect();
        if slice.is_empty() {
            self.pages.insert(page, Vec::new());
            return;
        }
        let net = cx.app.net();
        cx.spawn(move || BooksLoaded { page, result: api::load_books_by_ids(net.as_ref(), &slice) });
    }

    fn total_pages(&self, cx: &Cx<KinNovel>) -> usize {
        self.ids.len().div_ceil(per_page(cx)).max(1)
    }

    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        let total = self.total_pages(cx) as i64;
        let next = (self.page as i64 + delta).clamp(0, total - 1) as usize;
        if next != self.page {
            self.page = next;
            self.load_page(cx, next);
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }
}

impl Page<KinNovel> for HistoryPage {
    fn id(&self) -> PageId {
        PageId(110)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        self.start_load(cx);
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<crate::covers::CoverLoaded>() {
            Ok(loaded) => {
                self.covers.on_loaded(*loaded);
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        let msg = match msg.downcast::<IdsLoaded>() {
            Ok(ids) => {
                match ids.0 {
                    Ok(ids) => {
                        self.ids = ids;
                        self.page = 0;
                        self.status = Status::Ready;
                        self.load_page(cx, 0);
                    }
                    Err(e) => self.status = Status::Error(e),
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(books) = msg.downcast::<BooksLoaded>() {
            if let Ok(items) = books.result {
                self.pages.insert(books.page, items);
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let tabs_top = super::draw_tabs(cx, frame, Tab::History);

        // cx 整体借用 (分页计算、封面预取) 必须在创建下面的 `ink` 之前完成。
        let per = per_page(cx);
        let total = self.total_pages(cx);
        self.page = self.page.min(total.saturating_sub(1));
        if matches!(self.status, Status::Ready) && !self.ids.is_empty() {
            let row_h = m.row_h_cover();
            let list_bottom = tabs_top - m.pager_h() as i32;
            let header_bottom = m.header_h() as i32;
            let empty = Vec::new();
            let items = self.pages.get(&self.page).unwrap_or(&empty).clone();
            let cover_rect = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, row_h));
            for (row, book) in items.iter().enumerate() {
                if header_bottom + row as i32 * row_h as i32 + row_h as i32 > list_bottom {
                    break;
                }
                self.covers.request(cx, &book.cover, cover_rect.w, cover_rect.h);
            }
        }

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "历史", &super::status_text(), false, false);
        let area = Rect::new(0, bar.bottom(), cx.width, (tabs_top - bar.bottom()).max(0) as u32);
        match &self.status {
            Status::Loading => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                return;
            }
            Status::Offline => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载\n点击重试", Some(("重试", HIT_RETRY)));
                return;
            }
            Status::NeedLogin => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "查看阅读历史需要登录\n在 config.json 中设置\naccount_email / account_password", None);
                return;
            }
            Status::Error(e) => {
                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                return;
            }
            Status::Ready => {}
        }
        if self.ids.is_empty() {
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无阅读历史", None);
            return;
        }
        let row_h = m.row_h_cover();
        let list_bottom = tabs_top - m.pager_h() as i32;
        let empty = Vec::new();
        let items = self.pages.get(&self.page).unwrap_or(&empty);
        for (row, book) in items.iter().enumerate() {
            let r = Rect::new(0, bar.bottom() + row as i32 * row_h as i32, cx.width, row_h);
            if r.bottom() > list_bottom {
                break;
            }
            let cover_rect = widgets::cover_slot(&m, r);
            let img = self.covers.get(&book.cover, cover_rect.w, cover_rect.h);
            let meta = if book.last_update.is_empty() { String::new() } else { book.last_update.clone() };
            widgets::list_row_cover(&mut ink, frame, &theme, &m, r, img, &book.title, &book.author, &meta);
            cx.hits.add(HitId(ROW_BASE + row as u32), r);
        }
        if items.is_empty() {
            let loading_area = Rect::new(0, bar.bottom(), cx.width, (list_bottom - bar.bottom()).max(0) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, loading_area, "加载中…", None);
        }
        if total > 1 || per < self.ids.len() {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, total, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap && g.kind != GestureKind::SwipeUp && g.kind != GestureKind::SwipeDown && g.kind != GestureKind::SwipeLeft && g.kind != GestureKind::SwipeRight {
            return Transition::None;
        }
        if matches!(g.kind, GestureKind::SwipeUp | GestureKind::SwipeLeft) {
            return self.turn(cx, 1);
        }
        if matches!(g.kind, GestureKind::SwipeDown | GestureKind::SwipeRight) {
            return self.turn(cx, -1);
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        if let Some(t) = super::tab_transition(hit, Tab::History) {
            return t;
        }
        match hit {
            HIT_RETRY => {
                self.start_load(cx);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_PREV => self.turn(cx, -1),
            HIT_NEXT => self.turn(cx, 1),
            HitId(id) if id >= ROW_BASE => {
                let index = (id - ROW_BASE) as usize;
                match self.pages.get(&self.page).and_then(|v| v.get(index)) {
                    Some(book) => Transition::Push(Box::new(BookDetailPage::new(book.id))),
                    None => Transition::None,
                }
            }
            _ => Transition::None,
        }
    }
}
