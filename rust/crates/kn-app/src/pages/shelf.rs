//! 书架 (首页)。
//!
//! 顶部 "继续阅读" 卡片 (一点即回到上次的页), 下面是书目列表: 在线且登录时显示云端书架
//! (对照 Python `shelf.py`): 文件夹可以点进去 (顶栏返回箭头回上一层)。长按书或文件夹弹出操作:
//! 移动到别的文件夹、移出书架 / 重命名、删除文件夹 (里面的书移到上一层); 删除类操作先确认。
//! 列表上方的「新建文件夹」在当前这一层新建。写回见 `crate::api::shelf_set_book` 等,
//! 命名与移动在 `super::shelf_edit`。
//! 否则列出本地缓存里有章节的书, 点进去是该书已缓存的章节。

use std::any::Any;
use std::collections::BTreeMap;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::book::BookDetailPage;
use super::comic::ComicReaderPage;
use super::reader::{Entry, ReaderPage};
use super::shelf_edit::{FolderNamePage, MovePickerPage};
use super::Tab;
use crate::api::{self, ShelfData, ShelfEntry, ShelfRef};
use crate::covers::CoverCache;
use crate::store::{self, Chapter, LastRead};
use crate::KinNovel;

const HIT_CONTINUE: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_BACK: HitId = HitId(4);
const HIT_FOLDER_UP: HitId = HitId(5);
const HIT_CONFIRM: HitId = HitId(6);
const HIT_CANCEL: HitId = HitId(7);
const HIT_NEW_FOLDER: HitId = HitId(8);
const HIT_ACT_MOVE: HitId = HitId(9);
const HIT_ACT_REMOVE: HitId = HitId(10);
const HIT_ACT_RENAME: HitId = HitId(11);
const HIT_ACT_CLOSE: HitId = HitId(12);
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
    /// 继续阅读卡片的封面: 记录里的封面地址, 没有就用第一章的卷首图
    last_cover: String,
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
    let last_cover = match &last {
        Some(l) if !l.cover.is_empty() => l.cover.clone(),
        Some(l) => crate::sleep::known_cover(l.book_id).or_else(|| store::first_image_of_book(paths, l.book_id)).unwrap_or_default(),
        None => String::new(),
    };
    Loaded { last, last_cover, books }
}

/// 继续阅读卡片的高度与其中封面的位置 (卡片内边距之内, 宽高比 0.72)。
fn continue_layout(m: &kn_ui::widgets::Metrics, rect: Rect) -> Rect {
    let pad = m.margin as i32 / 2 + 8;
    let h = (rect.h as i32 - 2 * pad).max(1);
    Rect::new(rect.x + pad, rect.y + pad, ((h as f32) * 0.72) as u32, h as u32)
}

fn continue_h(m: &kn_ui::widgets::Metrics) -> u32 {
    (m.touch as f32 * 2.45) as u32
}

/// 列表分页: 可用高度里放得下几行。
fn rows_fit(area_h: i32, row_h: u32) -> usize {
    (area_h / row_h as i32).max(1) as usize
}

struct CloudLoaded(Result<ShelfData, String>);
/// 书架写回的结果: Ok(提示文字)
struct ShelfWritten(Result<String, String>);

/// 等待确认的书架操作
enum Confirm {
    RemoveBook { id: i64, title: String },
    DeleteFolder { id: String, title: String },
}

#[derive(Default)]
pub struct ShelfPage {
    data: Option<Loaded>,
    page: usize,
    /// `None` = 本地缓存模式 (离线或未登录); `Some` = 云端书架 (加载中时为空 Err 占位由 loading 标志区分)
    cloud: Option<Result<ShelfData, String>>,
    cloud_loading: bool,
    covers: CoverCache,
    /// 当前所在的文件夹路径 (空 = 根)
    folder: Vec<String>,
    /// 长按弹出的操作菜单
    menu: Option<ShelfEntry>,
    confirm: Option<Confirm>,
    /// 正在写回书架
    saving: bool,
    /// 一次性提示 (上次异常退出), 几秒后消失
    notice: Option<String>,
}

const NOTICE_TOKEN: u32 = 0x5e1f;

fn use_cloud(cx: &Cx<KinNovel>) -> bool {
    cx.app.signed_in()
}

/// 文件夹图标: 画在封面位置, 圆角框 + 左上的标签页。
fn draw_folder_icon(frame: &mut Bitmap, theme: &kn_ui::Theme, m: &kn_ui::widgets::Metrics, slot: Rect) {
    let w = (slot.w as f32 * 0.8) as u32;
    let h = (w as f32 * 0.7) as u32;
    let x = slot.x + (slot.w - w) as i32 / 2;
    let y = slot.y + (slot.h as i32 - h as i32) / 2 + (h / 10) as i32;
    let tab = Rect::new(x, y - (h / 6) as i32, w * 2 / 5, h / 4);
    frame.rounded_rect(tab, m.radius / 2, Some(theme.foreground), None, 0);
    frame.rounded_rect(Rect::new(x, y, w, h), m.radius / 2, Some(theme.light), Some(theme.foreground), 3);
}

/// "继续阅读" 卡片 (整张卡片可点)。
/// 有封面地址时左侧画封面 (未加载好是占位框), 文字右移。
#[allow(clippy::too_many_arguments)]
fn draw_continue(frame: &mut Bitmap, ink: &mut Ink, hits: &mut kn_ui::Hits, theme: kn_ui::Theme, m: kn_ui::widgets::Metrics, rect: Rect, last: &LastRead, cover: Option<Option<&Bitmap>>) {
    widgets::card(frame, &theme, &m, rect);
    let pad = m.margin as i32 / 2 + 8;
    let mut left = rect.x + pad;
    if let Some(img) = cover {
        let slot = continue_layout(&m, rect);
        match img {
            Some(img) => {
                let dx = slot.x + (slot.w as i32 - img.width() as i32) / 2;
                let dy = slot.y + (slot.h as i32 - img.height() as i32) / 2;
                frame.blit(img, img.bounds(), dx, dy);
            }
            None => crate::covers::placeholder(frame, &theme, &m, slot),
        }
        left = slot.right() + pad;
    }
    let inner = rect.right() - pad - left;
    let mut y = rect.y + pad;
    ink.text(frame, left, y, "继续阅读", m.tiny, theme.muted);
    let when = super::relative_time(last.time);
    let when_w = ink.width(&when, m.tiny).ceil() as i32;
    ink.text(frame, rect.right() - pad - when_w, y, &when, m.tiny, theme.muted);
    y += (m.tiny * 1.5) as i32;
    let name = ink.fit(&last.book_name, m.title, inner as f32);
    ink.text(frame, left, y, &name, m.title, theme.foreground);
    y += (m.title * 1.35) as i32;
    // 漫画的话名通常自带 "第N话", 不再加前缀; 页码直接写出来 (漫画没有章内百分比的概念)
    let chapter = if last.comic {
        let title = if last.chapter_title.is_empty() { format!("第 {} 话", last.sort_num) } else { last.chapter_title.clone() };
        format!("{title} · 第 {} / {} 页", last.page + 1, last.pages.max(1))
    } else {
        format!("第 {} 章 · {}", last.sort_num, last.chapter_title)
    };
    let chapter = ink.fit(&chapter, m.small, inner as f32);
    ink.text(frame, left, y, &chapter, m.small, theme.foreground);
    y += (m.small * 1.5) as i32;
    // 章内进度条
    let track_h = (8.0 * m.scale).max(6.0) as u32;
    let track = Rect::new(left, y + 6, inner as u32, track_h);
    frame.rounded_rect(track, track_h / 2, Some(theme.background), Some(theme.mid), 2);
    if last.pages > 0 {
        let filled = (inner as f32 * (last.page + 1) as f32 / last.pages as f32).max(track_h as f32) as u32;
        frame.rounded_rect(Rect::new(track.x, track.y, filled.min(inner as u32), track_h), track_h / 2, Some(theme.foreground), None, 0);
    }
    hits.add(HIT_CONTINUE, rect).rounded(m.radius);
}

impl Page<KinNovel> for ShelfPage {
    fn id(&self) -> PageId {
        PageId(100)
    }

    /// 本地数据读完、云端书架也回来了 (成功或失败)
    fn ready(&self) -> bool {
        self.data.is_some() && !self.cloud_loading
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        if let Some(text) = cx.app.startup_notice.take() {
            self.notice = Some(text);
            cx.after(std::time::Duration::from_secs(8), NOTICE_TOKEN);
        }
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
            cx.spawn(move || CloudLoaded(api::load_shelf(net.as_ref())));
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
        let msg = match msg.downcast::<CloudLoaded>() {
            Ok(cloud) => {
                self.cloud_loading = false;
                if let Ok(data) = &cloud.0 {
                    // 别处删掉的文件夹: 退回到还存在的那一层
                    while self.folder.last().is_some_and(|f| !data.has_folder(f)) {
                        self.folder.pop();
                    }
                }
                self.cloud = Some(cloud.0);
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(done) = msg.downcast::<ShelfWritten>() {
            self.saving = false;
            let text = match done.0 {
                Ok(text) => {
                    // 重新拉取 (书架以服务器为准)
                    self.cloud_loading = true;
                    let net = cx.app.net();
                    cx.spawn(move || CloudLoaded(api::load_shelf(net.as_ref())));
                    text
                }
                Err(e) => format!("保存失败: {e}"),
            };
            self.notice = Some(text);
            cx.after(std::time::Duration::from_secs(3), NOTICE_TOKEN);
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        self.render_shelf(cx, frame);
        if let Some(text) = &self.notice {
            let (theme, m) = (cx.theme, cx.app.metrics);
            let chain = cx.app.ui_fonts.clone();
            let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
            let bottom = cx.height as i32 - m.tab_bar_h() as i32 - m.margin as i32 / 2;
            widgets::toast(&mut ink, frame, &theme, &m, text, bottom);
        }
        super::tips::show(cx, frame, &super::tips::SHELF, self.showing_cloud(cx) && matches!(self.cloud, Some(Ok(_))) && self.menu.is_none() && self.confirm.is_none());
    }

    fn on_timer(&mut self, cx: &mut Cx<KinNovel>, token: u32) {
        if token == NOTICE_TOKEN && self.notice.take().is_some() {
            cx.request_redraw(RefreshHint::Clean);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        if super::tips::input(cx, event, &super::tips::SHELF) {
            return Transition::None;
        }
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if let Some(entry) = self.menu.clone() {
            if g.kind != GestureKind::Tap {
                return Transition::None;
            }
            let hit = cx.hits.at(Point { x: g.start.0, y: g.start.1 });
            if hit.is_some_and(|h| [HIT_ACT_MOVE, HIT_ACT_REMOVE, HIT_ACT_RENAME, HIT_ACT_CLOSE].contains(&h)) {
                self.menu = None;
                cx.request_redraw(RefreshHint::Clean);
            }
            let (what, title) = match &entry {
                ShelfEntry::Book(b) => (ShelfRef::Book(b.id), b.title.clone()),
                ShelfEntry::Folder { id, title, .. } => (ShelfRef::Folder(id.clone()), title.clone()),
            };
            return match hit {
                Some(HIT_ACT_MOVE) => match &self.cloud {
                    Some(Ok(data)) => Transition::Push(Box::new(MovePickerPage::new(data, what, title))),
                    _ => Transition::None,
                },
                Some(HIT_ACT_RENAME) => match what {
                    ShelfRef::Folder(id) => Transition::Push(Box::new(FolderNamePage::rename(id, &title))),
                    ShelfRef::Book(_) => Transition::None,
                },
                Some(HIT_ACT_REMOVE) => {
                    self.confirm = Some(match what {
                        ShelfRef::Book(id) => Confirm::RemoveBook { id, title },
                        ShelfRef::Folder(id) => Confirm::DeleteFolder { id, title },
                    });
                    cx.request_redraw(RefreshHint::Ui);
                    Transition::None
                }
                _ => Transition::None,
            };
        }
        if self.confirm.is_some() {
            if g.kind == GestureKind::Tap {
                match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
                    Some(HIT_CONFIRM) => {
                        let confirm = self.confirm.take().expect("checked");
                        self.write(cx, confirm);
                        cx.request_redraw(RefreshHint::Clean);
                    }
                    Some(HIT_CANCEL) => {
                        self.confirm = None;
                        cx.request_redraw(RefreshHint::Clean);
                    }
                    _ => {}
                }
            }
            return Transition::None;
        }
        match g.kind {
            GestureKind::Long if self.showing_cloud(cx) && !self.saving => {
                let entry = match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
                    Some(HitId(id)) if id >= ROW_BASE => self.entries().into_iter().nth((id - ROW_BASE) as usize),
                    _ => None,
                };
                if entry.is_some() {
                    self.menu = entry;
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            GestureKind::Tap => {
                let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
                if let Some(t) = super::tab_transition(hit, Tab::Shelf) {
                    return t;
                }
                if hit == HIT_NEW_FOLDER {
                    return Transition::Push(Box::new(FolderNamePage::create(self.folder.clone())));
                }
                if hit == HIT_FOLDER_UP {
                    self.folder.pop();
                    self.page = 0;
                    cx.request_redraw(RefreshHint::Ui);
                    return Transition::None;
                }
                let last = self.data.as_ref().and_then(|d| d.last.clone());
                let cloud_mode = self.showing_cloud(cx);
                match hit {
                    HIT_CONTINUE => match last {
                        Some(l) if l.comic => Transition::Push(Box::new(ComicReaderPage::new(l.book_id, l.chapter_id, Entry::Resume))),
                        Some(l) => Transition::Push(Box::new(ReaderPage::new(l.book_id, l.sort_num, Entry::Resume))),
                        None => Transition::None,
                    },
                    HIT_PREV => self.turn(cx, -1),
                    HIT_NEXT => self.turn(cx, 1),
                    HitId(id) if id >= ROW_BASE && cloud_mode => match self.entries().into_iter().nth((id - ROW_BASE) as usize) {
                        Some(ShelfEntry::Book(b)) => Transition::Push(Box::new(BookDetailPage::new(b.id))),
                        Some(ShelfEntry::Folder { id, .. }) => {
                            self.folder.push(id);
                            self.page = 0;
                            cx.request_redraw(RefreshHint::Ui);
                            Transition::None
                        }
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
    fn render_shelf(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let tabs_top = super::draw_tabs(cx, frame, Tab::Shelf);
        let cloud_mode = self.showing_cloud(cx);

        // cx 整体借用 (封面预取) 必须在创建下面的 `ink` (借用 cx.fonts/cx.glyphs) 之前完成;
        // 预取用的分页算法要和下面绘制时用的完全一致, 否则请求和绘制的书对不上。
        let in_folder = cloud_mode && !self.folder.is_empty();
        let show_continue = !in_folder && self.data.as_ref().is_some_and(|d| d.last.is_some());
        if cloud_mode && matches!(self.cloud, Some(Ok(_))) {
            let entries = self.entries();
            let list_bottom = tabs_top - m.pager_h() as i32;
            let mut y = m.header_h() as i32 + m.margin as i32 / 2;
            if show_continue {
                y += continue_h(&m) as i32 + m.margin as i32 / 2;
            }
            y += (m.small * 1.6) as i32;
            let row_h = m.row_h_cover();
            let per_page = rows_fit(list_bottom - y, row_h).max(1);
            let pages = entries.len().div_ceil(per_page).max(1);
            let page = self.page.min(pages - 1);
            let cover_rect = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, row_h));
            for entry in entries.iter().skip(page * per_page).take(per_page) {
                if let ShelfEntry::Book(book) = entry {
                    self.covers.request(cx, &book.cover, cover_rect.w, cover_rect.h);
                }
            }
        }

        // 继续阅读卡片的封面 (与下面绘制时的位置一致)
        let continue_rect = Rect::new(m.margin as i32, m.header_h() as i32 + m.margin as i32 / 2, cx.width - 2 * m.margin, continue_h(&m));
        let continue_slot = continue_layout(&m, continue_rect);
        if let Some(data) = &self.data {
            if data.last.is_some() && !data.last_cover.is_empty() {
                self.covers.request(cx, &data.last_cover, continue_slot.w, continue_slot.h);
            }
        }

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let folder_title = match (&self.cloud, self.folder.last()) {
            (Some(Ok(data)), Some(f)) if in_folder => data.folder_title(f),
            _ => "书架".to_string(),
        };
        let bar = widgets::header(&mut ink, frame, &theme, &m, &folder_title, &super::status_text(), in_folder, false);
        if in_folder {
            cx.hits.add(HIT_FOLDER_UP, Rect::new(0, 0, bar.h, bar.h));
        }
        let side = m.margin as i32;
        let mut y = bar.bottom() + m.margin as i32 / 2;
        let Some(data) = &self.data else {
            let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
            return;
        };
        if let Some(last) = data.last.clone().filter(|_| show_continue || !cloud_mode) {
            let rect = Rect::new(side, y, cx.width - 2 * m.margin, continue_h(&m));
            let cover = (!data.last_cover.is_empty()).then(|| self.covers.get(&data.last_cover, continue_slot.w, continue_slot.h));
            draw_continue(frame, &mut ink, cx.hits, theme, m, rect, &last, cover);
            y = rect.bottom() + m.margin as i32 / 2;
        }
        let list_bottom = tabs_top - m.pager_h() as i32;
        if cloud_mode {
            let label = if self.saving { "我的书架 · 正在保存…" } else if in_folder { "文件夹" } else { "我的书架" };
            ink.text(frame, side, y, label, m.small, theme.muted);
            if matches!(self.cloud, Some(Ok(_))) && !self.saving {
                // 右侧「＋ 新建文件夹」: 字小, 命中区域按触摸尺寸放大
                let text = "＋ 新建文件夹";
                let w = ink.width(text, m.small).ceil() as i32;
                let x = cx.width as i32 - side - w;
                ink.text(frame, x, y, text, m.small, theme.foreground);
                let text_h = (m.small * 1.6) as i32;
                let hit_h = (m.touch as i32).max(text_h);
                cx.hits.add(HIT_NEW_FOLDER, Rect::new(x - side / 2, y + text_h / 2 - hit_h / 2, (w + side) as u32, hit_h as u32)).rounded(m.radius);
            }
            y += (m.small * 1.6) as i32;
            frame.fill_rect(Rect::new(side, y - 2, cx.width - 2 * m.margin, 2), theme.foreground);
            let entries = self.entries();
            match &self.cloud {
                None if self.cloud_loading => {
                    let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                    return;
                }
                Some(Ok(_)) if entries.is_empty() => {
                    let area = Rect::new(0, y, cx.width, (tabs_top - y).max(0) as u32);
                    let text = if in_folder { "文件夹是空的" } else { "书架为空\n在书籍详情页点「加入书架」" };
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, text, None);
                }
                Some(Ok(_)) => {
                    let row_h = m.row_h_cover();
                    let per_page = rows_fit(list_bottom - y, row_h);
                    let pages = entries.len().div_ceil(per_page);
                    self.page = self.page.min(pages - 1);
                    for (row, (index, entry)) in entries.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
                        let r = Rect::new(0, y + row as i32 * row_h as i32, cx.width, row_h);
                        let cover_rect = widgets::cover_slot(&m, r);
                        match entry {
                            ShelfEntry::Book(book) => {
                                let img = self.covers.get(&book.cover, cover_rect.w, cover_rect.h);
                                widgets::list_row_cover_badged(&mut ink, frame, &theme, &m, r, img, &book.badge(), &book.title, &book.subtitle(), &book.last_update);
                            }
                            ShelfEntry::Folder { title, count, .. } => {
                                widgets::list_row_cover(&mut ink, frame, &theme, &m, r, None, title, &format!("文件夹 · {count} 项"), "");
                                draw_folder_icon(frame, &theme, &m, cover_rect);
                            }
                        }
                        cx.hits.add(HitId(ROW_BASE + index as u32), r);
                    }
                    if pages > 1 {
                        let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                        widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, HIT_PREV, HIT_NEXT);
                    }
                }
                Some(Err(_)) | None => {}
            }
            if let Some(entry) = &self.menu {
                let (title, info, remove) = match entry {
                    ShelfEntry::Book(b) => (b.title.clone(), b.subtitle(), "移出书架"),
                    ShelfEntry::Folder { title, count, .. } => (title.clone(), format!("文件夹 · {count} 项"), "删除"),
                };
                let mut buttons = vec![("移动到…", HIT_ACT_MOVE, widgets::ButtonStyle::Secondary)];
                if matches!(entry, ShelfEntry::Folder { .. }) {
                    buttons.push(("重命名", HIT_ACT_RENAME, widgets::ButtonStyle::Secondary));
                }
                buttons.push((remove, HIT_ACT_REMOVE, widgets::ButtonStyle::Secondary));
                buttons.push(("取消", HIT_ACT_CLOSE, widgets::ButtonStyle::Primary));
                widgets::dialog(&mut ink, frame, cx.hits, &theme, &m, &title, &[&info], &buttons);
            }
            if let Some(confirm) = &self.confirm {
                let (title, line) = match confirm {
                    Confirm::RemoveBook { title, .. } => ("从书架移出？".to_string(), title.clone()),
                    Confirm::DeleteFolder { title, .. } => (format!("删除文件夹「{title}」？"), "其中的书会移到上一层".to_string()),
                };
                let ok = if matches!(confirm, Confirm::RemoveBook { .. }) { "移出" } else { "删除" };
                widgets::dialog(&mut ink, frame, cx.hits, &theme, &m, &title, &[&line], &[("取消", HIT_CANCEL, widgets::ButtonStyle::Secondary), (ok, HIT_CONFIRM, widgets::ButtonStyle::Primary)]);
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

    /// 当前文件夹层的条目 (云端书架已加载时)。
    fn entries(&self) -> Vec<ShelfEntry> {
        match &self.cloud {
            Some(Ok(data)) => data.level(self.folder.last().map(String::as_str)),
            _ => Vec::new(),
        }
    }

    /// 执行确认过的书架操作 (后台: 拉取最新的表、改一项、整表写回)。
    fn write(&mut self, cx: &mut Cx<KinNovel>, confirm: Confirm) {
        self.saving = true;
        let net = cx.app.net();
        cx.spawn(move || {
            ShelfWritten(match confirm {
                Confirm::RemoveBook { id, .. } => api::shelf_set_book(net.as_ref(), id, false, false).map(|_| "已移出书架".to_string()),
                Confirm::DeleteFolder { id, .. } => api::shelf_delete_folder(net.as_ref(), &id).map(|_| "已删除文件夹".to_string()),
            })
        });
    }

    /// 云端书架加载失败 (断网、服务器故障) 时退回本地缓存, 已缓存的书照样能读。
    fn showing_cloud(&self, cx: &Cx<KinNovel>) -> bool {
        use_cloud(cx) && !matches!(self.cloud, Some(Err(_)))
    }

    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        let has_rows = if self.showing_cloud(cx) {
            !self.entries().is_empty()
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
