//! 分页, 对应 reader.py `ReaderDocument._paginate` / `page_for_path` 等。

use std::collections::VecDeque;

use crate::html::{Block, BlockKind};
use crate::wrap::{wrap, Measure};

#[derive(Debug, Clone, PartialEq)]
pub enum PageItem {
    Text {
        text: String,
        x: i64,
        y: i64,
        size: u32,
        path: String,
        offset: usize,
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
}

impl PageItem {
    pub fn path(&self) -> &str {
        match self {
            PageItem::Text { path, .. } => path,
            PageItem::Image { path, .. } => path,
        }
    }
    pub fn offset(&self) -> usize {
        match self {
            PageItem::Text { offset, .. } => *offset,
            PageItem::Image { offset, .. } => *offset,
        }
    }
}

pub type Page = Vec<PageItem>;

#[derive(Debug, Clone)]
pub struct LayoutParams {
    pub width: i64,
    pub height: i64,
    pub font_size: u32,
    pub line_spacing: f64,
    pub margin: i64,
    pub first_line_indent: bool,
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
        }
    }
}

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
    /// 自己持有块 (阅读页把分页器存在页面结构体里, 不能借用)
    blocks: std::vec::IntoIter<Block>,
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
}

impl Paginator {
    pub fn new(blocks: Vec<Block>, params: LayoutParams) -> Self {
        let usable_width = (params.width - 2 * params.margin).max(120) as f64;
        let usable_height = (params.height - 2 * params.margin).max(160);
        let body_size = params.font_size;
        let line_height = (((body_size as f64) * params.line_spacing) as i64).max(1);
        let heading_gap = (line_height as f64 * 0.5) as i64;
        Paginator {
            blocks: blocks.into_iter(),
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
        for (line, line_start) in wrap(&full, size, self.usable_width, m) {
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
            });
            self.y += actual_height;
        }
        self.y += gap_after;
    }

    fn process_block(&mut self, block: &Block, m: &mut impl Measure) {
        let path = block.path.clone();
        match block.kind {
            BlockKind::Image => {
                let max_image_height = (self.usable_height as f64 * 0.62) as i64;
                let image_height = if block.width > 0 && block.height > 0 {
                    let h = (self.usable_width * block.height as f64 / block.width as f64) as i64;
                    h.max(1).min(max_image_height)
                } else {
                    max_image_height.min((self.usable_width * 0.72) as i64)
                };
                if self.y + image_height > self.usable_height && !self.current.is_empty() {
                    self.new_page();
                }
                self.current.push(PageItem::Image {
                    url: block.source_url.clone(),
                    x: self.margin,
                    y: self.margin + self.y,
                    width: self.usable_width as i64,
                    height: image_height,
                    path: path.clone(),
                    offset: block.offset,
                });
                self.y += image_height + (self.line_height as f64 * 0.5) as i64;
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
                let gap_after = (self.line_height as f64 * 0.22) as i64;
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
                match self.blocks.next() {
                    Some(block) => self.process_block(&block, m),
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
    let mut paginator = Paginator::new(blocks.to_vec(), params);
    let mut pages = Vec::new();
    while let Some(page) = paginator.next_page(m) {
        pages.push(page);
    }
    pages
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
