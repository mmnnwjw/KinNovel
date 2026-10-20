//! 病态的深嵌套 HTML (用户上传章节里未闭合的 `<span>`/`<font>`): 抽块与分页都不能撑爆 512 KB 的工作线程栈,
//! 文字一个不丢; 不太深的嵌套照常分块。

use kn_text::{extract_document, paginate_all, LayoutParams, Measure, PageItem};

const BASE: &str = "https://www.lightnovel.life";

struct Fixed;
impl Measure for Fixed {
    fn char_width(&mut self, size: u32, ch: char) -> f32 {
        if (ch as u32) < 0x2E80 {
            size as f32 * 0.5
        } else {
            size as f32
        }
    }
}

/// 在 512 KB 栈 (同 kn-ui 工作线程) 的线程上跑。
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(512 * 1024).spawn(f).expect("spawn").join().expect("thread panicked (stack overflow?)")
}

/// `depth` 层嵌套, 每层开头一个「甲」; `open(i)` = 第 i 层的开始标签。
fn nested(depth: usize, open: impl Fn(usize) -> &'static str) -> String {
    let mut s = String::with_capacity(depth * 12);
    for i in 0..depth {
        s.push_str(open(i));
        s.push('甲');
    }
    s
}

fn count_text(html: String, rich: bool) -> (usize, usize) {
    let doc = extract_document(&html, BASE, rich);
    let in_blocks: usize = doc.blocks.iter().map(|b| b.text.matches('甲').count()).sum();
    // 分页 (换行、禁则) 也不能递归
    let params = LayoutParams { rich, strict_kinsoku: rich, ..LayoutParams::default() };
    let pages = paginate_all(&doc.blocks, params, &mut Fixed);
    let on_pages: usize = pages
        .iter()
        .flatten()
        .map(|item| match item {
            PageItem::Text { text, .. } => text.matches('甲').count(),
            _ => 0,
        })
        .sum();
    (in_blocks, on_pages)
}

#[test]
fn deeply_nested_spans_do_not_overflow_the_stack() {
    const N: usize = 100_000;
    for rich in [false, true] {
        let (blocks, pages) = on_small_stack(move || count_text(nested(N, |_| "<span>"), rich));
        assert_eq!(blocks, N, "rich={rich}");
        assert_eq!(pages, N, "rich={rich}");
        let (blocks, _) = on_small_stack(move || count_text(nested(N, |i| if i % 2 == 0 { "<font>" } else { "<span class=\"bold\" style=\"font-size:120%\">" }), rich));
        assert_eq!(blocks, N, "rich={rich}");
    }
}

#[test]
fn deeply_nested_blocks_do_not_overflow_the_stack() {
    // html5ever 遇到 div 等开始标签要检查作用域 (每次 O(深度), 整体 O(深度²)), 层数取小一些免得测试太慢;
    // 仍远超递归上限和原来 512 KB 栈的溢出深度 (约 500 层)
    const N: usize = 5_000;
    let open = |i: usize| match i % 5 {
        0 => "<div>",
        1 => "<blockquote>",
        2 => "<ul><li>",
        3 => "<section id=\"s\">",
        _ => "<span>",
    };
    for rich in [false, true] {
        let (blocks, pages) = on_small_stack(move || count_text(nested(N, open), rich));
        assert_eq!(blocks, N, "rich={rich}");
        assert_eq!(pages, N, "rich={rich}");
    }
    // 注释 (网页样式模式先找注释标记与目标) 与没有块的纯行内文档
    let notes = format!("<p><a class=\"duokan-footnote\" href=\"#n\">注</a></p><aside id=\"n\">{}</aside>", nested(N, |_| "<div>"));
    let doc = on_small_stack(move || extract_document(&notes, BASE, true));
    assert_eq!(doc.notes.len(), 1);
    assert_eq!(doc.notes[0].matches('甲').count(), N);
}

/// 上限以内照常分块: 每层 div 一块, XPath 逐层加深。
#[test]
fn moderate_nesting_keeps_one_block_per_level() {
    let depth = 150;
    let html = nested(depth, |_| "<div>");
    for rich in [false, true] {
        let html = html.clone();
        let doc = on_small_stack(move || extract_document(&html, BASE, rich));
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
    let mut html = nested(400, |_| "<div>");
    html.push_str("尾<br>行<script>x</script>");
    for rich in [false, true] {
        let doc = extract_document(&html, BASE, rich);
        let last = doc.blocks.last().unwrap();
        assert!(doc.blocks.len() < 400, "rich={rich}");
        assert!(last.text.ends_with("甲尾\n行"), "rich={rich}: {:?}", last.text);
        let total: usize = doc.blocks.iter().map(|b| b.text.matches('甲').count()).sum();
        assert_eq!(total, 400);
    }
}
