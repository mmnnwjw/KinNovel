//! 阅读器的插图排版: 封面占满一页, 其余插图按真实尺寸 (图片地址的 `size=`) 与正文混排。

use kn_text::{extract_blocks, paginate_all, url_size_hint, LayoutParams, Measure, PageItem};

struct Fixed;
impl Measure for Fixed {
    fn char_width(&mut self, size: u32, _ch: char) -> f32 {
        size as f32
    }
}

fn img(name: &str, w: u32, h: u32) -> String {
    format!("<img src=\"https://img.example/{name}.jpg?placeholder=x&amp;size={w}x{h}&amp;t=1\">")
}

fn reader_params(cover: bool) -> LayoutParams {
    LayoutParams {
        width: 1236,
        height: 1590,
        font_size: 48,
        margin: 48,
        cover_full_page: cover,
        real_image_size: true,
        ..LayoutParams::default()
    }
}

/// 每页的 (图片名, x, 宽, 高) 与是否有正文。
fn layout(html: &str, params: LayoutParams) -> Vec<(Vec<(String, i64, i64, i64)>, bool)> {
    let blocks = extract_blocks(html, "https://example.org");
    paginate_all(&blocks, params, &mut Fixed)
        .iter()
        .map(|page| {
            let images = page
                .iter()
                .filter_map(|it| match it {
                    PageItem::Image { url, x, width, height, .. } => {
                        let name = url.rsplit('/').next().unwrap().split('.').next().unwrap().to_string();
                        Some((name, *x, *width, *height))
                    }
                    _ => None,
                })
                .collect();
            let text = page.iter().any(|it| matches!(it, PageItem::Text { .. }));
            (images, text)
        })
        .collect()
}

#[test]
fn size_hint_from_url() {
    assert_eq!(url_size_hint("https://a/b.jpg?placeholder=J%7c&amp;size=1350x1920&amp;t=9"), Some((1350, 1920)));
    assert_eq!(url_size_hint("https://a/b.jpg?size=128x128"), Some((128, 128)));
    assert_eq!(url_size_hint("https://a/b.jpg?size=0x10"), None);
    assert_eq!(url_size_hint("https://a/b.jpg"), None);
}

#[test]
fn only_the_cover_fills_a_page() {
    let html = format!(
        "<div>{}</div><div>{}</div><div>{}</div><p>正文开始。</p>",
        img("cover", 1348, 1920),
        img("wide", 1920, 991),
        img("plate", 1350, 1920)
    );
    let pages = layout(&html, reader_params(true));
    let usable_w = 1236 - 2 * 48;
    let usable_h = 1590 - 2 * 48;
    // 封面单独一页, 占满正文区
    assert_eq!(pages[0].0, vec![("cover".to_string(), 48, usable_w, usable_h)]);
    // 横版彩页按比例; 竖版彩页限高 62%, 剩余空间放不下就另起一页, 正文接在它下面
    let wide_h = (usable_w as f64 * 991.0 / 1920.0) as i64;
    let plate_h = (usable_h as f64 * 0.62) as i64;
    assert_eq!(pages[1].0, vec![("wide".to_string(), 48, usable_w, wide_h)]);
    assert_eq!(pages[2].0, vec![("plate".to_string(), 48, usable_w, plate_h)]);
    assert!(pages[2].1);
    assert_eq!(pages.len(), 3);
}

#[test]
fn illustrations_mix_with_text_outside_the_first_chapter() {
    let html = format!("<p>第一段。</p><p>{}</p><p>第二段。</p>", img("wide", 1920, 991));
    let pages = layout(&html, reader_params(false));
    assert_eq!(pages.len(), 1);
    assert!(pages[0].1);
    assert_eq!(pages[0].0.len(), 1);
    // 第一章里正文后面的图也不是封面
    let pages = layout(&html, reader_params(true));
    assert_eq!(pages.len(), 1);
}

#[test]
fn small_images_keep_their_natural_size() {
    let html = format!("<p>「{}兼——……」</p>", img("gaiji", 128, 128));
    let pages = layout(&html, reader_params(false));
    let (name, x, w, h) = pages[0].0[0].clone();
    assert_eq!((name.as_str(), w, h), ("gaiji", 128, 128));
    assert_eq!(x, 48 + (1236 - 2 * 48 - 128) / 2);
}

#[test]
fn python_rules_without_the_flags() {
    // 金标准规则: 不读地址里的尺寸, 没写宽高 = 宽 × 0.72
    let html = format!("<div>{}</div>", img("cover", 1348, 1920));
    let pages = layout(&html, LayoutParams { width: 1236, height: 1590, margin: 48, ..LayoutParams::default() });
    let usable_w = 1236 - 2 * 48;
    assert_eq!(pages[0].0[0].3, (usable_w as f64 * 0.72) as i64);
}
