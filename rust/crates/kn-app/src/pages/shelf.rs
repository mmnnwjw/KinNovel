//! 书架 (首页)。
//!
//! 顶部 "继续阅读" 卡片 (一点即回到上次的页), 下面是书目列表: 在线且登录时显示云端书架
//! (对照 Python `shelf.py`, 不支持文件夹嵌套导航 —— 见 `crate::api::load_book_shelf`),
//! 否则列出本地缓存里有章节的书, 点进去是该书已缓存的章节。

use std::any::Any;
use std::collections::BTreeMap;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::book::BookDetailPage;
use super::reader::{Entry, ReaderPage};
use super::Tab;
use crate::api::{self, BookItem};
use crate::covers::CoverCache;
use crate::store::{self, Chapter, LastRead};
use crate::KinNovel;

const HIT_CONTINUE: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_BACK: HitId = HitId(4);
/// 列表行: ROW_BASE + 序号
const ROW_BASE: u32 = 1000;

/// 一本书在缓存里的章节 (按序号)。
#[derive(Clone, Debug)]
struct CachedBook {
    book_id: i64,
    name: String,
    chapters: Vec<Chapter>,
}

struct Loaded {
    last: Option<LastRead>,
    books: Vec<CachedBook>,
}

fn load(paths: &store::Paths) -> Loaded {
    let mut by_book: BTreeMap<i64, CachedBook> = BTreeMap::new();
    for (c, _) in store::list_cached_chapters(paths) {
        let entry = by_book.entry(c.book_id).or_insert_with(|| CachedBook { book_id: c.book_id, name: c.book_name.clone(), chapters: Vec::new() });
        entry.chapters.push(c);
    }
    let last = store::load_last_read(paths);
    let mut books: Vec<CachedBook> = by_book.into_values().collect();
    // 最近读的书排第一
    if let Some(l) = &last {
        books.sort_by_key(|b| b.book_id != l.book_id);
    }
    Loaded { last, books }
}

/// 列表分页: 可用高度里放得下几行。
fn rows_fit(area_h: i32, row_h: u32) -> usize {
    (area_h / row_h as i32).max(1) as usize
}

struct CloudLoaded(Result<Vec<BookItem>, String>);

#[derive(Default)]
pub struct ShelfPage {
    data: Option<Loaded>,
    page: usize,
    /// `None` = 本地缓存模式 (离线或未登录); `Some` = 云端书架 (加载中时为空 Err 占位由 loading 标志区分)
    cloud: Option<Result<Vec<BookItem>, String>>,
    cloud_loading: bool,
    covers: CoverCache,
}

fn use_cloud(cx: &Cx<KinNovel>) -> bool {
    cx.app.signed_in()
}

/// "继续阅读" 卡片 (整张卡片可点)。
fn draw_continue(frame: &mut Bitmap, ink: &mut Ink, hits: &mut kn_ui::Hits, theme: kn_ui::Theme, m: kn_ui::widgets::Metrics, rect: Rect, last: &LastRead) {
    widgets::card(frame, &theme, &m, rect);
    let pad = m.margin as i32 / 2 + 8;
    let inner = rect.w as i32 - 2 * pad;
    let mut y = rect.y + pad;
    ink.text(frame, rect.x + pad, y, "继续阅读", m.tiny, theme.muted);
    let when = super::relative_time(last.time);
    let when_w = ink.width(&when, m.tiny).ceil() as i32;
    ink.text(frame, rect.right() - pad - when_w, y, &when, m.tiny, theme.muted);
    y += (m.tiny * 1.5) as i32;
    let name = ink.fit(&last.book_name, m.title, inner as f32);
    ink.text(frame, rect.x + pad, y, &name, m.title, theme.foreground);
    y += (m.title * 1.35) as i32;
    let chapter = ink.fit(&format!("第 {} 章 · {}", last.sort_num, last.chapter_title), m.small, inner as f32);
    ink.text(frame, rect.x + pad, y, &chapter, m.small, theme.foreground);
    y += (m.small * 1.5) as i32;
    // 章内进度条
    let track_h = (8.0 * m.scale).max(6.0) as u32;
    let track = Rect::new(rect.x + pad, y + 6, inner as u32, track_h);
    frame.rounded_rect(track, track_h / 2, Some(theme.background), Some(theme.mid), 2);
    if last.pages > 0 {
        let filled = (inner as f32 * (last.page + 1) as f32 / last.pages as f32).max(track_h as f32) as u32;
        frame.rounded_rect(Rect::new(track.x, track.y, filled.min(inner as u32), track_h), track_h / 2, Some(theme.foreground), None, 0);
    }
    hits.add(HIT_CONTINUE, rect);
}

impl Page<KinNovel> for ShelfPage {
    fn id(&self) -> PageId {
        PageId(100)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        // 从阅读页返回时也重新读: "继续阅读" 要反映刚才的进度
        let paths = cx.app.paths.clone();
        cx.spawn(move || load(&paths));
        if use_cloud(cx) {
            let net = cx.app.net();
            if net.as_ref().is_some_and(|n| n.server_down()) {
                // 服务器刚刚还不可用: 直接显示本地缓存, 到探测时间后下次进入书架再试
                self.cloud_loading = false;
                self.cloud = Some(Err("服务器暂不可用".into()));
                return;
            }
            self.cloud_loading = true;
            cx.spawn(move || CloudLoaded(api::load_book_shelf(net.as_ref())));
        } else {
            self.cloud = None;
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
        let msg = match msg.downcast::<Loaded>() {
            Ok(data) => {
                self.data = Some(*data);
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(cloud) = msg.downcast::<CloudLoaded>() {
            self.cloud_loading = false;
            self.cloud = Some(cloud.0);
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let tabs_top = super::draw_tabs(cx, frame, Tab::Shelf);
        let cloud_mode = self.showing_cloud(cx);

        // cx 整体借用 (封面预取) 必须在创建下面的 `ink` (借用 cx.fonts/cx.glyphs) 之前完成;
        // 预取用的分页算法要和下面绘制时用的完全一致, 否则请求和绘制的书对不上。
        if cloud_mode {
            if let Some(Ok(books)) = &self.cloud {
                let list_bottom = tabs_top - m.pager_h() as i32;
                let mut y = m.header_h() as i32 + m.margin as i32 / 2;
                if self.data.as_ref().is_some_and(|d| d.last.is_some()) {
                    y += (m.touch as f32 * 2.45) as i32 + m.margin as i32 / 2;
                }
                y += (m.small * 1.6) as i32;
                let row_h = m.row_h_cover();
                let per_page = rows_fit(list_bottom - y, row_h).max(1);
                let pages = books.len().div_ceil(per_page).max(1);
                let page = self.page.min(pages - 1);
                let cover_rect = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, row_h));
                for book in books.iter().skip(page * per_page).take(per_page) {
                    self.covers.request(cx, &book.cover, cover_rect.w, cover_rect.h);
                }
            }
        }

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "书架", &super::status_text(), false, false);
        let side = m.margin as i32;
        let mut y = bar.bottom() + m.margin as i32 / 2;
        let Some(data) = &self.data else {
            let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
            return;
        };
        if let Some(last) = data.last.clone() {
            let h = (m.touch as f32 * 2.45) as u32;
            let rect = Rect::new(side, y, cx.width - 2 * m.margin, h);
            draw_continue(frame, &mut ink, cx.hits, theme, m, rect, &last);
            y = rect.bottom() + m.margin as i32 / 2;
        }
        let list_bottom = tabs_top - m.pager_h() as i32;
        if cloud_mode {
            ink.text(frame, side, y, "我的书架", m.small, theme.muted);
            y += (m.small * 1.6) as i32;
            frame.fill_rect(Rect::new(side, y - 2, cx.width - 2 * m.margin, 2), theme.foreground);
            match &self.cloud {
                None if self.cloud_loading => {
                    let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                    return;
                }
                Some(Ok(books)) if books.is_empty() => {
                    let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "书架为空", None);
                    return;
                }
                Some(Ok(books)) => {
                    let row_h = m.row_h_cover();
                    let per_page = rows_fit(list_bottom - y, row_h);
                    let pages = books.len().div_ceil(per_page);
                    self.page = self.page.min(pages - 1);
                    for (row, (index, book)) in books.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
                        let r = Rect::new(0, y + row as i32 * row_h as i32, cx.width, row_h);
                        let cover_rect = widgets::cover_slot(&m, r);
                        let img = self.covers.get(&book.cover, cover_rect.w, cover_rect.h);
                        let subtitle = if book.author.is_empty() { book.last_chapter.clone() } else { book.author.clone() };
                        widgets::list_row_cover(&mut ink, frame, &theme, &m, r, img, &book.title, &subtitle, &book.last_update);
                        cx.hits.add(HitId(ROW_BASE + index as u32), r);
                    }
                    if pages > 1 {
                        let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                        widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, HIT_PREV, HIT_NEXT);
                    }
                }
                Some(Err(_)) | None => {}
            }
            return;
        }
        let label = match &self.cloud {
            Some(Err(e)) => format!("本地缓存 · 云端书架加载失败: {e}"),
            _ => "本地缓存".to_string(),
        };
        let label = ink.fit(&label, m.small, (cx.width - 2 * m.margin) as f32);
        ink.text(frame, side, y, &label, m.small, theme.muted);
        y += (m.small * 1.6) as i32;
        frame.fill_rect(Rect::new(side, y - 2, cx.width - 2 * m.margin, 2), theme.foreground);
        let data = self.data.as_ref().expect("checked");
        if data.books.is_empty() {
            let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "还没有缓存的书", None);
            return;
        }
        let row_h = m.row_h();
        let per_page = rows_fit(list_bottom - y, row_h);
        let pages = data.books.len().div_ceil(per_page);
        self.page = self.page.min(pages - 1);
        for (row, (index, book)) in data.books.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
            let r = Rect::new(0, y + row as i32 * row_h as i32, cx.width, row_h);
            let first = book.chapters.first().map_or(0, |c| c.sort_num);
            let last = book.chapters.last().map_or(0, |c| c.sort_num);
            let subtitle = if first == last { format!("已缓存第 {first} 章") } else { format!("已缓存 {} 章 · 第 {first}–{last} 章", book.chapters.len()) };
            widgets::list_row(&mut ink, frame, &theme, &m, r, &book.name, &subtitle, "");
            cx.hits.add(HitId(ROW_BASE + index as u32), r);
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match g.kind {
            GestureKind::Tap => {
                let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
                if let Some(t) = super::tab_transition(hit, Tab::Shelf) {
                    return t;
                }
                let last = self.data.as_ref().and_then(|d| d.last.clone());
                let cloud_mode = self.showing_cloud(cx);
                match hit {
                    HIT_CONTINUE => match last {
                        Some(l) => Transition::Push(Box::new(ReaderPage::new(l.book_id, l.sort_num, Entry::Resume))),
                        None => Transition::None,
                    },
                    HIT_PREV => self.turn(cx, -1),
                    HIT_NEXT => self.turn(cx, 1),
                    HitId(id) if id >= ROW_BASE && cloud_mode => match self.cloud.as_ref().and_then(|r| r.as_ref().ok()).and_then(|v| v.get((id - ROW_BASE) as usize)) {
                        Some(b) => Transition::Push(Box::new(BookDetailPage::new(b.id))),
                        None => Transition::None,
                    },
                    HitId(id) if id >= ROW_BASE => match self.data.as_ref().and_then(|d| d.books.get((id - ROW_BASE) as usize)) {
                        Some(b) => Transition::Push(Box::new(CachedBookPage::new(b.clone()))),
                        None => Transition::None,
                    },
                    _ => Transition::None,
                }
            }
            GestureKind::SwipeUp | GestureKind::SwipeLeft => self.turn(cx, 1),
            GestureKind::SwipeDown | GestureKind::SwipeRight => self.turn(cx, -1),
            _ => Transition::None,
        }
    }
}

impl ShelfPage {
    /// 云端书架加载失败 (断网、服务器故障) 时退回本地缓存, 已缓存的书照样能读。
    fn showing_cloud(&self, cx: &Cx<KinNovel>) -> bool {
        use_cloud(cx) && !matches!(self.cloud, Some(Err(_)))
    }

    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        let has_rows = if self.showing_cloud(cx) {
            self.cloud.as_ref().and_then(|r| r.as_ref().ok()).is_some_and(|v| !v.is_empty())
        } else {
            self.data.as_ref().is_some_and(|d| !d.books.is_empty())
        };
        let next = self.page as i64 + delta;
        if next >= 0 && has_rows {
            // 上限在 render 里按可见行数钳制
            self.page = next as usize;
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }
}

/// 一本书已缓存的章节。
pub struct CachedBookPage {
    book: CachedBook,
    page: usize,
}

impl CachedBookPage {
    fn new(book: CachedBook) -> Self {
        CachedBookPage { book, page: 0 }
    }
}

impl Page<KinNovel> for CachedBookPage {
    fn id(&self) -> PageId {
        PageId(101)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, &self.book.name, &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let row_h = m.row_h();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        let per_page = rows_fit(list_bottom - bar.bottom(), row_h);
        let pages = self.book.chapters.len().div_ceil(per_page).max(1);
        self.page = self.page.min(pages - 1);
        let convert = cx.app.config.convert();
        for (row, (index, c)) in self.book.chapters.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
            let r = Rect::new(0, bar.bottom() + row as i32 * row_h as i32, cx.width, row_h);
            let meta = store::load_progress(&cx.app.paths.progress_file(c.book_id, c.sort_num, &convert))
                .map(|p| format!("读到第 {} 页", p.page + 1))
                .unwrap_or_default();
            widgets::list_row(&mut ink, frame, &theme, &m, r, &format!("第 {} 章 · {}", c.sort_num, c.title), "", &meta);
            cx.hits.add(HitId(ROW_BASE + index as u32), r);
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match g.kind {
            GestureKind::Tap => match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
                Some(HIT_BACK) => Transition::Back,
                Some(HIT_PREV) => self.turn(cx, -1),
                Some(HIT_NEXT) => self.turn(cx, 1),
                Some(HitId(id)) if id >= ROW_BASE => match self.book.chapters.get((id - ROW_BASE) as usize) {
                    Some(c) => Transition::Push(Box::new(ReaderPage::new(c.book_id, c.sort_num, Entry::Resume))),
                    None => Transition::None,
                },
                _ => Transition::None,
            },
            GestureKind::SwipeUp | GestureKind::SwipeLeft => self.turn(cx, 1),
            GestureKind::SwipeDown | GestureKind::SwipeRight => self.turn(cx, -1),
            _ => Transition::None,
        }
    }
}

impl CachedBookPage {
    fn turn(&mut self, cx: &mut Cx<KinNovel>, d: i64) -> Transition<KinNovel> {
        let n = self.page as i64 + d;
        if n >= 0 {
            self.page = n as usize;
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }
}
