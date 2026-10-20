//! Integration tests for kn-render's text engine.
//!
//! These rely on fonts available on the host rather than anything copyrighted/committed:
//! - System CJK fonts (Windows: simhei.ttf / msyh.ttc) for font-chain/fallback behavior.
//! - The gitignored spike-font WOFF2 sample for WOFF2 decoding, if present.
//! Any test whose font is missing on this host is skipped (prints a notice, does not fail).

use kn_render::{decode_woff2, FontStore, GlyphCache, TextStyle};
use std::path::{Path, PathBuf};

fn find_system_cjk_font() -> Option<PathBuf> {
    for candidate in [
        "C:/Windows/Fonts/simhei.ttf",
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/msyh.ttf",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn woff2_sample_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spike-font/testdata/chapter1.woff2")
}

macro_rules! skip_if_missing {
    ($opt:expr, $msg:expr) => {
        match $opt {
            Some(v) => v,
            None => {
                eprintln!("skipping: {}", $msg);
                return;
            }
        }
    };
}

#[test]
fn has_char_and_pick_font_fallback_order() {
    let font_path = skip_if_missing!(find_system_cjk_font(), "no system CJK font on this host");
    let mut store = FontStore::new();
    let font = store.load_file(&font_path).expect("load system font");

    // ASCII letter and a common Han character should both be present in a CJK system font.
    assert!(store.has_char(font, 'A'));
    assert!(store.has_char(font, '的'));

    // Whitespace is always reported available (mirrors reader.py's glyph_available()).
    assert!(store.has_char(font, ' '));

    // Single-font chain: pick_font must return that font regardless of coverage.
    let chain = [font];
    assert_eq!(store.pick_font(&chain, '的'), font);
    // Even for an exotic codepoint unlikely to be in the font, a single-element chain
    // still falls back to "the last font in the chain", i.e. itself.
    assert_eq!(store.pick_font(&chain, '\u{1F600}'), font);

    // Two-font chain where both entries are the same font: first-in-chain wins whenever
    // it has the glyph, and the last-in-chain is still used as the final fallback.
    let chain2 = [font, font];
    assert_eq!(store.pick_font(&chain2, '的'), font);
    assert_eq!(store.pick_font(&chain2, '\u{1F600}'), font);
}

#[test]
fn measure_equals_sum_of_advances() {
    let font_path = skip_if_missing!(find_system_cjk_font(), "no system CJK font on this host");
    let mut store = FontStore::new();
    let font = store.load_file(&font_path).expect("load system font");
    let style = TextStyle {
        fonts: vec![font],
        size_px: 32.0,
    };
    let text = "AB的字测试";
    let measured = store.measure(text, &style);

    let mut summed = 0.0f32;
    for ch in text.chars() {
        let f = store.pick_font(&style.fonts, ch);
        summed += store.advance(f, ch, style.size_px);
    }
    assert!(
        (measured - summed).abs() < 1e-3,
        "measure()={measured} vs manual sum={summed}"
    );
    assert!(measured > 0.0);
}

#[test]
fn draw_text_width_matches_measure_within_1px() {
    let font_path = skip_if_missing!(find_system_cjk_font(), "no system CJK font on this host");
    let mut store = FontStore::new();
    let font = store.load_file(&font_path).expect("load system font");
    let style = TextStyle {
        fonts: vec![font],
        size_px: 28.0,
    };
    let text = "测试Hello世界";
    let measured = store.measure(text, &style);

    let mut cache = GlyphCache::new(4 * 1024 * 1024);
    let mut bitmap = kn_render::Bitmap::new(800, 100, 255);
    let drawn_width = cache.draw_text(&store, &mut bitmap, 2.0, 60.0, text, &style, 0);

    assert!(
        (drawn_width - measured).abs() <= 1.0,
        "draw_text width={drawn_width} vs measure={measured}"
    );
}

#[test]
fn glyph_cache_hit_path_does_not_regrow_bytes() {
    let font_path = skip_if_missing!(find_system_cjk_font(), "no system CJK font on this host");
    let mut store = FontStore::new();
    let font = store.load_file(&font_path).expect("load system font");
    let style = TextStyle {
        fonts: vec![font],
        size_px: 40.0,
    };
    let text = "重复的字形缓存命中测试";

    let mut cache = GlyphCache::new(4 * 1024 * 1024);
    let mut bitmap = kn_render::Bitmap::new(1200, 120, 255);

    cache.draw_text(&store, &mut bitmap, 0.0, 80.0, text, &style, 0);
    let bytes_after_first = cache.bytes_used();
    assert!(bytes_after_first > 0, "expected some glyphs to be rasterized");

    // Second draw of the identical text at the identical size must be all cache hits:
    // bytes_used must not grow.
    cache.draw_text(&store, &mut bitmap, 0.0, 80.0, text, &style, 0);
    assert_eq!(cache.bytes_used(), bytes_after_first);
}

#[test]
fn glyph_cache_lru_eviction_by_bytes() {
    let font_path = skip_if_missing!(find_system_cjk_font(), "no system CJK font on this host");
    let mut store = FontStore::new();
    let font = store.load_file(&font_path).expect("load system font");
    let style = TextStyle {
        fonts: vec![font],
        size_px: 48.0,
    };
    // Many distinct CJK characters so we rasterize many distinct, sizeable glyphs.
    let text = "的一是在不了有和人这中大为上个国我以要他时来用们生到作地于出就分对成会可主发年动同工也能";

    // A tiny budget: only a few glyphs' worth of coverage bytes should survive.
    let max_bytes = 2000usize;
    let mut cache = GlyphCache::new(max_bytes);
    let mut bitmap = kn_render::Bitmap::new(2000, 200, 255);
    cache.draw_text(&store, &mut bitmap, 0.0, 150.0, text, &style, 0);

    assert!(
        cache.bytes_used() <= max_bytes,
        "bytes_used={} exceeds budget={}",
        cache.bytes_used(),
        max_bytes
    );

    cache.clear();
    assert_eq!(cache.bytes_used(), 0);
}

fn app_font(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../kn-app/data").join(name)).expect("read kn-app font")
}

fn draw(cache: &mut GlyphCache, store: &FontStore, style: &TextStyle, text: &str) -> Vec<u8> {
    let mut bitmap = kn_render::Bitmap::new(200, 80, 255);
    cache.draw_text(store, &mut bitmap, 4.0, 60.0, text, style, 0);
    (0..80).flat_map(|y| bitmap.row(y)[..200].to_vec()).collect()
}

/// 旧的 chain_key (每个 FontId 8 位) 把 [0] 和 [513, 0] 算成同一个键, 第二条链会沿用第一条链的
/// 字符映射。现在按整条链区分。
#[test]
fn font_chains_with_large_ids_do_not_share_char_mappings() {
    let emoji = app_font("NotoEmoji-subset.ttf");
    let symbols = app_font("KinNovelSymbols.ttf");
    let mut store = FontStore::new();
    let e = store.load_bytes(emoji).unwrap();
    assert_eq!(e.0, 0);
    // 换章时字体加载又卸载, FontId 单调增长
    for _ in 0..512 {
        let id = store.load_bytes(symbols.clone()).unwrap();
        store.unload(id);
    }
    let s = store.load_bytes(symbols).unwrap();
    assert_eq!(s.0, 513);
    let old_key = |chain: &[u32]| chain.iter().fold(chain.len() as u64, |k, &f| (k << 8) ^ f as u64);
    assert_eq!(old_key(&[0]), old_key(&[513, 0]));

    let ch = '♡';
    let a = TextStyle { fonts: vec![e], size_px: 40.0 };
    let b = TextStyle { fonts: vec![s, e], size_px: 40.0 };
    assert_eq!(store.pick_font(&a.fonts, ch), e);
    assert_eq!(store.pick_font(&b.fonts, ch), s);

    let fresh_b = draw(&mut GlyphCache::new(1 << 20), &store, &b, "♡");
    let mut cache = GlyphCache::new(1 << 20);
    let drawn_a = draw(&mut cache, &store, &a, "♡");
    let drawn_b = draw(&mut cache, &store, &b, "♡");
    assert_ne!(drawn_a, fresh_b, "test fonts must draw the char differently");
    assert_eq!(drawn_b, fresh_b);

    // 卸载后含该字体的链与映射都清掉, 其它链不受影响
    cache.forget_font(s);
    assert_eq!(draw(&mut cache, &store, &a, "♡"), drawn_a);
}

/// 超限时一批淘汰到约 75%, 最近用过的字形留下; 统计计数正确。
#[test]
fn glyph_cache_batch_eviction_keeps_recent_glyphs() {
    let mut store = FontStore::new();
    let font = store.load_bytes(app_font("KinNovelSymbols.ttf")).unwrap();
    let style = TextStyle { fonts: vec![font], size_px: 32.0 };
    // 符号字体里有的字符
    let chars: Vec<char> = ('\u{2190}'..='\u{2BFF}').filter(|&c| store.has_char(font, c)).take(200).collect();
    assert!(chars.len() >= 100, "symbol font coverage: {}", chars.len());

    let mut probe = GlyphCache::new(usize::MAX);
    let text: String = chars.iter().collect();
    draw(&mut probe, &store, &style, &text);
    let total = probe.bytes_used();
    let st = probe.stats();
    // 个别字符共用一个字形, 所以第一遍也可能有命中
    assert_eq!((st.misses + st.hits, st.evicted, st.entries as u64), (chars.len() as u64, 0, st.misses));
    draw(&mut probe, &store, &style, &text);
    assert_eq!(probe.stats().hits, st.hits + chars.len() as u64);

    let max = total / 3;
    let mut cache = GlyphCache::new(max);
    for &c in &chars {
        draw(&mut cache, &store, &style, &c.to_string());
        assert!(cache.bytes_used() <= max);
    }
    let st = cache.stats();
    assert!(st.evicted > 0 && st.entries < chars.len());
    assert_eq!(st.entries as u64 + st.evicted, st.misses);
    // 最后画的那个字形一定还在 (再画一次是命中)
    let hits = st.hits;
    draw(&mut cache, &store, &style, &chars.last().unwrap().to_string());
    assert_eq!(cache.stats().hits, hits + 1);
}

#[test]
fn woff2_decode_and_load_if_sample_present() {
    let path = woff2_sample_path();
    if !path.is_file() {
        eprintln!(
            "skipping: woff2 sample not present at {} (gitignored spike testdata)",
            path.display()
        );
        return;
    }
    let bytes = std::fs::read(&path).expect("read woff2 sample");
    let ttf = decode_woff2(&bytes).expect("decode woff2 -> ttf");
    assert!(ttf.len() > 4);

    let mut store = FontStore::new();
    let font = store.load_bytes(ttf).expect("load decoded ttf");
    // The chapter font should at least resolve something for a common Han character
    // without panicking, regardless of whether it actually covers it.
    let _ = store.has_char(font, '的');
    let style = TextStyle {
        fonts: vec![font],
        size_px: 32.0,
    };
    let w = store.measure("的", &style);
    assert!(w > 0.0);
}
