//! 搜索 (从发现页顶栏的放大镜进入), 对照网页版 `pages/Search.vue` + `components/SearchInput.vue`:
//! 小说 / 漫画两个标签, 搜索维度 模糊 (默认) / 精确 / 书名 / 作者 / 系列 / 标签; 换标签或维度时用同一关键词重新搜索。
//! 书籍详情页点作者 / 标签也进入这里 (网页版 BookInfo 的链接)。
//!
//! 两种状态:
//! - 输入: 底部是屏幕键盘 (`kn_ui::keyboard`, 拼音输入参照 KOReader), 上方是最近搜索 (本地, 最多 10 条, 存 config `search_history`)。
//! - 结果: 顶部是搜索框 (点它回到输入), 下面是结果列表 + 分页, 每页条数按屏幕能放下的行数。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::keyboard::{EditorEvent, TextEditor};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};
use serde_json::Value;

use super::book::BookDetailPage;
use crate::api::{self, Filters, ListPage, SearchMode};
use crate::covers::CoverCache;
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_FIELD: HitId = HitId(5);
const HIT_HISTORY_CLEAR: HitId = HitId(6);
const SEG_KIND_BASE: u32 = 10;
const SEG_MODE_BASE: u32 = 20;
const HISTORY_BASE: u32 = 100;
const ROW_BASE: u32 = 1000;
const EDITOR_BASE: u32 = 5000;

const HISTORY_KEY: &str = "search_history";
const HISTORY_MAX: usize = 10;
const KINDS: [&str; 2] = ["小说", "漫画"];

enum Results {
    /// 还没搜过
    Idle,
    Loading,
    Offline,
    Error(String),
    Ready(ListPage),
}

/// (请求序号, 结果)
struct Loaded(u64, Result<ListPage, String>);

pub struct SearchPage {
    editor: TextEditor,
    editing: bool,
    comic: bool,
    mode: SearchMode,
    /// 最近一次提交的关键词
    query: String,
    page: i64,
    results: Results,
    request: u64,
    covers: CoverCache,
}

impl Default for SearchPage {
    fn default() -> Self {
        SearchPage {
            editor: TextEditor::new(EDITOR_BASE, ""),
            editing: true,
            comic: false,
            mode: SearchMode::Fuzzy,
            query: String::new(),
            page: 1,
            results: Results::Idle,
            request: 0,
            covers: CoverCache::default(),
        }
    }
}

impl SearchPage {
    /// 直接搜索 (书籍详情页的作者 / 标签链接)。
    pub fn with_query(keywords: &str, mode: SearchMode, comic: bool) -> Self {
        SearchPage { editor: TextEditor::new(EDITOR_BASE, keywords), editing: false, comic, mode, query: keywords.to_string(), ..SearchPage::default() }
    }

    fn search(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if self.query.is_empty() {
            self.results = Results::Idle;
            return;
        }
        self.request += 1;
        self.page = page.max(1);
        if cx.app.net().is_none() && !api::fake_mode() {
            self.results = Results::Offline;
            return;
        }
        self.results = Results::Loading;
        let net = cx.app.net();
        let (query, mode, comic, page, id) = (self.query.clone(), self.mode, self.comic, self.page, self.request);
        let size = per_page(cx) as i64;
        let filters = Filters::from_config(&cx.app.config);
        cx.spawn(move || {
            let r = if comic {
                api::search_comics(net.as_ref(), &query, mode, page, size, filters)
            } else {
                api::search_books(net.as_ref(), &query, mode, page, size, filters)
            };
            Loaded(id, r)
        });
    }

    fn submit(&mut self, cx: &mut Cx<KinNovel>, keywords: &str) {
        let keywords = keywords.trim().to_string();
        if keywords.is_empty() {
            return;
        }
        let mut history = cx.app.config.strings(HISTORY_KEY);
        history.retain(|h| h != &keywords);
        history.insert(0, keywords.clone());
        history.truncate(HISTORY_MAX);
        cx.app.config.set(HISTORY_KEY, Value::from(history));
        self.editor.set_text(&keywords);
        self.query = keywords;
        self.editing = false;
        self.search(cx, 1);
        // 键盘收起是大面积变化, 区域闪刷清掉残影
        cx.request_redraw(RefreshHint::Clean);
    }

    fn start_editing(&mut self, cx: &mut Cx<KinNovel>) {
        self.editing = true;
        cx.request_redraw(RefreshHint::Clean);
    }

    /// 返回: 有结果时先收起键盘, 否则离开本页。
    fn back(&mut self, cx: &mut Cx<KinNovel>) -> Transition<KinNovel> {
        if self.editing && !self.query.is_empty() {
            self.editor.finish();
            self.editor.set_text(&self.query);
            self.editing = false;
            cx.request_redraw(RefreshHint::Clean);
            return Transition::None;
        }
        Transition::Back
    }

    fn turn(&mut self, cx: &mut Cx<KinNovel>, delta: i64) {
        let total = match &self.results {
            Results::Ready(list) => list.total_pages.max(1),
            _ => return,
        };
        let page = (self.page + delta).clamp(1, total);
        if page != self.page {
            self.search(cx, page);
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    /// 两行分段 (小说/漫画, 搜索维度), 返回下一行的 y。
    #[allow(clippy::too_many_arguments)]
    fn draw_options(&self, ink: &mut Ink, frame: &mut Bitmap, hits: &mut kn_ui::Hits, theme: &kn_ui::Theme, m: &widgets::Metrics, width: u32, y: i32) -> i32 {
        let side = m.margin as i32;
        let gap = m.margin as i32 / 3;
        let r = Rect::new(side, y, width - 2 * m.margin, m.touch);
        widgets::segmented(ink, frame, hits, theme, m, r, &KINDS, usize::from(self.comic), SEG_KIND_BASE);
        let r2 = Rect::new(side, r.bottom() + gap, width - 2 * m.margin, m.touch);
        let labels: Vec<&str> = SearchMode::ALL.iter().map(|md| md.label()).collect();
        let active = SearchMode::ALL.iter().position(|md| *md == self.mode).unwrap_or(0);
        widgets::segmented(ink, frame, hits, theme, m, r2, &labels, active, SEG_MODE_BASE);
        r2.bottom() + gap
    }
}

/// 结果模式下一页放几行 (与 render 的版式一致)。
fn per_page(cx: &Cx<KinNovel>) -> usize {
    let m = cx.app.metrics;
    let gap = m.margin as i32 / 3;
    let top = m.header_h() as i32 + gap + m.touch as i32 + gap + 2 * (m.touch as i32 + gap);
    let area = cx.height as i32 - top - m.pager_h() as i32;
    (area / m.row_h_cover() as i32).max(1) as usize
}

impl Page<KinNovel> for SearchPage {
    fn id(&self) -> PageId {
        PageId(350)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning && !self.query.is_empty() && matches!(self.results, Results::Idle) {
            self.search(cx, 1);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<crate::covers::CoverLoaded>() {
            Ok(loaded) => {
                self.covers.on_loaded(*loaded);
                if !self.editing {
                    cx.request_redraw(RefreshHint::Ui);
                }
                return;
            }
            Err(other) => other,
        };
        if let Ok(l) = msg.downcast::<Loaded>() {
            if l.0 == self.request {
                self.results = match l.1 {
                    Ok(list) => {
                        self.page = list.page.max(1);
                        Results::Ready(list)
                    }
                    Err(e) => Results::Error(e),
                };
                if !self.editing {
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let side = m.margin as i32;
        let gap = m.margin as i32 / 3;

        // 封面预取 (cx 整体借用, 必须在创建 `ink` 之前)
        let items = match (&self.results, self.editing) {
            (Results::Ready(list), false) => list.items.clone(),
            _ => Vec::new(),
        };
        let cover_slot = widgets::cover_slot(&m, Rect::new(0, 0, cx.width, m.row_h_cover()));
        for book in &items {
            self.covers.request(cx, &book.cover, cover_slot.w, cover_slot.h);
        }
        let history = cx.app.config.strings(HISTORY_KEY);

        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "搜索", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));

        if self.editing {
            let y = self.draw_options(&mut ink, frame, cx.hits, &theme, &m, cx.width, bar.bottom() + gap);
            let kb_h = TextEditor::height(&m);
            let kb = Rect::new(0, cx.height as i32 - kb_h as i32, cx.width, kb_h);
            let placeholder = match (self.mode, self.comic) {
                (SearchMode::Tags, _) => "多个标签用逗号分隔",
                (_, true) => "搜索漫画",
                (_, false) => "搜索小说",
            };
            self.editor.render(&mut ink, frame, cx.hits, &theme, &m, kb, placeholder, "搜索");

            // 最近搜索: 一行一行排的标签, 放不下的不画
            let area = Rect::new(0, y, cx.width, (kb.y - y).max(0) as u32);
            let label_h = (m.small * 1.8) as i32;
            if history.is_empty() {
                let tip = "拼音输入：空格上屏首选，点候选选字；\n“符号”切换标点，“中/英”切换输入语言";
                let mut ty = area.y + gap;
                for line in tip.lines() {
                    let t = ink.fit(line, m.small, (cx.width - 2 * m.margin) as f32);
                    if ty + label_h > area.bottom() {
                        break;
                    }
                    ink.text(frame, side, ty, &t, m.small, theme.muted);
                    ty += label_h;
                }
                return;
            }
            if area.h as i32 >= label_h + m.touch as i32 {
                ink.text(frame, side, area.y + gap, "最近搜索", m.small, theme.muted);
                let clear_w = (ink.width("清除", m.small) as i32 + side).max(m.touch as i32);
                let clear = Rect::new(cx.width as i32 - side - clear_w, area.y, clear_w as u32, label_h as u32 + gap as u32);
                ink.text_centered(frame, clear, "清除", m.small, theme.foreground);
                cx.hits.add(HIT_HISTORY_CLEAR, clear);
                let chip_h = (m.touch as f32 * 0.8) as i32;
                let (mut x, mut y) = (side, area.y + gap + label_h);
                for (i, h) in history.iter().enumerate() {
                    let text = ink.fit(h, m.small, (cx.width as i32 - 2 * side - 2 * side / 2) as f32);
                    let w = ink.width(&text, m.small).ceil() as i32 + side;
                    if x + w > cx.width as i32 - side {
                        x = side;
                        y += chip_h + gap;
                    }
                    if y + chip_h > area.bottom() - gap {
                        break;
                    }
                    let r = Rect::new(x, y, w as u32, chip_h as u32);
                    widgets::button(&mut ink, frame, &theme, &m, r, &text, ButtonStyle::Secondary);
                    cx.hits.add(HitId(HISTORY_BASE + i as u32), r).rounded(m.radius);
                    x += w + gap;
                }
            }
            return;
        }

        // 结果模式: 搜索框 (点了回到输入)
        let field = Rect::new(side, bar.bottom() + gap, cx.width - 2 * m.margin, m.touch);
        frame.rounded_rect(field, m.radius, Some(theme.background), Some(theme.foreground), 2);
        let (text, color) = if self.query.is_empty() { ("点此输入关键词".to_string(), theme.muted) } else { (self.query.clone(), theme.foreground) };
        let label = ink.fit(&text, m.body, (field.w - m.margin) as f32);
        let line_h = (m.body * 1.25) as i32;
        ink.text(frame, field.x + side / 2, field.y + (field.h as i32 - line_h) / 2, &label, m.body, color);
        cx.hits.add(HIT_FIELD, field).rounded(m.radius);
        let y = self.draw_options(&mut ink, frame, cx.hits, &theme, &m, cx.width, field.bottom() + gap);

        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        let area = Rect::new(0, y, cx.width, (list_bottom - y).max(0) as u32);
        match &self.results {
            Results::Idle => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "输入关键词开始搜索", None),
            Results::Loading => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "搜索中…", None),
            Results::Offline => widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法搜索", Some(("重试", HIT_RETRY))),
            Results::Error(e) => {
                let text = format!("搜索失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
            }
            Results::Ready(list) => {
                if list.items.is_empty() {
                    let what = if self.comic { "漫画" } else { "小说" };
                    let text = format!("没有找到相关{what}\n可以换个关键词或搜索维度");
                    widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, None);
                } else {
                    let row_h = m.row_h_cover();
                    for (row, book) in list.items.iter().enumerate() {
                        let r = Rect::new(0, area.y + row as i32 * row_h as i32, cx.width, row_h);
                        if r.bottom() > area.bottom() {
                            break;
                        }
                        let slot = widgets::cover_slot(&m, r);
                        let img = self.covers.get(&book.cover, slot.w, slot.h);
                        widgets::list_row_cover(&mut ink, frame, &theme, &m, r, img, &book.title, &book.subtitle(), &book.last_update);
                        cx.hits.add(HitId(ROW_BASE + row as u32), r);
                    }
                }
                if list.total_pages > 1 {
                    let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
                    widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (self.page - 1).max(0) as usize, list.total_pages as usize, HIT_PREV, HIT_NEXT);
                }
            }
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind == GestureKind::Down {
            return Transition::None;
        }
        let hit = cx.hits.at(Point { x: g.start.0, y: g.start.1 });
        if self.editing {
            if let Some(hit) = hit.filter(|h| self.editor.owns(*h)) {
                match self.editor.on_gesture(hit, g.kind) {
                    EditorEvent::None => {}
                    EditorEvent::Edited => cx.request_redraw(RefreshHint::Ui),
                    EditorEvent::Submit => {
                        let text = self.editor.text();
                        if text.trim().is_empty() {
                            cx.request_redraw(RefreshHint::Ui);
                        } else {
                            self.submit(cx, &text);
                        }
                    }
                }
                return Transition::None;
            }
        } else if matches!(g.kind, GestureKind::SwipeUp | GestureKind::SwipeLeft) {
            self.turn(cx, 1);
            return Transition::None;
        } else if matches!(g.kind, GestureKind::SwipeDown | GestureKind::SwipeRight) {
            self.turn(cx, -1);
            return Transition::None;
        }
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(HitId(id)) = hit else { return Transition::None };
        match HitId(id) {
            HIT_BACK => return self.back(cx),
            HIT_RETRY => {
                self.search(cx, self.page);
                cx.request_redraw(RefreshHint::Ui);
            }
            HIT_PREV => self.turn(cx, -1),
            HIT_NEXT => self.turn(cx, 1),
            HIT_FIELD => self.start_editing(cx),
            HIT_HISTORY_CLEAR => {
                cx.app.config.set(HISTORY_KEY, Value::Array(Vec::new()));
                cx.request_redraw(RefreshHint::Ui);
            }
            _ if (SEG_KIND_BASE..SEG_KIND_BASE + 2).contains(&id) => {
                let comic = id == SEG_KIND_BASE + 1;
                if comic != self.comic {
                    self.comic = comic;
                    if !self.editing {
                        self.search(cx, 1);
                    }
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
            _ if (SEG_MODE_BASE..SEG_MODE_BASE + SearchMode::ALL.len() as u32).contains(&id) => {
                let mode = SearchMode::ALL[(id - SEG_MODE_BASE) as usize];
                if mode != self.mode {
                    self.mode = mode;
                    if !self.editing {
                        self.search(cx, 1);
                    }
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
            _ if (HISTORY_BASE..HISTORY_BASE + HISTORY_MAX as u32).contains(&id) => {
                if let Some(h) = cx.app.config.strings(HISTORY_KEY).get((id - HISTORY_BASE) as usize).cloned() {
                    self.submit(cx, &h);
                }
            }
            _ if id >= ROW_BASE && id < EDITOR_BASE => {
                if let Results::Ready(list) = &self.results {
                    if let Some(book) = list.items.get((id - ROW_BASE) as usize) {
                        return Transition::Push(Box::new(BookDetailPage::new(book.id)));
                    }
                }
            }
            _ => {}
        }
        Transition::None
    }
}
