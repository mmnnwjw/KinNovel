//! HTML -> Block 列表, 对应 reader.py 的 `sanitize_html` / `extract_blocks`。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use html5ever::driver::ParseOpts;
use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, QualName};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom};

use crate::clean::{clean_text, py_isspace};
use crate::style::{Align, BlockStyle, Element, InlineStyle};

const BLOCK_TAGS: &[&str] = &[
    "p", "div", "section", "article", "blockquote", "h1", "h2", "h3", "h4", "h5", "h6", "li",
    "dt", "dd", "pre", "figcaption", "aside",
];
const HEADING_TAGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];
const SKIP_TAGS: &[&str] = &["script", "style", "iframe", "object", "embed", "svg", "canvas"];
/// 网页样式模式下额外当作块的元素 (浏览器默认 display:block / table-row)。
const RICH_BLOCK_TAGS: &[&str] = &[
    "center", "figure", "header", "footer", "main", "nav", "address", "ul", "ol", "dl", "table", "tr", "caption",
    "details", "summary", "hgroup",
];
/// 网页样式模式下可以放行内小图 (外字等) 的块。
const TEXT_BLOCK_TAGS: &[&str] = &["p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "dt", "dd", "td", "th", "figcaption"];
/// 行内小图的最大原始高度 (像素): 更高的仍当插图另起一块。
const INLINE_IMAGE_MAX: u32 = 96;

/// 行内对象在块文字里的占位字符 (不计入阅读位置的字符偏移)。
pub const OBJECT_CHAR: char = '\u{FFFC}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Heading,
    Footnote,
    Image,
    /// `<hr>` 分隔线 (只在网页样式模式下产生)
    Rule,
}

impl BlockKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockKind::Text => "text",
            BlockKind::Heading => "heading",
            BlockKind::Footnote => "footnote",
            BlockKind::Image => "image",
            BlockKind::Rule => "rule",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
    pub level: u8,
    pub path: String,
    pub offset: usize,
    pub source_url: String,
    pub width: u32,
    pub height: u32,
    /// 行内样式 (字符下标区间, 覆盖整段文字; 空 = 全部默认样式)。只在网页样式模式下有
    pub spans: Vec<Span>,
    /// 行内对象: (在 `text` 里的字符下标, 对象); 对应字符是 [`OBJECT_CHAR`]
    pub objects: Vec<(usize, Inline)>,
    pub style: BlockStyle,
}

/// 一段行内样式, `start..end` 是 `Block::text` 的字符下标。
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: InlineStyle,
}

/// 行内对象。
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// 注释图标; n = `Document::notes` 的序号 (从 1 起), 0 = 找不到注释内容
    Note(u16),
    /// 行内小图 (外字等), 与文字同高; 宽高是图片本身的尺寸
    Image { url: String, width: u32, height: u32 },
    /// 空白 (em 的百分比), 表格单元格之间
    Space(u16),
}

impl Block {
    /// 字符下标 → 阅读位置的字符偏移 (行内对象不计)。
    pub fn logical_offset(&self, index: usize) -> usize {
        self.offset + index - self.objects.iter().filter(|(at, _)| *at < index).count()
    }
}

/// 一章的排版输入: 块 + 注释内容 (点击正文里的注释标记弹出)。
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub notes: Vec<String>,
}

impl Default for Block {
    fn default() -> Self {
        Block {
            kind: BlockKind::Text,
            text: String::new(),
            level: 0,
            path: ".".to_string(),
            offset: 0,
            source_url: String::new(),
            width: 0,
            height: 0,
            spans: Vec::new(),
            objects: Vec::new(),
            style: BlockStyle::default(),
        }
    }
}

/// 对应 utils.py 的 `absolute_url`: 用 base 的 scheme+host 解析相对/根相对/
/// 协议相对 URL; base 在本应用中始终是无路径的站点根 (如
/// "https://www.lightnovel.app"), 因此不需要完整的 RFC 3986 合并算法。
pub fn absolute_url(base: &str, value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return value.to_string();
    }
    let (scheme, rest) = match base.find("://") {
        Some(i) => (&base[..i], &base[i + 3..]),
        None => ("https", base),
    };
    if value.starts_with("//") {
        return format!("{}:{}", scheme, value);
    }
    // 其它 scheme 的绝对 URL (javascript:, data:, mailto: ...): urljoin 对
    // 与 base scheme 不同的绝对引用原样返回, 不会拼到站点域名下面。
    if has_scheme(value) {
        return value.to_string();
    }
    let host = rest.trim_end_matches('/');
    if value.starts_with('/') {
        return format!("{}://{}{}", scheme, host, value);
    }
    format!("{}://{}/{}", scheme, host, value)
}

fn has_scheme(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' {
            return i > 0;
        }
        if b == b'/' || b == b'?' || b == b'#' {
            return false;
        }
        if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') {
            return false;
        }
    }
    false
}

fn parse_py_int(s: &str) -> i64 {
    let t = s.trim();
    if t.is_empty() {
        return 0;
    }
    match t.parse::<i64>() {
        Ok(v) => v,
        Err(_) => 0,
    }
}

fn parse_dim(attrs: &HashMap<String, String>, name: &str) -> u32 {
    let raw = attrs.get(name).map(|s| s.as_str()).unwrap_or("0");
    let raw = if raw.is_empty() { "0" } else { raw };
    let v = parse_py_int(raw);
    v.max(0) as u32
}

fn child_struct_path(parent_path: &str, tag: &str, index: usize) -> String {
    if parent_path == "." {
        format!("./{}[{}]", tag, index)
    } else {
        format!("{}/{}[{}]", parent_path, tag, index)
    }
}

fn anchor_path(attrs: &HashMap<String, String>, struct_path: &str) -> String {
    match attrs.get("id") {
        Some(id) if !id.is_empty() => format!("//*[@id=\"{}\"]", id.replace('"', "\\\"")),
        _ => struct_path.to_string(),
    }
}

fn tag_name(name: &QualName) -> String {
    name.local.to_string().to_ascii_lowercase()
}

fn element_attrs(handle: &Handle) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let NodeData::Element { attrs, .. } = &handle.data {
        for a in attrs.borrow().iter() {
            out.insert(
                a.name.local.to_string().to_ascii_lowercase(),
                a.value.to_string(),
            );
        }
    }
    out
}

fn is_element(handle: &Handle) -> bool {
    matches!(handle.data, NodeData::Element { .. })
}

/// 在整棵树里查找 id="kinnovel-root" 的元素 (对应 lxml 的
/// `.//*[@id='kinnovel-root']`)。
fn find_root(handle: &Handle) -> Option<Handle> {
    if is_element(handle) {
        let attrs = element_attrs(handle);
        if attrs.get("id").map(|s| s.as_str()) == Some("kinnovel-root") {
            return Some(handle.clone());
        }
    }
    for child in handle.children.borrow().iter() {
        if let Some(found) = find_root(child) {
            return Some(found);
        }
    }
    None
}

/// 解析并返回 (RcDom, 根节点)。注意: markup5ever_rcdom 的 `RcDom::Drop` 会递归清空
/// 每个节点的 children 以打破 Rc 环, 所以调用方必须让返回的 `RcDom`和这里拿到的
/// `Handle` 一起存活, 不能只留 `Handle` 让 `RcDom` 先被析构 (否则子树会被清空)。
pub fn sanitize_html(content: &str) -> (RcDom, Handle) {
    let html = format!("<div id='kinnovel-root'>{}</div>", content);
    let dom: RcDom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .expect("html parsing never fails (recover mode)");
    let root = find_root(&dom.document).unwrap_or_else(|| dom.document.clone());
    (dom, root)
}

/// 收集中的一段文字: 每个字符的样式编号 (`Extractor::styles` 的下标) 与其中的行内对象。
#[derive(Default)]
struct Buf {
    text: String,
    styles: Vec<u16>,
    objects: Vec<Inline>,
}

impl Buf {
    fn push_str(&mut self, s: &str, style: u16, rich: bool) {
        for c in s.chars() {
            // 原文里的对象占位符会和行内对象混淆
            if rich && c == OBJECT_CHAR {
                continue;
            }
            self.text.push(c);
            self.styles.push(style);
        }
    }

    fn push_object(&mut self, object: Inline, style: u16) {
        self.text.push(OBJECT_CHAR);
        self.styles.push(style);
        self.objects.push(object);
    }

    fn clear(&mut self) {
        self.text.clear();
        self.styles.clear();
        self.objects.clear();
    }
}

/// 与 [`clean_text`] 逐字相同的清理, 同时保留每个字符的样式编号。
fn clean_styled(text: &str, styles: &[u16]) -> (String, Vec<u16>) {
    let mut chars: Vec<(char, u16)> = Vec::with_capacity(styles.len());
    let mut last_was_fold_space = false;
    for (c, &s) in text.chars().zip(styles) {
        match c {
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{ad}' | '\u{2060}' => continue,
            ' ' | '\t' | '\r' | '\x0c' | '\x0b' => {
                if last_was_fold_space {
                    continue;
                }
                chars.push((' ', s));
                last_was_fold_space = true;
            }
            _ => {
                chars.push((c, s));
                last_was_fold_space = false;
            }
        }
    }
    let start = chars.iter().position(|(c, _)| !py_isspace(*c)).unwrap_or(chars.len());
    let end = chars.iter().rposition(|(c, _)| !py_isspace(*c)).map_or(start, |i| i + 1);
    let kept = &chars[start..end.max(start)];
    (kept.iter().map(|(c, _)| *c).collect(), kept.iter().map(|(_, s)| *s).collect())
}

/// 网页样式模式下沿树向下传递的上下文。
#[derive(Clone)]
struct Ctx {
    style: InlineStyle,
    align: Align,
    /// 块字号 (正文字号的百分比)
    scale: u16,
    quote: u8,
    inset_left: f32,
    inset_right: f32,
    /// 在段落一类的文字块里 (可以放行内小图)
    in_text: bool,
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx {
            style: InlineStyle::default(),
            align: Align::Start,
            scale: 100,
            quote: 0,
            inset_left: 0.0,
            inset_right: 0.0,
            in_text: false,
        }
    }
}

struct Extractor<'a> {
    base_url: &'a str,
    blocks: Vec<Block>,
    /// 本章已出现的注释标记数 (见 [`is_footnote_icon`], 兼容模式用)
    footnotes: usize,
    /// 网页样式模式
    rich: bool,
    /// 样式表, 下标 = 样式编号; 0 = 默认样式
    styles: Vec<InlineStyle>,
    /// 注释标记 `<a>` → 注释序号 (从 1 起)
    markers: HashMap<*const Node, u16>,
    /// 被注释标记引用的注释正文: 不排进正文 (网页版同样隐藏, 点击标记时弹出)
    note_targets: HashSet<*const Node>,
}

/// 多看格式的注释图标 `<a class="duokan-footnote"><sup><img class="footnote" src="/img/note.png"></sup></a>`:
/// 网页版把它画成 1em 高的行内小图标 (图标是网页版自己的静态文件)。它不是插图, 当作插图会把段落从中间
/// 切开、还要去下载。兼容模式改成行内文字标记「[注n]」(n = 本章第几个, 对应章末 aside 里的「注n：…」),
/// 网页样式模式画成注释图标, 点击弹出注释。
fn is_footnote_icon(attrs: &HashMap<String, String>, raw_src: &str) -> bool {
    let class_footnote = attrs.get("class").is_some_and(|c| c.split_ascii_whitespace().any(|c| c.eq_ignore_ascii_case("footnote")));
    let src = raw_src.split(['?', '#']).next().unwrap_or("");
    class_footnote || (!src.contains("://") && src.ends_with("/img/note.png"))
}

fn image_src(attrs: &HashMap<String, String>) -> String {
    attrs
        .get("src")
        .filter(|s| !s.is_empty())
        .or_else(|| attrs.get("data-system-image-url"))
        .cloned()
        .unwrap_or_default()
}

/// `%XX` 解码 (注释链接的锚点可能是转义过的中文 id)。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// 注释正文的纯文字: 块之间、`<br>` 处换行, 每行按正文规则清理。
fn note_text(node: &Handle) -> String {
    fn walk(node: &Handle, out: &mut String) {
        match &node.data {
            NodeData::Text { contents } => out.push_str(&contents.borrow()),
            NodeData::Element { name, .. } => {
                let tag = tag_name(name);
                if SKIP_TAGS.contains(&tag.as_str()) {
                    return;
                }
                let attrs = element_attrs(node);
                // 返回正文的链接 (↩︎) 和注释图标不要
                if attrs.get("class").is_some_and(|c| c.contains("footnote-backref")) {
                    return;
                }
                if tag == "img" {
                    return;
                }
                if tag == "br" {
                    out.push('\n');
                    return;
                }
                let block = BLOCK_TAGS.contains(&tag.as_str()) || RICH_BLOCK_TAGS.contains(&tag.as_str());
                if block {
                    out.push('\n');
                }
                for child in node.children.borrow().iter() {
                    walk(child, out);
                }
                if block {
                    out.push('\n');
                }
            }
            _ => {
                for child in node.children.borrow().iter() {
                    walk(child, out);
                }
            }
        }
    }
    let mut raw = String::new();
    walk(node, &mut raw);
    raw.split('\n').map(clean_text).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// 找出注释标记和它们指向的注释 (对应网页版 `prepareReaderFootnotes`): `.duokan-footnote` 链接,
/// 也认 EPUB3 的 `epub:type="noteref"` 和 markdown-it 的 `<sup class="footnote-ref"><a href="#fn1">`。
/// 目标按 id 查找 (重复的 id 取文档里的第一个, 同 `querySelector`)。
fn find_notes(root: &Handle) -> (HashMap<*const Node, u16>, HashSet<*const Node>, Vec<String>) {
    fn walk(node: &Handle, in_ref_sup: bool, ids: &mut HashMap<String, Handle>, anchors: &mut Vec<(Handle, String)>) {
        let NodeData::Element { name, .. } = &node.data else {
            for child in node.children.borrow().iter() {
                walk(child, false, ids, anchors);
            }
            return;
        };
        let tag = tag_name(name);
        let attrs = element_attrs(node);
        if let Some(id) = attrs.get("id").filter(|id| !id.is_empty()) {
            ids.entry(id.clone()).or_insert_with(|| node.clone());
        }
        let has_class = |c: &str| attrs.get("class").is_some_and(|v| v.split_ascii_whitespace().any(|x| x.eq_ignore_ascii_case(c)));
        if tag == "a" {
            if let Some(href) = attrs.get("href").and_then(|h| h.strip_prefix('#')).filter(|h| !h.is_empty()) {
                let noteref = has_class("duokan-footnote")
                    || attrs.get("epub:type").or_else(|| attrs.get("type")).is_some_and(|t| t.contains("noteref"))
                    || attrs.get("role").is_some_and(|r| r == "doc-noteref")
                    || in_ref_sup;
                if noteref {
                    anchors.push((node.clone(), percent_decode(href)));
                }
            }
        }
        let ref_sup = tag == "sup" && has_class("footnote-ref");
        for child in node.children.borrow().iter() {
            walk(child, ref_sup, ids, anchors);
        }
    }
    let mut ids = HashMap::new();
    let mut anchors = Vec::new();
    walk(root, false, &mut ids, &mut anchors);

    let mut markers = HashMap::new();
    let mut targets: HashMap<*const Node, u16> = HashMap::new();
    let mut notes = Vec::new();
    for (anchor, id) in anchors {
        let Some(target) = ids.get(&id) else { continue };
        // 指向自己或自己的祖先的链接不是注释
        let mut up = Some(anchor.clone());
        let mut inside = false;
        while let Some(n) = up {
            if Rc::ptr_eq(&n, target) {
                inside = true;
                break;
            }
            up = n.parent.take().and_then(|w| {
                let p = w.upgrade();
                n.parent.set(Some(w));
                p
            });
        }
        if inside {
            continue;
        }
        let n = *targets.entry(Rc::as_ptr(target)).or_insert_with(|| {
            notes.push(note_text(target));
            notes.len() as u16
        });
        markers.insert(Rc::as_ptr(&anchor), n);
    }
    (markers, targets.into_keys().collect(), notes)
}

impl<'a> Extractor<'a> {
    fn style_id(&mut self, style: InlineStyle) -> u16 {
        if let Some(i) = self.styles.iter().position(|s| *s == style) {
            return i as u16;
        }
        self.styles.push(style);
        (self.styles.len() - 1) as u16
    }

    fn flush_text(&mut self, buffer: &mut Buf, offset: usize, kind: BlockKind, level: u8, path: &str, bstyle: &BlockStyle) -> usize {
        // <pre> 也照常折叠空格: 保留连续空格会改变字符偏移, 阅读位置就和网页版、兼容模式对不上了 (换行本来就保留)
        let (text, ids) = if self.rich {
            clean_styled(&buffer.text, &buffer.styles)
        } else {
            let text = clean_text(&buffer.text);
            let n = text.chars().count();
            (text, vec![0; n])
        };
        let mut objects = std::mem::take(&mut buffer.objects).into_iter();
        buffer.clear();
        if text.is_empty() {
            return offset;
        }
        let len = text.chars().count();
        let mut spans = Vec::new();
        if ids.iter().any(|&i| i != 0) {
            let mut start = 0;
            for i in 1..=ids.len() {
                if i == ids.len() || ids[i] != ids[start] {
                    spans.push(Span { start, end: i, style: self.styles[ids[start] as usize] });
                    start = i;
                }
            }
        }
        // 清理不会删掉对象占位符, 按顺序一一对应
        let placed: Vec<(usize, Inline)> = text
            .chars()
            .enumerate()
            .filter(|(_, c)| self.rich && *c == OBJECT_CHAR)
            .filter_map(|(i, _)| objects.next().map(|o| (i, o)))
            .collect();
        let logical_len = len - placed.len();
        self.blocks.push(Block {
            kind,
            text,
            level,
            path: path.to_string(),
            offset,
            spans,
            objects: placed,
            style: bstyle.clone(),
            ..Default::default()
        });
        offset + logical_len
    }

    /// 块元素的上下文与块样式 (网页样式模式)。`marker` = 列表项标记。
    fn block_context(&self, el: &Element, tag: &str, ctx: &Ctx, marker: String) -> (Ctx, BlockStyle) {
        let mut c = ctx.clone();
        el.apply_inline(&mut c.style);
        let scale = el.font_scale().unwrap_or(1.0);
        c.scale = ((c.scale as f32 * scale).round() as u16).clamp(30, 400);
        if let Some(a) = el.align() {
            c.align = a;
        }
        let in_quote = ctx.quote > 0;
        match tag {
            "blockquote" => {
                c.quote = c.quote.saturating_add(1);
                c.style.muted = true;
                c.inset_left += 2.0;
                c.inset_right += 1.0;
            }
            "ul" | "ol" | "dd" => c.inset_left += 2.0,
            _ => {}
        }
        if el.has_class("author") {
            c.inset_right += 1.0;
        }
        c.in_text = TEXT_BLOCK_TAGS.contains(&tag);
        let (mt, mb) = el.margins();
        let em = c.scale as f32 / 100.0;
        let style = BlockStyle {
            align: c.align,
            indent: el.indent(in_quote),
            scale: c.scale,
            line_height: el.line_height(),
            margin_top: mt * em,
            margin_bottom: mb * em,
            inset_left: c.inset_left,
            inset_right: c.inset_right,
            quote: c.quote,
            marker,
        };
        (c, style)
    }

    /// 对应 `append_node`: 沿 DOM 顺序遍历子节点, 文本节点进 buffer, 块级
    /// 元素先 flush 再递归(自己的 offset 从 0 重新计), 内联元素原地递归共享
    /// buffer/offset, img/br 特殊处理。网页样式模式另外带上 `ctx` (继承的样式) 与 `bstyle` (当前块的样式)。
    #[allow(clippy::too_many_arguments)]
    fn append_node(
        &mut self,
        node: &Handle,
        kind: BlockKind,
        level: u8,
        path: &str,
        struct_path: &str,
        buffer: &mut Buf,
        mut offset: usize,
        ctx: &Ctx,
        bstyle: &BlockStyle,
    ) -> usize {
        let mut counters: HashMap<String, usize> = HashMap::new();
        let parent_tag = match &node.data {
            NodeData::Element { name, .. } => tag_name(name),
            _ => String::new(),
        };
        let parent_attrs = if self.rich { element_attrs(node) } else { HashMap::new() };
        let style_id = if self.rich { self.style_id(ctx.style) } else { 0 };
        let children = node.children.borrow();
        for child in children.iter() {
            match &child.data {
                NodeData::Text { contents } => {
                    buffer.push_str(&contents.borrow(), style_id, self.rich);
                }
                NodeData::Element { name, .. } => {
                    let tag = tag_name(name);
                    if SKIP_TAGS.contains(&tag.as_str()) {
                        continue;
                    }
                    let index = counters.get(&tag).copied().unwrap_or(0) + 1;
                    counters.insert(tag.clone(), index);
                    let child_struct = child_struct_path(struct_path, &tag, index);
                    let attrs = element_attrs(child);
                    let el = Element::new(&tag, &attrs);
                    if self.rich && (el.hidden() || self.note_targets.contains(&Rc::as_ptr(child))) {
                        continue;
                    }

                    if tag == "img" {
                        let raw_src = image_src(&attrs);
                        if is_footnote_icon(&attrs, &raw_src) {
                            if self.rich {
                                buffer.push_object(Inline::Note(ctx.style.note), style_id);
                            } else {
                                // 行内标记: 不切段
                                self.footnotes += 1;
                                buffer.push_str(&format!("[注{}]", self.footnotes), 0, false);
                            }
                            continue;
                        }
                        let src = absolute_url(self.base_url, &raw_src);
                        let scheme = src.split(':').next().unwrap_or("").to_ascii_lowercase();
                        let scheme_ok = if src.contains(':') {
                            matches!(scheme.as_str(), "http" | "https")
                        } else {
                            true
                        };
                        let width = parse_dim(&attrs, "width");
                        let height = parse_dim(&attrs, "height");
                        if self.rich && scheme_ok && ctx.in_text {
                            let natural = if width > 0 && height > 0 {
                                Some((width, height))
                            } else {
                                crate::paginate::url_size_hint(&src)
                            };
                            if let Some((w, h)) = natural.filter(|&(_, h)| h <= INLINE_IMAGE_MAX) {
                                buffer.push_object(Inline::Image { url: src, width: w, height: h }, style_id);
                                continue;
                            }
                        }
                        offset = self.flush_text(buffer, offset, kind, level, path, bstyle);
                        if scheme_ok {
                            self.blocks.push(Block {
                                kind: BlockKind::Image,
                                path: anchor_path(&attrs, &child_struct),
                                offset,
                                source_url: src,
                                width,
                                height,
                                ..Default::default()
                            });
                        }
                    } else if tag == "br" {
                        buffer.push_str("\n", style_id, false);
                    } else if self.rich && tag == "hr" {
                        offset = self.flush_text(buffer, offset, kind, level, path, bstyle);
                        self.blocks.push(Block {
                            kind: BlockKind::Rule,
                            path: String::new(),
                            offset,
                            style: BlockStyle { inset_left: ctx.inset_left, inset_right: ctx.inset_right, quote: ctx.quote, ..BlockStyle::default() },
                            ..Default::default()
                        });
                    } else if BLOCK_TAGS.contains(&tag.as_str()) || (self.rich && RICH_BLOCK_TAGS.contains(&tag.as_str())) {
                        offset = self.flush_text(buffer, offset, kind, level, path, bstyle);
                        let child_kind = if HEADING_TAGS.contains(&tag.as_str()) {
                            BlockKind::Heading
                        } else if !self.rich && (tag == "aside" || kind == BlockKind::Footnote) {
                            BlockKind::Footnote
                        } else {
                            BlockKind::Text
                        };
                        let child_level = if child_kind == BlockKind::Heading {
                            tag[1..2].parse::<u8>().unwrap_or(0)
                        } else {
                            0
                        };
                        let child_path = anchor_path(&attrs, &child_struct);
                        let (child_ctx, child_bstyle) = if self.rich {
                            let marker = if tag == "li" {
                                list_marker(&parent_tag, &parent_attrs, &attrs, index)
                            } else {
                                String::new()
                            };
                            self.block_context(&el, &tag, ctx, marker)
                        } else {
                            (Ctx::default(), BlockStyle::default())
                        };
                        let first = self.blocks.len();
                        let child_offset = self.append_node(
                            child,
                            child_kind,
                            child_level,
                            &child_path,
                            &child_struct,
                            buffer,
                            0,
                            &child_ctx,
                            &child_bstyle,
                        );
                        self.flush_text(buffer, child_offset, child_kind, child_level, &child_path, &child_bstyle);
                        // 外边距加在这个元素产生的第一块之前、最后一块之后 (相邻外边距取大者, 同 CSS 外边距合并)
                        if self.rich && self.blocks.len() > first {
                            let (mt, mb) = (child_bstyle.margin_top, child_bstyle.margin_bottom);
                            let last = self.blocks.len() - 1;
                            let top = &mut self.blocks[first].style.margin_top;
                            *top = top.max(mt);
                            let bottom = &mut self.blocks[last].style.margin_bottom;
                            *bottom = bottom.max(mb);
                        }
                    } else if self.rich {
                        let mut child_ctx = ctx.clone();
                        el.apply_inline(&mut child_ctx.style);
                        if let Some(f) = el.font_scale() {
                            child_ctx.style.scale = ((child_ctx.style.scale as f32 * f).round() as u16).clamp(30, 400);
                        }
                        let marker = self.markers.get(&Rc::as_ptr(child)).copied();
                        if let Some(n) = marker {
                            child_ctx.style.note = n;
                        }
                        // 表格同一行的单元格之间空一格
                        if matches!(tag.as_str(), "td" | "th") && counters.get("td").copied().unwrap_or(0) + counters.get("th").copied().unwrap_or(0) > 1 {
                            buffer.push_object(Inline::Space(100), style_id);
                        }
                        let before = (buffer.text.len(), buffer.objects.len());
                        offset = self.append_node(child, kind, level, path, &child_struct, buffer, offset, &child_ctx, bstyle);
                        // 没有文字也没有图标的注释链接: 补一个注释图标, 不然点不到
                        if let Some(n) = marker {
                            let added = &buffer.text[before.0..];
                            if buffer.objects.len() == before.1 && added.chars().all(py_isspace) {
                                let id = self.style_id(child_ctx.style);
                                buffer.push_object(Inline::Note(n), id);
                            }
                        }
                    } else {
                        offset = self.append_node(child, kind, level, path, &child_struct, buffer, offset, ctx, bstyle);
                    }
                }
                _ => {}
            }
        }
        offset
    }
}

/// 列表项标记: `<ol>` 里是序号 (支持 start / value), `<ul>` 里是圆点。
fn list_marker(parent_tag: &str, parent_attrs: &HashMap<String, String>, attrs: &HashMap<String, String>, index: usize) -> String {
    match parent_tag {
        "ol" => {
            let start = parent_attrs.get("start").and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(1);
            let n = attrs.get("value").and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(start + index as i64 - 1);
            format!("{n}.")
        }
        // 圆点画成图形 (系统字体不一定有 •/・)
        "ul" | "menu" => crate::paginate::BULLET.to_string(),
        _ => String::new(),
    }
}

fn plain_text(node: &Handle, out: &mut String) {
    if let NodeData::Element { name, .. } = &node.data {
        if tag_name(name) == "br" {
            out.push('\n');
        }
    }
    if let NodeData::Text { contents } = &node.data {
        out.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        plain_text(child, out);
    }
}

/// 对应 reader.py 的 `extract_blocks` (兼容模式, 金标准测试用)。
pub fn extract_blocks(content: &str, base_url: &str) -> Vec<Block> {
    extract_document(content, base_url, false).blocks
}

/// HTML → 块。`rich` = 网页样式模式 (见 [`crate::style`]): 块和字符带上网页版的样式, 注释标记变成可点击的
/// 图标、注释正文移到 [`Document::notes`], `<hr>` 成为分隔线; 块的划分、XPath 与字符偏移和兼容模式一致
/// (阅读位置两种模式通用)。false = 与 Python 版逐项一致。
pub fn extract_document(content: &str, base_url: &str, rich: bool) -> Document {
    let (_dom, root) = sanitize_html(content);
    let (markers, note_targets, notes) = if rich { find_notes(&root) } else { Default::default() };
    let mut extractor = Extractor {
        base_url,
        blocks: Vec::new(),
        footnotes: 0,
        rich,
        styles: vec![InlineStyle::default()],
        markers,
        note_targets,
    };
    let mut buffer = Buf::default();
    let ctx = Ctx::default();
    let root_style = BlockStyle { indent: if rich { Some(0.0) } else { None }, ..BlockStyle::default() };
    let offset = extractor.append_node(&root, BlockKind::Text, 0, ".", ".", &mut buffer, 0, &ctx, &root_style);
    extractor.flush_text(&mut buffer, offset, BlockKind::Text, 0, ".", &root_style);
    let mut blocks = extractor.blocks;
    if blocks.is_empty() {
        let mut raw = String::new();
        plain_text(&root, &mut raw);
        let text = clean_text(&raw);
        if !text.is_empty() {
            blocks.push(Block {
                text,
                ..Default::default()
            });
        }
    }
    Document { blocks, notes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_paragraph_becomes_one_text_block() {
        let blocks = extract_blocks("<p>hello</p>", "https://www.lightnovel.app");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Text);
        assert_eq!(blocks[0].text, "hello");
        assert_eq!(blocks[0].path, "./p[1]");
    }

    /// 真实章节 (书 1854 第 5 章) 的注释写法: 图标变成行内标记, 不切段、不产生插图块。
    #[test]
    fn footnote_icon_becomes_inline_marker() {
        let html = concat!(
            r##"<p>甲乙<a class="duokan-footnote no-d" epub:type="noteref" href="#n1" id="A_1"><sup><img alt="note" class="footnote" src="/img/note.png"></sup></a>丙</p>"##,
            r##"<p>丁<img src="/img/note.png?v=2"></p>"##,
            r##"<aside epub:type="footnote" id="n1"><ol class="duokan-footnote-content"><li class="duokan-footnote-item" id="n1" value="1">注1：说明</li></ol></aside>"##,
        );
        let blocks = extract_blocks(html, "https://www.lightnovel.life");
        assert!(blocks.iter().all(|b| b.kind != BlockKind::Image));
        assert_eq!(blocks[0].text, "甲乙[注1]丙");
        assert_eq!(blocks[0].path, "./p[1]");
        assert_eq!(blocks[1].text, "丁[注2]");
        assert!(blocks.iter().any(|b| b.kind == BlockKind::Footnote && b.text.contains("注1：说明")));
        // 普通插图不受影响
        let img = extract_blocks(r##"<div class="illus"><img src="/images/a.jpg"></div>"##, "https://www.lightnovel.life");
        assert_eq!(img[0].kind, BlockKind::Image);
        assert_eq!(img[0].source_url, "https://www.lightnovel.life/images/a.jpg");
    }

    #[test]
    fn absolute_url_handles_relative_root_and_protocol_relative() {
        let base = "https://www.lightnovel.app";
        assert_eq!(absolute_url(base, "/a/b.png"), "https://www.lightnovel.app/a/b.png");
        assert_eq!(absolute_url(base, "a/b.png"), "https://www.lightnovel.app/a/b.png");
        assert_eq!(absolute_url(base, "//cdn.example.com/x.png"), "https://cdn.example.com/x.png");
        assert_eq!(absolute_url(base, "https://x.com/y.png"), "https://x.com/y.png");
        assert_eq!(absolute_url(base, "javascript:alert(1)"), "javascript:alert(1)");
        assert_eq!(absolute_url(base, "data:image/png;base64,AAAA"), "data:image/png;base64,AAAA");
        assert_eq!(absolute_url(base, ""), "");
    }
}
