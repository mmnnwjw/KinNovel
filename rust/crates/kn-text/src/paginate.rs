//! 分页, 对应 reader.py `ReaderDocument._paginate` / `page_for_path` 等。

use std::collections::VecDeque;
use std::sync::Arc;

use crate::html::{Block, BlockKind, Inline};
use crate::style::{Align, InlineStyle, WEB_LINE_HEIGHT};
use crate::wrap::{wrap, wrap_widths, Measure};

#[derive(Debug, Clone, PartialEq)]
pub enum PageItem {
    Text {
        text: String,
        x: i64,
        y: i64,
        size: u32,
        path: String,
        offset: usize,
        /// 网页样式模式: 行内样式分段; None = 整行同一字号、默认样式 (兼容模式)
        rich: Option<Box<RichLine>>,
    },
    Image {
        url: String,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
        path: String,
        offset: usize,
    },
    /// 注释标记的点击区域 (标记本身由所在的文字行画出); `index` = `Document::notes` 序号 (从 1 起)
    Note { index: u16, x: i64, y: i64, width: i64, height: i64 },
    /// 分隔线 / 引用块左侧竖线
    Rule { x: i64, y: i64, width: i64, height: i64 },
    /// 无序列表的圆点 (左上角与直径)
    Dot { x: i64, y: i64, size: i64 },
}

/// 无序列表标记 (排版时画成 [`PageItem::Dot`])。
pub const BULLET: &str = "•";

/// 一行带样式的文字。
#[derive(Debug, Clone, PartialEq)]
pub struct RichLine {
    /// 行内最大字号 (基线按它定)
    pub max_size: u32,
    /// 行高 (像素)
    pub height: i64,
    pub runs: Vec<Run>,
}

/// 同一样式、同一字号的一段字符。
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// 在行文字里的字节区间
    pub start: usize,
    pub end: usize,
    /// 相对行起点的横坐标与宽度 (像素)
    pub x: f32,
    pub width: f32,
    pub size: u32,
    pub style: InlineStyle,
    /// 行内对象 (这段是一个 `OBJECT_CHAR`)
    pub object: Option<Inline>,
}

impl PageItem {
    pub fn path(&self) -> &str {
        match self {
            PageItem::Text { path, .. } => path,
            PageItem::Image { path, .. } => path,
            PageItem::Note { .. } | PageItem::Rule { .. } | PageItem::Dot { .. } => "",
        }
    }
    pub fn offset(&self) -> usize {
        match self {
            PageItem::Text { offset, .. } => *offset,
            PageItem::Image { offset, .. } => *offset,
            PageItem::Note { .. } | PageItem::Rule { .. } | PageItem::Dot { .. } => 0,
        }
    }
}

pub type Page = Vec<PageItem>;

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutParams {
    pub width: i64,
    pub height: i64,
    pub font_size: u32,
    pub line_spacing: f64,
    pub margin: i64,
    pub first_line_indent: bool,
    /// 段落之后的间距, 以行高为单位 (Python 版固定 0.22)
    pub paragraph_spacing: f64,
    /// 插图最大高度占可用高度的比例 (Python 版 0.62, 阅读器也用它)。
    pub image_max_ratio: f64,
    /// 封面占满一页: 本章第一张图且它前面没有正文时, 给它整页高度。阅读器只在每卷第一章打开
    /// (第一章开头的第一张图就是封面; 后面的彩页、插图照常与正文混排)。
    pub cover_full_page: bool,
    /// 用真实图片尺寸排版: `<img>` 没写宽高时读图片地址里的 `size=宽x高` (轻书架的图片地址都带);
    /// 宽度不到可用宽度一半的小图 (替代生僻字的"外字"等) 按原始像素大小显示, 不放大到整行宽。
    /// false = Python 规则 (金标准测试用)。
    pub real_image_size: bool,
    /// 严格标点禁则 (见 [`crate::wrap::wrap`]): 」』）不放行首, 标点串放不下时带上前一个字换行。
    /// false = Python 规则 (金标准测试用)。
    pub strict_kinsoku: bool,
    /// 网页样式模式 (块来自 `extract_document(.., rich: true)`): 粗体/字号/对齐/缩进/外边距/引用块/分隔线/
    /// 行内对象按网页版排; 行距、段距、首行缩进开关仍用阅读设置。false = Python 规则 (金标准测试用)。
    pub rich: bool,
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams {
            width: 1236,
            height: 1400,
            font_size: 48,
            line_spacing: 1.42,
            margin: 34,
            first_line_indent: true,
            paragraph_spacing: PYTHON_PARAGRAPH_SPACING,
            image_max_ratio: PYTHON_IMAGE_MAX_RATIO,
            cover_full_page: false,
            real_image_size: false,
            strict_kinsoku: false,
            rich: false,
        }
    }
}

/// Python 版 `reader.py` 的插图高度上限比例。
pub const PYTHON_IMAGE_MAX_RATIO: f64 = 0.62;
/// Python 版的段后间距 (行高的倍数)。
pub const PYTHON_PARAGRAPH_SPACING: f64 = 0.22;

const FIRST_LINE_INDENT_PREFIX: &str = "\u{3000}\u{3000}";

fn heading_size(body: u32, level: u8) -> u32 {
    let factor = (1.30 - level as f64 * 0.05).max(1.08);
    (body as f64 * factor) as u32
}

fn small_size(body: u32) -> u32 {
    (body as f64 * 0.82).max(20.0) as u32
}

/// 进度式分页器: 逐块喂入, `next_page` 排够一页就吐一页, 不必等整章排完。
/// 脚注在所有正文块处理完之后统一追加在最后 (对应 Python `_paginate` 末尾
/// 的脚注处理), 结尾的 "if current or not pages" 规则保证章节至少一页。
pub struct Paginator {
    /// 共享持有块 (阅读页把分页器存在页面结构体里, 不能借用; 重排时不必深拷贝整章)
    blocks: Arc<[Block]>,
    /// 下一个要排的块
    next_block: usize,
    params: LayoutParams,
    usable_width: f64,
    usable_height: i64,
    line_height: i64,
    heading_gap: i64,
    margin: i64,
    body_size: u32,
    small_size: u32,

    current: Page,
    y: i64,
    footnotes: Vec<(String, String, usize)>, // (text, path, offset)
    ready: VecDeque<Page>,

    blocks_done: bool,
    footnotes_done: bool,
    finished: bool,
    any_page_pushed: bool,
    /// 已经排过正文 / 插图 (判断封面用)
    seen_text: bool,
    seen_image: bool,
}

/// 图片地址里的 `size=宽x高` 提示 (轻书架 CDN 地址, 例如 `...jpg?placeholder=..&amp;size=1350x1920&amp;t=..`)。
pub fn url_size_hint(url: &str) -> Option<(u32, u32)> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let kv = kv.strip_prefix("amp;").unwrap_or(kv);
        let (w, h) = kv.strip_prefix("size=")?.split_once(['x', 'X'])?;
        let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
        (w > 0 && h > 0).then_some((w, h))
    })
}

impl Paginator {
    pub fn new(blocks: impl Into<Arc<[Block]>>, params: LayoutParams) -> Self {
        let usable_width = (params.width - 2 * params.margin).max(120) as f64;
        let usable_height = (params.height - 2 * params.margin).max(160);
        let body_size = params.font_size;
        let line_height = (((body_size as f64) * params.line_spacing) as i64).max(1);
        let heading_gap = (line_height as f64 * 0.5) as i64;
        Paginator {
            blocks: blocks.into(),
            next_block: 0,
            margin: params.margin,
            body_size,
            small_size: small_size(body_size),
            params,
            usable_width,
            usable_height,
            line_height,
            heading_gap,
            current: Vec::new(),
            y: 0,
            footnotes: Vec::new(),
            ready: VecDeque::new(),
            blocks_done: false,
            footnotes_done: false,
            finished: false,
            any_page_pushed: false,
            seen_text: false,
            seen_image: false,
        }
    }

    fn new_page(&mut self) {
        if !self.current.is_empty() {
            self.ready.push_back(std::mem::take(&mut self.current));
            self.any_page_pushed = true;
        }
        self.y = 0;
    }

    #[allow(clippy::too_many_arguments)]
    fn add_line(
        &mut self,
        m: &mut impl Measure,
        text: &str,
        size: u32,
        indent: bool,
        gap_before: i64,
        gap_after: i64,
        base_offset: usize,
        path: &str,
    ) {
        let actual_height = (((size as f64) * self.params.line_spacing) as i64).max(self.line_height);
        if self.y + gap_before + actual_height > self.usable_height && !self.current.is_empty() {
            self.new_page();
        }
        self.y += gap_before;
        let prefix = if indent { FIRST_LINE_INDENT_PREFIX } else { "" };
        let prefix_len = prefix.chars().count();
        let full = format!("{}{}", prefix, text);
        for (line, line_start) in wrap(&full, size, self.usable_width, self.params.strict_kinsoku, m) {
            if self.y + actual_height > self.usable_height && !self.current.is_empty() {
                self.new_page();
            }
            let offset = base_offset + line_start.saturating_sub(prefix_len);
            self.current.push(PageItem::Text {
                text: line,
                x: self.margin,
                y: self.margin + self.y,
                size,
                path: path.to_string(),
                offset,
                rich: None,
            });
            self.y += actual_height;
        }
        self.y += gap_after;
    }

    /// 网页样式模式的文字块。
    fn add_rich_block(&mut self, m: &mut impl Measure, block: &Block) {
        let st = &block.style;
        let body = self.body_size as f64;
        let fs = ((body * st.scale as f64 / 100.0).round() as u32).max(8);
        // 网页版正文行高 1.8em 对应阅读设置的行距, 其它行高按比例换算
        let lh_factor = (self.params.line_spacing * st.line_height.map_or(1.0, |l| l as f64 / WEB_LINE_HEIGHT as f64)).max(1.05);
        let left = st.inset_left as f64 * body;
        let right = st.inset_right as f64 * body;
        let avail = (self.usable_width - left - right).max(body * 4.0);

        let chars: Vec<char> = block.text.chars().map(|c| if c == '\u{a0}' { ' ' } else { c }).collect();
        let mut styles = vec![InlineStyle::default(); chars.len()];
        for span in &block.spans {
            for s in styles.iter_mut().take(span.end.min(chars.len())).skip(span.start) {
                *s = span.style;
            }
        }
        let mut objects: Vec<Option<&Inline>> = vec![None; chars.len()];
        for (at, object) in &block.objects {
            if let Some(slot) = objects.get_mut(*at) {
                *slot = Some(object);
            }
        }
        let sizes: Vec<u32> = styles.iter().map(|s| ((fs as f64 * s.scale as f64 / 100.0).round() as u32).max(6)).collect();
        let widths: Vec<f64> = (0..chars.len())
            .map(|i| {
                let size = sizes[i] as f64;
                match objects[i] {
                    Some(Inline::Note(_)) => size * 1.1,
                    Some(Inline::Image { width, height, .. }) => (fs as f64 * *width as f64 / (*height).max(1) as f64).min(avail),
                    Some(Inline::Space(em)) => fs as f64 * *em as f64 / 100.0,
                    None if styles[i].hidden || styles[i].ruby => 0.0,
                    None => m.char_width(sizes[i], chars[i]) as f64,
                }
            })
            .collect();
        let indent = match st.indent {
            None if self.params.first_line_indent => 2.0 * m.char_width(fs, '\u{3000}') as f64,
            None => 0.0,
            Some(em) => em as f64 * fs as f64,
        };
        let gap_before = if self.current.is_empty() { 0 } else { (st.margin_top as f64 * body) as i64 };
        let paragraph_gap = (self.line_height as f64 * self.params.paragraph_spacing.clamp(0.0, 3.0)) as i64;
        let gap_after = paragraph_gap.max((st.margin_bottom as f64 * body) as i64);

        self.y += gap_before;
        let lines = wrap_widths(&chars, &widths, avail, indent, self.params.strict_kinsoku);
        let line_count = lines.len();
        for (line_index, (line_text, start)) in lines.into_iter().enumerate() {
            // wrap 返回的行文字 = 从 start 起连续的字符去掉行尾空白
            let range = start..(start + line_text.chars().count()).min(chars.len());
            let max_size = range.clone().filter(|&i| !styles[i].hidden && !styles[i].ruby).map(|i| sizes[i]).max().unwrap_or(fs).max(fs * 3 / 4);
            let height = ((max_size as f64 * lh_factor) as i64).max(1);
            if self.y + height > self.usable_height && !self.current.is_empty() {
                self.new_page();
            }
            let line_w: f64 = widths[range.clone()].iter().sum();
            let first_indent = if line_index == 0 { indent } else { 0.0 };
            let x0 = left
                + first_indent
                + match st.align {
                    Align::Start => 0.0,
                    Align::Center => ((avail - first_indent - line_w) / 2.0).max(0.0),
                    Align::End => (avail - first_indent - line_w).max(0.0),
                };
            let item_x = self.margin + x0 as i64;
            let item_y = self.margin + self.y;
            let mut text = String::new();
            let mut runs: Vec<Run> = Vec::new();
            let mut x = 0.0f64;
            for i in range.clone() {
                let byte = text.len();
                text.push(chars[i]);
                let object = objects[i].cloned();
                let merge = object.is_none()
                    && runs.last().is_some_and(|r: &Run| r.object.is_none() && r.style == styles[i] && r.size == sizes[i]);
                if merge {
                    let r = runs.last_mut().expect("run");
                    r.end = text.len();
                    r.width += widths[i] as f32;
                } else {
                    runs.push(Run { start: byte, end: text.len(), x: x as f32, width: widths[i] as f32, size: sizes[i], style: styles[i], object });
                }
                x += widths[i];
            }
            for r in &runs {
                let rx = item_x + r.x as i64;
                if r.style.note > 0 {
                    // 点击区域: 整行高
                    self.current.push(PageItem::Note { index: r.style.note, x: rx, y: item_y, width: r.width.ceil() as i64, height });
                }
                if let Some(Inline::Image { url, .. }) = &r.object {
                    // 底边约在基线上
                    let h = fs as i64;
                    let top = item_y + ((max_size as f64 * 0.9) as i64 - h).max(0);
                    self.current.push(PageItem::Image {
                        url: url.clone(),
                        x: rx,
                        y: top,
                        width: (r.width as i64).max(1),
                        height: h,
                        path: block.path.clone(),
                        offset: block.logical_offset(start),
                    });
                }
            }
            if line_index == 0 && st.marker == BULLET {
                let d = ((fs as f64 * 0.3) as i64).max(4);
                let cx = self.margin + (left - fs as f64 * 0.9).max(0.0) as i64;
                let cy = item_y + (max_size as f64 * 0.55) as i64 - d / 2;
                self.current.push(PageItem::Dot { x: cx, y: cy, size: d });
            } else if line_index == 0 && !st.marker.is_empty() {
                let mw: f64 = st.marker.chars().map(|c| m.char_width(fs, c) as f64).sum();
                let mx = self.margin + (left - mw - fs as f64 * 0.4).max(0.0) as i64;
                self.current.push(PageItem::Text {
                    text: st.marker.clone(),
                    x: mx,
                    y: item_y,
                    size: fs,
                    path: String::new(),
                    offset: 0,
                    rich: Some(Box::new(RichLine {
                        max_size,
                        height,
                        runs: vec![Run { start: 0, end: st.marker.len(), x: 0.0, width: mw as f32, size: fs, style: styles.first().copied().unwrap_or_default(), object: None }],
                    })),
                });
            }
            let bar_h = if line_index + 1 == line_count { height + gap_after } else { height };
            self.quote_bars(st.quote, item_y, bar_h);
            self.current.push(PageItem::Text {
                text,
                x: item_x,
                y: item_y,
                size: fs,
                path: block.path.clone(),
                offset: block.logical_offset(start),
                rich: Some(Box::new(RichLine { max_size, height, runs })),
            });
            self.y += height;
        }
        self.y += gap_after;
    }

    /// 引用块左侧竖线 (每层一条)。
    fn quote_bars(&mut self, depth: u8, y: i64, height: i64) {
        let body = self.body_size as f64;
        for d in 0..depth as i64 {
            let x = self.margin + (d as f64 * 2.0 * body) as i64;
            let w = ((body * 0.12) as i64).max(2);
            self.current.push(PageItem::Rule { x, y, width: w, height });
        }
    }

    fn add_rule(&mut self, block: &Block) {
        let body = self.body_size as f64;
        let gap = self.line_height / 2;
        let h = ((body / 24.0) as i64).max(2);
        if self.y + 2 * gap + h > self.usable_height && !self.current.is_empty() {
            self.new_page();
        }
        let left = (block.style.inset_left as f64 * body) as i64;
        let right = (block.style.inset_right as f64 * body) as i64;
        let top = self.margin + self.y;
        self.quote_bars(block.style.quote, top, 2 * gap + h);
        self.current.push(PageItem::Rule {
            x: self.margin + left,
            y: top + gap,
            width: (self.usable_width as i64 - left - right).max(1),
            height: h,
        });
        self.y += 2 * gap + h;
    }

    fn process_block(&mut self, block: &Block, m: &mut impl Measure) {
        let path = block.path.clone();
        match block.kind {
            BlockKind::Image => {
                let max_image_height = (self.usable_height as f64 * self.params.image_max_ratio.clamp(0.1, 1.0)) as i64;
                let is_cover = self.params.cover_full_page && !self.seen_image && !self.seen_text;
                self.seen_image = true;
                let (natural_w, natural_h) = if block.width > 0 && block.height > 0 {
                    (block.width, block.height)
                } else if self.params.real_image_size {
                    url_size_hint(&block.source_url).unwrap_or((0, 0))
                } else {
                    (0, 0)
                };
                let mut image_width = self.usable_width as i64;
                let image_height = if is_cover {
                    self.usable_height
                } else if natural_w > 0 && natural_h > 0 {
                    if self.params.real_image_size && (natural_w as f64) < self.usable_width * 0.5 {
                        let h = (natural_h as i64).min(max_image_height).max(1);
                        image_width = ((natural_w as f64 * h as f64 / natural_h as f64) as i64).max(1);
                        h
                    } else {
                        let h = (self.usable_width * natural_h as f64 / natural_w as f64) as i64;
                        h.max(1).min(max_image_height)
                    }
                } else {
                    max_image_height.min((self.usable_width * 0.72) as i64)
                };
                if self.y + image_height > self.usable_height && !self.current.is_empty() {
                    self.new_page();
                }
                self.current.push(PageItem::Image {
                    url: block.source_url.clone(),
                    x: self.margin + (self.usable_width as i64 - image_width) / 2,
                    y: self.margin + self.y,
                    width: image_width,
                    height: image_height,
                    path: path.clone(),
                    offset: block.offset,
                });
                self.y += image_height + (self.line_height as f64 * 0.5) as i64;
            }
            BlockKind::Rule => self.add_rule(block),
            BlockKind::Text | BlockKind::Heading if self.params.rich => {
                if !block.text.trim().is_empty() {
                    self.seen_text = true;
                }
                self.add_rich_block(m, block);
            }
            BlockKind::Footnote => {
                self.footnotes.push((block.text.clone(), path, block.offset));
            }
            BlockKind::Heading => {
                let size = heading_size(self.body_size, block.level);
                let gap_before = if !self.current.is_empty() { self.heading_gap } else { 0 };
                let gap_after = (self.line_height as f64 * 0.35) as i64;
                self.add_line(m, &block.text, size, false, gap_before, gap_after, block.offset, &path);
            }
            BlockKind::Text => {
                if !block.text.trim().is_empty() {
                    self.seen_text = true;
                }
                let gap_after = (self.line_height as f64 * self.params.paragraph_spacing.clamp(0.0, 3.0)) as i64;
                self.add_line(
                    m,
                    &block.text,
                    self.body_size,
                    self.params.first_line_indent,
                    0,
                    gap_after,
                    block.offset,
                    &path,
                );
            }
        }
    }

    fn emit_footnotes(&mut self, m: &mut impl Measure) {
        if self.footnotes.is_empty() {
            return;
        }
        let first_path = self.footnotes[0].1.clone();
        self.add_line(m, "注释", self.small_size, false, self.heading_gap, 0, 0, &first_path);
        let footnotes = std::mem::take(&mut self.footnotes);
        for (text, path, offset) in footnotes {
            let gap_after = (self.line_height as f64 * 0.15) as i64;
            self.add_line(m, &text, self.small_size, false, 0, gap_after, offset, &path);
        }
    }

    /// 产出下一页, 可能需要先排几个块才有页可出; 整章排完返回 `None`。
    pub fn next_page(&mut self, m: &mut impl Measure) -> Option<Page> {
        loop {
            if let Some(p) = self.ready.pop_front() {
                return Some(p);
            }
            if self.finished {
                return None;
            }
            if !self.blocks_done {
                let blocks = Arc::clone(&self.blocks);
                match blocks.get(self.next_block) {
                    Some(block) => {
                        self.next_block += 1;
                        self.process_block(block, m);
                    }
                    None => self.blocks_done = true,
                }
                continue;
            }
            if !self.footnotes_done {
                self.footnotes_done = true;
                self.emit_footnotes(m);
                continue;
            }
            self.finished = true;
            if !self.current.is_empty() || !self.any_page_pushed {
                self.any_page_pushed = true;
                self.ready.push_back(std::mem::take(&mut self.current));
            }
        }
    }
}

/// 一次性排完整章, 结果与逐页调用 `next_page` 等价。
pub fn paginate_all(blocks: &[Block], params: LayoutParams, m: &mut impl Measure) -> Vec<Page> {
    let mut paginator = Paginator::new(blocks, params);
    let mut pages = Vec::new();
    while let Some(page) = paginator.next_page(m) {
        pages.push(page);
    }
    pages
}

/// 渐进分页时判断锚点 (XPath) 的页码是否已经确定: 带这个 XPath 的条目都已进了排好的页,
/// 这时对已排的页调用 [`page_for_path`] 与整章排完的结果相同。
///
/// 条目按块的顺序进页, 所以排好的页里出现了某个更靠后的块的条目, 前面的块就都排完了。
/// 脚注块的条目在整章最后才排 (`emit_footnotes`), 指向脚注的锚点要等整章排完。
#[derive(Debug, Default)]
pub struct AnchorIndex {
    /// XPath → (第一个块的下标, 最后一个块的下标, 是否有脚注块)
    paths: std::collections::HashMap<String, (usize, usize, bool)>,
    /// 已排好的页里出现过的块下标 (按 XPath 第一个块下标估计, 只会偏小) 的最大值 + 1
    passed: usize,
}

impl AnchorIndex {
    pub fn new(blocks: &[Block]) -> Self {
        let mut paths = std::collections::HashMap::<String, (usize, usize, bool)>::new();
        // 空 XPath (列表序号等) 不对应某个块, 不参与判断
        for (i, b) in blocks.iter().enumerate().filter(|(_, b)| !b.path.is_empty()) {
            let e = paths.entry(b.path.clone()).or_insert((i, i, false));
            e.1 = i;
            e.2 |= b.kind == BlockKind::Footnote;
        }
        AnchorIndex { paths, passed: 0 }
    }

    /// 从头重新分页时调用。
    pub fn reset(&mut self) {
        self.passed = 0;
    }

    /// 分页器吐出一页后调用。
    pub fn add_page(&mut self, page: &Page) {
        for item in page {
            if let Some(&(first, _, _)) = self.paths.get(item.path()) {
                self.passed = self.passed.max(first + 1);
            }
        }
    }

    /// `done` = 整章已排完。不认识的 XPath (不在任何块里, 或为空) 不会出现在页里, 直接算确定。
    pub fn settled(&self, path: &str, done: bool) -> bool {
        if done || path.is_empty() {
            return true;
        }
        match self.paths.get(path) {
            None => true,
            Some(&(_, _, true)) => false,
            Some(&(_, last, false)) => self.passed > last + 1,
        }
    }
}

/// 对应 `ReaderDocument.page_for_path`。
pub fn page_for_path(pages: &[Page], xpath: &str, offset: Option<i64>, missing: usize) -> usize {
    if xpath.is_empty() {
        return missing;
    }
    if let Some(target_offset) = offset {
        let mut first_page: Option<usize> = None;
        let mut best_page: Option<usize> = None;
        let mut best_offset: Option<i64> = None;
        for (index, page) in pages.iter().enumerate() {
            for item in page {
                if item.path() != xpath {
                    continue;
                }
                if first_page.is_none() {
                    first_page = Some(index);
                }
                let item_offset = item.offset() as i64;
                if item_offset > target_offset {
                    continue;
                }
                if best_offset.is_none() || item_offset > best_offset.unwrap() {
                    best_page = Some(index);
                    best_offset = Some(item_offset);
                }
            }
        }
        if let Some(p) = best_page {
            return p;
        }
        if let Some(p) = first_page {
            return p;
        }
        return missing;
    }
    for (index, page) in pages.iter().enumerate() {
        if page.iter().any(|item| item.path() == xpath) {
            return index;
        }
    }
    missing
}

/// 对应 `first_anchor_on_page`。
pub fn first_anchor_on_page(pages: &[Page], page_index: i64) -> (String, usize) {
    if pages.is_empty() {
        return (".".to_string(), 0);
    }
    let idx = page_index.max(0).min(pages.len() as i64 - 1) as usize;
    for item in &pages[idx] {
        if !item.path().is_empty() {
            return (item.path().to_string(), item.offset());
        }
    }
    (".".to_string(), 0)
}

/// 对应 `first_path_on_page`。
pub fn first_path_on_page(pages: &[Page], page_index: i64) -> String {
    if pages.is_empty() {
        return ".".to_string();
    }
    let idx = page_index.max(0).min(pages.len() as i64 - 1) as usize;
    for item in &pages[idx] {
        if !item.path().is_empty() {
            return item.path().to_string();
        }
    }
    ".".to_string()
}
