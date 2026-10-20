//! 网页版阅读样式在本阅读器里的对应 (只在 [`crate::extract_document`] 的 `rich` 模式下使用)。
//!
//! 网页版 (LightNovelShelf/Web) 把章节 HTML 交给浏览器渲染, 套一份阅读样式表 `src/css/read.scss`,
//! 其余是浏览器默认样式; DOMPurify 保留了 `style` 属性。这里把其中影响排版的部分 (粗体、斜体、字号、
//! 对齐、缩进、行高、上下间距、引用块、列表、上下标、注音、着重号、删除线/下划线、隐藏) 转成块样式与
//! 行内样式; 颜色、浮动、边框之类在墨水屏上没有意义的属性忽略。

/// 块内文字对齐。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// 行内样式 (作用于一段字符)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// 着重号 (`.dot` / `.em-dot` / `text-emphasis`): 每个字下方一个点
    pub dots: bool,
    /// 浅色 (引用块里的文字)
    pub muted: bool,
    /// 不显示也不占位 (`<rp>`、行内 `display:none`), 但仍计入阅读位置的字符偏移
    pub hidden: bool,
    /// 注音 (`<rt>`): 不占位, 画在前面的基字上方
    pub ruby: bool,
    /// 注音的基字 (`<ruby>` 里 `<rt>` 以外的字): 单独成段, 注音按它居中
    pub ruby_base: bool,
    /// 字号, 相对所在块字号的百分比
    pub scale: u16,
    /// 1 = 上标, -1 = 下标
    pub shift: i8,
    /// 注释标记: 点击弹出 `Document::notes[note - 1]`; 0 = 不是注释标记
    pub note: u16,
}

impl Default for InlineStyle {
    fn default() -> Self {
        InlineStyle {
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            dots: false,
            muted: false,
            hidden: false,
            ruby: false,
            ruby_base: false,
            scale: 100,
            shift: 0,
            note: 0,
        }
    }
}

/// 块样式。长度单位都是 em: `indent` 相对块字号, 其余相对正文字号。
#[derive(Clone, Debug, PartialEq)]
pub struct BlockStyle {
    pub align: Align,
    /// 首行缩进; None = 跟随阅读设置 (网页版只给 `<p>` 缩进)
    pub indent: Option<f32>,
    /// 块字号, 相对正文字号的百分比
    pub scale: u16,
    /// CSS 行高 (em); None = 阅读设置的行距。网页版正文行高 1.8em, 其它值按比例换算到阅读设置上
    pub line_height: Option<f32>,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub inset_left: f32,
    pub inset_right: f32,
    /// 引用块层数 (左侧画竖线)
    pub quote: u8,
    /// 列表项标记 ("1." / "・"), 画在左侧留白里
    pub marker: String,
}

impl Default for BlockStyle {
    fn default() -> Self {
        BlockStyle {
            align: Align::Start,
            indent: None,
            scale: 100,
            line_height: None,
            margin_top: 0.0,
            margin_bottom: 0.0,
            inset_left: 0.0,
            inset_right: 0.0,
            quote: 0,
            marker: String::new(),
        }
    }
}

/// 网页版正文行高 (read.scss `p { line-height: 1.8em }`)。
pub const WEB_LINE_HEIGHT: f32 = 1.8;

/// 按名字取元素属性 (名字是小写)。
pub(crate) trait AttrMap {
    fn attr(&self, name: &str) -> Option<&str>;
}

#[cfg(test)]
impl AttrMap for std::collections::HashMap<String, String> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.get(name).map(|s| s.as_str())
    }
}

/// 一个元素的标签、class 与内联样式声明。
pub(crate) struct Element<'a> {
    pub tag: &'a str,
    attrs: &'a dyn AttrMap,
    classes: Vec<String>,
    decls: Vec<(String, String)>,
}

impl<'a> Element<'a> {
    pub fn new(tag: &'a str, attrs: &'a dyn AttrMap) -> Self {
        let classes = attrs
            .attr("class")
            .map(|c| c.split_ascii_whitespace().map(|c| c.to_ascii_lowercase()).collect())
            .unwrap_or_default();
        let decls = attrs.attr("style").map(parse_decls).unwrap_or_default();
        Element { tag, attrs, classes, decls }
    }

    pub fn has_class(&self, name: &str) -> bool {
        self.classes.iter().any(|c| c == name)
    }

    fn decl(&self, prop: &str) -> Option<&str> {
        self.decls.iter().rev().find(|(p, _)| p == prop).map(|(_, v)| v.as_str())
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.attr(name)
    }

    fn is_heading(&self) -> bool {
        matches!(self.tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
    }

    /// `.pius1` 等: 网页版当 h4 一样的小标题
    fn is_subtitle(&self) -> bool {
        self.tag == "h4" || ["pius1", "ph4", "pius2"].iter().any(|c| self.has_class(c))
    }

    /// 整个元素不显示: `display:none`、`hidden` 属性、网页版隐藏的 `.footnotes` 段。
    pub fn hidden(&self) -> bool {
        self.decl("display") == Some("none")
            || self.attrs.attr("hidden").is_some()
            || self.has_class("footnotes")
            || matches!(self.tag, "template" | "noscript" | "head" | "title" | "meta" | "link")
    }

    /// 字号倍数 (相对父元素)。
    pub fn font_scale(&self) -> Option<f32> {
        if let Some(v) = self.decl("font-size") {
            return match v {
                "smaller" | "small" => Some(0.83),
                "x-small" => Some(0.75),
                "xx-small" => Some(0.6),
                "larger" | "large" => Some(1.2),
                "x-large" => Some(1.5),
                "xx-large" => Some(2.0),
                "medium" => Some(1.0),
                v => css_em(v),
            };
        }
        for c in &self.classes {
            if let Some(n) = c.strip_prefix("em").and_then(|n| n.parse::<u32>().ok()) {
                if c.len() == 4 && (5..=30).contains(&n) {
                    return Some(n as f32 / 10.0);
                }
            }
        }
        if self.has_class("stress") {
            return Some(1.1);
        }
        if self.has_class("author") {
            return Some(1.2);
        }
        if self.has_class("meg") {
            return Some(1.3);
        }
        if self.is_subtitle() {
            return Some(1.5);
        }
        match self.tag {
            "h1" => Some(1.65),
            "h2" => Some(1.25),
            "h3" => Some(0.95),
            "h5" => Some(0.83),
            "h6" => Some(0.67),
            "small" | "sup" | "sub" => Some(0.83),
            "big" => Some(1.2),
            "rt" => Some(0.5),
            _ => None,
        }
    }

    /// 字号以外的行内样式 (子元素继承)。
    pub fn apply_inline(&self, s: &mut InlineStyle) {
        match self.tag {
            "b" | "strong" | "th" => s.bold = true,
            "i" | "em" | "cite" | "dfn" | "var" | "address" => s.italic = true,
            "u" | "ins" => s.underline = true,
            "s" | "strike" | "del" => s.strike = true,
            "sup" => s.shift = 1,
            "sub" => s.shift = -1,
            "ruby" => s.ruby_base = true,
            "rt" => s.ruby = true,
            "rp" => s.hidden = true,
            _ => {}
        }
        if self.is_heading() || self.is_subtitle() || ["bold", "stress", "author"].iter().any(|c| self.has_class(c)) {
            s.bold = true;
        }
        if self.has_class("ita") || self.has_class("author") {
            s.italic = true;
        }
        if self.has_class("dot") || self.has_class("em-dot") {
            s.dots = true;
        }
        if let Some(v) = self.decl("font-weight") {
            s.bold = match v {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                v => v.parse::<u32>().map(|w| w >= 600).unwrap_or(s.bold),
            };
        }
        if let Some(v) = self.decl("font-style") {
            s.italic = v.starts_with("italic") || v.starts_with("oblique");
        }
        for prop in ["text-decoration", "text-decoration-line"] {
            if let Some(v) = self.decl(prop) {
                if v.contains("none") {
                    s.underline = false;
                    s.strike = false;
                }
                if v.contains("underline") {
                    s.underline = true;
                }
                if v.contains("line-through") {
                    s.strike = true;
                }
            }
        }
        for prop in ["text-emphasis", "text-emphasis-style", "-webkit-text-emphasis", "-webkit-text-emphasis-style"] {
            if let Some(v) = self.decl(prop) {
                s.dots = v != "none";
            }
        }
        match self.decl("vertical-align") {
            Some("super") => s.shift = 1,
            Some("sub") => s.shift = -1,
            _ => {}
        }
        if self.decl("visibility") == Some("hidden") {
            s.hidden = true;
        }
    }

    /// 文字对齐 (不写 = 继承父元素)。
    pub fn align(&self) -> Option<Align> {
        if let Some(v) = self.decl("text-align") {
            return Some(match v {
                "center" => Align::Center,
                "right" | "end" => Align::End,
                _ => Align::Start,
            });
        }
        if self.has_class("center") {
            return Some(Align::Center);
        }
        if self.has_class("right") || self.has_class("author") {
            return Some(Align::End);
        }
        if self.has_class("left") {
            return Some(Align::Start);
        }
        if let Some(v) = self.attr("align") {
            match v.to_ascii_lowercase().as_str() {
                "center" | "middle" => return Some(Align::Center),
                "right" => return Some(Align::End),
                "left" | "justify" => return Some(Align::Start),
                _ => {}
            }
        }
        match self.tag {
            "center" | "h1" | "h2" | "h3" => Some(Align::Center),
            _ => None,
        }
    }

    /// 首行缩进: `<p>` 默认跟随阅读设置 (None), 其它块没有缩进; 引用块里的段落也不缩进。
    pub fn indent(&self, in_quote: bool) -> Option<f32> {
        if let Some(v) = self.decl("text-indent") {
            if let Some(em) = css_em(v) {
                return Some(em);
            }
        }
        if ["right", "left", "center", "zin", "message", "cut-line", "meg"].iter().any(|c| self.has_class(c)) {
            return Some(0.0);
        }
        if self.is_subtitle() {
            return Some(1.333);
        }
        if self.tag == "p" && !in_quote {
            return None;
        }
        Some(0.0)
    }

    /// 行高 (em); None = 正文行高。
    pub fn line_height(&self) -> Option<f32> {
        if let Some(v) = self.decl("line-height") {
            if v == "normal" {
                return Some(1.2);
            }
            if let Ok(n) = v.parse::<f32>() {
                return Some(n);
            }
            return css_em(v);
        }
        if self.has_class("lh") {
            return Some(1.0);
        }
        if self.has_class("message") || self.has_class("cut-line") || matches!(self.tag, "h1" | "h2" | "h3") {
            return Some(1.2);
        }
        if self.has_class("meg") {
            return Some(1.3);
        }
        None
    }

    /// 上下外边距 (em, 相对本元素字号)。
    pub fn margins(&self) -> (f32, f32) {
        let mut m = if self.has_class("m0") {
            (0.0, 0.0)
        } else if self.is_subtitle() {
            (0.5, 1.0)
        } else if self.has_class("stress") {
            (0.3, 0.3)
        } else if self.has_class("message") || self.has_class("cut-line") {
            (0.2, 0.2)
        } else if self.has_class("meg") {
            (0.5, 0.0)
        } else {
            match self.tag {
                "h1" => (0.1, 0.4),
                "h2" => (0.3, 0.5),
                "h3" => (0.2, 0.2),
                "h5" | "h6" => (0.5, 0.5),
                "blockquote" => (1.0, 1.0),
                "ul" | "ol" | "dl" | "figure" | "pre" => (0.5, 0.5),
                _ => (0.0, 0.0),
            }
        };
        if let Some(v) = self.decl("margin") {
            let parts: Vec<Option<f32>> = v.split_ascii_whitespace().map(css_em).collect();
            match parts.as_slice() {
                [Some(a)] | [Some(a), _] => m = (*a, *a),
                [Some(t), _, Some(b)] | [Some(t), _, Some(b), _] => m = (*t, *b),
                _ => {}
            }
        }
        if let Some(t) = self.decl("margin-top").and_then(css_em) {
            m.0 = t;
        }
        if let Some(b) = self.decl("margin-bottom").and_then(css_em) {
            m.1 = b;
        }
        (m.0.clamp(0.0, 5.0), m.1.clamp(0.0, 5.0))
    }
}

/// `style` 属性 → (属性, 值), 都转小写、去掉 `!important`。
fn parse_decls(style: &str) -> Vec<(String, String)> {
    style
        .split(';')
        .filter_map(|d| {
            let (p, v) = d.split_once(':')?;
            let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
            Some((p.trim().to_ascii_lowercase(), v))
        })
        .filter(|(p, v)| !p.is_empty() && !v.is_empty())
        .collect()
}

/// CSS 长度 → em (px 按浏览器默认 16px 换算, 百分比按父元素字号)。
fn css_em(v: &str) -> Option<f32> {
    let v = v.trim();
    let num = |s: &str| s.trim().parse::<f32>().ok().filter(|n| n.is_finite());
    if v == "0" {
        return Some(0.0);
    }
    if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
        return num(n);
    }
    if let Some(n) = v.strip_suffix('%') {
        return num(n).map(|n| n / 100.0);
    }
    if let Some(n) = v.strip_suffix("px") {
        return num(n).map(|n| n / 16.0);
    }
    if let Some(n) = v.strip_suffix("pt") {
        return num(n).map(|n| n * 4.0 / 3.0 / 16.0);
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn el<'a>(tag: &'a str, attrs: &'a HashMap<String, String>) -> Element<'a> {
        Element::new(tag, attrs)
    }

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn read_scss_classes() {
        let a = attrs(&[("class", "em15 bold center")]);
        let e = el("p", &a);
        assert_eq!(e.font_scale(), Some(1.5));
        assert_eq!(e.align(), Some(Align::Center));
        assert_eq!(e.indent(false), Some(0.0));
        let mut s = InlineStyle::default();
        e.apply_inline(&mut s);
        assert!(s.bold && !s.italic);

        let a = attrs(&[("class", "author")]);
        let e = el("p", &a);
        assert_eq!(e.align(), Some(Align::End));
        let mut s = InlineStyle::default();
        e.apply_inline(&mut s);
        assert!(s.bold && s.italic);

        let a = attrs(&[]);
        assert_eq!(el("p", &a).indent(false), None);
        assert_eq!(el("p", &a).indent(true), Some(0.0));
        assert_eq!(el("div", &a).indent(false), Some(0.0));
        assert_eq!(el("h1", &a).align(), Some(Align::Center));
        assert_eq!(el("h4", &a).indent(false), Some(1.333));
    }

    #[test]
    fn inline_style_attribute() {
        let a = attrs(&[("style", "font-weight: 700; FONT-STYLE: italic; text-align:right; font-size:120%; text-indent:2em !important; display:block")]);
        let e = el("span", &a);
        let mut s = InlineStyle::default();
        e.apply_inline(&mut s);
        assert!(s.bold && s.italic);
        assert_eq!(e.align(), Some(Align::End));
        assert_eq!(e.font_scale(), Some(1.2));
        assert_eq!(e.indent(false), Some(2.0));
        assert!(!e.hidden());
        let a = attrs(&[("style", "display:none")]);
        assert!(el("p", &a).hidden());
        let a = attrs(&[("style", "margin: 1em 0 2em")]);
        assert_eq!(el("div", &a).margins(), (1.0, 2.0));
    }
}
