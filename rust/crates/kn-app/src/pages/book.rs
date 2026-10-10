//! 书籍详情 (对照 Python `book.py`, 版式按 UI-DESIGN 重新设计) + 目录 (章节列表)。
//! 两者都是压栈页面: 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::comments::CommentsPage;
use super::reader::{Entry, ReaderPage};
use super::series::SeriesPage;
use crate::api::{self, BookInfo, ChapterRef};
use crate::covers::CoverCache;
use crate::store;
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_READ: HitId = HitId(2);
const HIT_CATALOG: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_COMMENTS: HitId = HitId(5);
const HIT_SERIES: HitId = HitId(6);

enum Status {
    Loading,
    Offline,
    Error(String),
    Ready(BookInfo),
}

struct Loaded(Result<BookInfo, String>);

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

pub struct BookDetailPage {
    book_id: i64,
    status: Status,
    covers: CoverCache,
}

impl BookDetailPage {
    pub fn new(book_id: i64) -> Self {
        BookDetailPage { book_id, status: Status::Loading, covers: CoverCache::default() }
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
        if let Ok(l) = msg.downcast::<Loaded>() {
            self.status = match l.0 {
                Ok(info) => Status::Ready(info),
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
            resume_sort_num(cx, info)
        } else {
            None
        };

        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "书籍详情", &super::status_text(), true, false);
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
        let author = if info.book.author.is_empty() { "未知作者".to_string() } else { info.book.author.clone() };
        let author = ink.fit(&author, m.small, info_w);
        ink.text(frame, info_x, y, &author, m.small, theme.muted);
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
            let tags = ink.fit(&format!("标签: {}", info.tags.join("、")), m.small, info_w);
            ink.text(frame, info_x, y, &tags, m.small, theme.muted);
            y += (m.small * 1.5) as i32;
        }

        // 简介 (限几行, 超宽截断, 不做真正的折行以保持简单可靠)
        let intro_y = (cover_rect.bottom() + m.margin as i32 / 2).max(y + m.margin as i32 / 2);
        ink.text(frame, side, intro_y, "简介", m.body, theme.foreground);
        let mut iy = intro_y + (m.body * 1.4) as i32;
        let intro_w = (cx.width - 2 * m.margin) as f32;
        for line in wrap_plain(&info.intro, &ink, m.small, intro_w, 4) {
            ink.text(frame, side, iy, &line, m.small, theme.muted);
            iy += (m.small * 1.4) as i32;
        }

        let btn_h = m.touch as i32;
        let gap = m.margin as i32 / 3;
        let bottom_y = cx.height as i32 - btn_h - m.margin as i32 / 2;
        let cols = 3;
        let col_w = (cx.width as i32 - 2 * side - gap * (cols - 1)) / cols;
        let has_chapters = !info.chapters.is_empty();
        let read_label = if resume.is_some() { "继续阅读" } else { "开始阅读" };
        let read_rect = Rect::new(side, bottom_y, col_w as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, read_rect, read_label, if has_chapters { ButtonStyle::Primary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_READ, read_rect).enabled(has_chapters);
        let catalog_rect = Rect::new(read_rect.right() + gap, bottom_y, col_w as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, catalog_rect, "目录", if has_chapters { ButtonStyle::Secondary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_CATALOG, catalog_rect).enabled(has_chapters);
        let comments_rect = Rect::new(catalog_rect.right() + gap, bottom_y, col_w as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, comments_rect, "评论", ButtonStyle::Secondary);
        cx.hits.add(HIT_COMMENTS, comments_rect);
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
                let sort_num = resume_sort_num(cx, info).or_else(|| info.chapters.first().map(|c| c.sort_num));
                match sort_num {
                    Some(sort_num) => Transition::Push(Box::new(ReaderPage::new(self.book_id, sort_num, Entry::Resume))),
                    None => Transition::None,
                }
            }
            HIT_CATALOG => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                let current = resume_sort_num(cx, info);
                Transition::Push(Box::new(CatalogPage::new(self.book_id, info.chapters.clone(), current)))
            }
            HIT_COMMENTS => Transition::Push(Box::new(CommentsPage::new("Book", self.book_id))),
            HIT_SERIES => {
                let Status::Ready(info) = &self.status else { return Transition::None };
                if info.series_name.is_empty() {
                    return Transition::None;
                }
                Transition::Push(Box::new(SeriesPage::new(info.series_name.clone(), self.book_id)))
            }
            _ => Transition::None,
        }
    }
}

/// 极简折行: 按字符宽度累计换行 (不做真正的 kinsoku/标点规则, 够用于简介这种短文本)。
fn wrap_plain(text: &str, ink: &Ink, size: f32, max_width: f32, max_lines: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let mut candidate = current.clone();
        candidate.push(ch);
        if ink.width(&candidate, size) > max_width && !current.is_empty() {
            lines.push(current);
            current = String::new();
            if lines.len() == max_lines {
                return lines;
            }
        }
        current.push(ch);
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    if lines.len() == max_lines {
        if let Some(last) = lines.last_mut() {
            if ink.width(last, size) > max_width {
                *last = ink.fit(last, size, max_width);
            } else {
                last.push('…');
            }
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// 目录
// ---------------------------------------------------------------------------

const CAT_HIT_BACK: HitId = HitId(1);
const CAT_HIT_PREV: HitId = HitId(2);
const CAT_HIT_NEXT: HitId = HitId(3);
const CAT_ROW_BASE: u32 = 1000;

pub struct CatalogPage {
    book_id: i64,
    chapters: Vec<ChapterRef>,
    current: Option<i64>,
    page: usize,
    /// 首次渲染时跳到当前章节所在页
    jumped: bool,
}

impl CatalogPage {
    pub fn new(book_id: i64, chapters: Vec<ChapterRef>, current: Option<i64>) -> Self {
        CatalogPage { book_id, chapters, current, page: 0, jumped: false }
    }
}

impl Page<KinNovel> for CatalogPage {
    fn id(&self) -> PageId {
        PageId(201)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "目录", &super::status_text(), true, false);
        cx.hits.add(CAT_HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        if self.chapters.is_empty() {
            let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无章节", None);
            return;
        }
        let row_h = m.row_h();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        let per_page = ((list_bottom - bar.bottom()) / row_h as i32).max(1) as usize;
        let pages = self.chapters.len().div_ceil(per_page).max(1);
        if !self.jumped {
            self.jumped = true;
            if let Some(i) = self.current.and_then(|c| self.chapters.iter().position(|ch| ch.sort_num == c)) {
                self.page = i / per_page;
            }
        }
        self.page = self.page.min(pages - 1);
        for (row, (index, ch)) in self.chapters.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
            let r = Rect::new(0, bar.bottom() + row as i32 * row_h as i32, cx.width, row_h);
            let title = if ch.title.is_empty() { format!("第 {} 章", ch.sort_num) } else { ch.title.clone() };
            let meta = if self.current == Some(ch.sort_num) { "当前" } else { "" };
            widgets::list_row(&mut ink, frame, &theme, &m, r, &title, "", meta);
            cx.hits.add(HitId(CAT_ROW_BASE + index as u32), r);
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, CAT_HIT_PREV, CAT_HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match g.kind {
            GestureKind::Tap => match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
                Some(CAT_HIT_BACK) => Transition::Back,
                Some(CAT_HIT_PREV) => self.turn(cx, -1),
                Some(CAT_HIT_NEXT) => self.turn(cx, 1),
                Some(HitId(id)) if id >= CAT_ROW_BASE => match self.chapters.get((id - CAT_ROW_BASE) as usize) {
                    Some(ch) => Transition::Push(Box::new(ReaderPage::new(self.book_id, ch.sort_num, Entry::Resume))),
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
        let next = self.page as i64 + delta;
        if next >= 0 {
            self.page = next as usize;
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }
}
