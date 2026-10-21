//! 常规嵌套的分块锚点, 以及超过提取深度上限时的正文保留。

use kn_text::extract_document;

const BASE: &str = "https://www.lightnovel.life";

/// `depth` 层嵌套, 每层开头一个「甲」; `open(i)` = 第 i 层的开始标签。
fn nested(depth: usize, open: impl Fn(usize) -> &'static str) -> String {
    let mut s = String::with_capacity(depth * 12);
    for i in 0..depth {
        s.push_str(open(i));
        s.push('甲');
    }
    s
}

/// 上限以内照常分块: 每层 div 一块, XPath 逐层加深。
#[test]
fn moderate_nesting_keeps_one_block_per_level() {
    let depth = 8;
    let html = nested(depth, |_| "<div>");
    for rich in [false, true] {
        let doc = extract_document(&html, BASE, rich);
        assert_eq!(doc.blocks.len(), depth, "rich={rich}");
        let mut path = String::from(".");
        for b in &doc.blocks {
            path.push_str("/div[1]");
            assert_eq!(b.path, path);
            assert_eq!(b.text, "甲");
            assert_eq!(b.offset, 0);
        }
    }
}

/// 超过上限的部分并进上限那一层的块, 文字顺序不变, `<br>` 仍换行。
#[test]
fn beyond_the_cap_text_goes_into_the_current_block() {
    let mut html = nested(220, |_| "<div>");
    html.push_str("尾<br>行<script>x</script>");
    for rich in [false, true] {
        let doc = extract_document(&html, BASE, rich);
        let last = doc.blocks.last().unwrap();
        assert!(doc.blocks.len() < 220, "rich={rich}");
        assert!(last.text.ends_with("甲尾\n行"), "rich={rich}: {:?}", last.text);
        let total: usize = doc.blocks.iter().map(|b| b.text.matches('甲').count()).sum();
        assert_eq!(total, 220);
    }
}
