//! 书籍详情 (对照 Python `book.py`, 版式按 UI-DESIGN 重新设计) + 目录 (章节列表)。
//! 两者都是压栈页面: 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, FontId, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::comic::ComicReaderPage;
use super::comments::CommentsPage;
use super::reader::{Entry, ReaderJump, ReaderPage};
use super::search::SearchPage;
use super::series::SeriesPage;
use crate::api::{self, BookInfo, ChapterRef, SearchMode};
use crate::covers::CoverCache;
use crate::store;
use crate::userdata::{self, Bookmark};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_READ: HitId = HitId(2);
const HIT_CATALOG: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_COMMENTS: HitId = HitId(5);
const HIT_SERIES: HitId = HitId(6);
const HIT_AUTHOR: HitId = HitId(7);
const HIT_SHELF: HitId = HitId(8);
const HIT_TAG_BASE: u32 = 100;

enum Status {
    Loading,
    Offline,
    Error(String),
    Ready(BookInfo),
}

struct Loaded(Result<BookInfo, String>);
/// 这本书在不在书架上 (查询或加入/移出之后)
struct ShelfState(Result<bool, String>);

/// 继续阅读的目标章节 (优先本地最近阅读, 再找任一章节的本地进度文件, 最后用服务器阅读位置)。
/// 对照 Python `book.py` 的 `_resume_sort_num` (做了简化: 不区分会话内/云端谁更靠后)。
fn resume_sort_num(cx: &Cx<KinNovel>, info: &BookInfo) -> Option<i64> {
    if let Some(last) = store::load_last_read(&cx.app.paths) {
        if last.book_id == info.book.id {
            return Some(last.sort_num);
        }
    }
    let convert = cx.app.config.convert();
    for ch in &info.chapters {
        if store::load_progress(&cx.app.paths.progress_file(info.book.id, ch.sort_num, &convert)).is_some() {
            return Some(ch.sort_num);
        }
    }
    if info.read_position_chapter_id > 0 {
        if let Some(ch) = info.chapters.iter().find(|c| c.id == info.read_position_chapter_id) {
            return Some(ch.sort_num);
        }
    }
    None
}

/// 漫画继续阅读的目标话 (章节 id): 本地进度, 再用服务器阅读位置。
fn resume_comic_chapter(cx: &Cx<KinNovel>, info: &BookInfo) -> Option<i64> {
    if let Some(p) = crate::comic::load_progress(&cx.app.paths, info.book.id) {
        if info.chapters.iter().any(|c| c.id == p.chapter_id) {
            return Some(p.chapter_id);
        }
    }
    (info.read_position_chapter_id > 0 && info.chapters.iter().any(|c| c.id == info.read_position_chapter_id)).then_some(info.read_position_chapter_id)
}

/// "继续阅读/开始阅读" 是否有进度 (小说按章节序号, 漫画按章节 id)。
fn has_resume(cx: &Cx<KinNovel>, info: &BookInfo) -> bool {
    if info.is_comic { resume_comic_chapter(cx, info).is_some() } else { resume_sort_num(cx, info).is_some() }
}

pub struct BookDetailPage {
    book_id: i64,
    status: Status,
    covers: CoverCache,
    /// 书架状态 (登录时才有这个按钮): None = 查询中/写回中
    on_shelf: Option<bool>,
    shelf_error: bool,
}

impl BookDetailPage {
    pub fn new(book_id: i64) -> Self {
        BookDetailPage { book_id, status: Status::Loading, covers: CoverCache::default(), on_shelf: None, shelf_error: false }
    }

    fn shelf_button(cx: &Cx<KinNovel>) -> bool {
        cx.app.signed_in() && (cx.app.net().is_some() || api::fake_mode())
    }

    fn load(&mut self, cx: &mut Cx<KinNovel>) {
        if cx.app.net().is_none() && !api::fake_mode() {
            self.status = Status::Offline;
            return;
        }
        self.status = Status::Loading;
        let net = cx.app.net();
        let book_id = self.book_id;
        cx.spawn(move || Loaded(api::load_book_info(net.as_ref(), book_id)));
    }
}

impl Page<KinNovel> for BookDetailPage {
    fn id(&self) -> PageId {
        PageId(200)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx);
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
        let msg = match msg.downcast::<ShelfState>() {
            Ok(s) => {
                match s.0 {
                    Ok(v) => {
                        self.on_shelf = Some(v);
                        self.shelf_error = false;
                    }
                    Err(e) => {
                        eprintln!("[book] 书架: {e}");
                        self.shelf_error = true;
                    }
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(l) = msg.downcast::<Loaded>() {
            self.status = match l.0 {
                Ok(info) => {
                    // 漫画: 话列表写缓存, 阅读页换话、离线目录用
                    if info.is_comic {
                        crate::comic::ComicBook::from_info(&info).save(&cx.app.paths);
                    }
                    if Self::shelf_button(cx) && self.on_shelf.is_none() {
                        let (net, id) = (cx.app.net(), self.book_id);
                        cx.spawn(move || ShelfState(api::shelf_contains(net.as_ref(), id)));
                    }
                    Status::Ready(info)
                }
                Err(e) => Status::Error(e),
            };
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let side = m.margin as i32;
        let cover_w = (cx.width as f32 * 0.26) as u32;
        let cover_h = (cover_w as f32 * 1.42) as u32;

        // cx 整体借用 (封面请求、继续阅读计算) 必须在创建 `ink` (借用 cx.fonts/cx.glyphs) 之前完成。
        let resume = if let Status::Ready(info) = &self.status {
            self.covers.request(cx, &info.book.cover, cover_w, cover_h);
            has_resume(cx, info)
        } else {
            false
        };

        let shelf_button = Self::shelf_button(cx);
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let heading = if matches!(&self.status, Status::Ready(info) if info.is_comic) { "漫画详情" } else { "书籍详情" };
        let bar = widgets::header(&mut ink, frame, &theme, &m, heading, &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);

        let info = match &self.status {
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
            Status::Ready(info) => info,
        };

        let cover_rect = Rect::new(side, bar.bottom() + m.margin as i32 / 2, cover_w, cover_h);
        match self.covers.get(&info.book.cover, cover_rect.w, cover_rect.h) {
            Some(img) => {
                let dx = cover_rect.x + (cover_rect.w as i32 - img.width() as i32) / 2;
                let dy = cover_rect.y + (cover_rect.h as i32 - img.height() as i32) / 2;
                frame.blit(img, img.bounds(), dx, dy);
            }
            None => crate::covers::placeholder(frame, &theme, &m, cover_rect),
        }

        let info_x = cover_rect.right() + side;
        let info_w = (cx.width as i32 - info_x - side).max(0) as f32;
        let mut y = cover_rect.y;
        let title = ink.fit(&info.book.title, m.title, info_w);
        ink.text(frame, info_x, y, &title, m.title, theme.foreground);
        y += (m.title * 1.3) as i32;
        let has_author = !info.book.author.is_empty();
        let author = if has_author { info.book.author.clone() } else { "未知作者".to_string() };
        let author = if info.is_comic { format!("{author} · 漫画 · {} 话", info.chapters.len()) } else { author };
        let author = ink.fit(&author, m.small, info_w);
        let author_w = ink.text(frame, info_x, y, &author, m.small, if has_author { theme.foreground } else { theme.muted });
        if has_author {
            // 点作者 → 按作者搜索 (网页版 BookInfo 的作者链接)
            cx.hits.add(HIT_AUTHOR, Rect::new(info_x, y, (author_w.ceil() as u32).max(m.touch), (m.small * 1.5) as u32));
        }
        y += (m.small * 1.5) as i32;
        if !info.series_name.is_empty() {
            let series = ink.fit(&format!("系列: {} >", info.series_name), m.small, info_w);
            let series_top = y;
            ink.text(frame, info_x, y, &series, m.small, theme.muted);
            y += (m.small * 1.5) as i32;
            // 触控区域扩大到最小触控高, 向下延伸 (该区域内没有其它命中)。
            cx.hits.add(HIT_SERIES, Rect::new(info_x, series_top, info_w as u32, m.touch));
        }
        if !info.book.last_chapter.is_empty() {
            let last = ink.fit(&format!("最新: {}", info.book.last_chapter), m.small, info_w);
            ink.text(frame, info_x, y, &last, m.small, theme.muted);
            y += (m.small * 1.5) as i32;
        }
        if !info.tags.is_empty() {
            // 每个标签单独可点 → 按标签搜索; 放不下的省略
            let line_h = (m.small * 1.5) as i32;
            let mut x = info_x as f32 + ink.text(frame, info_x, y, "标签: ", m.small, theme.muted);
            let right = info_x as f32 + info_w;
            for (i, tag) in info.tags.iter().enumerate() {
                let sep = if i > 0 { "、" } else { "" };
                let w = ink.width(sep, m.small) + ink.width(tag, m.small);
                let ellipsis = ink.width("…", m.small);
                let last = i + 1 == info.tags.len();
                if x + w + if last { 0.0 } else { ellipsis } > right {
                    ink.text(frame, x as i32, y, "…", m.small, theme.muted);
                    break;
                }
                x += ink.text(frame, x as i32, y, sep, m.small, theme.muted);
                let tw = ink.text(frame, x as i32, y, tag, m.small, theme.foreground);
                cx.hits.add(HitId(HIT_TAG_BASE + i as u32), Rect::new(x as i32, y, tw.ceil() as u32, (line_h + m.margin as i32 / 2) as u32));
                x += tw;
            }
            y += line_h;
        }

        let btn_h = m.touch as i32;
        let gap = m.margin as i32 / 3;
        let bottom_y = cx.height as i32 - btn_h - m.margin as i32 / 2;

        // 简介: 占满到底部按钮上方, 放不下的最后一行以 … 结尾
        let intro_y = (cover_rect.bottom() + m.margin as i32 / 2).max(y + m.margin as i32 / 2);
        ink.text(frame, side, intro_y, "简介", m.body, theme.foreground);
        let mut iy = intro_y + (m.body * 1.4) as i32;
        let intro_w = (cx.width - 2 * m.margin) as f32;
        let intro_line_h = (m.small * 1.4) as i32;
        let max_lines = ((bottom_y - m.margin as i32 / 2 - iy) / intro_line_h).max(1) as usize;
        for line in wrap_plain(&info.intro, &ink, m.small, intro_w, max_lines) {
            ink.text(frame, side, iy, &line, m.small, theme.muted);
            iy += intro_line_h;
        }
        let cols = if shelf_button { 4 } else { 3 };
        let col_w = (cx.width as i32 - 2 * side - gap * (cols - 1)) / cols;
        let has_chapters = !info.chapters.is_empty();
        let read_label = if resume { "继续阅读" } else { "开始阅读" };
        let read_rect = Rect::new(side, bottom_y, col_w as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, read_rect, read_label, if has_chapters { ButtonStyle::Primary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_READ, read_rect).rounded(m.radius).enabled(has_chapters);
        let catalog_rect = Rect::new(read_rect.right() + gap, bottom_y, col_w as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, catalog_rect, "目录", if has_chapters { ButtonStyle::Secondary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_CATALOG, catalog_rect).rounded(m.radius).enabled(has_chapters);
        let mut next_x = catalog_rect.right() + gap;
        if shelf_button {
            let shelf_rect = Rect::new(next_x, bottom_y, col_w as u32, btn_h as u32);
            let (label, style) = match (self.on_shelf, self.shelf_error) {
                (_, true) => ("书架重试", ButtonStyle::Secondary),
                (None, _) => ("书架…", ButtonStyle::Disabled),
                (Some(true), _) => ("移出书架", ButtonStyle::Secondary),
                (Some(false), _) => ("加入书架", ButtonStyle::Secondary),
            };
            widgets::button(&mut ink, frame, &theme, &m, shelf_rect, label, style);
            cx.hits.add(HIT_SHELF, shelf_rect).rounded(m.radius).enabled(self.on_shelf.is_some() || self.shelf_error);
            next_x = shelf_rect.right() + gap;
        }
        let comments_rect = Rect::new(next_x, bottom_y, col_w as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, comments_rect, "评论", ButtonStyle::Secondary);
        cx.hits.add(HIT_COMMENTS, comments_rect).rounded(m.radius);
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
                self.load(cx);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_READ => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                if info.is_comic {
                    return match resume_comic_chapter(cx, info).or_else(|| info.chapters.first().map(|c| c.id)) {
                        Some(cid) => Transition::Push(Box::new(ComicReaderPage::new(self.book_id, cid, Entry::Resume))),
                        None => Transition::None,
                    };
                }
                let sort_num = resume_sort_num(cx, info).or_else(|| info.chapters.first().map(|c| c.sort_num));
                match sort_num {
                    Some(sort_num) => Transition::Push(Box::new(ReaderPage::new(self.book_id, sort_num, Entry::Resume))),
                    None => Transition::None,
                }
            }
            HIT_CATALOG => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                if info.is_comic {
                    let current = resume_comic_chapter(cx, info);
                    return Transition::Push(Box::new(CatalogPage::comic(self.book_id, info.chapters.clone(), current)));
                }
                let current = resume_sort_num(cx, info);
                Transition::Push(Box::new(CatalogPage::new(self.book_id, info.chapters.clone(), current)))
            }
            HIT_COMMENTS => Transition::Push(Box::new(CommentsPage::new("Book", self.book_id))),
            HIT_SHELF => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                let (net, id, comic) = (cx.app.net(), self.book_id, info.is_comic);
                match self.on_shelf.take() {
                    // 上次查询失败: 重新查询
                    None => cx.spawn(move || ShelfState(api::shelf_contains(net.as_ref(), id))),
                    Some(current) => cx.spawn(move || ShelfState(api::shelf_set_book(net.as_ref(), id, comic, !current))),
                }
                self.shelf_error = false;
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_SERIES => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                if info.series_name.is_empty() {
                    return Transition::None;
                }
                // 漫画: 系列里的各卷随详情一起返回 (`Series`); 按系列名查书的接口只认小说
                if info.is_comic {
                    return Transition::Push(Box::new(SeriesPage::with_books(info.series_name.clone(), self.book_id, info.series_books.clone())));
                }
                Transition::Push(Box::new(SeriesPage::new(info.series_name.clone(), self.book_id)))
            }
            HIT_AUTHOR => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                Transition::Push(Box::new(SearchPage::with_query(&info.book.author, SearchMode::Author, info.is_comic)))
            }
            HitId(id) if (HIT_TAG_BASE..HIT_TAG_BASE + 100).contains(&id) => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                match info.tags.get((id - HIT_TAG_BASE) as usize) {
                    Some(tag) => Transition::Push(Box::new(SearchPage::with_query(tag, SearchMode::Tags, info.is_comic))),
                    None => Transition::None,
                }
            }
            _ => Transition::None,
        }
    }
}

/// 简介折行 (同公告: 段内换行保留, 句末标点不放行首); 超过 `max_lines` 时最后一行以 … 结尾。
fn wrap_plain(text: &str, ink: &Ink, size: f32, max_width: f32, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .flat_map(|l| super::announce::wrap_all(l, ink, size, max_width))
        .collect();
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            last.push('…');
            if ink.width(last, size) > max_width {
                *last = ink.fit(last, size, max_width);
            }
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// 目录 (+ 从阅读页打开时的「书签」标签)
// ---------------------------------------------------------------------------

const CAT_HIT_BACK: HitId = HitId(1);
const CAT_HIT_PREV: HitId = HitId(2);
const CAT_HIT_NEXT: HitId = HitId(3);
const CAT_HIT_DELETE: HitId = HitId(4);
const CAT_HIT_CANCEL: HitId = HitId(5);
const CAT_SEG_BASE: u32 = 10;
const CAT_ROW_BASE: u32 = 1000;
const CAT_MARK_BASE: u32 = 100_000;

pub struct CatalogPage {
    book_id: i64,
    chapters: Vec<ChapterRef>,
    /// 小说: 当前章节序号; 漫画: 当前话的章节 id
    current: Option<i64>,
    comic: bool,
    /// 从阅读页打开: 选章/选书签后返回阅读页让它就地跳转 (`ReaderJump`), 并显示「书签」标签
    from_reader: bool,
    /// 0 = 目录, 1 = 书签
    tab: usize,
    bookmarks: Vec<Bookmark>,
    /// 长按书签后待确认删除的下标
    confirm_delete: Option<usize>,
    /// 画书签摘录用的章节字体: (文件名, 字体, 是否本页加载的 [离开时卸载])
    fonts: Vec<(String, FontId, bool)>,
    page: usize,
    mark_page: usize,
    /// 首次渲染时跳到当前章节所在页
    jumped: bool,
}

impl CatalogPage {
    pub fn new(book_id: i64, chapters: Vec<ChapterRef>, current: Option<i64>) -> Self {
        CatalogPage {
            book_id,
            chapters,
            current,
            comic: false,
            from_reader: false,
            tab: 0,
            bookmarks: Vec::new(),
            confirm_delete: None,
            fonts: Vec::new(),
            page: 0,
            mark_page: 0,
            jumped: false,
        }
    }

    /// 漫画目录: 每行显示页数, 点击用漫画阅读页打开。`current` 是章节 id。
    pub fn comic(book_id: i64, chapters: Vec<ChapterRef>, current: Option<i64>) -> Self {
        CatalogPage { comic: true, ..CatalogPage::new(book_id, chapters, current) }
    }

    /// 阅读页菜单里的目录: 带书签标签; 选中后回到阅读页跳转。
    pub fn from_reader(book_id: i64, chapters: Vec<ChapterRef>, current: i64) -> Self {
        CatalogPage { from_reader: true, ..CatalogPage::new(book_id, chapters, Some(current)) }
    }

    fn is_current(&self, ch: &ChapterRef) -> bool {
        self.current == Some(if self.comic { ch.id } else { ch.sort_num })
    }

    /// 打开章节 (或书签位置)。
    fn open(&self, cx: &mut Cx<KinNovel>, sort_num: i64, at: Option<store::Progress>) -> Transition<KinNovel> {
        if self.from_reader {
            cx.app.reader_jump = Some(ReaderJump { book_id: self.book_id, sort_num, at });
            return Transition::Back;
        }
        Transition::Push(Box::new(ReaderPage::new(self.book_id, sort_num, Entry::Resume)))
    }
}

impl Page<KinNovel> for CatalogPage {
    fn id(&self) -> PageId {
        PageId(201)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        if !self.from_reader {
            return;
        }
        self.bookmarks = userdata::load_bookmarks(&cx.app.paths, self.book_id);
        // 摘录是混淆码位, 要用记下的章节字体画: 阅读页正在用的直接借用, 其余从字体缓存读 (TTF 旁路, 几十毫秒)
        for b in &self.bookmarks {
            if b.font.is_empty() || self.fonts.iter().any(|(name, _, _)| *name == b.font) {
                continue;
            }
            let current = cx.app.chapter_font.as_ref().filter(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some(&b.font));
            if let Some((_, id)) = current {
                self.fonts.push((b.font.clone(), *id, false));
                continue;
            }
            let path = cx.app.paths.cache_dir().join("fonts").join(&b.font);
            match store::load_chapter_font(&path).and_then(|bytes| cx.fonts.load_bytes(bytes).map_err(|e| format!("{e:?}"))) {
                Ok(id) => self.fonts.push((b.font.clone(), id, true)),
                Err(e) => eprintln!("[catalog] 书签字体 {}: {e}", b.font),
            }
        }
    }

    fn leave(&mut self, cx: &mut Cx<KinNovel>) {
        for (_, id, owned) in self.fonts.drain(..) {
            if owned {
                cx.fonts.unload(id);
                cx.glyphs.forget_font(id);
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let title = if self.from_reader { "目录与书签" } else { "目录" };
        let bar = widgets::header(&mut ink, frame, &theme, &m, title, &super::status_text(), true, false);
        cx.hits.add(CAT_HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let mut top = bar.bottom();
        if self.from_reader {
            let side = m.margin as i32;
            let gap = side / 3;
            let seg = Rect::new(side, top + gap, cx.width - 2 * m.margin, m.touch);
            let marks = if self.bookmarks.is_empty() { "书签".to_string() } else { format!("书签 ({})", self.bookmarks.len()) };
            widgets::segmented(&mut ink, frame, cx.hits, &theme, &m, seg, &["目录", &marks], self.tab, CAT_SEG_BASE);
            top = seg.bottom() + gap;
            frame.fill_rect(Rect::new(0, top - 1, cx.width, 1), theme.mid);
        }
        let row_h = m.row_h();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        let per_page = ((list_bottom - top) / row_h as i32).max(1) as usize;
        if self.tab == 1 {
            self.render_bookmarks(cx, frame, top, per_page, list_bottom);
            return;
        }
        if self.chapters.is_empty() {
            let area = Rect::new(0, top, cx.width, (cx.height as i32 - top) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无章节", None);
            return;
        }
        let pages = self.chapters.len().div_ceil(per_page).max(1);
        if !self.jumped {
            self.jumped = true;
            if let Some(i) = self.chapters.iter().position(|ch| self.is_current(ch)) {
                self.page = i / per_page;
            }
        }
        self.page = self.page.min(pages - 1);
        for (row, (index, ch)) in self.chapters.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
            let r = Rect::new(0, top + row as i32 * row_h as i32, cx.width, row_h);
            let unit = if self.comic { "话" } else { "章" };
            let title = if ch.title.is_empty() { format!("第 {} {unit}", ch.sort_num) } else { ch.title.clone() };
            let meta = match (self.is_current(ch), self.comic && ch.page_count > 0) {
                (true, true) => format!("当前 · {}P", ch.page_count),
                (true, false) => "当前".to_string(),
                (false, true) => format!("{}P", ch.page_count),
                (false, false) => String::new(),
            };
            widgets::list_row(&mut ink, frame, &theme, &m, r, &title, "", &meta);
            cx.hits.add(HitId(CAT_ROW_BASE + index as u32), r);
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, CAT_HIT_PREV, CAT_HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        let hit = cx.hits.at(Point { x: g.start.0, y: g.start.1 });
        if let Some(index) = self.confirm_delete {
            if g.kind == GestureKind::Tap {
                match hit {
                    Some(CAT_HIT_DELETE) => {
                        if index < self.bookmarks.len() {
                            self.bookmarks.remove(index);
                            if let Err(e) = userdata::save_bookmarks(&cx.app.paths, self.book_id, &self.bookmarks) {
                                eprintln!("[catalog] 书签保存失败: {e}");
                            }
                        }
                        self.confirm_delete = None;
                        cx.request_redraw(RefreshHint::Clean);
                    }
                    Some(CAT_HIT_CANCEL) => {
                        self.confirm_delete = None;
                        cx.request_redraw(RefreshHint::Clean);
                    }
                    _ => {}
                }
            }
            return Transition::None;
        }
        match g.kind {
            GestureKind::Long => match hit {
                Some(HitId(id)) if id >= CAT_MARK_BASE && ((id - CAT_MARK_BASE) as usize) < self.bookmarks.len() => {
                    self.confirm_delete = Some((id - CAT_MARK_BASE) as usize);
                    cx.request_redraw(RefreshHint::Ui);
                    Transition::None
                }
                _ => Transition::None,
            },
            GestureKind::Tap => match hit {
                Some(CAT_HIT_BACK) => Transition::Back,
                Some(CAT_HIT_PREV) => self.turn(cx, -1),
                Some(CAT_HIT_NEXT) => self.turn(cx, 1),
                Some(HitId(id)) if (CAT_SEG_BASE..CAT_SEG_BASE + 2).contains(&id) => {
                    let tab = (id - CAT_SEG_BASE) as usize;
                    if tab != self.tab {
                        self.tab = tab;
                        cx.request_redraw(RefreshHint::Ui);
                    }
                    Transition::None
                }
                Some(HitId(id)) if id >= CAT_MARK_BASE => match self.bookmarks.get((id - CAT_MARK_BASE) as usize) {
                    Some(b) => {
                        let at = store::Progress { path: b.path.clone(), offset: b.offset, page: 0 };
                        self.open(cx, b.sort_num, Some(at))
                    }
                    None => Transition::None,
                },
                Some(HitId(id)) if id >= CAT_ROW_BASE => match self.chapters.get((id - CAT_ROW_BASE) as usize) {
                    Some(ch) if self.comic => Transition::Push(Box::new(ComicReaderPage::new(self.book_id, ch.id, Entry::Resume))),
                    Some(ch) => {
                        let sort_num = ch.sort_num;
                        self.open(cx, sort_num, None)
                    }
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

impl CatalogPage {
    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        let page = if self.tab == 1 { &mut self.mark_page } else { &mut self.page };
        let next = *page as i64 + delta;
        if next >= 0 {
            *page = next as usize;
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }

    fn render_bookmarks(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap, top: i32, per_page: usize, list_bottom: i32) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let ink = &mut Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        if self.bookmarks.is_empty() {
            let area = Rect::new(0, top, cx.width, (cx.height as i32 - top) as u32);
            widgets::state_message(ink, frame, cx.hits, &theme, &m, area, "还没有书签", None);
            return;
        }
        let pages = self.bookmarks.len().div_ceil(per_page).max(1);
        self.mark_page = self.mark_page.min(pages - 1);
        let row_h = m.row_h() as i32;
        let mut y = top;
        for (index, b) in self.bookmarks.iter().enumerate().skip(self.mark_page * per_page).take(per_page) {
            let r = Rect::new(0, y, cx.width, row_h as u32);
            let title = if b.chapter.is_empty() { format!("第 {} 章", b.sort_num) } else { b.chapter.clone() };
            // 副标题 (摘录) 自己画: 字体链不同; list_row 只给它留位置
            widgets::list_row(ink, frame, &theme, &m, r, &title, " ", &super::relative_time(b.time));
            let font = self.fonts.iter().find(|(name, _, _)| *name == b.font).map(|(_, id, _)| *id);
            let excerpt = match font {
                Some(_) if !b.excerpt.is_empty() => b.excerpt.clone(),
                None if b.font.is_empty() && !b.excerpt.is_empty() => b.excerpt.clone(),
                _ => format!("第 {} 章内的书签", b.sort_num),
            };
            let chain: Vec<FontId> = font.into_iter().chain(chain.iter().copied()).collect();
            let mut sub = Ink { fonts: &*ink.fonts, glyphs: &mut *ink.glyphs, chain: &chain };
            let (title_h, sub_h) = ((m.body * 1.25) as i32, (m.small * 1.25) as i32);
            let gap = (m.small * 0.3) as i32;
            let sub_y = r.y + (r.h as i32 - (title_h + sub_h + gap)) / 2 + title_h + gap;
            let side = m.margin as i32;
            let excerpt = sub.fit(&excerpt, m.small, (r.w as i32 - 2 * side) as f32);
            sub.text(frame, r.x + side, sub_y, &excerpt, m.small, theme.muted);
            cx.hits.add(HitId(CAT_MARK_BASE + index as u32), r);
            y += row_h;
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(ink, frame, cx.hits, &theme, &m, pr, self.mark_page, pages, CAT_HIT_PREV, CAT_HIT_NEXT);
        }
        if let Some(index) = self.confirm_delete {
            let label = self.bookmarks.get(index).map(|b| b.excerpt.clone()).unwrap_or_default();
            widgets::dialog(
                ink,
                frame,
                cx.hits,
                &theme,
                &m,
                "删除这个书签？",
                &[&label],
                &[("取消", CAT_HIT_CANCEL, ButtonStyle::Secondary), ("删除", CAT_HIT_DELETE, ButtonStyle::Primary)],
            );
        }
    }
}
