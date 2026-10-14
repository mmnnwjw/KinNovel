//! 网页样式模式: 块划分、XPath 与字符偏移必须与兼容模式一致 (阅读位置两种模式、网页版通用),
//! 样式按 read.scss 生效。

use std::fs;
use std::path::Path;

use serde_json::Value;

use kn_text::{
    extract_blocks, extract_document, first_anchor_on_page, page_for_path, paginate_all, Align, BlockKind, Inline, LayoutParams, Measure, PageItem,
    OBJECT_CHAR,
};

const BASE: &str = "https://www.lightnovel.life";

struct Fixed;
impl Measure for Fixed {
    fn char_width(&mut self, size: u32, ch: char) -> f32 {
        if ch == ' ' {
            size as f32 * 0.25
        } else if (ch as u32) < 0x2E80 {
            size as f32 * 0.5
        } else {
            size as f32
        }
    }
}

fn chapters() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["tests/fixtures", "tests/fixtures/private"] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.contains(".golden.") {
                continue;
            }
            let content = if name.ends_with(".html") {
                fs::read_to_string(&p).unwrap()
            } else if name.ends_with(".json") {
                let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
                let c = v.get("Chapter").cloned().unwrap_or(v);
                c.get("Content").and_then(|c| c.as_str()).unwrap_or("").to_string()
            } else {
                continue;
            };
            out.push((name, content));
        }
    }
    // 提交的合成章节一定有; 真实章节 (private/, gitignored) 只在本机
    assert!(out.len() >= 2, "fixtures missing");
    out
}

/// 去掉兼容模式的「[注n]」标记
fn strip_markers(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("[注") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "[注".len()..];
        match after.find(']') {
            Some(j) if after[..j].chars().all(|c| c.is_ascii_digit()) => rest = &after[j + 1..],
            _ => {
                out.push_str("[注");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn rich_blocks_keep_compat_anchors() {
    for (name, content) in chapters() {
        let compat = extract_blocks(&content, BASE);
        let rich = extract_document(&content, BASE, true);
        let footnote_paths: Vec<&str> = compat.iter().filter(|b| b.kind == BlockKind::Footnote).map(|b| b.path.as_str()).collect();
        let a: Vec<(String, usize, String)> = compat
            .iter()
            .filter(|b| b.kind != BlockKind::Footnote)
            .map(|b| (b.path.clone(), b.offset, strip_markers(&b.text)))
            .collect();
        let b: Vec<(String, usize, String)> = rich
            .blocks
            .iter()
            .filter(|b| b.kind != BlockKind::Rule && !footnote_paths.contains(&b.path.as_str()))
            .map(|b| (b.path.clone(), b.offset, b.text.chars().filter(|&c| c != OBJECT_CHAR).collect()))
            .collect();
        assert_eq!(a.len(), b.len(), "{name}: block count");
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x, y, "{name}");
        }
        // 网页样式排版后每个块的锚点都能找到
        let params = LayoutParams { rich: true, real_image_size: true, strict_kinsoku: true, ..LayoutParams::default() };
        let pages = paginate_all(&rich.blocks, params, &mut Fixed);
        for blk in rich.blocks.iter().filter(|b| matches!(b.kind, BlockKind::Text | BlockKind::Heading)) {
            let p = page_for_path(&pages, &blk.path, Some(blk.offset as i64), usize::MAX);
            assert_ne!(p, usize::MAX, "{name}: {}", blk.path);
        }
        for i in 0..pages.len() {
            let (path, _) = first_anchor_on_page(&pages, i as i64);
            assert!(!path.is_empty());
        }
    }
}

#[test]
fn duokan_footnote_becomes_popup() {
    let html = concat!(
        r##"<p>今天是<b>燕舞</b>的考试，在组太刀<a class="duokan-footnote no-d" epub:type="noteref" href="#n1" id="A_1"><sup><img alt="note" class="footnote" src="/img/note.png"></sup></a>时受了伤</p>"##,
        r##"<aside epub:type="footnote" id="n1"><ol class="duokan-footnote-content"><li class="duokan-footnote-item" id="n1" value="1">注1：说明<br>第二行</li></ol></aside>"##,
    );
    let doc = extract_document(html, BASE, true);
    assert_eq!(doc.notes, vec!["注1：说明\n第二行".to_string()]);
    assert_eq!(doc.blocks.len(), 1, "注释正文不进正文");
    let b = &doc.blocks[0];
    assert_eq!(b.objects, vec![(13, Inline::Note(1))]);
    let bold: Vec<_> = b.spans.iter().filter(|s| s.style.bold).collect();
    assert_eq!(bold.len(), 1);
    assert_eq!((bold[0].start, bold[0].end), (3, 5));
    // 对象后面的偏移不计对象
    assert_eq!(b.logical_offset(14), 13);
    let sup = b.spans.iter().find(|s| s.start == 13).unwrap();
    assert_eq!((sup.style.note, sup.style.shift), (1, 1));

    let params = LayoutParams { rich: true, ..LayoutParams::default() };
    let pages = paginate_all(&doc.blocks, params, &mut Fixed);
    let notes: Vec<_> = pages[0].iter().filter(|i| matches!(i, PageItem::Note { index: 1, .. })).collect();
    assert_eq!(notes.len(), 1);
    let PageItem::Text { rich: Some(line), .. } = &pages[0][pages[0].len() - 1] else { panic!() };
    assert!(line.runs.iter().any(|r| r.style.bold));
}

#[test]
fn markdown_footnotes_and_text_markers() {
    let html = concat!(
        r##"<p>正文<sup class="footnote-ref"><a href="#fn1" id="fnref1">[1]</a></sup>后文</p>"##,
        r##"<section class="footnotes"><ol><li id="fn1"><p>脚注内容 <a href="#fnref1" class="footnote-backref">↩︎</a></p></li></ol></section>"##,
    );
    let doc = extract_document(html, BASE, true);
    assert_eq!(doc.notes, vec!["脚注内容".to_string()]);
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text, "正文[1]后文");
    assert!(doc.blocks[0].spans.iter().any(|s| s.style.note == 1 && s.start == 2 && s.end == 5));
}

#[test]
fn web_styles_map_to_block_styles() {
    let html = concat!(
        r#"<h1>第一章</h1>"#,
        r#"<p class="center">居中</p>"#,
        r#"<p>普通段落</p>"#,
        r#"<hr>"#,
        r#"<blockquote><p>引用</p></blockquote>"#,
        r#"<ol start="3"><li>甲</li><li>乙</li></ol>"#,
        r#"<p><ruby>漢<rp>(</rp><rt>かん</rt><rp>)</rp></ruby>字<span class="em15">大</span><s>删</s><em>斜</em><span style="font-weight:bold">粗</span></p>"#,
        r#"<p class="author">作者</p>"#,
    );
    let doc = extract_document(html, BASE, true);
    let kinds: Vec<_> = doc.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        vec![BlockKind::Heading, BlockKind::Text, BlockKind::Text, BlockKind::Rule, BlockKind::Text, BlockKind::Text, BlockKind::Text, BlockKind::Text, BlockKind::Text]
    );
    let b = &doc.blocks;
    assert_eq!((b[0].style.align, b[0].style.scale), (Align::Center, 165));
    assert!(b[0].spans.iter().all(|s| s.style.bold));
    assert_eq!((b[1].style.align, b[1].style.indent), (Align::Center, Some(0.0)));
    assert_eq!(b[2].style.indent, None);
    assert_eq!((b[4].style.quote, b[4].style.indent), (1, Some(0.0)));
    assert!(b[4].spans.iter().all(|s| s.style.muted));
    assert_eq!((b[5].style.marker.as_str(), b[6].style.marker.as_str()), ("3.", "4."));
    let ruby = &b[7];
    assert_eq!(ruby.text, "漢(かん)字大删斜粗");
    let style_at = |i: usize| ruby.spans.iter().find(|s| s.start <= i && i < s.end).map(|s| s.style).unwrap_or_default();
    assert!(style_at(1).hidden && style_at(2).ruby && style_at(4).hidden);
    assert_eq!(style_at(6).scale, 150);
    assert!(style_at(7).strike && style_at(8).italic && style_at(9).bold);
    assert_eq!(b[8].style.align, Align::End);

    let params = LayoutParams { rich: true, ..LayoutParams::default() };
    let pages = paginate_all(&doc.blocks, params, &mut Fixed);
    let items = &pages[0];
    // 居中: 两个全角字在 1168 宽里居中
    let centered = items.iter().find_map(|i| match i {
        PageItem::Text { text, x, .. } if text == "居中" => Some(*x),
        _ => None,
    });
    assert_eq!(centered, Some(34 + (1168 - 96) / 2));
    assert!(items.iter().any(|i| matches!(i, PageItem::Rule { width, .. } if *width == 1168)), "分隔线");
    assert!(items.iter().any(|i| matches!(i, PageItem::Rule { x: 34, .. })), "引用竖线");
    assert!(items.iter().any(|i| matches!(i, PageItem::Text { text, .. } if text == "3.")), "列表序号");
}
