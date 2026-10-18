//! 阅读页。
//!
//! 版式 (1.0 重新设计, 不沿用 Python 版):
//! - 正文铺满全屏; 顶部细栏 (高 3.5% 屏): 左边章节名后接页码, 右边时间 · 电量; 底边一条细进度线。
//! - 点击: 插图 (仅图片实际绘制区域) 进入全屏预览; 注释图标「注」弹出注释卡片 (点任意处关闭); 其余左 30% 上一页, 右 30% 下一页, 中间呼出控件层;
//!   顶栏 (时间/电量那一条) 不响应点击; 左右滑动翻页; 下滑呼出 / 上滑收起控件层; 翻页键。
//! - 控件层是覆盖在正文上的浮层 (不触发重排): 顶栏 (返回 / 书名 / 书签 / 主页) + 底部面板
//!   (章节信息与进度条 [点按/滑动跳页]、上一章 / 目录 / 设置 / 下一章、字号 − / + 、日夜切换), 按钮 ≥ 9 mm。
//! - 书签: 控件层顶栏的书签按钮切换当前页的书签 (存页首锚点, 改字号后仍准确); 有书签的页右上角显示书签角标;
//!   目录页的「书签」标签列出全书书签。从阅读页打开的目录选章/选书签后回到本页就地跳转 (`ReaderJump`)。
//!   从设置返回时: 排版参数变了就重排 (保持阅读位置), 简繁转换变了就重新加载章节 (同样保持位置)。
//!
//! 性能结构:
//! - 章节 JSON 解析、HTML 分块、字体读取 (WOFF2 解码只做一次, 结果旁路缓存为 TTF) 在后台线程。
//! - 分页在 UI 线程渐进进行: 进入时只排到目标页 (有阅读进度时排完整章以便定位), 其余在空闲时每次一页。
//! - 正文按页渲染成位图并缓存 (当前页 ± 1, 空闲时预渲染), 翻页 = 拷贝位图 + 画顶栏, 走 REAGL 整屏刷新。
//! - 正文按网页版的样式排 (kn-text 网页样式模式: 粗体、字号、对齐、引用、分隔线、注音、注释等);
//!   块划分与码点偏移和 Python 版一致, 阅读进度 (XPath + 码点偏移) 与 Python 版、网页版通用。

use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use kn_platform::{GestureKind, InputEvent, KeyCode, SwipeDir};
use kn_render::{Bitmap, FontId, Point, Rect, Synth, TextStyle};
use kn_text::{first_anchor_on_page, page_for_path, Block, FontMeasure, Inline, LayoutParams, Page as TextPage, PageItem, Paginator, RichLine, WidthCache, OBJECT_CHAR};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Orientation, Page, PageId, RefreshHint, Transition};

use crate::store::{self, Chapter, Progress};
use crate::userdata::{self, Bookmark};
use crate::KinNovel;

/// 进入章节时停在哪里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// 本地/服务器进度 (取更靠后的), 没有则第一页
    Resume,
    First,
    /// 从下一章往回翻进来
    Last,
}

/// 正文字号 (像素) 的 (最小, 最大, 默认, 步长): 300 ppi 下 28 / 80 / 48 / 4, 其它 ppi 按比例换算。
/// 已保存的字号原样使用 (只钳到范围内), 与 Python 版配置兼容。
pub fn font_range(app: &KinNovel) -> (i64, i64, i64, i64) {
    (app.dpi_px(28.0), app.dpi_px(80.0), app.dpi_px(48.0), app.dpi_px(4.0).max(1))
}

/// 页边距默认值 (300 ppi 下 34 px)。
pub fn default_margin(app: &KinNovel) -> i64 {
    app.dpi_px(34.0)
}

/// 进度落盘节流 (与 Python 版一致), 离开/休眠时强制落盘。
const SAVE_INTERVAL_SECS: u64 = 5;
/// 缓存的正文位图页数 (每页 ~2 MB)。
const PAGE_CACHE: usize = 4;

const HIT_BACK: HitId = HitId(1);
const HIT_HOME: HitId = HitId(2);
const HIT_PREV_CHAPTER: HitId = HitId(3);
const HIT_NEXT_CHAPTER: HitId = HitId(4);
const HIT_TOC: HitId = HitId(5);
const HIT_FONT_DOWN: HitId = HitId(6);
const HIT_FONT_UP: HitId = HitId(7);
const HIT_NIGHT: HitId = HitId(8);
const HIT_PANEL: HitId = HitId(9);
const HIT_SETTINGS: HitId = HitId(10);
const HIT_BOOKMARK: HitId = HitId(11);
const HIT_TRACK: HitId = HitId(12);
const HIT_ORIENTATION: HitId = HitId(13);

/// 书签摘录的字数
const EXCERPT_CHARS: usize = 40;

/// 从阅读页打开的目录里选了章节或书签: 回到阅读页后就地跳转。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderJump {
    pub book_id: i64,
    pub sort_num: i64,
    /// None = 章节的阅读进度 (与从目录打开章节相同)
    pub at: Option<Progress>,
}

/// 离开阅读页的原因 (决定是否上传阅读进度)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leave {
    /// 退出阅读界面 (返回、回主页、退出): 上传
    Exit,
    /// 换章: 上传, 但用后台优先级 (让位给用户正在等的请求)
    Chapter,
    /// 打开目录 / 插图预览, 之后还会回到阅读页: 不上传
    Child,
}

/// 最近一次上传成功的 (书, 章节, XPath): 位置没变就不再上传。
/// 换章会新建阅读页实例, 所以放在模块级。
/// 漫画阅读页共用 (键里的 XPath 换成页码)。
pub(super) static LAST_UPLOAD: Mutex<Option<(i64, i64, String)>> = Mutex::new(None);

/// 后台加载结果。
struct Loaded {
    chapter: Chapter,
    blocks: Vec<Block>,
    notes: Vec<String>,
    font_path: Option<PathBuf>,
    /// None = 不需要读 (与已加载的章节字体相同) 或没有字体
    font_bytes: Option<Result<Vec<u8>, String>>,
    local: Option<Progress>,
    millis: u128,
}

/// 插图状态 (按 URL)。
enum ImageSlot {
    Pending,
    Ready(Bitmap),
    /// 本地没有缓存 (下载要等 kn-net)
    Missing,
    /// 失败的时刻: 过了 `crate::net::RETRY_AFTER` 再显示到这页时重试
    Failed(Instant),
}

/// 后台解码结果。
struct ImageLoaded {
    /// 请求的排版框尺寸 (横竖屏切换后旧尺寸的结果作废)
    size: (u32, u32),
    url: String,
    result: Result<Option<Bitmap>, String>,
}

enum Status {
    Loading,
    Failed(String),
    Ready,
}

pub struct ReaderPage {
    book_id: i64,
    sort_num: i64,
    entry: Entry,
    status: Status,
    chapter: Chapter,
    blocks: Vec<Block>,
    /// 本章注释内容 (点击正文里的注释标记弹出), 下标 = 序号 - 1
    notes: Vec<String>,
    /// 当前页注释标记的点击区域 (屏幕坐标) 与注释序号
    note_hits: Vec<(Rect, u16)>,
    /// 正在显示的注释 (序号)
    footnote: Option<u16>,
    chain: Vec<FontId>,
    widths: WidthCache,
    params: Option<LayoutParams>,
    paginator: Option<Paginator>,
    pages: Vec<TextPage>,
    page: usize,
    /// (页号, 正文位图), 最近使用的在末尾
    cache: Vec<(usize, Bitmap)>,
    images: HashMap<String, ImageSlot>,
    /// 当前页已显示插图的实际绘制区域 (屏幕坐标) 与 (URL, 请求高度), 点击预览用
    image_hits: Vec<(Rect, String, u32)>,
    chrome: bool,
    /// 下一次 `leave` 的原因 (返回 Push/Replace 前设置, 回到本页时复位)
    next_leave: Leave,
    /// 加载章节时的简繁转换设置 (从设置页返回时比较)
    convert: String,
    /// 重新加载 (简繁转换变化) 时要回到的位置, 优先于本地/服务器进度
    resume_override: Option<Progress>,
    /// 控件层上方的提示 (例如 "下一章尚未缓存")
    note: String,
    saved: Option<Progress>,
    last_write: Option<Instant>,
    /// 本书的书签 (进入时读取, 从目录返回时重读)
    bookmarks: Vec<Bookmark>,
    /// 控件层进度条的位置 (点按/滑动跳页时换算页号)
    track: Rect,
    /// 阅读统计计时
    tracker: crate::stats::Tracker,
}

impl ReaderPage {
    pub fn new(book_id: i64, sort_num: i64, entry: Entry) -> Self {
        ReaderPage {
            book_id,
            sort_num,
            entry,
            status: Status::Loading,
            chapter: Chapter::default(),
            blocks: Vec::new(),
            notes: Vec::new(),
            note_hits: Vec::new(),
            footnote: None,
            chain: Vec::new(),
            widths: WidthCache::default(),
            params: None,
            paginator: None,
            pages: Vec::new(),
            page: 0,
            cache: Vec::new(),
            images: HashMap::new(),
            image_hits: Vec::new(),
            chrome: false,
            next_leave: Leave::Exit,
            convert: String::new(),
            resume_override: None,
            note: String::new(),
            saved: None,
            last_write: None,
            bookmarks: Vec::new(),
            track: Rect::new(0, 0, 0, 0),
            tracker: crate::stats::Tracker::default(),
        }
    }

    fn stop_tracking(&mut self, cx: &mut Cx<KinNovel>) {
        self.tracker.stop(&cx.app.paths, self.book_id, &self.chapter.book_name, false);
    }

    /// 顶栏高度与正文区域高度 (正文区域从 top 到屏幕底; 底部进度线画在正文的下边距里)。
    fn geometry(width: u32, height: u32) -> (i32, u32) {
        let _ = width;
        let top = ((height as f32 * 0.035) as i32).max(40);
        (top, height - top as u32)
    }

    fn layout_params(cx: &Cx<KinNovel>, sort_num: i64) -> LayoutParams {
        let (_, content_h) = Self::geometry(cx.width, cx.height);
        let c = &cx.app.config;
        let (min_font, max_font, default_font, _) = font_range(cx.app);
        LayoutParams {
            width: cx.width as i64,
            height: content_h as i64,
            font_size: c.int("font_size", default_font).clamp(min_font, max_font) as u32,
            line_spacing: c.float("line_spacing", 1.42),
            margin: c.int("reader_margin", default_margin(cx.app)),
            first_line_indent: c.bool("first_line_indent", true),
            paragraph_spacing: c.float("paragraph_spacing", 0.2),
            // 只有封面 (每卷第一章开头的第一张图) 占满一页; 彩页和插图按真实尺寸与正文混排,
            // 高度不超过正文区的 62% (同 Python 版), 放不下当前页剩余空间时另起一页
            image_max_ratio: kn_text::PYTHON_IMAGE_MAX_RATIO,
            cover_full_page: sort_num == 1,
            real_image_size: true,
            // 」』）不放行首, 「…了。」 不会把 」 留给下一行
            strict_kinsoku: true,
            // 粗体、字号、居中、引用、分隔线、注释弹窗等按网页版的样式排 (kn_text::style)
            rich: true,
        }
    }

    fn done(&self) -> bool {
        self.paginator.is_none()
    }

    /// 再排一页; 整章排完返回 false。
    fn paginate_step(&mut self, cx: &mut Cx<KinNovel>) -> bool {
        let Some(paginator) = self.paginator.as_mut() else {
            return false;
        };
        let mut m = FontMeasure::new(&*cx.fonts, &self.chain, &mut self.widths);
        match paginator.next_page(&mut m) {
            Some(page) => {
                self.pages.push(page);
                true
            }
            None => {
                self.paginator = None;
                false
            }
        }
    }

    fn paginate_all(&mut self, cx: &mut Cx<KinNovel>) {
        while self.paginate_step(cx) {}
    }

    /// 从头开始分页 (首次加载、字号变化)。
    fn restart_layout(&mut self, cx: &mut Cx<KinNovel>) {
        let params = Self::layout_params(cx, self.sort_num);
        self.paginator = Some(Paginator::new(self.blocks.clone(), params.clone()));
        self.params = Some(params);
        self.pages.clear();
        self.cache.clear();
    }

    fn anchor(&self) -> Option<Progress> {
        if self.pages.is_empty() {
            return None;
        }
        let (path, offset) = first_anchor_on_page(&self.pages, self.page as i64);
        Some(Progress { path, offset, page: self.page })
    }

    /// 进度落盘 (节流; force = 离开/休眠/换章)。
    fn save_progress(&mut self, cx: &mut Cx<KinNovel>, force: bool) {
        let Some(p) = self.anchor() else { return };
        if self.saved.as_ref() == Some(&p) {
            return;
        }
        if !force {
            if let Some(t) = self.last_write {
                if t.elapsed().as_secs() < SAVE_INTERVAL_SECS {
                    return;
                }
            }
        }
        let file = cx.app.paths.progress_file(self.book_id, self.sort_num, &cx.app.config.convert());
        match store::save_progress(&file, &p) {
            Ok(()) => {
                self.saved = Some(p);
                self.last_write = Some(Instant::now());
            }
            Err(e) => eprintln!("[reader] 进度保存失败: {e}"),
        }
        let last = store::LastRead {
            book_id: self.book_id,
            sort_num: self.sort_num,
            comic: false,
            chapter_id: self.chapter.chapter_id,
            book_name: self.chapter.book_name.clone(),
            chapter_title: self.chapter.title.clone(),
            page: self.page,
            pages: self.pages.len(),
            time: store::unix_now(),
            cover: crate::sleep::known_cover(self.book_id).unwrap_or_default(),
        };
        if let Err(e) = store::save_last_read(&cx.app.paths, &last) {
            eprintln!("[reader] 最近阅读保存失败: {e}");
        }
    }

    /// 把当前页的首个锚点上传为服务器阅读位置 (登录时; 后台进行, 失败忽略 —— 与 Python 版的 best-effort 一致)。
    /// 只在退出阅读界面、换章 (后台优先级) 与休眠时调用; 与上次成功上传的位置相同则跳过。
    fn upload_position(&mut self, cx: &mut Cx<KinNovel>, priority: i32) {
        let Some(net) = cx.app.net() else { return };
        // 服务器故障期间不上传 (本地进度已保存), 免得每次换章都多一个注定失败的请求
        if net.user().is_none() || net.server_down() || self.pages.is_empty() || self.chapter.chapter_id == 0 {
            return;
        }
        let xpath = kn_text::first_path_on_page(&self.pages, self.page as i64);
        let (book_id, chapter_id) = (self.book_id, self.chapter.chapter_id);
        let key = (book_id, chapter_id, xpath);
        if LAST_UPLOAD.lock().unwrap_or_else(|e| e.into_inner()).as_ref() == Some(&key) {
            return;
        }
        eprintln!("[reader] 上传进度 {}#{} (优先级 {priority})", key.0, key.1);
        std::thread::spawn(move || match net.save_read_position(key.0, key.1, &key.2, priority) {
            Ok(_) => *LAST_UPLOAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(key),
            Err(e) => eprintln!("[reader] 进度上传失败: {e}"),
        });
    }

    fn on_loaded(&mut self, cx: &mut Cx<KinNovel>, loaded: Loaded) {
        let started = Instant::now();
        self.chapter = loaded.chapter;
        self.blocks = loaded.blocks;
        self.notes = loaded.notes;
        self.footnote = None;
        let mut chapter_font = None;
        match (loaded.font_path, loaded.font_bytes) {
            (Some(path), Some(Ok(bytes))) => match cx.fonts.load_bytes(bytes) {
                Ok(id) => {
                    if let Some((_, old)) = cx.app.chapter_font.replace((path, id)) {
                        cx.fonts.unload(old);
                        cx.glyphs.forget_font(old);
                    }
                    chapter_font = Some(id);
                }
                Err(e) => self.note = format!("章节字体无法解析: {e}"),
            },
            (Some(path), None) => {
                // 与已加载的章节字体相同, 直接复用
                chapter_font = cx.app.chapter_font.as_ref().filter(|(p, _)| *p == path).map(|(_, id)| *id);
            }
            (Some(_), Some(Err(e))) => {
                eprintln!("[reader] 章节字体: {e}");
                self.note = "章节字体未缓存, 正文可能显示为乱码".to_string();
            }
            (None, _) => {}
        }
        // 章节字体没有的字 (♡ 之类) 依次找系统字体、补充符号字体、emoji 字体; 都没有画空白而不是方框
        self.chain = chapter_font.into_iter().chain(cx.app.ui_fonts.iter().copied()).collect();
        self.widths.clear();
        self.restart_layout(cx);

        // 目标页
        let target = match self.entry {
            Entry::First => {
                self.paginate_step(cx);
                0
            }
            Entry::Last => {
                self.paginate_all(cx);
                self.pages.len().saturating_sub(1)
            }
            Entry::Resume => {
                let server = self
                    .chapter
                    .server_position
                    .clone()
                    .filter(|(id, path)| *id == self.chapter.chapter_id && !path.is_empty())
                    .map(|(_, path)| (path, None));
                let local = loaded.local.map(|p| (p.path, Some(p.offset as i64)));
                let anchors: Vec<(String, Option<i64>)> = match self.resume_override.take() {
                    // 设置里改了简繁转换后重新加载: 回到改之前的位置 (XPath 与转换无关)
                    Some(p) => vec![(p.path, Some(p.offset as i64))],
                    None => server.into_iter().chain(local).collect(),
                };
                if anchors.is_empty() {
                    self.paginate_step(cx);
                    0
                } else {
                    self.paginate_all(cx);
                    // 多端进度取最远页 (与 Python 版一致)
                    anchors
                        .iter()
                        .map(|(path, offset)| page_for_path(&self.pages, path, *offset, usize::MAX))
                        .filter(|&p| p != usize::MAX)
                        .max()
                        .unwrap_or(0)
                }
            }
        };
        self.page = target.min(self.pages.len().saturating_sub(1));
        self.status = Status::Ready;
        eprintln!(
            "[reader] {}#{} load {} ms (bg), layout {} ms, {} pages{}",
            self.book_id,
            self.sort_num,
            loaded.millis,
            started.elapsed().as_millis(),
            self.pages.len(),
            if self.done() { "" } else { "+" }
        );
        self.save_progress(cx, true);
        self.prefetch_neighbors(cx);
        self.tracker.start();
        crate::sleep::on_book_opened(&cx.app.paths, cx.app.net(), self.book_id, cx.width.max(cx.height));
        // 进入阅读器/换章: 正文第一屏闪刷, 清掉上一页面与 "正在打开章节" 的残影
        cx.request_redraw(RefreshHint::Flash);
    }

    /// 设置了 "预加载前后章节" 时, 后台把前后两章 (及其字体) 下载进缓存 (Python `_prefetch_neighbors`)。
    fn prefetch_neighbors(&self, cx: &mut Cx<KinNovel>) {
        if !cx.app.config.bool("prefetch_chapters", false) {
            return;
        }
        let Some(net) = cx.app.net().filter(|n| !n.server_down()) else { return };
        let total = self.chapter.chapters.len() as i64;
        let convert = cx.app.config.convert();
        let api = cx.app.config.api_server();
        for target in [self.sort_num + 1, self.sort_num - 1] {
            if target < 1 || (total > 0 && target > total) || cx.app.paths.chapter_file(self.book_id, target, &convert).exists() {
                continue;
            }
            let (paths, net, convert, api, book_id) = (cx.app.paths.clone(), net.clone(), convert.clone(), api.clone(), self.book_id);
            std::thread::spawn(move || match crate::net::download_chapter(&paths, &net, book_id, target, &convert, crate::net::BACKGROUND_PRIORITY) {
                Ok(bytes) => {
                    if let Some(font) = Chapter::parse(&bytes, false).map(|c| c.font).filter(|f| !f.is_empty()) {
                        crate::net::ensure_font(&paths, Some(&net), &api, &font);
                    }
                }
                Err(e) => eprintln!("[reader] 预加载 {book_id}#{target} 失败: {e}"),
            });
        }
    }

    fn content_bitmap(&mut self, cx: &mut Cx<KinNovel>, index: usize) -> usize {
        if let Some(pos) = self.cache.iter().position(|(i, _)| *i == index) {
            let entry = self.cache.remove(pos);
            self.cache.push(entry);
            return self.cache.len() - 1;
        }
        let bmp = self.render_content(cx, index);
        if self.cache.len() >= PAGE_CACHE {
            self.cache.remove(0);
        }
        self.cache.push((index, bmp));
        self.cache.len() - 1
    }

    fn render_content(&self, cx: &mut Cx<KinNovel>, index: usize) -> Bitmap {
        let theme = cx.theme;
        let (_, content_h) = Self::geometry(cx.width, cx.height);
        let mut bmp = Bitmap::new(cx.width, content_h, theme.background);
        let Some(items) = self.pages.get(index) else { return bmp };
        // 基线按系统字体的 ascent 定位 (章节字体是反爬用的子集字体, 度量不可靠)
        let metrics_chain = [cx.app.ui_fonts[0]];
        for item in items {
            match item {
                PageItem::Text { text, x, y, rich: Some(line), .. } => {
                    self.draw_rich_line(cx, &mut bmp, text, *x as f32, *y as f32, line);
                }
                PageItem::Rule { x, y, width, height } => {
                    bmp.fill_rect(Rect::new(*x as i32, *y as i32, *width as u32, *height as u32), theme.mid);
                }
                PageItem::Dot { x, y, size } => {
                    let d = *size as u32;
                    bmp.rounded_rect(Rect::new(*x as i32, *y as i32, d, d), d / 2, Some(theme.foreground), None, 0);
                }
                // 注释标记由所在的文字行画出, 这里只是点击区域
                PageItem::Note { .. } => {}
                PageItem::Text { text, x, y, size, .. } => {
                    let size_px = *size as f32;
                    let ascent = cx.fonts.line_metrics(&TextStyle { fonts: metrics_chain.to_vec(), size_px }).ascent;
                    let style = TextStyle { fonts: self.chain.clone(), size_px };
                    cx.glyphs.draw_text(&*cx.fonts, &mut bmp, *x as f32, (*y as f32 + ascent).round(), text, &style, theme.foreground);
                }
                PageItem::Image { url, x, y, width, height, .. } => {
                    let r = Rect::new(*x as i32, *y as i32, *width as u32, *height as u32);
                    if let Some(rect) = self.image_rect(url, r) {
                        if let Some(ImageSlot::Ready(img)) = self.images.get(url) {
                            bmp.blit(img, img.bounds(), rect.x, rect.y);
                        }
                        continue;
                    }
                    let label = match self.images.get(url) {
                        Some(ImageSlot::Missing) => "插图未缓存",
                        Some(ImageSlot::Failed(_)) => "插图无法显示",
                        _ => "插图加载中…",
                    };
                    let m = cx.app.metrics;
                    bmp.rounded_rect(r.inflate(-4), m.radius, None, Some(theme.mid), 2);
                    let chain = cx.app.ui_fonts.clone();
                    let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
                    ink.text_centered(&mut bmp, r, label, m.small, theme.muted);
                }
            }
        }
        bmp
    }

    /// 一行带样式的文字 (网页样式模式)。基线按行内最大字号定, 与兼容模式的整行文字一致。
    fn draw_rich_line(&self, cx: &mut Cx<KinNovel>, bmp: &mut Bitmap, text: &str, x: f32, y: f32, line: &RichLine) {
        let theme = cx.theme;
        let metrics_chain = vec![cx.app.ui_fonts[0]];
        let ascent = |cx: &Cx<KinNovel>, size: u32| cx.fonts.line_metrics(&TextStyle { fonts: metrics_chain.clone(), size_px: size as f32 }).ascent;
        let baseline = (y + ascent(cx, line.max_size)).round();
        // 最近的注音基字 (x, 宽, 字号)
        let mut base: Option<(f32, f32, u32)> = None;
        for r in &line.runs {
            let s = r.style;
            if s.hidden {
                continue;
            }
            let color = if s.muted { theme.muted } else { theme.foreground };
            let size = r.size as f32;
            let rx = x + r.x;
            let bl = baseline
                + match s.shift {
                    1 => -(line.max_size as f32 * 0.33).round(),
                    -1 => (line.max_size as f32 * 0.15).round(),
                    _ => 0.0,
                };
            match &r.object {
                Some(Inline::Note(_)) => {
                    // 注释图标: 圆角框里一个「注」(用系统字体: 章节字体的字形是打乱的)
                    let h = (size * 0.95).round();
                    let w = (r.width - size * 0.1).max(h * 0.8).round();
                    let rect = Rect::new((rx + size * 0.05).round() as i32, (bl - size * 0.85).round() as i32, w as u32, h as u32);
                    let lw = ((size / 24.0).round() as u32).max(1);
                    bmp.rounded_rect(rect, (h * 0.2) as u32, None, Some(color), lw);
                    let chain = cx.app.ui_fonts.clone();
                    let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
                    ink.text_centered(bmp, rect, "注", size * 0.62, color);
                }
                Some(_) => {}
                None if s.ruby => {
                    let Some((bx, bw, bsize)) = base else { continue };
                    let style = TextStyle { fonts: self.chain.clone(), size_px: size };
                    let rt = &text[r.start..r.end];
                    let w = cx.fonts.measure(rt, &style);
                    let rbl = (baseline - ascent(cx, bsize) - size * 0.15).round();
                    cx.glyphs.draw_text_synth(&*cx.fonts, bmp, (bx + (bw - w) / 2.0).round(), rbl, rt, &style, color, Synth { bold: s.bold, italic: s.italic });
                }
                None => {
                    let style = TextStyle { fonts: self.chain.clone(), size_px: size };
                    let run = &text[r.start..r.end];
                    cx.glyphs.draw_text_synth(&*cx.fonts, bmp, rx, bl, run, &style, color, Synth { bold: s.bold, italic: s.italic });
                    let thick = ((size / 20.0).round() as u32).max(1);
                    if s.underline {
                        bmp.fill_rect(Rect::new(rx.round() as i32, (bl + size * 0.12).round() as i32, r.width.round() as u32, thick), color);
                    }
                    if s.strike {
                        bmp.fill_rect(Rect::new(rx.round() as i32, (bl - size * 0.32).round() as i32, r.width.round() as u32, thick), color);
                    }
                    if s.dots {
                        // 着重号: 每个字下方居中一个点
                        let mut pen = rx;
                        let d = ((size * 0.14).round() as u32).max(2);
                        for ch in run.chars() {
                            let font = cx.fonts.pick_font(&self.chain, ch);
                            let adv = cx.fonts.advance(font, ch, size);
                            if !ch.is_whitespace() {
                                let dx = (pen + adv / 2.0 - d as f32 / 2.0).round() as i32;
                                bmp.rounded_rect(Rect::new(dx, (bl + size * 0.2).round() as i32, d, d), d / 2, Some(color), None, 0);
                            }
                            pen += adv;
                        }
                    }
                    base = Some((rx, r.width, r.size));
                }
            }
        }
    }

    /// 已解码插图在排版框 `r` 里的实际绘制区域 (水平居中, 顶对齐, 与 Python 版一致); 未就绪返回 None。
    fn image_rect(&self, url: &str, r: Rect) -> Option<Rect> {
        let Some(ImageSlot::Ready(img)) = self.images.get(url) else { return None };
        let dx = r.x + (r.w as i32 - img.width() as i32) / 2;
        Some(Rect::new(dx, r.y, img.width(), img.height()))
    }

    /// 当前页插图的点击区域 (屏幕坐标; 正文位图画在 y = top 处)。
    fn update_image_hits(&mut self, top: i32) {
        self.image_hits.clear();
        self.note_hits.clear();
        let Some(items) = self.pages.get(self.page) else { return };
        for item in items {
            if let PageItem::Note { index, x, y, width, height } = item {
                // 标记只有一个字宽: 左右各放宽半行高, 好点中
                let pad = *height as i32 / 2;
                self.note_hits.push((Rect::new(*x as i32 - pad, *y as i32 + top, (*width as i32 + 2 * pad) as u32, *height as u32), *index));
                continue;
            }
            let PageItem::Image { url, x, y, width, height, .. } = item else { continue };
            let r = Rect::new(*x as i32, *y as i32, *width as u32, *height as u32);
            if let Some(rect) = self.image_rect(url, r) {
                self.image_hits.push((Rect::new(rect.x, rect.y + top, rect.w, rect.h), url.clone(), *height as u32));
            }
        }
    }

    /// 为某页的插图发起后台解码 (已请求过的跳过)。
    fn request_images(&mut self, cx: &mut Cx<KinNovel>, index: usize) {
        let Some(items) = self.pages.get(index) else { return };
        for item in items {
            let PageItem::Image { url, width, height, .. } = item else { continue };
            match self.images.get(url) {
                Some(ImageSlot::Failed(at)) if at.elapsed() >= crate::net::RETRY_AFTER => {}
                Some(_) => continue,
                None => {}
            }
            self.images.insert(url.clone(), ImageSlot::Pending);
            let paths = cx.app.paths.clone();
            let net = cx.app.net();
            let (url, w, h) = (url.clone(), *width as u32, *height as u32);
            cx.spawn(move || {
                let result = crate::net::image_bytes(&paths, net.as_ref(), &url, h).and_then(|bytes| match bytes {
                    None => Ok(None),
                    Some(bytes) => kn_render::decode_gray(&bytes, Some((w, h)))
                        .map(|img| Some(img.fit_within(w, h).dithered()))
                        .map_err(|e| e.to_string()),
                });
                ImageLoaded { url, size: (w, h), result }
            });
        }
    }

    fn on_image(&mut self, cx: &mut Cx<KinNovel>, loaded: ImageLoaded) {
        // 按切换方向之前的排版框解码的: 丢掉, 显示到时按新尺寸重新请求
        let (w, h) = loaded.size;
        let current = self.pages.iter().flatten().any(|it| matches!(it, PageItem::Image { url, width, height, .. } if *url == loaded.url && *width as u32 == w && *height as u32 == h));
        if !current {
            self.images.remove(&loaded.url);
            return;
        }
        let slot = match loaded.result {
            Ok(Some(img)) => ImageSlot::Ready(img),
            Ok(None) => ImageSlot::Missing,
            Err(e) => {
                eprintln!("[reader] 插图 {}: {e}", loaded.url);
                ImageSlot::Failed(Instant::now())
            }
        };
        self.images.insert(loaded.url.clone(), slot);
        // 含这张图的页位图作废; 当前页需要重画
        let pages = &self.pages;
        let has = |i: usize| pages.get(i).is_some_and(|p| p.iter().any(|it| matches!(it, PageItem::Image { url, .. } if *url == loaded.url)));
        self.cache.retain(|(i, _)| !has(*i));
        if has(self.page) {
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn page_label(&self) -> String {
        if self.done() {
            format!("{} / {}", self.page + 1, self.pages.len())
        } else {
            format!("{} / {}+", self.page + 1, self.pages.len())
        }
    }

    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        if !matches!(self.status, Status::Ready) {
            return Transition::None;
        }
        let target = self.page as i64 + delta;
        if target < 0 {
            return self.change_chapter(cx, -1);
        }
        let target = target as usize;
        while target >= self.pages.len() && self.paginate_step(cx) {}
        if target >= self.pages.len() {
            return self.change_chapter(cx, 1);
        }
        self.page = target;
        self.note.clear();
        self.save_progress(cx, false);
        self.tracker.page(&cx.app.paths, self.book_id, &self.chapter.book_name, false);
        // MTK 原生翻页动画 (Python `page_turn_animation`, 默认开): 向后翻内容左移, 向前翻右移
        super::apply_swipe_steps(cx.app);
        let swipe = cx.app.config.bool("page_turn_animation", true).then_some(if delta > 0 { SwipeDir::Left } else { SwipeDir::Right });
        cx.request_turn(swipe);
        Transition::None
    }

    fn change_chapter(&mut self, cx: &mut Cx<KinNovel>, delta: i64) -> Transition<KinNovel> {
        let target = self.sort_num + delta;
        let total = self.chapter.chapters.len() as i64;
        if target < 1 || (total > 0 && target > total) {
            self.note = if delta < 0 { "已经是第一章".into() } else { "已经是最后一章".into() };
            self.chrome = true;
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        let file = cx.app.paths.chapter_file(self.book_id, target, &cx.app.config.convert());
        if !file.exists() && cx.app.net().is_none() {
            let name = self.chapter.chapters.get((target - 1) as usize).cloned().unwrap_or_default();
            self.note = format!("「{}」尚未缓存 (离线)", if name.is_empty() { format!("第 {target} 章") } else { name });
            self.chrome = true;
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        self.save_progress(cx, true);
        let entry = if delta < 0 { Entry::Last } else { Entry::First };
        self.next_leave = Leave::Chapter;
        Transition::Replace(Box::new(ReaderPage::new(self.book_id, target, entry)))
    }

    fn change_font_size(&mut self, cx: &mut Cx<KinNovel>, steps: i64) {
        let (min_font, max_font, default_font, step) = font_range(cx.app);
        let current = cx.app.config.int("font_size", default_font).clamp(min_font, max_font);
        let size = (current + steps * step).clamp(min_font, max_font);
        if size == current {
            return;
        }
        cx.app.config.set("font_size", serde_json::Value::from(size));
        self.relayout_keep_position(cx);
        cx.request_redraw(RefreshHint::Ui);
    }

    /// 按当前配置重新分页, 停在原来第一行所在的页。
    fn relayout_keep_position(&mut self, cx: &mut Cx<KinNovel>) {
        let anchor = self.anchor();
        self.restart_layout(cx);
        self.paginate_all(cx);
        self.page = anchor
            .map(|a| page_for_path(&self.pages, &a.path, Some(a.offset as i64), 0))
            .unwrap_or(0)
            .min(self.pages.len().saturating_sub(1));
    }

    /// 从上层页面 (设置、目录、插图预览) 返回。
    fn on_return(&mut self, cx: &mut Cx<KinNovel>) {
        // 主题可能变了: 正文位图重画
        self.cache.clear();
        // 书签可能在目录页里删过
        self.bookmarks = userdata::load_bookmarks(&cx.app.paths, self.book_id);
        let jump = cx.app.reader_jump.take().filter(|j| j.book_id == self.book_id);
        if let Some(jump) = jump.clone().filter(|j| j.sort_num != self.sort_num) {
            // 换章: 就地重新加载 (离开本章前上传进度, 与翻到下一章一致)
            self.upload_position(cx, crate::net::BACKGROUND_PRIORITY);
            self.load_chapter(cx, jump.sort_num, jump.at);
            return;
        }
        if !matches!(self.status, Status::Ready) {
            return;
        }
        if cx.app.config.convert() != self.convert {
            // 简繁转换变了: 章节内容不同, 重新加载, 回到当前位置
            self.save_progress(cx, true);
            self.resume_override = self.anchor();
            self.entry = Entry::Resume;
            self.status = Status::Loading;
            self.pages.clear();
            self.paginator = None;
            self.start_load(cx);
        } else if self.params.as_ref() != Some(&Self::layout_params(cx, self.sort_num)) {
            // 字号/行距/页边距/首行缩进变了
            self.relayout_keep_position(cx);
            self.save_progress(cx, true);
        }
        if let Some(at) = jump.and_then(|j| j.at) {
            // 本章内的书签
            self.paginate_all(cx);
            let page = page_for_path(&self.pages, &at.path, Some(at.offset as i64), self.page);
            self.page = page.min(self.pages.len().saturating_sub(1));
            self.note.clear();
            self.chrome = false;
            self.save_progress(cx, true);
        }
    }

    /// 在本实例里打开另一章 (目录/书签跳转), `at` 为要停的位置 (None = 阅读进度)。
    fn load_chapter(&mut self, cx: &mut Cx<KinNovel>, sort_num: i64, at: Option<Progress>) {
        self.save_progress(cx, true);
        self.stop_tracking(cx);
        self.sort_num = sort_num;
        self.entry = Entry::Resume;
        self.resume_override = at;
        self.status = Status::Loading;
        self.chapter = Chapter::default();
        self.blocks.clear();
        self.notes.clear();
        self.note_hits.clear();
        self.footnote = None;
        self.paginator = None;
        self.params = None;
        self.pages.clear();
        self.page = 0;
        self.cache.clear();
        self.images.clear();
        self.image_hits.clear();
        self.chrome = false;
        self.note.clear();
        self.saved = None;
        self.last_write = None;
        self.start_load(cx);
        cx.request_redraw(RefreshHint::Ui);
    }

    /// 当前页上的书签 (下标)。
    fn bookmark_on_page(&self) -> Option<usize> {
        if self.pages.is_empty() {
            return None;
        }
        self.bookmarks
            .iter()
            .position(|b| b.sort_num == self.sort_num && page_for_path(&self.pages, &b.path, Some(b.offset as i64), usize::MAX) == self.page)
    }

    /// 当前页开头的几十个字 (书签列表里显示)。
    fn page_excerpt(&self) -> String {
        let mut text = String::new();
        for item in self.pages.get(self.page).into_iter().flatten() {
            if let PageItem::Text { text: t, path, .. } = item {
                if path.is_empty() {
                    continue; // 列表序号
                }
                text.extend(t.trim().chars().filter(|&c| c != OBJECT_CHAR));
                if text.chars().count() >= EXCERPT_CHARS {
                    break;
                }
            }
        }
        text.chars().take(EXCERPT_CHARS).collect()
    }

    fn toggle_bookmark(&mut self, cx: &mut Cx<KinNovel>) {
        if let Some(i) = self.bookmark_on_page() {
            self.bookmarks.remove(i);
            self.note = "已删除书签".into();
        } else if let Some(anchor) = self.anchor() {
            self.bookmarks.push(Bookmark {
                sort_num: self.sort_num,
                chapter: self.chapter.title.clone(),
                path: anchor.path,
                offset: anchor.offset,
                excerpt: self.page_excerpt(),
                // 正文用了章节字体 (字体链比界面字体链长) 时记下它, 书签列表用它画摘录
                font: match &cx.app.chapter_font {
                    Some((path, _)) if self.chain.len() > cx.app.ui_fonts.len() => path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
                    _ => String::new(),
                },
                time: store::unix_now(),
            });
            self.note = "已添加书签".into();
        } else {
            return;
        }
        if let Err(e) = userdata::save_bookmarks(&cx.app.paths, self.book_id, &self.bookmarks) {
            eprintln!("[reader] 书签保存失败: {e}");
            self.note = "书签保存失败".into();
        }
        cx.request_redraw(RefreshHint::Ui);
    }

    /// 进度条上点按/滑动到 x: 跳到章内对应的页 (先排完整章)。
    fn jump_to_track(&mut self, cx: &mut Cx<KinNovel>, x: i32) {
        if !matches!(self.status, Status::Ready) || self.track.w == 0 {
            return;
        }
        self.paginate_all(cx);
        let n = self.pages.len();
        if n == 0 {
            return;
        }
        let frac = ((x - self.track.x) as f32 / self.track.w as f32).clamp(0.0, 1.0);
        let target = ((frac * n as f32) as usize).min(n - 1);
        if target != self.page {
            self.page = target;
            self.note.clear();
            self.save_progress(cx, false);
        }
        cx.request_redraw(RefreshHint::Ui);
    }

    /// 后台读取章节、字体与本地进度 (结果回到 on_message)。
    fn start_load(&mut self, cx: &mut Cx<KinNovel>) {
        self.convert = cx.app.config.convert();
        self.bookmarks = userdata::load_bookmarks(&cx.app.paths, self.book_id);
        let paths = cx.app.paths.clone();
        let convert = cx.app.config.convert();
        let api = cx.app.config.api_server();
        let loaded_font = cx.app.chapter_font.as_ref().map(|(p, _)| p.clone());
        let net = cx.app.net();
        let (book_id, sort_num) = (self.book_id, self.sort_num);
        cx.spawn(move || -> Result<Loaded, String> {
            let started = Instant::now();
            let bytes = crate::net::chapter_bytes(&paths, net.as_ref(), book_id, sort_num, &convert)?;
            let chapter = Chapter::parse(&bytes, true).ok_or("章节数据损坏")?;
            let doc = kn_text::extract_document(&chapter.content, store::SITE_BASE, true);
            let (blocks, notes) = (doc.blocks, doc.notes);
            let font_path = match &chapter.font {
                f if f.is_empty() => None,
                f => crate::net::ensure_font(&paths, net.as_ref(), &api, f).or_else(|| paths.font_file(&api, f)),
            };
            let font_bytes = match &font_path {
                Some(p) if Some(p) == loaded_font.as_ref() => None,
                Some(p) => Some(store::load_chapter_font(p)),
                None => None,
            };
            let local = store::load_progress(&paths.progress_file(book_id, sort_num, &convert));
            let mut chapter = chapter;
            chapter.content = String::new();
            Ok(Loaded { chapter, blocks, notes, font_path, font_bytes, local, millis: started.elapsed().as_millis() })
        });
    }

    fn draw_header(&self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap, top: i32) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let margin = Self::layout_params(cx, self.sort_num).margin as i32;
        frame.fill_rect(Rect::new(0, 0, cx.width, top as u32), theme.background);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let style_top = (top - (m.tiny * 1.2) as i32) / 2 + 4;
        // 右侧: 时间 · 电量 (与其它页面顶栏一致, 每分钟随 on_minute 重绘, 只刷顶栏这一小块)
        let status = super::status_text();
        let status_w = ink.width(&status, m.tiny).ceil() as i32;
        let status_x = cx.width as i32 - margin - status_w;
        ink.text(frame, status_x, style_top, &status, m.tiny, theme.muted);
        // 左侧: 章节名, 后面隔一点接章内页码
        let label = self.page_label();
        let label_w = ink.width(&label, m.tiny).ceil() as i32;
        let gap = (m.tiny * 0.8) as i32;
        let title_w = (status_x - margin - margin / 2 - gap - label_w).max(0) as f32;
        let title = ink.fit(&self.chapter.title, m.tiny, title_w);
        let title_end = margin + ink.text(frame, margin, style_top, &title, m.tiny, theme.muted).ceil() as i32;
        let label_x = if title.is_empty() { margin } else { title_end + gap };
        ink.text(frame, label_x, style_top, &label, m.tiny, theme.muted);
        // 书签角标: 右侧页边距里, 从屏幕顶边垂下
        if self.bookmark_on_page().is_some() {
            let rw = (margin as f32 * 0.42).max(10.0) as u32;
            let rx = cx.width as i32 - margin / 2 - rw as i32 / 2;
            ribbon(frame, Rect::new(rx, 0, rw, (top as f32 * 0.95) as u32), true, 2.0, theme.foreground);
        }
        // 底部进度线
        if !self.pages.is_empty() {
            let y = cx.height as i32 - (margin / 2).max(10);
            let w = cx.width as i32 - 2 * margin;
            let total = if self.done() { self.pages.len() } else { self.pages.len() + 1 };
            let filled = (w as f32 * (self.page + 1) as f32 / total.max(1) as f32) as i32;
            frame.fill_rect(Rect::new(margin, y, w as u32, 2), theme.mid);
            frame.fill_rect(Rect::new(margin, y - 1, filled.max(0) as u32, 4), theme.foreground);
        }
    }

    fn draw_chrome(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let (w, h) = (cx.width, cx.height);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };

        // 顶栏: 返回 / 书名 / 书签 / 主页 (书名给书签按钮让出位置)
        let title = if self.chapter.book_name.is_empty() { "阅读".to_string() } else { self.chapter.book_name.clone() };
        let bar_h = m.header_h() as i32;
        let title = ink.fit(&title, m.title, (w as i32 - 4 * bar_h - 2 * m.margin as i32).max(0) as f32);
        let bar = widgets::header(&mut ink, frame, &theme, &m, &title, "", true, true);
        frame.fill_rect(Rect::new(0, bar.bottom(), w, 2), theme.foreground);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        cx.hits.add(HIT_HOME, Rect::new(w as i32 - bar.h as i32, 0, bar.h, bar.h));
        if matches!(self.status, Status::Ready) {
            let mark = Rect::new(w as i32 - 2 * bar.h as i32, 0, bar.h, bar.h);
            let icon_h = (bar.h as f32 * 0.46) as u32;
            let icon_w = (icon_h as f32 * 0.68) as u32;
            let icon = Rect::new(mark.x + (mark.w - icon_w) as i32 / 2, (bar.h - icon_h) as i32 / 2, icon_w, icon_h);
            ribbon(frame, icon, self.bookmark_on_page().is_some(), (4.0 * m.scale).max(3.0), theme.foreground);
            cx.hits.add(HIT_BOOKMARK, mark);
        }
        super::light::draw(&mut ink, frame, cx.hits, &theme, &m, &cx.app.light, w, bar.bottom() + 2);

        // 底部面板
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

        let mut y = panel.y + pad;
        let pages = self.page_label() + " 页";
        let pages_w = ink.width(&pages, m.small).ceil() as i32;
        ink.text(frame, w as i32 - side - pages_w, y + (info_h - (m.small * 1.25) as i32) / 2, &pages, m.small, theme.muted);
        let chapter_label = format!("第 {} 章 · {}", self.sort_num, self.chapter.title);
        let chapter_label = ink.fit(&chapter_label, m.small, (inner_w - pages_w - side / 2) as f32);
        ink.text(frame, side, y + (info_h - (m.small * 1.25) as i32) / 2, &chapter_label, m.small, theme.foreground);
        y += info_h + gap / 2;

        // 章内进度条 (点按/滑动跳页; 触摸区域上下放宽到一个按钮高)
        let track = Rect::new(side, y, inner_w as u32, track_h as u32);
        self.track = track;
        let touch = Rect::new(side - gap, y + track_h / 2 - btn_h / 2, (inner_w + 2 * gap) as u32, btn_h as u32);
        cx.hits.add(HIT_TRACK, touch).no_feedback();
        let total = if self.done() { self.pages.len() } else { self.pages.len() + 1 };
        let filled = (inner_w as f32 * (self.page + 1) as f32 / total.max(1) as f32) as u32;
        frame.rounded_rect(track, track_h as u32 / 2, Some(theme.background), Some(theme.mid), 2);
        if filled > 0 {
            frame.rounded_rect(Rect::new(side, y, filled.max(track_h as u32), track_h as u32), track_h as u32 / 2, Some(theme.foreground), None, 0);
        }
        y += track_h + gap;

        // 上一章 / 目录 / 设置 / 下一章
        let cols = 4;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let row = [(HIT_PREV_CHAPTER, "上一章"), (HIT_TOC, "目录"), (HIT_SETTINGS, "设置"), (HIT_NEXT_CHAPTER, "下一章")];
        for (i, (id, label)) in row.iter().enumerate() {
            let r = Rect::new(side + i as i32 * (col_w + gap), y, col_w as u32, btn_h as u32);
            widgets::button(&mut ink, frame, &theme, &m, r, label, ButtonStyle::Secondary);
            cx.hits.add(*id, r).rounded(m.radius);
        }
        y += btn_h + gap;

        // 字号 − 当前 + / 横竖屏 / 日夜
        let (min_font, max_font, default_font, _) = font_range(cx.app);
        let size = cx.app.config.int("font_size", default_font).clamp(min_font, max_font);
        let cols = 5;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let cell = |i: i32| Rect::new(side + i * (col_w + gap), y, col_w as u32, btn_h as u32);
        let down = cell(0);
        widgets::button(&mut ink, frame, &theme, &m, down, "A－", if size > min_font { ButtonStyle::Secondary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_FONT_DOWN, down).rounded(m.radius).enabled(size > min_font);
        ink.text_centered(frame, cell(1), &format!("字号 {size}"), m.small, theme.foreground);
        let up = cell(2);
        widgets::button(&mut ink, frame, &theme, &m, up, "A＋", if size < max_font { ButtonStyle::Secondary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_FONT_UP, up).rounded(m.radius).enabled(size < max_font);
        let rotate = cell(3);
        widgets::button(&mut ink, frame, &theme, &m, rotate, super::orientation_label(cx.app, super::READER_ORIENTATION), ButtonStyle::Secondary);
        cx.hits.add(HIT_ORIENTATION, rotate).rounded(m.radius);
        let night = cell(4);
        widgets::button(&mut ink, frame, &theme, &m, night, if theme.night { "日间" } else { "夜间" }, ButtonStyle::Secondary);
        cx.hits.add(HIT_NIGHT, night).rounded(m.radius);

        // 提示条: 面板上方居中的反色圆角条
        if !self.note.is_empty() {
            let text_w = ink.width(&self.note, m.small).ceil() as u32;
            let nw = (text_w + 2 * m.margin).min(w - 2 * m.margin);
            let nh = (m.small * 2.2) as u32;
            let r = Rect::new((w - nw) as i32 / 2, panel.y - nh as i32 - gap, nw, nh);
            frame.rounded_rect(r, nh / 2, Some(theme.foreground), None, 0);
            ink.text_centered(frame, r.inflate(-(m.margin as i32) / 2), &self.note, m.small, theme.background);
        }
    }

    fn set_chrome(&mut self, cx: &mut Cx<KinNovel>, visible: bool) {
        if self.chrome != visible {
            self.chrome = visible;
            if visible {
                // 系统可能改过亮度 (自动亮度、系统快捷设置), 打开菜单时重读
                cx.app.light.refresh();
            } else {
                self.note.clear();
            }
            // 夜间关菜单: 按钮细框在 GCK16 局部刷新后留淡淡的残影 (相机实拍), 对变化区域闪刷
            cx.request_redraw(if !visible && cx.theme.night { RefreshHint::Clean } else { RefreshHint::Ui });
        }
    }

    fn on_chrome_hit(&mut self, cx: &mut Cx<KinNovel>, id: HitId) -> Transition<KinNovel> {
        match id {
            HIT_BACK => Transition::Back,
            HIT_HOME => Transition::Home,
            HIT_PREV_CHAPTER => self.change_chapter(cx, -1),
            HIT_NEXT_CHAPTER => self.change_chapter(cx, 1),
            HIT_TOC => {
                let chapters = self
                    .chapter
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(i, title)| crate::api::ChapterRef { id: 0, sort_num: i as i64 + 1, title: title.clone(), page_count: 0 })
                    .collect();
                self.next_leave = Leave::Child;
                Transition::Push(Box::new(super::book::CatalogPage::from_reader(self.book_id, chapters, self.sort_num)))
            }
            HIT_BOOKMARK => {
                self.toggle_bookmark(cx);
                Transition::None
            }
            HIT_SETTINGS => {
                self.next_leave = Leave::Child;
                Transition::Push(Box::new(super::settings::SettingsPage::default()))
            }
            HIT_FONT_DOWN => {
                self.change_font_size(cx, -1);
                Transition::None
            }
            HIT_FONT_UP => {
                self.change_font_size(cx, 1);
                Transition::None
            }
            HIT_ORIENTATION => {
                // 新方向的宽高在下一次重绘时生效, render 发现尺寸变了就重排 (保持阅读位置)
                super::toggle_orientation(cx, super::READER_ORIENTATION);
                Transition::None
            }
            HIT_NIGHT => {
                cx.app.night = !cx.app.night;
                cx.app.config.set("night_mode", serde_json::Value::from(cx.app.night));
                self.cache.clear();
                cx.request_redraw(RefreshHint::Flash);
                Transition::None
            }
            _ => Transition::None,
        }
    }
}

impl Page<KinNovel> for ReaderPage {
    fn id(&self) -> PageId {
        PageId(10)
    }

    fn orientation(&self, app: &KinNovel) -> Option<Orientation> {
        Some(super::reader_orientation(app, super::READER_ORIENTATION))
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        self.next_leave = Leave::Exit;
        if returning {
            self.on_return(cx);
            if matches!(self.status, Status::Ready) {
                self.tracker.start();
            }
        } else if matches!(self.status, Status::Loading) {
            self.start_load(cx);
        }
    }

    fn leave(&mut self, cx: &mut Cx<KinNovel>) {
        self.save_progress(cx, true);
        self.stop_tracking(cx);
        match self.next_leave {
            Leave::Exit => self.upload_position(cx, 0),
            Leave::Chapter => self.upload_position(cx, crate::net::BACKGROUND_PRIORITY),
            Leave::Child => {}
        }
    }

    fn on_suspend(&mut self, cx: &mut Cx<KinNovel>) {
        self.save_progress(cx, true);
        self.stop_tracking(cx);
        self.upload_position(cx, 0);
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<ImageLoaded>() {
            Ok(img) => return self.on_image(cx, *img),
            Err(other) => other,
        };
        let Ok(result) = msg.downcast::<Result<Loaded, String>>() else { return };
        match *result {
            Ok(loaded) => self.on_loaded(cx, loaded),
            Err(e) => {
                self.status = Status::Failed(e);
                cx.request_redraw(RefreshHint::Ui);
            }
        }
    }

    fn opaque(&self) -> bool {
        matches!(self.status, Status::Ready)
    }

    fn flash_on_enter(&self) -> bool {
        // 加载中: 等正文就绪再闪刷 (on_loaded)
        !matches!(self.status, Status::Loading)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        'body: {
            let theme = cx.theme;
            let m = cx.app.metrics;
            let (top, _) = Self::geometry(cx.width, cx.height);
            match &self.status {
                Status::Loading | Status::Failed(_) => {
                    let text = match &self.status {
                        Status::Failed(e) => format!("章节打开失败
{e}

轻触屏幕返回"),
                        _ => "正在打开章节…".to_string(),
                    };
                    let chain = cx.app.ui_fonts.clone();
                    let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
                    let mut y = cx.height as i32 / 2 - m.body as i32;
                    for line in text.lines() {
                        ink.text_centered(frame, Rect::new(m.margin as i32, y, cx.width - 2 * m.margin, (m.body * 1.5) as u32), line, m.body, theme.foreground);
                        y += (m.body * 1.6) as i32;
                    }
                    if self.chrome {
                        self.draw_chrome(cx, frame);
                    }
                    break 'body;
                }
                Status::Ready => {}
            }
            if self.params.as_ref().is_some_and(|p| (p.width, p.height) != (cx.width as i64, Self::geometry(cx.width, cx.height).1 as i64)) {
                // 横竖屏切换了: 按新尺寸重排, 停在原来第一行所在的页; 插图按新排版框重新解码
                let started = Instant::now();
                self.images.clear();
                self.relayout_keep_position(cx);
                self.save_progress(cx, true);
                eprintln!("[reader] 换方向重排 {} ms, {} 页", started.elapsed().as_millis(), self.pages.len());
            }
            self.request_images(cx, self.page);
            self.update_image_hits(top);
            let slot = self.content_bitmap(cx, self.page);
            let bmp = &self.cache[slot].1;
            frame.blit(bmp, bmp.bounds(), 0, top);
            self.draw_header(cx, frame, top);
            if self.chrome {
                self.draw_chrome(cx, frame);
            }
            if let Some(n) = self.footnote {
                self.draw_footnote(cx, frame, n);
            }
        }
        super::tips::show(cx, frame, &super::tips::READER, matches!(self.status, Status::Ready));
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        if super::tips::input(cx, event, &super::tips::READER) {
            return Transition::None;
        }
        // 注释弹窗: 点任意处 (或滑动、翻页键) 关闭
        if self.footnote.is_some() {
            if matches!(event, InputEvent::Gesture(g) if g.kind != GestureKind::Down) || matches!(event, InputEvent::Key { pressed: true, .. }) {
                self.footnote = None;
                cx.request_redraw(RefreshHint::Clean);
            }
            return Transition::None;
        }
        match event {
            InputEvent::Key { code: KeyCode::PageForward, pressed: true } => self.turn(cx, 1),
            InputEvent::Key { code: KeyCode::PageBack, pressed: true } => self.turn(cx, -1),
            InputEvent::Gesture(g) => {
                let p = Point { x: g.start.0, y: g.start.1 };
                let (top, _) = Self::geometry(cx.width, cx.height);
                match g.kind {
                    GestureKind::Tap | GestureKind::Long => {
                        if self.chrome {
                            return match cx.hits.at(p) {
                                Some(HIT_TRACK) => {
                                    self.jump_to_track(cx, p.x);
                                    Transition::None
                                }
                                Some(id) if super::light::on_tap(cx, id, p) => Transition::None,
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
                        // 注释标记: 弹出注释 (同网页版点击注释图标)
                        if let Some((_, n)) = self.note_hits.iter().find(|(r, _)| r.contains(p)) {
                            if self.notes.get(*n as usize - 1).is_some() {
                                self.footnote = Some(*n);
                                cx.request_redraw(RefreshHint::Ui);
                                return Transition::None;
                            }
                        }
                        // 只认插图实际绘制区域 (不含排版框两侧留白)
                        if g.kind == GestureKind::Tap {
                            if let Some((_, url, h)) = self.image_hits.iter().find(|(r, _, _)| r.contains(p)) {
                                self.next_leave = Leave::Child;
                                return Transition::Push(Box::new(super::image::ImagePage::new(url.clone(), *h)));
                            }
                        }
                        // 顶栏 (章节名、时间、页码) 不翻页
                        if p.y < top {
                            return Transition::None;
                        }
                        let w = cx.width as i32;
                        if p.x < w * 3 / 10 {
                            self.turn(cx, -1)
                        } else if p.x > w * 7 / 10 {
                            self.turn(cx, 1)
                        } else {
                            self.set_chrome(cx, true);
                            Transition::None
                        }
                    }
                    GestureKind::SwipeLeft | GestureKind::SwipeRight | GestureKind::SwipeUp | GestureKind::SwipeDown
                        if self.chrome && cx.hits.at(p) == Some(HIT_TRACK) =>
                    {
                        self.jump_to_track(cx, g.end.0);
                        Transition::None
                    }
                    GestureKind::SwipeLeft | GestureKind::SwipeRight | GestureKind::SwipeUp | GestureKind::SwipeDown
                        if self.chrome && super::light::on_swipe(cx, g) =>
                    {
                        Transition::None
                    }
                    GestureKind::SwipeLeft => {
                        self.chrome = false;
                        self.turn(cx, 1)
                    }
                    GestureKind::SwipeRight => {
                        self.chrome = false;
                        self.turn(cx, -1)
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
        // 1. 相邻页的插图先开始解码, 再预渲染相邻页 (翻页只需拷贝)
        for index in [self.page + 1, self.page.wrapping_sub(1)] {
            self.request_images(cx, index);
        }
        for index in [self.page + 1, self.page.wrapping_sub(1)] {
            if index < self.pages.len() && !self.cache.iter().any(|(i, _)| *i == index) {
                let current = self.page;
                self.content_bitmap(cx, index);
                // 预渲染不应把当前页挤出缓存
                if let Some(pos) = self.cache.iter().position(|(i, _)| *i == current) {
                    let entry = self.cache.remove(pos);
                    self.cache.push(entry);
                }
                return true;
            }
        }
        // 2. 继续分页 (一次一页)
        if !self.done() {
            if !self.paginate_step(cx) {
                // 排完: 页码 "n+" 变成确定值
                cx.request_redraw(RefreshHint::Ui);
            }
            return true;
        }
        false
    }
}

impl ReaderPage {
    /// 注释弹窗: 屏幕中部的卡片, 标题「注释」, 正文用章节字体 (注释文字和正文一样经过字形打乱)。
    fn draw_footnote(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap, n: u16) {
        let Some(note) = self.notes.get(n as usize - 1).cloned() else { return };
        let theme = cx.theme;
        let m = cx.app.metrics;
        let font = self.params.as_ref().map_or(m.body, |p| p.font_size as f32 * 0.85).round().max(m.small);
        let pad = m.margin as i32;
        let card_w = cx.width as i32 - 2 * pad;
        let text_w = (card_w - 2 * pad).max(100);
        let mut lines: Vec<String> = {
            let mut measure = FontMeasure::new(&*cx.fonts, &self.chain, &mut self.widths);
            note.split('\n').flat_map(|para| kn_text::wrap(para, font as u32, text_w as f64, true, &mut measure)).map(|(l, _)| l).collect()
        };
        let line_h = (font * 1.5).round() as i32;
        let title_h = (m.small * 1.6).round() as i32;
        let max_lines = ((cx.height as i32 * 7 / 10 - 2 * pad - title_h) / line_h).max(1) as usize;
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            if let Some(last) = lines.last_mut() {
                last.pop();
                last.push('…');
            }
        }
        let card_h = pad + title_h + lines.len() as i32 * line_h + pad / 4;
        let card = Rect::new(pad, (cx.height as i32 - card_h) / 2, card_w as u32, card_h as u32);
        frame.rounded_rect(card, m.radius, Some(theme.background), Some(theme.foreground), 3);
        let chain = cx.app.ui_fonts.clone();
        {
            let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
            ink.text(frame, card.x + pad, card.y + pad / 2, "注释", m.small, theme.muted);
            let hint = "轻触任意处关闭";
            let hw = ink.width(hint, m.tiny).ceil() as i32;
            ink.text(frame, card.x + card_w - pad - hw, card.y + pad / 2 + ((m.small - m.tiny) / 2.0) as i32, hint, m.tiny, theme.muted);
        }
        let metrics_chain = vec![cx.app.ui_fonts[0]];
        let ascent = cx.fonts.line_metrics(&TextStyle { fonts: metrics_chain, size_px: font }).ascent;
        let style = TextStyle { fonts: self.chain.clone(), size_px: font };
        let mut y = card.y + pad / 2 + title_h + (line_h - font as i32) / 2;
        for line in &lines {
            cx.glyphs.draw_text(&*cx.fonts, frame, (card.x + pad) as f32, (y as f32 + ascent).round(), line, &style, theme.foreground);
            y += line_h;
        }
    }
}

/// 书签图标: 竖长条, 下端 V 形缺口。filled = 实心 (有书签), 否则描边。
fn ribbon(frame: &mut Bitmap, r: Rect, filled: bool, line: f32, color: u8) {
    if r.w < 4 || r.h < 4 {
        return;
    }
    let notch = (r.w as f32 * 0.45).min(r.h as f32 * 0.4);
    let body = r.h as f32 - notch;
    let (x0, y0, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    if filled {
        frame.fill_rect(Rect::new(r.x, r.y, r.w, body as u32), color);
        // 缺口两侧的三角: 越往下, 中间的缺口越宽
        for dy in 0..notch.ceil() as i32 {
            let t = (dy as f32 + 0.5) / notch;
            let half = (w / 2.0 * t).round() as u32;
            let side = (r.w / 2).saturating_sub(half);
            let y = r.y + body as i32 + dy;
            frame.fill_rect(Rect::new(r.x, y, side, 1), color);
            frame.fill_rect(Rect::new(r.x + r.w as i32 - side as i32, y, side, 1), color);
        }
    } else {
        let mid = x0 + w / 2.0;
        let (l, rr) = (x0 + line / 2.0, x0 + w - line / 2.0);
        frame.line(l, y0 + line / 2.0, rr, y0 + line / 2.0, line, color);
        frame.line(l, y0, l, y0 + h, line, color);
        frame.line(rr, y0, rr, y0 + h, line, color);
        frame.line(l, y0 + h, mid, y0 + body, line, color);
        frame.line(mid, y0 + body, rr, y0 + h, line, color);
    }
}
