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
