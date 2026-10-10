//! HTML -> Block 列表, 对应 reader.py 的 `sanitize_html` / `extract_blocks`。

use std::collections::HashMap;

use html5ever::driver::ParseOpts;
use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::clean::clean_text;

const BLOCK_TAGS: &[&str] = &[
    "p", "div", "section", "article", "blockquote", "h1", "h2", "h3", "h4", "h5", "h6", "li",
    "dt", "dd", "pre", "figcaption", "aside",
];
const HEADING_TAGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];
const SKIP_TAGS: &[&str] = &["script", "style", "iframe", "object", "embed", "svg", "canvas"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Heading,
    Footnote,
    Image,
}

impl BlockKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockKind::Text => "text",
            BlockKind::Heading => "heading",
            BlockKind::Footnote => "footnote",
            BlockKind::Image => "image",
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

struct Extractor<'a> {
    base_url: &'a str,
    blocks: Vec<Block>,
}

impl<'a> Extractor<'a> {
    fn flush_text(&mut self, buffer: &mut String, offset: usize, kind: BlockKind, level: u8, path: &str) -> usize {
        let text = clean_text(buffer);
        buffer.clear();
        if text.is_empty() {
            return offset;
        }
        let len = text.chars().count();
        self.blocks.push(Block {
            kind,
            text,
            level,
            path: path.to_string(),
            offset,
            ..Default::default()
        });
        offset + len
    }

    /// 对应 `append_node`: 沿 DOM 顺序遍历子节点, 文本节点进 buffer, 块级
    /// 元素先 flush 再递归(自己的 offset 从 0 重新计), 内联元素原地递归共享
    /// buffer/offset, img/br 特殊处理。
    fn append_node(
        &mut self,
        node: &Handle,
        kind: BlockKind,
        level: u8,
        path: &str,
        struct_path: &str,
        buffer: &mut String,
        mut offset: usize,
    ) -> usize {
        let mut counters: HashMap<String, usize> = HashMap::new();
        let children = node.children.borrow();
        for child in children.iter() {
            match &child.data {
                NodeData::Text { contents } => {
                    buffer.push_str(&contents.borrow());
                }
                NodeData::Element { name, .. } => {
                    let tag = tag_name(name);
                    if SKIP_TAGS.contains(&tag.as_str()) {
                        continue;
                    }
                    let index = counters.get(&tag).copied().unwrap_or(0) + 1;
                    counters.insert(tag.clone(), index);
                    let child_struct = child_struct_path(struct_path, &tag, index);

                    if tag == "img" {
                        offset = self.flush_text(buffer, offset, kind, level, path);
                        let attrs = element_attrs(child);
                        let raw_src = attrs
                            .get("src")
                            .filter(|s| !s.is_empty())
                            .or_else(|| attrs.get("data-system-image-url"))
                            .cloned()
                            .unwrap_or_default();
                        let src = absolute_url(self.base_url, &raw_src);
                        let scheme = src.split(':').next().unwrap_or("").to_ascii_lowercase();
                        let scheme_ok = if src.contains(':') {
                            matches!(scheme.as_str(), "http" | "https")
                        } else {
                            true
                        };
                        if scheme_ok {
                            let width = parse_dim(&attrs, "width");
                            let height = parse_dim(&attrs, "height");
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
                        buffer.push('\n');
                    } else if BLOCK_TAGS.contains(&tag.as_str()) {
                        offset = self.flush_text(buffer, offset, kind, level, path);
                        let child_kind = if HEADING_TAGS.contains(&tag.as_str()) {
                            BlockKind::Heading
                        } else if tag == "aside" || kind == BlockKind::Footnote {
                            BlockKind::Footnote
                        } else {
                            BlockKind::Text
                        };
                        let child_level = if child_kind == BlockKind::Heading {
                            tag[1..2].parse::<u8>().unwrap_or(0)
                        } else {
                            0
                        };
                        let attrs = element_attrs(child);
                        let child_path = anchor_path(&attrs, &child_struct);
                        let child_offset = self.append_node(
                            child,
                            child_kind,
                            child_level,
                            &child_path,
                            &child_struct,
                            buffer,
                            0,
                        );
                        self.flush_text(buffer, child_offset, child_kind, child_level, &child_path);
                    } else {
                        offset = self.append_node(child, kind, level, path, &child_struct, buffer, offset);
                    }
                }
                _ => {}
            }
        }
        offset
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

/// 对应 reader.py 的 `extract_blocks`。
pub fn extract_blocks(content: &str, base_url: &str) -> Vec<Block> {
    let (_dom, root) = sanitize_html(content);
    let mut extractor = Extractor {
        base_url,
        blocks: Vec::new(),
    };
    let mut buffer = String::new();
    let offset = extractor.append_node(&root, BlockKind::Text, 0, ".", ".", &mut buffer, 0);
    extractor.flush_text(&mut buffer, offset, BlockKind::Text, 0, ".");
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
    blocks
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
