//! 发现: 最新 / 排行 / 分类 (对照 Python `browse.py` / `rank.py`) / 漫画 (对照网页版 `Manga/Discover.vue`)。
//! 顶栏右端的放大镜进入搜索 (`search.rs`)。
//!
//! 四个子页在同一个标签页内切换 (顶部分段控件), 不压栈。排行与漫画各有第二行分段 (榜单 / 排序)。分类分两步: 先列分类, 点一个分类后
//! 在同一屏显示该分类的书 (有一个"返回分类"按钮清掉选中, 不是真正的页面栈)。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::book::BookDetailPage;
use super::search::SearchPage;
use super::Tab;
use crate::api::{self, BookItem, Category, ListPage};
use crate::covers::CoverCache;
use crate::KinNovel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sub {
    Latest,
    Rank,
    Category,
    Comic,
}

const SUB_OPTIONS: [(Sub, &str); 4] = [(Sub::Latest, "最新"), (Sub::Rank, "排行"), (Sub::Category, "分类"), (Sub::Comic, "漫画")];
const RANK_OPTIONS: [(i64, &str); 3] = [(1, "日榜"), (7, "周榜"), (31, "月榜")];
/// 漫画排序 (`GetComicList` 的 `Order`)
const COMIC_ORDERS: [(&str, &str); 3] = [("latest", "最近更新"), ("new", "上架时间"), ("view", "总点击量")];

enum Fetch<T> {
    Loading,
    Offline,
    Error(String),
    Ready(T),
}

impl<T> Fetch<T> {
    fn ready(&self) -> Option<&T> {
        match self {
            Fetch::Ready(v) => Some(v),
            _ => None,
        }
    }
}

const SEG_SUB_BASE: u32 = 10;
const SEG_RANK_BASE: u32 = 20;
const SEG_ORDER_BASE: u32 = 30;
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_BACK_CATEGORY: HitId = HitId(5);
const HIT_SEARCH: HitId = HitId(6);
const ROW_BASE: u32 = 1000;

struct LatestLoaded(i64, Result<ListPage, String>);
struct RankLoaded(i64, Result<Vec<BookItem>, String>);
struct CategoriesLoaded(Result<Vec<Category>, String>);
struct CategoryListLoaded(i64, i64, Result<ListPage, String>);
struct ComicLoaded(&'static str, i64, Result<ListPage, String>);

pub struct DiscoverPage {
    sub: Sub,
    latest: Fetch<ListPage>,
    latest_page: i64,
    rank_kind: i64,
    rank: Fetch<Vec<BookItem>>,
    rank_page: usize,
    categories: Fetch<Vec<Category>>,
    selected: Option<Category>,
    category_list: Fetch<ListPage>,
    category_page: i64,
    comic_order: &'static str,
    comic: Fetch<ListPage>,
    comic_page: i64,
    covers: CoverCache,
}

impl Default for DiscoverPage {
    fn default() -> Self {
        DiscoverPage {
            sub: Sub::Latest,
            latest: Fetch::Loading,
            latest_page: 1,
            rank_kind: 1,
            rank: Fetch::Loading,
            rank_page: 0,
            categories: Fetch::Loading,
            selected: None,
            category_list: Fetch::Loading,
            category_page: 1,
            comic_order: "latest",
            comic: Fetch::Loading,
            comic_page: 1,
            covers: CoverCache::default(),
        }
    }
}

fn online(cx: &Cx<KinNovel>) -> bool {
    cx.app.net().is_some() || api::fake_mode()
}

fn per_page(cx: &Cx<KinNovel>) -> usize {
    let m = cx.app.metrics;
    let top = m.header_h() as i32 + m.margin as i32;
    let bottom = m.tab_bar_h() as i32 + m.pager_h() as i32;
    let area = cx.height as i32 - top - bottom;
    (area / m.row_h_cover() as i32).max(1) as usize
}

impl DiscoverPage {
    fn load_latest(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if !online(cx) {
            self.latest = Fetch::Offline;
            return;
        }
        self.latest = Fetch::Loading;
        self.latest_page = page;
        let net = cx.app.net();
        let size = per_page(cx) as i64;
        let filters = api::Filters::from_config(&cx.app.config);
        cx.spawn(move || LatestLoaded(page, api::load_book_list(net.as_ref(), page, size.max(1), None, filters)));
    }

    fn load_rank(&mut self, cx: &mut Cx<KinNovel>, days: i64) {
        if !online(cx) {
            self.rank = Fetch::Offline;
            return;
        }
        self.rank = Fetch::Loading;
        self.rank_kind = days;
        self.rank_page = 0;
        let net = cx.app.net();
        cx.spawn(move || RankLoaded(days, api::load_rank(net.as_ref(), days)));
    }

    fn load_categories(&mut self, cx: &mut Cx<KinNovel>) {
        if !online(cx) {
            self.categories = Fetch::Offline;
            return;
        }
        self.categories = Fetch::Loading;
        let net = cx.app.net();
        cx.spawn(move || CategoriesLoaded(api::load_categories(net.as_ref())));
    }

    fn load_category(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        let Some(cat) = self.selected.clone() else { return };
        if !online(cx) {
            self.category_list = Fetch::Offline;
            return;
        }
        self.category_list = Fetch::Loading;
        self.category_page = page;
        let net = cx.app.net();
        let size = per_page(cx) as i64;
        let id = cat.id;
        let filters = api::Filters::from_config(&cx.app.config);
        cx.spawn(move || CategoryListLoaded(id, page, api::load_book_list(net.as_ref(), page, size.max(1), Some(id), filters)));
    }

    fn load_comic(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if !online(cx) {
            self.comic = Fetch::Offline;
            return;
        }
        self.comic = Fetch::Loading;
        self.comic_page = page;
        let net = cx.app.net();
        let size = per_page(cx) as i64;
        let order = self.comic_order;
        cx.spawn(move || ComicLoaded(order, page, api::load_comic_list(net.as_ref(), page, size.max(1), order)));
    }

    fn switch(&mut self, cx: &mut Cx<KinNovel>, sub: Sub) {
        if self.sub == sub {
            return;
        }
        self.sub = sub;
        match sub {
            Sub::Latest => {
                if matches!(self.latest, Fetch::Loading) || !matches!(self.latest, Fetch::Ready(_)) {
                    self.load_latest(cx, 1);
                }
            }
            Sub::Rank => {
                if !matches!(self.rank, Fetch::Ready(_)) {
                    self.load_rank(cx, self.rank_kind);
                }
            }
            Sub::Category => {
                if !matches!(self.categories, Fetch::Ready(_)) {
                    self.load_categories(cx);
                }
            }
            Sub::Comic => {
                if !matches!(self.comic, Fetch::Ready(_)) {
                    self.load_comic(cx, self.comic_page.max(1));
                }
            }
        }
        cx.request_redraw(RefreshHint::Ui);
    }

    fn navigate_to(book_id: i64) -> Transition<KinNovel> {
        Transition::Push(Box::new(BookDetailPage::new(book_id)))
    }

    /// 只读 `self.covers` (不借用 `cx`), 可以在 `Ink` 借着 `cx.fonts`/`cx.glyphs` 时调用;
    /// 封面必须在创建 `Ink` 之前用 [`Self::request_covers`] 请求过。
    fn draw_books(&self, ink: &mut Ink, frame: &mut Bitmap, theme: &kn_ui::Theme, m: &widgets::Metrics, hits: &mut kn_ui::Hits, width: u32, area: Rect, items: &[BookItem]) {
        let row_h = m.row_h_cover();
        for (row, book) in items.iter().enumerate() {
            let r = Rect::new(0, area.y + row as i32 * row_h as i32, width, row_h);
            if r.bottom() > area.bottom() {
                break;
            }
            let cover_rect = widgets::cover_slot(m, r);
            let img = self.covers.get(&book.cover, cover_rect.w, cover_rect.h);
            widgets::list_row_cover(ink, frame, theme, m, r, img, &book.title, &book.subtitle(), &book.last_update);
            hits.add(HitId(ROW_BASE + row as u32), r);
        }
    }

    /// 为即将绘制的书目预取封面 (必须在创建 `Ink` 之前调用, 见模块顶部说明)。
    fn request_covers(&mut self, cx: &Cx<KinNovel>, items: &[BookItem]) {
        let m = cx.app.metrics;
        let row_h = m.row_h_cover();
        let dummy = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, row_h));
        for book in items {
            self.covers.request(cx, &book.cover, dummy.w, dummy.h);
        }
    }
}

impl Page<KinNovel> for DiscoverPage {
    fn id(&self) -> PageId {
        PageId(120)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        if matches!(self.latest, Fetch::Loading) {
            self.load_latest(cx, self.latest_page.max(1));
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
        let msg = match msg.downcast::<LatestLoaded>() {
            Ok(l) => {
                if l.0 == self.latest_page {
                    self.latest = match l.1 {
                        Ok(p) => Fetch::Ready(p),
                        Err(e) => Fetch::Error(e),
                    };
                    cx.request_redraw(RefreshHint::Ui);
                }
                return;
            }
            Err(other) => other,
        };
        let msg = match msg.downcast::<RankLoaded>() {
            Ok(l) => {
                if l.0 == self.rank_kind {
                    self.rank = match l.1 {
                        Ok(v) => Fetch::Ready(v),
                        Err(e) => Fetch::Error(e),
                    };
                    cx.request_redraw(RefreshHint::Ui);
                }
                return;
            }
            Err(other) => other,
        };
        let msg = match msg.downcast::<CategoriesLoaded>() {
            Ok(l) => {
                self.categories = match l.0 {
                    Ok(v) => Fetch::Ready(v),
                    Err(e) => Fetch::Error(e),
                };
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        let msg = match msg.downcast::<ComicLoaded>() {
            Ok(l) => {
                if l.0 == self.comic_order && l.1 == self.comic_page {
                    self.comic = match l.2 {
                        Ok(p) => Fetch::Ready(p),
                        Err(e) => Fetch::Error(e),
                    };
                    cx.request_redraw(RefreshHint::Ui);
                }
                return;
            }
            Err(other) => other,
        };
        if let Ok(l) = msg.downcast::<CategoryListLoaded>() {
            let matches_selection = self.selected.as_ref().is_some_and(|c| c.id == l.0) && l.1 == self.category_page;
            if matches_selection {
                self.category_list = match l.2 {
                    Ok(p) => Fetch::Ready(p),
                    Err(e) => Fetch::Error(e),
                };
                cx.request_redraw(RefreshHint::Ui);
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let tabs_top = super::draw_tabs(cx, frame, Tab::Discover);
        let side = m.margin as i32;
        let seg_h = m.touch as u32;

        // 纯几何 (不需要 `Ink`), 和下面实际绘制时用的完全一致: 下面不会重新推导, 直接复用这里
        // 算好的矩形, 避免预取 (封面请求必须在创建 `Ink` 之前) 和实际绘制的分页对不上。
        let header_bottom = m.header_h() as i32;
        let seg_rect = Rect::new(side, header_bottom + m.margin as i32 / 3, cx.width - 2 * m.margin, seg_h);
        let mut y = seg_rect.bottom() + m.margin as i32 / 3;
        let rank_rect = matches!(self.sub, Sub::Rank | Sub::Comic).then(|| {
            let r = Rect::new(side, y, cx.width - 2 * m.margin, seg_h);
            y = r.bottom() + m.margin as i32 / 3;
            r
        });
        let list_bottom = tabs_top - m.pager_h() as i32;
        let area = Rect::new(0, y, cx.width, (list_bottom - y).max(0) as u32);
        let back_rect = Rect::new(side, area.y, cx.width - 2 * m.margin, m.touch);
        let books_area = Rect::new(0, back_rect.bottom() + m.margin as i32 / 3, cx.width, (list_bottom - back_rect.bottom()).max(0) as u32);

        // 封面预取 (cx 整体借用, 必须在创建 `ink` 之前)。
        let per = per_page(cx);
        match self.sub {
            Sub::Latest => {
                if let Fetch::Ready(list) = &self.latest {
                    let items = list.items.clone();
                    self.request_covers(cx, &items);
                }
            }
            Sub::Rank => {
                if let Fetch::Ready(all) = &self.rank {
                    let pages = all.len().div_ceil(per).max(1);
                    self.rank_page = self.rank_page.min(pages - 1);
                    let slice: Vec<BookItem> = all.iter().skip(self.rank_page * per).take(per).cloned().collect();
                    self.request_covers(cx, &slice);
                }
            }
            Sub::Category => {
                if self.selected.is_some() {
                    if let Fetch::Ready(list) = &self.category_list {
                        let items = list.items.clone();
                        self.request_covers(cx, &items);
                    }
                }
            }
            Sub::Comic => {
                if let Fetch::Ready(list) = &self.comic {
                    let items = list.items.clone();
                    self.request_covers(cx, &items);
                }
            }
        }

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header_with_icon(&mut ink, frame, &theme, &m, "发现", &super::status_text(), false, Some(widgets::HeaderIcon::Search));
        cx.hits.add(HIT_SEARCH, Rect::new(bar.right() - bar.h as i32, 0, bar.h, bar.h));
        let labels: Vec<&str> = SUB_OPTIONS.iter().map(|(_, l)| *l).collect();
        let active = SUB_OPTIONS.iter().position(|(s, _)| *s == self.sub).unwrap_or(0);
        widgets::segmented(&mut ink, frame, cx.hits, &theme, &m, seg_rect, &labels, active, SEG_SUB_BASE);
        if let Some(rank_rect) = rank_rect {
            if self.sub == Sub::Comic {
                let labels: Vec<&str> = COMIC_ORDERS.iter().map(|(_, l)| *l).collect();
                let active = COMIC_ORDERS.iter().position(|(o, _)| *o == self.comic_order).unwrap_or(0);
                widgets::segmented(&mut ink, frame, cx.hits, &theme, &m, rank_rect, &labels, active, SEG_ORDER_BASE);
            } else {
                let rank_labels: Vec<&str> = RANK_OPTIONS.iter().map(|(_, l)| *l).collect();
                let rank_active = RANK_OPTIONS.iter().position(|(d, _)| *d == self.rank_kind).unwrap_or(0);
                widgets::segmented(&mut ink, frame, cx.hits, &theme, &m, rank_rect, &rank_labels, rank_active, SEG_RANK_BASE);
            }
        }

        let width = cx.width;
        match self.sub {
            Sub::Comic => match &self.comic {
                Fetch::Loading => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None),
                Fetch::Offline => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY))),
                Fetch::Error(e) => {
                    let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                }
                Fetch::Ready(list) => {
                    let items = list.items.clone();
                    let (page, total_pages) = (list.page, list.total_pages);
                    if items.is_empty() {
                        widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无漫画", None);
                    } else {
                        self.draw_books(&mut ink, frame, &theme, &m, cx.hits, width, area, &items);
                    }
                    let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                    widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (page - 1).max(0) as usize, total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
                }
            },
            Sub::Latest => match &self.latest {
                Fetch::Loading => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None),
                Fetch::Offline => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY))),
                Fetch::Error(e) => {
                    let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                }
                Fetch::Ready(list) => {
                    let items = list.items.clone();
                    let (page, total_pages) = (list.page, list.total_pages);
                    if items.is_empty() {
                        widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无内容", None);
                    } else {
                        self.draw_books(&mut ink, frame, &theme, &m, cx.hits, width, area, &items);
                    }
                    let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                    widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (page - 1).max(0) as usize, total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
                }
            },
            Sub::Rank => match &self.rank {
                Fetch::Loading => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None),
                Fetch::Offline => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY))),
                Fetch::Error(e) => {
                    let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                }
                Fetch::Ready(all) => {
                    if all.is_empty() {
                        widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无内容", None);
                    } else {
                        let pages = all.len().div_ceil(per).max(1);
                        let start = self.rank_page * per;
                        let slice: Vec<BookItem> = all.iter().skip(start).take(per).cloned().collect();
                        self.draw_books(&mut ink, frame, &theme, &m, cx.hits, width, area, &slice);
                        let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                        widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.rank_page, pages, HIT_PREV, HIT_NEXT);
                    }
                }
            },
            Sub::Category => {
                if let Some(cat) = self.selected.clone() {
                    widgets::button(&mut ink, frame, &theme, &m, back_rect, &format!("← 分类 · {}", cat.name), kn_ui::widgets::ButtonStyle::Secondary);
                    cx.hits.add(HIT_BACK_CATEGORY, back_rect);
                    match &self.category_list {
                        Fetch::Loading => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, books_area, "加载中…", None),
                        Fetch::Offline => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, books_area, "离线，无法加载", Some(("重试", HIT_RETRY))),
                        Fetch::Error(e) => {
                            let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, books_area, &text, Some(("重试", HIT_RETRY)));
                        }
                        Fetch::Ready(list) => {
                            let items = list.items.clone();
                            let (page, total_pages) = (list.page, list.total_pages);
                            if items.is_empty() {
                                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, books_area, "暂无内容", None);
                            } else {
                                self.draw_books(&mut ink, frame, &theme, &m, cx.hits, width, books_area, &items);
                            }
                            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (page - 1).max(0) as usize, total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
                        }
                    }
                } else {
                    match self.categories.ready().cloned() {
                        None => match &self.categories {
                            Fetch::Offline => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY))),
                            Fetch::Error(e) => {
                                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                            }
                            _ => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None),
                        },
                        Some(cats) => {
                            if cats.is_empty() {
                                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无分类", None);
                            } else {
                                let row_h = m.row_h();
                                for (row, cat) in cats.iter().enumerate() {
                                    let r = Rect::new(0, area.y + row as i32 * row_h as i32, cx.width, row_h);
                                    if r.bottom() > area.bottom() {
                                        break;
                                    }
                                    widgets::list_row(&mut ink, frame, &theme, &m, r, &cat.name, "", "");
                                    cx.hits.add(HitId(ROW_BASE + row as u32), r);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        if let Some(t) = super::tab_transition(hit, Tab::Discover) {
            return t;
        }
        {
            let HitId(id) = hit;
            if (SEG_SUB_BASE..SEG_SUB_BASE + SUB_OPTIONS.len() as u32).contains(&id) {
                let (sub, _) = SUB_OPTIONS[(id - SEG_SUB_BASE) as usize];
                self.switch(cx, sub);
                return Transition::None;
            }
            if (SEG_ORDER_BASE..SEG_ORDER_BASE + COMIC_ORDERS.len() as u32).contains(&id) {
                let (order, _) = COMIC_ORDERS[(id - SEG_ORDER_BASE) as usize];
                if order != self.comic_order {
                    self.comic_order = order;
                    self.load_comic(cx, 1);
                    cx.request_redraw(RefreshHint::Ui);
                }
                return Transition::None;
            }
            if (SEG_RANK_BASE..SEG_RANK_BASE + 3).contains(&id) {
                let (days, _) = RANK_OPTIONS[(id - SEG_RANK_BASE) as usize];
                if days != self.rank_kind {
                    self.load_rank(cx, days);
                    cx.request_redraw(RefreshHint::Ui);
                }
                return Transition::None;
            }
        }
        match hit {
            HIT_SEARCH => Transition::Push(Box::new(SearchPage::default())),
            HIT_RETRY => {
                match self.sub {
                    Sub::Latest => self.load_latest(cx, self.latest_page),
                    Sub::Rank => self.load_rank(cx, self.rank_kind),
                    Sub::Category => {
                        if self.selected.is_some() {
                            self.load_category(cx, self.category_page);
                        } else {
                            self.load_categories(cx);
                        }
                    }
                    Sub::Comic => self.load_comic(cx, self.comic_page),
                }
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_BACK_CATEGORY => {
                self.selected = None;
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_PREV | HIT_NEXT => {
                let delta = if hit == HIT_NEXT { 1 } else { -1 };
                match self.sub {
                    Sub::Latest => {
                        let page = (self.latest_page + delta).max(1);
                        self.load_latest(cx, page);
                    }
                    Sub::Rank => {
                        if let Fetch::Ready(all) = &self.rank {
                            let per = per_page(cx);
                            let pages = all.len().div_ceil(per).max(1) as i64;
                            self.rank_page = (self.rank_page as i64 + delta).clamp(0, pages - 1) as usize;
                        }
                    }
                    Sub::Category => {
                        let page = (self.category_page + delta).max(1);
                        self.load_category(cx, page);
                    }
                    Sub::Comic => {
                        let total = self.comic.ready().map_or(i64::MAX, |l| l.total_pages);
                        let page = (self.comic_page + delta).clamp(1, total.max(1));
                        if page != self.comic_page {
                            self.load_comic(cx, page);
                        }
                    }
                }
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HitId(id) if id >= ROW_BASE => {
                let index = (id - ROW_BASE) as usize;
                match self.sub {
                    Sub::Comic => match self.comic.ready().and_then(|l| l.items.get(index)) {
                        Some(b) => Self::navigate_to(b.id),
                        None => Transition::None,
                    },
                    Sub::Latest => match self.latest.ready().and_then(|l| l.items.get(index)) {
                        Some(b) => Self::navigate_to(b.id),
                        None => Transition::None,
                    },
                    Sub::Rank => match &self.rank {
                        Fetch::Ready(all) => {
                            let per = per_page(cx);
                            match all.get(self.rank_page * per + index) {
                                Some(b) => Self::navigate_to(b.id),
                                None => Transition::None,
                            }
                        }
                        _ => Transition::None,
                    },
                    Sub::Category => {
                        if self.selected.is_some() {
                            match self.category_list.ready().and_then(|l| l.items.get(index)) {
                                Some(b) => Self::navigate_to(b.id),
                                None => Transition::None,
                            }
                        } else if let Some(cats) = self.categories.ready() {
                            if let Some(cat) = cats.get(index).cloned() {
                                self.selected = Some(cat);
                                self.category_list = Fetch::Loading;
                                self.load_category(cx, 1);
                                cx.request_redraw(RefreshHint::Ui);
                            }
                            Transition::None
                        } else {
                            Transition::None
                        }
                    }
                }
            }
            _ => Transition::None,
        }
    }
}
