//! 历史: 阅读历史 (云端, 对照 Python `history.py` 与网页版 `History.vue`)。
//!
//! 顶部分段切换小说 / 漫画。只取当前页的书籍元数据, 翻页时按需补。漫画历史是分卷 id,
//! 服务器按系列聚合返回 (与网页版一致), 同一系列跨页重复出现时只保留第一次。

use std::any::Any;
use std::collections::{HashMap, HashSet};

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::book::BookDetailPage;
use super::Tab;
use crate::api::{self, BookItem, HistoryIds};
use crate::covers::CoverCache;
use crate::KinNovel;

const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const SEG_KIND_BASE: u32 = 10;
const ROW_BASE: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Novel,
    Comic,
}

const KINDS: [(Kind, &str); 2] = [(Kind::Novel, "小说"), (Kind::Comic, "漫画")];

enum Status {
    Loading,
    /// 离线且不是 fixture 模式
    Offline,
    /// 在线但未登录
    NeedLogin,
    Error(String),
    Ready,
}

struct IdsLoaded(Result<HistoryIds, String>);
struct BooksLoaded {
    kind: Kind,
    page: usize,
    result: Result<Vec<BookItem>, String>,
}

pub struct HistoryPage {
    status: Status,
    kind: Kind,
    ids: HistoryIds,
    pages: HashMap<(Kind, usize), Vec<BookItem>>,
    /// 漫画: 已经列出过的系列 (按标题去重)
    seen_series: HashSet<String>,
    page: usize,
    covers: CoverCache,
}

impl Default for HistoryPage {
    fn default() -> Self {
        HistoryPage {
            status: Status::Loading,
            kind: Kind::Novel,
            ids: HistoryIds::default(),
            pages: HashMap::new(),
            seen_series: HashSet::new(),
            page: 0,
            covers: CoverCache::default(),
        }
    }
}

/// 分段控件所在的一行 (表头下方)。
fn seg_rect(cx: &Cx<KinNovel>) -> Rect {
    let m = cx.app.metrics;
    Rect::new(m.margin as i32, m.header_h() as i32 + m.margin as i32 / 3, cx.width - 2 * m.margin, m.touch)
}

fn list_top(cx: &Cx<KinNovel>) -> i32 {
    seg_rect(cx).bottom() + cx.app.metrics.margin as i32 / 3
}

fn per_page(cx: &Cx<KinNovel>) -> usize {
    let m = cx.app.metrics;
    let bottom = m.tab_bar_h() as i32 + m.pager_h() as i32;
    let area = cx.height as i32 - list_top(cx) - bottom;
    (area / m.row_h_cover() as i32).max(1) as usize
}

fn online(cx: &Cx<KinNovel>) -> bool {
    cx.app.net().is_some() || api::fake_mode()
}

fn logged_in(cx: &Cx<KinNovel>) -> bool {
    cx.app.signed_in()
}

impl HistoryPage {
    fn ids(&self) -> &[i64] {
        match self.kind {
            Kind::Novel => &self.ids.novel,
            Kind::Comic => &self.ids.comic,
        }
    }

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
        let kind = self.kind;
        if self.pages.contains_key(&(kind, page)) {
            return;
        }
        let per = per_page(cx);
        let slice: Vec<i64> = self.ids().iter().skip(page * per).take(per).copied().collect();
        if slice.is_empty() {
            self.pages.insert((kind, page), Vec::new());
            return;
        }
        let net = cx.app.net();
        cx.spawn(move || BooksLoaded {
            kind,
            page,
            result: match kind {
                Kind::Novel => api::load_books_by_ids(net.as_ref(), &slice),
                Kind::Comic => api::load_comic_series_by_ids(net.as_ref(), &slice),
            },
        });
    }

    fn total_pages(&self, cx: &Cx<KinNovel>) -> usize {
        self.ids().len().div_ceil(per_page(cx)).max(1)
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

    fn switch(&mut self, cx: &mut Cx<KinNovel>, kind: Kind) {
        if kind == self.kind {
            return;
        }
        self.kind = kind;
        self.page = 0;
        if matches!(self.status, Status::Ready) {
            self.load_page(cx, 0);
        }
        cx.request_redraw(RefreshHint::Ui);
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
                        self.pages.clear();
                        self.seen_series.clear();
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
            if let Ok(mut items) = books.result {
                if books.kind == Kind::Comic {
                    items.retain(|b| self.seen_series.insert(b.title.clone()));
                }
                self.pages.insert((books.kind, books.page), items);
            }
            if books.kind == self.kind {
                cx.request_redraw(RefreshHint::Ui);
            }
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
        let seg = seg_rect(cx);
        let top = list_top(cx);
        let row_h = m.row_h_cover();
        let list_bottom = tabs_top - m.pager_h() as i32;
        let items = self.pages.get(&(self.kind, self.page)).cloned().unwrap_or_default();
        if matches!(self.status, Status::Ready) {
            let cover_rect = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, row_h));
            for (row, book) in items.iter().enumerate() {
                if top + (row as i32 + 1) * row_h as i32 > list_bottom {
                    break;
                }
                self.covers.request(cx, &book.cover, cover_rect.w, cover_rect.h);
            }
        }

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        widgets::header(&mut ink, frame, &theme, &m, "历史", &super::status_text(), false, false);
        let labels: Vec<&str> = KINDS.iter().map(|(_, l)| *l).collect();
        let active = KINDS.iter().position(|(k, _)| *k == self.kind).unwrap_or(0);
        widgets::segmented(&mut ink, frame, cx.hits, &theme, &m, seg, &labels, active, SEG_KIND_BASE);
        let area = Rect::new(0, top, cx.width, (tabs_top - top).max(0) as u32);
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
        if self.ids().is_empty() {
            let text = if self.kind == Kind::Comic { "暂无漫画阅读历史" } else { "暂无阅读历史" };
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, text, None);
            return;
        }
        for (row, book) in items.iter().enumerate() {
            let r = Rect::new(0, top + row as i32 * row_h as i32, cx.width, row_h);
            if r.bottom() > list_bottom {
                break;
            }
            let cover_rect = widgets::cover_slot(&m, r);
            let img = self.covers.get(&book.cover, cover_rect.w, cover_rect.h);
            widgets::list_row_cover(&mut ink, frame, &theme, &m, r, img, &book.title, &book.subtitle(), &book.last_update);
            cx.hits.add(HitId(ROW_BASE + row as u32), r);
        }
        if !self.pages.contains_key(&(self.kind, self.page)) {
            let loading_area = Rect::new(0, top, cx.width, (list_bottom - top).max(0) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, loading_area, "加载中…", None);
        }
        if total > 1 || per < self.ids().len() {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, total, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match g.kind {
            GestureKind::SwipeUp | GestureKind::SwipeLeft => return self.turn(cx, 1),
            GestureKind::SwipeDown | GestureKind::SwipeRight => return self.turn(cx, -1),
            GestureKind::Tap => {}
            _ => return Transition::None,
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        if let Some(t) = super::tab_transition(hit, Tab::History) {
            return t;
        }
        match hit {
            HitId(id) if (SEG_KIND_BASE..SEG_KIND_BASE + KINDS.len() as u32).contains(&id) => {
                self.switch(cx, KINDS[(id - SEG_KIND_BASE) as usize].0);
                Transition::None
            }
            HIT_RETRY => {
                self.start_load(cx);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_PREV => self.turn(cx, -1),
            HIT_NEXT => self.turn(cx, 1),
            HitId(id) if id >= ROW_BASE => {
                let index = (id - ROW_BASE) as usize;
                match self.pages.get(&(self.kind, self.page)).and_then(|v| v.get(index)) {
                    Some(book) => Transition::Push(Box::new(BookDetailPage::new(book.id))),
                    None => Transition::None,
                }
            }
            _ => Transition::None,
        }
    }
}
