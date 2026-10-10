//! 漫画阅读页 (版式按 Kindle 重新设计, 交互对照网页版 `pages/Manga/Reader.vue`)。
//!
//! - 单页全屏: 图片按屏幕等比适配、居中, 平时不画任何栏 (漫画需要整块屏幕)。
//! - 点击: 左 30% / 右 30% 翻页, 中间呼出菜单; 翻页方向可设为从右往左 (日漫), 此时左侧是下一页、
//!   向右滑是下一页。下滑呼出 / 上滑收起菜单; 长按打开放大预览 (复用插图预览页); 翻页键。
//! - 菜单: 顶栏 (返回 / 书名 / 主页) + 底部面板 (话名与页码、进度条、上一话 / 目录 / 放大 / 下一话、
//!   翻页方向 / 每页全刷 / 设置)。
//! - 图片地址按 6 张一批向服务器要 (与网页版一致), 清单写缓存, 离线可读已缓存的页 (见 `crate::comic`)。
//!   空闲时预取并解码前后页, 地址提前一批预取 (后台优先级)。
//! - 刷新: 默认每页闪刷 (漫画大面积灰阶, 局部刷新残影明显); 关掉后走翻页刷新 (REAGL + 残影预算, 有翻页动画)。
//!   翻到还没解码好的页时先画提示, 图片到了再按翻页的方式刷新。
//! - 进度: 本地 `progress/comic-<bid>.json` + 书架的 "继续阅读"; 服务器 `SaveReadPosition`
//!   (XPath = 1 起的页码, 与网页版一致), 上传时机与小说阅读页相同: 退出、休眠时上传, 换话时低优先级上传,
//!   打开目录 / 放大 / 设置时不上传, 位置没变不重复上传。

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use kn_platform::{GestureKind, InputEvent, KeyCode, SwipeDir};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::reader::{Entry, LAST_UPLOAD};
use crate::api;
use crate::comic::{self, ComicBook, ComicProgress, Manifest};
use crate::store;
use crate::KinNovel;

/// 进度落盘节流 (与小说阅读页一致), 离开/休眠时强制落盘。
const SAVE_INTERVAL: Duration = Duration::from_secs(5);
/// 保留解码结果的范围 (当前页前后几页, 每页 ~2 MB)。
const KEEP_RADIUS: i64 = 3;

const HIT_BACK: HitId = HitId(1);
const HIT_HOME: HitId = HitId(2);
const HIT_PREV_CHAPTER: HitId = HitId(3);
const HIT_NEXT_CHAPTER: HitId = HitId(4);
const HIT_TOC: HitId = HitId(5);
const HIT_ZOOM: HitId = HitId(6);
const HIT_DIRECTION: HitId = HitId(7);
const HIT_FLASH: HitId = HitId(8);
const HIT_SETTINGS: HitId = HitId(9);
const HIT_PANEL: HitId = HitId(10);

/// 离开阅读页的原因 (决定是否上传阅读进度, 与小说阅读页相同)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leave {
    Exit,
    Chapter,
    Child,
}

enum Status {
    Loading,
    Failed(String),
    Ready,
}

enum Slot {
    Pending,
    Ready(Bitmap),
    /// 离线且未缓存
    Missing,
    Failed(String),
}

struct Loaded {
    book: Option<ComicBook>,
    manifest: Manifest,
    page: usize,
}

struct BatchLoaded {
    chapter_id: i64,
    skip: usize,
    result: Result<Manifest, String>,
}

struct PageLoaded {
    chapter_id: i64,
    index: usize,
    result: Result<Option<Bitmap>, String>,
}

pub struct ComicReaderPage {
    book_id: i64,
    chapter_id: i64,
    entry: Entry,
    status: Status,
    book: Option<ComicBook>,
    manifest: Manifest,
    page: usize,
    slots: HashMap<usize, Slot>,
    /// 正在请求的地址批次 (起点)
    batches: HashSet<usize>,
    chrome: bool,
    note: String,
    /// 当前页的图片到达时按翻页的方式刷新 (翻到未就绪的页、刚进入时)
    flash_pending: bool,
    next_leave: Leave,
    last_saved: Option<Instant>,
}

impl ComicReaderPage {
    pub fn new(book_id: i64, chapter_id: i64, entry: Entry) -> Self {
        ComicReaderPage {
            book_id,
            chapter_id,
            entry,
            status: Status::Loading,
            book: None,
            manifest: Manifest::default(),
            page: 0,
            slots: HashMap::new(),
            batches: HashSet::new(),
            chrome: false,
            note: String::new(),
            flash_pending: true,
            next_leave: Leave::Exit,
            last_saved: None,
        }
    }

    fn rtl(cx: &Cx<KinNovel>) -> bool {
        cx.app.config.string("comic_direction", "ltr") == "rtl"
    }

    fn online(cx: &Cx<KinNovel>) -> bool {
        cx.app.net().is_some() || api::fake_mode()
    }

    fn total(&self) -> usize {
        self.manifest.total()
    }

    fn chapter_title(&self) -> String {
        let title = self.manifest.title.clone();
        let from_book = || self.book.as_ref().and_then(|b| b.index_of(self.chapter_id).map(|i| b.chapters[i].title.clone()));
        if !title.is_empty() { title } else { from_book().unwrap_or_default() }
    }

    fn book_title(&self) -> String {
        if !self.manifest.book_name.is_empty() {
            return self.manifest.book_name.clone();
        }
        self.book.as_ref().map(|b| b.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "漫画".into())
    }

    // ---- 加载 ----

    fn start_load(&mut self, cx: &mut Cx<KinNovel>) {
        let paths = cx.app.paths.clone();
        let net = cx.app.net();
        let (book_id, chapter_id, entry) = (self.book_id, self.chapter_id, self.entry);
        let local = comic::load_progress(&paths, book_id);
        cx.spawn(move || -> Result<Loaded, String> {
            let started = Instant::now();
            let book = comic::load_book(&paths, net.as_ref(), book_id);
            let mut server = None;
            let mut manifest = match Manifest::load(&paths, chapter_id) {
                Some(m) if m.total() > 0 => m,
                _ => {
                    let (m, position) = comic::fetch_batch(&paths, net.as_ref(), chapter_id, 0, 0)?;
                    server = position;
                    m
                }
            };
            if manifest.total() == 0 {
                return Err("本话没有图片".into());
            }
            let last = manifest.total() - 1;
            let page = match entry {
                Entry::First => 0,
                Entry::Last => last,
                // 本地与服务器 (仅当二者指向本话) 取更靠后的, 与小说阅读页一致
                Entry::Resume => {
                    let local = local.filter(|p| p.chapter_id == chapter_id).map(|p| p.page);
                    let server = server.filter(|(c, _)| *c == chapter_id).map(|(_, p)| p.saturating_sub(1));
                    local.max(server).unwrap_or(0)
                }
            }
            .min(last);
            if manifest.needs_batch(page) {
                match comic::fetch_batch(&paths, net.as_ref(), chapter_id, Manifest::batch_start(page), 0) {
                    Ok((m, _)) => manifest = m,
                    Err(e) => eprintln!("[comic] 第 {} 页所在批次: {e}", page + 1),
                }
            }
            eprintln!("[comic] 打开 {book_id}/{chapter_id}: {} 页, 第 {} 页, {} ms", manifest.total(), page + 1, started.elapsed().as_millis());
            Ok(Loaded { book, manifest, page })
        });
    }

    fn request_batch(&mut self, cx: &mut Cx<KinNovel>, skip: usize, priority: i32) {
        if !self.batches.insert(skip) {
            return;
        }
        let paths = cx.app.paths.clone();
        let net = cx.app.net();
        let chapter_id = self.chapter_id;
        cx.spawn(move || BatchLoaded { chapter_id, skip, result: comic::fetch_batch(&paths, net.as_ref(), chapter_id, skip, priority).map(|(m, _)| m) });
    }

    /// 解码第 `index` 页 (地址未知时先要地址所在的批次)。
    fn request_page(&mut self, cx: &mut Cx<KinNovel>, index: usize, priority: i32) {
        if index >= self.total() || self.slots.contains_key(&index) {
            return;
        }
        let Some(url) = self.manifest.url(index).map(str::to_string) else {
            if self.manifest.needs_batch(index) {
                if Self::online(cx) {
                    self.request_batch(cx, Manifest::batch_start(index), priority);
                } else {
                    self.slots.insert(index, Slot::Missing);
                }
            } else {
                self.slots.insert(index, Slot::Failed("图片地址为空".into()));
            }
            return;
        };
        self.slots.insert(index, Slot::Pending);
        let paths = cx.app.paths.clone();
        let net = cx.app.net();
        let (w, h) = (cx.width, cx.height);
        let chapter_id = self.chapter_id;
        cx.spawn(move || {
            let height = comic::request_height(&url, w, h);
            let result = comic::page_bytes(&paths, net.as_ref(), &url, height).and_then(|bytes| match bytes {
                None => Ok(None),
                Some(bytes) => kn_render::decode_gray(&bytes, Some((w, h))).map(|img| Some(img.fit_within(w, h))).map_err(|e| e.to_string()),
            });
            PageLoaded { chapter_id, index, result }
        });
    }

    fn on_loaded(&mut self, cx: &mut Cx<KinNovel>, loaded: Loaded) {
        self.book = loaded.book;
        self.manifest = loaded.manifest;
        self.page = loaded.page;
        self.status = Status::Ready;
        self.flash_pending = true;
        self.request_page(cx, self.page, 0);
        self.save_progress(cx, true);
        cx.request_redraw(RefreshHint::Ui);
    }

    fn on_batch(&mut self, cx: &mut Cx<KinNovel>, b: BatchLoaded) {
        self.batches.remove(&b.skip);
        if b.chapter_id != self.chapter_id {
            return;
        }
        let range = b.skip..b.skip + comic::BATCH;
        match b.result {
            Ok(m) => {
                self.manifest = m;
                if range.contains(&self.page) {
                    self.request_page(cx, self.page, 0);
                }
            }
            Err(e) => {
                eprintln!("[comic] 地址批次 {}: {e}", b.skip);
                for i in range.clone() {
                    self.slots.entry(i).or_insert_with(|| Slot::Failed(e.clone()));
                }
                if range.contains(&self.page) {
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }

    fn on_page(&mut self, cx: &mut Cx<KinNovel>, p: PageLoaded) {
        if p.chapter_id != self.chapter_id || (p.index as i64 - self.page as i64).abs() > KEEP_RADIUS {
            self.slots.remove(&p.index);
            return;
        }
        let slot = match p.result {
            Ok(Some(img)) => Slot::Ready(img),
            Ok(None) => Slot::Missing,
            Err(e) => {
                eprintln!("[comic] 第 {} 页: {e}", p.index + 1);
                Slot::Failed(e)
            }
        };
        let ready = matches!(slot, Slot::Ready(_));
        self.slots.insert(p.index, slot);
        if p.index == self.page {
            if ready && std::mem::take(&mut self.flash_pending) {
                self.present(cx, 0);
            } else {
                cx.request_redraw(RefreshHint::Ui);
            }
        }
    }

    // ---- 翻页 ----

    /// 刷新当前页: 每页全刷时闪刷, 否则走翻页刷新 (`delta` 决定翻页动画方向, 0 = 无动画)。
    fn present(&self, cx: &mut Cx<KinNovel>, delta: i64) {
        if cx.app.config.bool("comic_page_flash", true) {
            cx.request_redraw(RefreshHint::Flash);
            return;
        }
        let animate = delta != 0 && cx.app.config.bool("page_turn_animation", true);
        // 向后翻: 从左往右读时内容左移, 从右往左读时右移
        let swipe = animate.then(|| if (delta > 0) != Self::rtl(cx) { SwipeDir::Left } else { SwipeDir::Right });
        cx.request_turn(swipe);
    }

    /// `delta` 是逻辑方向: +1 = 下一页 (与屏幕左右无关)。
    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        if !matches!(self.status, Status::Ready) {
            return Transition::None;
        }
        let target = self.page as i64 + delta;
        if target < 0 {
            return self.change_chapter(cx, -1);
        }
        if target as usize >= self.total() {
            return self.change_chapter(cx, 1);
        }
        self.page = target as usize;
        self.note.clear();
        let page = self.page as i64;
        self.slots.retain(|i, s| (*i as i64 - page).abs() <= KEEP_RADIUS || matches!(s, Slot::Pending));
        self.request_page(cx, self.page, 0);
        self.save_progress(cx, false);
        if matches!(self.slots.get(&self.page), Some(Slot::Ready(_))) {
            self.flash_pending = false;
            self.present(cx, delta);
        } else {
            self.flash_pending = true;
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }

    fn change_chapter(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        let target = self.book.as_ref().and_then(|b| {
            let i = b.index_of(self.chapter_id)? as i64 + delta;
            (i >= 0).then(|| b.chapters.get(i as usize).cloned()).flatten()
        });
        let Some(target) = target else {
            self.note = if delta < 0 { "已经是第一话".into() } else { "已经是最后一话".into() };
            self.chrome = true;
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        };
        if !Self::online(cx) && Manifest::load(&cx.app.paths, target.id).is_none() {
            let name = if target.title.is_empty() { format!("第 {} 话", target.sort_num) } else { target.title.clone() };
            self.note = format!("「{name}」尚未缓存 (离线)");
            self.chrome = true;
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        self.save_progress(cx, true);
        self.next_leave = Leave::Chapter;
        let entry = if delta < 0 { Entry::Last } else { Entry::First };
        Transition::Replace(Box::new(ComicReaderPage::new(self.book_id, target.id, entry)))
    }

    // ---- 进度 ----

    fn save_progress(&mut self, cx: &mut Cx<KinNovel>, force: bool) {
        if !matches!(self.status, Status::Ready) || self.total() == 0 {
            return;
        }
        if !force && self.last_saved.is_some_and(|t| t.elapsed() < SAVE_INTERVAL) {
            return;
        }
        self.last_saved = Some(Instant::now());
        let now = store::unix_now();
        let progress = ComicProgress { chapter_id: self.chapter_id, page: self.page, total: self.total(), time: now };
        if let Err(e) = comic::save_progress(&cx.app.paths, self.book_id, &progress) {
            eprintln!("[comic] 进度保存失败: {e}");
        }
        let sort_num = match self.manifest.sort_num {
            n if n > 0 => n,
            _ => self.book.as_ref().and_then(|b| b.index_of(self.chapter_id)).map(|i| i as i64 + 1).unwrap_or(1),
        };
        let last = store::LastRead {
            book_id: self.book_id,
            sort_num,
            comic: true,
            chapter_id: self.chapter_id,
            book_name: self.book_title(),
            chapter_title: self.chapter_title(),
            page: self.page,
            pages: self.total(),
            time: now,
        };
        if let Err(e) = store::save_last_read(&cx.app.paths, &last) {
            eprintln!("[comic] 最近阅读保存失败: {e}");
        }
    }

    /// 服务器阅读位置: XPath 填 1 起的页码 (网页版 `saveReadPosition({Bid, Cid, XPath: String(page)})`)。
    fn upload_position(&mut self, cx: &mut Cx<KinNovel>, priority: i32) {
        let Some(net) = cx.app.net() else { return };
        if net.user().is_none() || net.server_down() || self.total() == 0 || !matches!(self.status, Status::Ready) {
            return;
        }
        let key = (self.book_id, self.chapter_id, (self.page + 1).to_string());
        if LAST_UPLOAD.lock().unwrap_or_else(|e| e.into_inner()).as_ref() == Some(&key) {
            return;
        }
        eprintln!("[comic] 上传进度 {}#{} 第 {} 页 (优先级 {priority})", key.0, key.1, key.2);
        std::thread::spawn(move || match net.save_read_position(key.0, key.1, &key.2, priority) {
            Ok(_) => *LAST_UPLOAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(key),
            Err(e) => eprintln!("[comic] 进度上传失败: {e}"),
        });
    }

    // ---- 菜单 ----

    fn set_chrome(&mut self, cx: &mut Cx<KinNovel>, visible: bool) {
        if self.chrome != visible {
            self.chrome = visible;
            if !visible {
                self.note.clear();
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn zoom(&mut self, cx: &mut Cx<KinNovel>) -> Transition<KinNovel> {
        let Some(url) = self.manifest.url(self.page).map(str::to_string) else { return Transition::None };
        let height = comic::request_height(&url, cx.width, cx.height);
        self.next_leave = Leave::Child;
        Transition::Push(Box::new(super::image::ImagePage::comic(url, height)))
    }

    fn on_chrome_hit(&mut self, cx: &mut Cx<KinNovel>, id: HitId) -> Transition<KinNovel> {
        match id {
            HIT_BACK => Transition::Back,
            HIT_HOME => Transition::Home,
            HIT_PREV_CHAPTER => self.change_chapter(cx, -1),
            HIT_NEXT_CHAPTER => self.change_chapter(cx, 1),
            HIT_TOC => {
                let Some(book) = &self.book else { return Transition::None };
                self.next_leave = Leave::Child;
                Transition::Push(Box::new(super::book::CatalogPage::comic(self.book_id, book.chapters.clone(), Some(self.chapter_id))))
            }
            HIT_ZOOM => self.zoom(cx),
            HIT_DIRECTION => {
                let next = if Self::rtl(cx) { "ltr" } else { "rtl" };
                cx.app.config.set("comic_direction", serde_json::Value::from(next));
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_FLASH => {
                let on = cx.app.config.bool("comic_page_flash", true);
                cx.app.config.set("comic_page_flash", serde_json::Value::from(!on));
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_SETTINGS => {
                self.next_leave = Leave::Child;
                Transition::Push(Box::new(super::settings::SettingsPage::comic()))
            }
            _ => Transition::None,
        }
    }

    fn draw_message(cx: &mut Cx<KinNovel>, frame: &mut Bitmap, text: &str) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let lines: Vec<&str> = text.lines().collect();
        let mut y = cx.height as i32 / 2 - (lines.len() as f32 * m.body * 0.8) as i32;
        for line in lines {
            let line = ink.fit(line, m.body, (cx.width - 2 * m.margin) as f32);
            ink.text_centered(frame, Rect::new(m.margin as i32, y, cx.width - 2 * m.margin, (m.body * 1.5) as u32), &line, m.body, theme.foreground);
            y += (m.body * 1.6) as i32;
        }
    }

    fn draw_chrome(&self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let (w, h) = (cx.width, cx.height);
        let rtl = Self::rtl(cx);
        let flash = cx.app.config.bool("comic_page_flash", true);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };

        let bar = widgets::header(&mut ink, frame, &theme, &m, &self.book_title(), "", true, true);
        frame.fill_rect(Rect::new(0, bar.bottom(), w, 2), theme.foreground);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        cx.hits.add(HIT_HOME, Rect::new(w as i32 - bar.h as i32, 0, bar.h, bar.h));

        let pad = m.margin as i32 / 2;
        let gap = (m.margin as f32 * 0.5) as i32;
        let btn_h = m.touch as i32;
        let info_h = (m.small * 1.6) as i32;
        let track_h = (10.0 * m.scale).max(8.0) as i32;
        let panel_h = pad + info_h + gap / 2 + track_h + gap + btn_h + gap + btn_h + pad;
        let panel = Rect::new(0, h as i32 - panel_h, w, panel_h as u32);
        frame.fill_rect(panel, theme.background);
        frame.fill_rect(Rect::new(0, panel.y, w, 2), theme.foreground);
        cx.hits.add(HIT_PANEL, panel).no_feedback();
        let side = m.margin as i32;
        let inner_w = w as i32 - 2 * side;

        // 话名 · 页码
        let mut y = panel.y + pad;
        let total = self.total().max(1);
        let pages = format!("{} / {} 页", (self.page + 1).min(total), total);
        let pages_w = ink.width(&pages, m.small).ceil() as i32;
        let text_y = y + (info_h - (m.small * 1.25) as i32) / 2;
        ink.text(frame, w as i32 - side - pages_w, text_y, &pages, m.small, theme.muted);
        let title = self.chapter_title();
        let label = if title.is_empty() { format!("第 {} 话", self.manifest.sort_num.max(1)) } else { title };
        let label = ink.fit(&label, m.small, (inner_w - pages_w - side / 2) as f32);
        ink.text(frame, side, text_y, &label, m.small, theme.foreground);
        y += info_h + gap / 2;

        // 话内进度条 (从右往左读时从右侧填充)
        let track = Rect::new(side, y, inner_w as u32, track_h as u32);
        let filled = ((inner_w as f32 * (self.page + 1) as f32 / total as f32) as u32).max(track_h as u32).min(inner_w as u32);
        frame.rounded_rect(track, track_h as u32 / 2, Some(theme.background), Some(theme.mid), 2);
        let fill_x = if rtl { side + inner_w - filled as i32 } else { side };
        frame.rounded_rect(Rect::new(fill_x, y, filled, track_h as u32), track_h as u32 / 2, Some(theme.foreground), None, 0);
        y += track_h + gap;

        // 上一话 / 目录 / 放大 / 下一话
        let has_book = self.book.as_ref().is_some_and(|b| b.chapters.len() > 1);
        let can_zoom = self.manifest.url(self.page).is_some();
        let cols = 4;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let cell = |i: i32, y: i32| Rect::new(side + i * (col_w + gap), y, col_w as u32, btn_h as u32);
        let style = |on: bool| if on { ButtonStyle::Secondary } else { ButtonStyle::Disabled };
        let row = [(HIT_PREV_CHAPTER, "上一话", has_book), (HIT_TOC, "目录", self.book.is_some()), (HIT_ZOOM, "放大", can_zoom), (HIT_NEXT_CHAPTER, "下一话", has_book)];
        for (i, (id, label, on)) in row.iter().enumerate() {
            let r = cell(i as i32, y);
            widgets::button(&mut ink, frame, &theme, &m, r, label, style(*on));
            cx.hits.add(*id, r).enabled(*on);
        }
        y += btn_h + gap;

        // 翻页方向 / 每页全刷 / 设置
        let cols = 3;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let cell = |i: i32| Rect::new(side + i * (col_w + gap), y, col_w as u32, btn_h as u32);
        let row = [
            (HIT_DIRECTION, if rtl { "从右往左" } else { "从左往右" }),
            (HIT_FLASH, if flash { "每页全刷: 开" } else { "每页全刷: 关" }),
            (HIT_SETTINGS, "设置"),
        ];
        for (i, (id, label)) in row.iter().enumerate() {
            widgets::button(&mut ink, frame, &theme, &m, cell(i as i32), label, ButtonStyle::Secondary);
            cx.hits.add(*id, cell(i as i32));
        }

        if !self.note.is_empty() {
            let text_w = ink.width(&self.note, m.small).ceil() as u32;
            let nw = (text_w + 2 * m.margin).min(w - 2 * m.margin);
            let nh = (m.small * 2.2) as u32;
            let r = Rect::new((w - nw) as i32 / 2, panel.y - nh as i32 - gap, nw, nh);
            frame.rounded_rect(r, nh / 2, Some(theme.foreground), None, 0);
            ink.text_centered(frame, r.inflate(-(m.margin as i32) / 2), &self.note, m.small, theme.background);
        }
    }
}

impl Page<KinNovel> for ComicReaderPage {
    fn id(&self) -> PageId {
        PageId(12)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        self.next_leave = Leave::Exit;
        if !returning && matches!(self.status, Status::Loading) {
            self.start_load(cx);
        }
    }

    fn leave(&mut self, cx: &mut Cx<KinNovel>) {
        self.save_progress(cx, true);
        match self.next_leave {
            Leave::Exit => self.upload_position(cx, 0),
            Leave::Chapter => self.upload_position(cx, crate::net::BACKGROUND_PRIORITY),
            Leave::Child => {}
        }
    }

    fn on_suspend(&mut self, cx: &mut Cx<KinNovel>) {
        self.save_progress(cx, true);
        self.upload_position(cx, 0);
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<PageLoaded>() {
            Ok(p) => return self.on_page(cx, *p),
            Err(other) => other,
        };
        let msg = match msg.downcast::<BatchLoaded>() {
            Ok(b) => return self.on_batch(cx, *b),
            Err(other) => other,
        };
        let Ok(result) = msg.downcast::<Result<Loaded, String>>() else { return };
        match *result {
            Ok(loaded) => self.on_loaded(cx, loaded),
            Err(e) => {
                self.status = Status::Failed(e);
                cx.request_redraw(RefreshHint::Flash);
            }
        }
    }

    fn opaque(&self) -> bool {
        matches!(self.status, Status::Ready)
    }

    fn flash_on_enter(&self) -> bool {
        // 图片还没就绪: 等它到了再闪刷 (on_page)
        matches!(self.status, Status::Ready) && matches!(self.slots.get(&self.page), Some(Slot::Ready(_)))
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let (w, h) = (cx.width, cx.height);
        frame.fill_rect(Rect::new(0, 0, w, h), theme.background);
        match &self.status {
            Status::Loading => Self::draw_message(cx, frame, "正在打开…"),
            Status::Failed(e) => {
                let text = format!("无法打开本话\n{e}\n\n轻触屏幕返回");
                Self::draw_message(cx, frame, &text);
            }
            Status::Ready => {
                let n = format!("{} / {}", self.page + 1, self.total());
                match self.slots.get(&self.page) {
                    Some(Slot::Ready(img)) => {
                        let x = (w as i32 - img.width() as i32) / 2;
                        let y = (h as i32 - img.height() as i32) / 2;
                        frame.blit(img, img.bounds(), x, y);
                    }
                    Some(Slot::Missing) => Self::draw_message(cx, frame, &format!("第 {n} 页尚未缓存 (离线)")),
                    Some(Slot::Failed(e)) => {
                        let text = format!("第 {n} 页无法显示\n{e}");
                        Self::draw_message(cx, frame, &text);
                    }
                    Some(Slot::Pending) | None => Self::draw_message(cx, frame, &format!("第 {n} 页加载中…")),
                }
            }
        }
        if self.chrome {
            self.draw_chrome(cx, frame);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        match event {
            InputEvent::Key { code: KeyCode::PageForward, pressed: true } => self.turn(cx, 1),
            InputEvent::Key { code: KeyCode::PageBack, pressed: true } => self.turn(cx, -1),
            InputEvent::Gesture(g) => {
                let p = Point { x: g.start.0, y: g.start.1 };
                let rtl = Self::rtl(cx);
                let forward = if rtl { -1 } else { 1 };
                match g.kind {
                    GestureKind::Tap | GestureKind::Long => {
                        if self.chrome {
                            return match cx.hits.at(p) {
                                Some(id) => self.on_chrome_hit(cx, id),
                                None => {
                                    self.set_chrome(cx, false);
                                    Transition::None
                                }
                            };
                        }
                        if matches!(self.status, Status::Failed(_)) {
                            return Transition::Back;
                        }
                        if g.kind == GestureKind::Long {
                            return self.zoom(cx);
                        }
                        let w = cx.width as i32;
                        if p.x < w * 3 / 10 {
                            self.turn(cx, -forward)
                        } else if p.x > w * 7 / 10 {
                            self.turn(cx, forward)
                        } else {
                            self.set_chrome(cx, true);
                            Transition::None
                        }
                    }
                    // 手指向左滑: 从左往右读是下一页, 从右往左读是上一页
                    GestureKind::SwipeLeft => {
                        self.chrome = false;
                        self.turn(cx, forward)
                    }
                    GestureKind::SwipeRight => {
                        self.chrome = false;
                        self.turn(cx, -forward)
                    }
                    GestureKind::SwipeDown => {
                        self.set_chrome(cx, true);
                        Transition::None
                    }
                    GestureKind::SwipeUp => {
                        self.set_chrome(cx, false);
                        Transition::None
                    }
                    GestureKind::Down => Transition::None,
                }
            }
            _ => Transition::None,
        }
    }

    fn on_idle(&mut self, cx: &mut Cx<KinNovel>) -> bool {
        if !matches!(self.status, Status::Ready) {
            return false;
        }
        // 先解码前后页 (翻页只需拷贝), 再提前要下一批地址
        let page = self.page;
        for index in [page + 1, page + 2, page.wrapping_sub(1)] {
            if index < self.total() && !self.slots.contains_key(&index) {
                self.request_page(cx, index, crate::net::BACKGROUND_PRIORITY);
                // 地址还在路上时没有占位, 不算做了事 (否则空闲循环会一直空转)
                if self.slots.contains_key(&index) {
                    return true;
                }
            }
        }
        let ahead = page + comic::BATCH / 2 + 1;
        if Self::online(cx) && self.manifest.needs_batch(ahead) {
            self.request_batch(cx, Manifest::batch_start(ahead), crate::net::BACKGROUND_PRIORITY);
        }
        false
    }
}
