//! Device benchmark for kn-render. Run on the Kindle (armv7 musl) via:
//!
//!   cargo zigbuild --release --target armv7-unknown-linux-musleabihf -p kn-render --example bench
//!
//! then copy the binary to /tmp on the device and run it there (see rust/SPIKE-GUIDE.md for
//! toolchain + device-access details). Takes two optional args: the system font path and an
//! optional cached chapter font path (TTF, already-decoded WOFF2) to prepend to the fallback chain.
//!
//! Measures: cold vs warm full-page CJK text rendering (24 lines x 24 chars @ 48px on a
//! 1236x1648 page), diff_bbox on two full frames (identical, and one-pixel-different),
//! fill_rect of the full screen, and rounded_rect of a button-sized rect with an outline.

use kn_render::{Bitmap, FontStore, GlyphCache, Rect, TextStyle};
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

const PAGE_WIDTH: u32 = 1236;
const PAGE_HEIGHT: u32 = 1648;
const FONT_SIZE: f32 = 48.0;
const CHARS_PER_LINE: usize = 24;
const LINES_PER_PAGE: usize = 24;
const LINE_H: i32 = 54;

/// A pool of frequently-used Han characters (no copyrighted text, just a repeating spread
/// of common Unicode code points), used as Lorem-Ipsum-style sample CJK text.
fn sample_text(target_chars: usize) -> Vec<char> {
    const POOL_RAW: &str = "的一是在不了有和人这中大为上个国我以要他时来用们生到作地于出就分对成会可主发年动同工也能下过子说产种面而方后多定行学法所民得经十三之进着等部度家电力里如水化高自二理起小物现实加量都两体制机当使点从业本去把性好应开它合还因由其些然前外天政四日那社义事平形相全表间样与关各重新线内数正心反你明看原又么利比或但质气第向道命此变条只没结解问意建月公无系军很情者最立代想已通并提直题党程展五果料象员革位入常文总次品式活设及管特件长求老头基资边流路级少图山统接知较将组见计别她手角期根论运农指几九区强放决西被干做必战先回则任取据处队南给色光门即保治北造百规热领七海口东导器压志世金增争济阶油思术极交受联什认六共权收证改清己美再采转更单风切打白教速花带安场身车例真务具万每目至达走积示议声报斗完类八离华名确才科张信马节话米整空元况今集温传土许步群广石记需段研界拉程热林";
    let mut seen = std::collections::HashSet::new();
    let pool: Vec<char> = POOL_RAW.chars().filter(|c| seen.insert(*c)).collect();
    let mut out = Vec::with_capacity(target_chars);
    let mut idx: usize = 0;
    while out.len() < target_chars {
        idx = (idx * 31 + 7) % pool.len();
        out.push(pool[idx]);
    }
    out
}

fn median_of(n: usize, mut f: impl FnMut() -> std::time::Duration) -> std::time::Duration {
    let mut v: Vec<_> = (0..n).map(|_| f()).collect();
    v.sort();
    v[n / 2]
}

fn main() {
    let sth_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/usr/java/lib/fonts/STHeitiMedium.ttf".to_string());
    let chapter_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "/mnt/us/extensions/kinnovel/cache/fonts/chapter1.ttf".to_string());

    let t_load = Instant::now();
    let mut store = FontStore::new();
    let sth = store
        .load_file(Path::new(&sth_path))
        .expect("load system font (STHeitiMedium.ttf)");
    let mut chain = Vec::new();
    if Path::new(&chapter_path).is_file() {
        match store.load_file(Path::new(&chapter_path)) {
            Ok(f) => {
                println!("[bench] loaded cached chapter font: {chapter_path}");
                chain.push(f);
            }
            Err(e) => println!("[bench] chapter font present but failed to load: {e}"),
        }
    }
    chain.push(sth);
    println!("[bench] font load: {:?}", t_load.elapsed());

    let style = TextStyle {
        fonts: chain,
        size_px: FONT_SIZE,
    };

    let sample = sample_text(CHARS_PER_LINE * LINES_PER_PAGE);
    let lines: Vec<String> = sample
        .chunks(CHARS_PER_LINE)
        .map(|c| c.iter().collect())
        .collect();

    // ---- correctness: blend_mask (NEON on device) vs scalar reference, odd widths/offsets ----
    {
        let mut seed = 12345u32;
        let mut rnd = || { seed = seed.wrapping_mul(1103515245).wrapping_add(12345); (seed >> 16) as u8 };
        let (mw, mh) = (53u32, 7u32);
        let mask: Vec<u8> = (0..mw * mh).map(|_| rnd()).collect();
        let mut bmp = Bitmap::new(70, 10, 0);
        for y in 0..10 { for x in 0..70 { let v = rnd(); bmp.fill_rect(Rect::new(x, y, 1, 1), v); } }
        let before = bmp.clone();
        let color = 37u8;
        bmp.blend_mask(&mask, mw, mh, 3, 2, color);
        let mut bad = 0;
        for y in 0..mh { for x in 0..mw {
            let d = before.get(x + 3, y + 2) as u32;
            let cov = mask[(y * mw + x) as usize] as u32;
            let expect = ((d * (255 - cov) + color as u32 * cov) as f64 / 255.0).round() as u8;
            if bmp.get(x + 3, y + 2) != expect { bad += 1; }
        } }
        println!("[bench] blend_mask correctness vs round(x/255): {} mismatches of {}", bad, mw * mh);
    }

    // ---- cold page render (fresh glyph cache) ----
    let mut cache = GlyphCache::new(4 * 1024 * 1024);
    let mut page = Bitmap::new(PAGE_WIDTH, PAGE_HEIGHT, 255);
    let t_cold = Instant::now();
    for (i, line) in lines.iter().enumerate() {
        let baseline_y = i as i32 * LINE_H + LINE_H - 14;
        cache.draw_text(&store, &mut page, 0.0, baseline_y as f32, line, &style, 0);
    }
    let cold_elapsed = t_cold.elapsed();
    println!(
        "[bench] cold page render (24x24 @ {}px, {}x{}): {:?}",
        FONT_SIZE as u32, PAGE_WIDTH, PAGE_HEIGHT, cold_elapsed
    );
    println!("[bench] glyph cache bytes_used after cold render: {}", cache.bytes_used());

    // ---- warm page render (same text, cache fully populated), median of 15 ----
    let mut page_warm = Bitmap::new(PAGE_WIDTH, PAGE_HEIGHT, 255);
    let warm = median_of(15, || {
        page_warm.fill_rect(page_warm.bounds(), 255);
        let t = Instant::now();
        for (i, line) in lines.iter().enumerate() {
            let baseline_y = i as i32 * LINE_H + LINE_H - 14;
            cache.draw_text(&store, &mut page_warm, 0.0, baseline_y as f32, line, &style, 0);
        }
        t.elapsed()
    });
    println!("[bench] warm page render (target < 15 ms): {:?}", warm);
    black_box(&page_warm);

    // ---- breakdown: glyph lookup only (draw into a 1x1 bitmap, every blend clips away) ----
    let mut tiny = Bitmap::new(1, 1, 255);
    let lookup = median_of(15, || {
        let t = Instant::now();
        for (i, line) in lines.iter().enumerate() {
            let baseline_y = i as i32 * LINE_H + LINE_H - 14;
            cache.draw_text(&store, &mut tiny, 2000.0, baseline_y as f32, line, &style, 0);
        }
        t.elapsed()
    });
    println!("[bench]   breakdown: glyph lookup/advance only: {:?}", lookup);
    // ---- breakdown: blend only (576 blends of a 48x48 mask with realistic coverage) ----
    let mask: Vec<u8> = (0..48 * 48).map(|i| [0u8, 0, 0, 255, 255, 128, 64, 0, 0, 200][i % 10]).collect();
    let blend = median_of(15, || {
        let t = Instant::now();
        for i in 0..576 {
            page_warm.blend_mask(black_box(&mask), 48, 48, (i % 24) as i32 * 50, (i / 24) as i32 * 54, 0);
        }
        t.elapsed()
    });
    println!("[bench]   breakdown: 576 x blend_mask 48x48: {:?}", blend);

    // ---- diff_bbox: identical frames, median of 15 ----
    let page_clone = page.clone();
    let mut same = None;
    let diff_same = median_of(15, || {
        let t = Instant::now();
        same = black_box(&page).diff_bbox(black_box(&page_clone));
        t.elapsed()
    });
    println!("[bench] diff_bbox identical frames (target < 5 ms): {:?} (result: {:?})", diff_same, same);

    // ---- diff_bbox: one-pixel-different frames ----
    let mut page_diff = page.clone();
    let old = page_diff.get(600, 800);
    page_diff.fill_rect(Rect::new(600, 800, 1, 1), old.wrapping_add(1));
    let mut one = None;
    let diff_one = median_of(15, || {
        let t = Instant::now();
        one = black_box(&page).diff_bbox(black_box(&page_diff));
        t.elapsed()
    });
    println!("[bench] diff_bbox one-pixel-different (target < 5 ms): {:?} (result: {:?})", diff_one, one);

    // ---- diff_bbox: full page of text vs blank (worst case: every row differs) ----
    let blank = Bitmap::new(PAGE_WIDTH, PAGE_HEIGHT, 255);
    let mut full = None;
    let diff_full = median_of(15, || {
        let t = Instant::now();
        full = black_box(&page).diff_bbox(black_box(&blank));
        t.elapsed()
    });
    println!("[bench] diff_bbox text page vs blank: {:?} (result: {:?})", diff_full, full);

    // ---- fill_rect: full screen ----
    // black_box around the buffer and bounds so LTO can't prove the writes are dead
    // (fill_buf is otherwise never read again, and whole-program LTO has been seen to
    // eliminate such "write then drop" buffers entirely).
    let mut fill_buf = black_box(Bitmap::new(PAGE_WIDTH, PAGE_HEIGHT, 0));
    let t_fill = Instant::now();
    let fb_bounds = fill_buf.bounds();
    fill_buf.fill_rect(black_box(fb_bounds), black_box(255));
    let fill_elapsed = t_fill.elapsed();
    black_box(&fill_buf);
    println!("[bench] fill_rect full screen ({}x{}): {:?}", PAGE_WIDTH, PAGE_HEIGHT, fill_elapsed);
    println!(
        "[bench] fill_rect checksum (prevents DCE): {}",
        fill_buf.data().iter().fold(0u64, |a, &b| a.wrapping_add(b as u64))
    );

    // ---- rounded_rect: a button-sized rect (1100x100, radius 10, 2px outline) ----
    let mut btn_buf = black_box(Bitmap::new(PAGE_WIDTH, PAGE_HEIGHT, 255));
    let btn_rect = black_box(Rect::new(68, 774, 1100, 100));
    let t_rrect = Instant::now();
    btn_buf.rounded_rect(btn_rect, black_box(10), Some(black_box(0)), Some(black_box(128)), black_box(2));
    let rrect_elapsed = t_rrect.elapsed();
    black_box(&btn_buf);
    println!(
        "[bench] rounded_rect 1100x100 r=10 outline=2 (target < 1 ms): {:?}",
        rrect_elapsed
    );
    println!(
        "[bench] rounded_rect checksum (prevents DCE): {}",
        btn_buf.data().iter().fold(0u64, |a, &b| a.wrapping_add(b as u64))
    );

    println!("[bench] done");
}
