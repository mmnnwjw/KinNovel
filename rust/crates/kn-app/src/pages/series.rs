//! 系列: 同系列的其它书籍 (服务端分页, 对照 Python `series.py` 的接口分页分支)。
//! 封面列表, 用法与 `discover.rs`/`history.rs` 一致。压栈页面, 有返回箭头, 没有标签栏。

use std::any::Any;
use std::collections::HashMap;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::book::BookDetailPage;
use crate::api::{self, BookItem, Filters};
use crate::covers::CoverCache;
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const ROW_BASE: u32 = 1000;

enum Status {
    Loading,
    Offline,
    Error(String),
    Ready,
}

struct Loaded(i64, Result<(Vec<BookItem>, i64, i64), String>);

pub struct SeriesPage {
    series_name: String,
    current_book_id: i64,
    status: Status,
    pages: HashMap<i64, Vec<BookItem>>,
    page: i64,
    total_pages: i64,
    covers: CoverCache,
}

impl SeriesPage {
    pub fn new(series_name: impl Into<String>, current_book_id: i64) -> Self {
        SeriesPage {
            series_name: series_name.into(),
            current_book_id,
            status: Status::Loading,
            pages: HashMap::new(),
            page: 1,
            total_pages: 1,
            covers: CoverCache::default(),
        }
    }

    fn online(&self, cx: &Cx<KinNovel>) -> bool {
        cx.app.net().is_some() || api::fake_mode()
    }

    fn load(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if !self.online(cx) {
            self.status = Status::Offline;
            return;
        }
        self.status = Status::Loading;
        self.page = page;
        let net = cx.app.net();
        let name = self.series_name.clone();
        let size = per_page(cx);
        let filters = Filters::from_config(&cx.app.config);
        cx.spawn(move || {
            let r = api::load_books_by_series(net.as_ref(), &name, page, size, filters);
            Loaded(page, r.map(|p| (p.items, p.page, p.total_pages)))
        });
    }
}

fn per_page(cx: &Cx<KinNovel>) -> i64 {
    let m = cx.app.metrics;
    let top = m.header_h() as i32 + m.margin as i32 / 2;
    let bottom = m.pager_h() as i32;
    let area = cx.height as i32 - top - bottom;
    (area / m.row_h_cover() as i32).max(1) as i64
}

impl Page<KinNovel> for SeriesPage {
    fn id(&self) -> PageId {
        PageId(340)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx, 1);
        }
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
        if let Ok(l) = msg.downcast::<Loaded>() {
            if l.0 == self.page {
                match l.1 {
                    Ok((items, page, total)) => {
                        self.pages.insert(page, items);
                        self.page = page;
                        self.total_pages = total;
                        self.status = Status::Ready;
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

        // 封面预取 (cx 整体借用必须在创建 `ink` 之前完成)。
        if matches!(self.status, Status::Ready) {
            let row_h = m.row_h_cover();
            let cover_rect = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, row_h));
            let empty = Vec::new();
            let items = self.pages.get(&self.page).unwrap_or(&empty).clone();
            for book in &items {
                self.covers.request(cx, &book.cover, cover_rect.w, cover_rect.h);
            }
        }

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "系列", &super::status_text(), true, false);
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
            Status::Ready => {}
        }

        let empty = Vec::new();
        let items = self.pages.get(&self.page).unwrap_or(&empty);
        if items.is_empty() {
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "本系列暂无其他书籍", None);
            return;
        }
        let row_h = m.row_h_cover();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        for (row, book) in items.iter().enumerate() {
            let r = Rect::new(0, bar.bottom() + row as i32 * row_h as i32, cx.width, row_h);
            if r.bottom() > list_bottom {
                break;
            }
            let cover_rect = widgets::cover_slot(&m, r);
            let img = self.covers.get(&book.cover, cover_rect.w, cover_rect.h);
            let current = book.id == self.current_book_id;
            let meta = if current { "当前书籍" } else { "" };
            widgets::list_row_cover(&mut ink, frame, &theme, &m, r, img, &book.title, &book.author, meta);
            cx.hits.add(HitId(ROW_BASE + row as u32), r);
        }
        if self.total_pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (self.page - 1).max(0) as usize, self.total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap && g.kind != GestureKind::SwipeUp && g.kind != GestureKind::SwipeDown && g.kind != GestureKind::SwipeLeft && g.kind != GestureKind::SwipeRight {
            return Transition::None;
        }
        if matches!(g.kind, GestureKind::SwipeUp | GestureKind::SwipeLeft) {
            if self.page < self.total_pages {
                self.load(cx, self.page + 1);
                cx.request_redraw(RefreshHint::Ui);
            }
            return Transition::None;
        }
        if matches!(g.kind, GestureKind::SwipeDown | GestureKind::SwipeRight) {
            if self.page > 1 {
                self.load(cx, self.page - 1);
                cx.request_redraw(RefreshHint::Ui);
            }
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
            HitId(id) if id >= ROW_BASE => {
                let index = (id - ROW_BASE) as usize;
                match self.pages.get(&self.page).and_then(|v| v.get(index)) {
                    Some(book) if book.id != self.current_book_id => Transition::Push(Box::new(BookDetailPage::new(book.id))),
                    _ => Transition::None,
                }
            }
            _ => Transition::None,
        }
    }
}
