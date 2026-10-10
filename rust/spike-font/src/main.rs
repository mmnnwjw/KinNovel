use skrifa::{
    outline::{DrawSettings, OutlinePen},
    prelude::{LocationRef, Size},
    FontRef, GlyphId, MetadataProvider,
};
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;
use zeno::{Command, Fill, Mask, PathBuilder};

const PAGE_WIDTH: usize = 1236;
const FONT_SIZE: f32 = 48.0;
const CELL_W: usize = 51; // ~1236/24
const LINE_H: usize = 54;
const CHARS_PER_LINE: usize = 24;
const LINES_PER_PAGE: usize = 24;

/// Collects glyph outline commands via skrifa's OutlinePen trait, into a zeno path.
struct PenCollector {
    cmds: Vec<Command>,
}

// Font outlines are Y-up (em space); raster images are Y-down, so flip Y here.
impl OutlinePen for PenCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cmds.move_to((x, -y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cmds.line_to((x, -y));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.cmds.quad_to((cx0, -cy0), (x, -y));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.cmds.curve_to((cx0, -cy0), (cx1, -cy1), (x, -y));
    }
    fn close(&mut self) {
        self.cmds.close();
    }
}

/// A rasterized glyph bitmap: tightly-cropped L8 alpha mask plus placement/advance info.
struct GlyphBitmap {
    width: u32,
    height: u32,
    left: i32,  // offset from pen x to bitmap left
    top: i32,   // offset from baseline to bitmap top (positive = above baseline)
    advance: f32,
    pixels: Vec<u8>,
}

/// Wraps a parsed font + a glyph bitmap cache, keyed by (glyph_id, size_bits).
struct LoadedFont<'a> {
    font: FontRef<'a>,
    cache: HashMap<(u32, u32), GlyphBitmap>,
}

impl<'a> LoadedFont<'a> {
    fn new(font: FontRef<'a>) -> Self {
        Self {
            font,
            cache: HashMap::new(),
        }
    }

    fn cmap_has(&self, ch: char) -> bool {
        self.font.charmap().map(ch).is_some()
    }

    fn glyph_id(&self, ch: char) -> Option<GlyphId> {
        self.font.charmap().map(ch)
    }

    /// Rasterize (or fetch from cache) the glyph for `ch` at FONT_SIZE.
    fn rasterize(&mut self, ch: char) -> Option<&GlyphBitmap> {
        let gid = self.glyph_id(ch)?;
        let key = (gid.to_u32(), FONT_SIZE.to_bits());
        if !self.cache.contains_key(&key) {
            let bitmap = Self::rasterize_glyph(&self.font, gid, FONT_SIZE)?;
            self.cache.insert(key, bitmap);
        }
        self.cache.get(&key)
    }

    fn advance_width(&self, ch: char) -> f32 {
        if let Some(gid) = self.glyph_id(ch) {
            let metrics = self
                .font
                .glyph_metrics(Size::new(FONT_SIZE), LocationRef::default());
            metrics.advance_width(gid).unwrap_or(FONT_SIZE)
        } else {
            FONT_SIZE
        }
    }

    fn rasterize_glyph(font: &FontRef, gid: GlyphId, size: f32) -> Option<GlyphBitmap> {
        let outlines = font.outline_glyphs();
        let outline = outlines.get(gid)?;
        let mut pen = PenCollector { cmds: Vec::new() };
        let settings = DrawSettings::unhinted(Size::new(size), LocationRef::default());
        outline.draw(settings, &mut pen).ok()?;
        if pen.cmds.is_empty() {
            // Space or empty glyph (e.g. notdef with no contours): zero-size bitmap.
            let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
            let advance = metrics.advance_width(gid).unwrap_or(size);
            return Some(GlyphBitmap {
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                advance,
                pixels: Vec::new(),
            });
        }
        // zeno's Mask can compute a tight bounding box for us.
        let (mask_data, placement) = Mask::new(&pen.cmds)
            .style(Fill::NonZero)
            .render();
        let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
        let advance = metrics.advance_width(gid).unwrap_or(size);
        Some(GlyphBitmap {
            width: placement.width,
            height: placement.height,
            left: placement.left,
            top: placement.top,
            advance,
            pixels: mask_data,
        })
    }
}

/// Decide which font to use for a char: chapter font if its cmap covers it (mirrors
/// reader.py's glyph_available()/char_font() using FT_Get_Char_Index-style cmap lookup).
fn pick_font<'a>(
    ch: char,
    chapter: &LoadedFont<'a>,
    fallback: &LoadedFont<'a>,
) -> bool /* true = use chapter font */ {
    if ch.is_whitespace() {
        return true;
    }
    chapter.cmap_has(ch) || !fallback.cmap_has(ch)
}

fn vm_hwm_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            return digits.parse().ok();
        }
    }
    None
}

/// Generate ~2000 CJK sample characters from a pool of common Han characters
/// (no network, no copyrighted text — just a repeating pseudo-random spread of
/// unicode code points in the common Han ranges, like a Lorem Ipsum stand-in).
fn sample_text(target_chars: usize) -> Vec<char> {
    // A pool of frequently-used Han characters (public domain concept: Unicode code
    // points, not literary text) plus basic punctuation, repeated/shuffled.
    const POOL_RAW: &str = "的一是在不了有和人这中大为上个国我以要他时来用们生到作地于出就分对成会可主发年动同工也能下过子说产种面而方后多定行学法所民得经十三之进着等部度家电力里如水化高自二理起小物现实加量都两体制机当使点从业本去把性好应开它合还因由其些然前外天政四日那社义事平形相全表间样与关各重新线内数正心反你明看原又么利比或但质气第向道命此变条只没结解问意建月公无系军很情者最立代想已通并提直题党程展五果料象员革位入常文总次品式活设及管特件长求老头基资边流路级少图山统接知较将组见计别她手角期根论运农指几九区强放决西被干做必战先回则任取据处队南给色光门即保治北造百规热领七海口东导器压志世金增争济阶油思术极交受联什认六共权收证改清己美再采转更单风切打白教速花带安场身车例真务具万每目至达走积示议声报斗完类八离华名确才科张信马节话米整空元况今集温传土许步群广石记需段研界拉程热林";
    let mut seen = std::collections::HashSet::new();
    let pool: Vec<char> = POOL_RAW
        .chars()
        .filter(|c| seen.insert(*c))
        .collect::<Vec<char>>();
    let mut out = Vec::with_capacity(target_chars);
    let mut idx: usize = 0;
    while out.len() < target_chars {
        // simple LCG-ish stride through the pool so it's not a trivial repeat
        idx = (idx * 31 + 7) % pool.len();
        out.push(pool[idx]);
    }
    out
}

/// Greedy wrap into fixed CHARS_PER_LINE-character lines (naive, matches the spike's
/// "24 chars per line" sizing target rather than reader.py's punctuation-aware wrap).
fn wrap_lines(chars: &[char], chars_per_line: usize) -> Vec<&[char]> {
    chars.chunks(chars_per_line).collect()
}

/// Rasterize one page of text (lines x chars_per_line) into an L8 grayscale buffer,
/// using per-character font selection + glyph cache. Returns (buffer, width, height).
fn render_page(
    lines: &[&[char]],
    chapter: &mut LoadedFont,
    fallback: &mut LoadedFont,
) -> (Vec<u8>, usize, usize) {
    let height = lines.len() * LINE_H;
    let mut buf = vec![0u8; PAGE_WIDTH * height];
    for (row, line) in lines.iter().enumerate() {
        let baseline_y = (row * LINE_H + LINE_H - 14) as i32;
        let mut pen_x: f32 = 0.0;
        for &ch in line.iter() {
            let use_chapter = pick_font(ch, chapter, fallback);
            let bmp = if use_chapter {
                chapter.rasterize(ch)
            } else {
                fallback.rasterize(ch)
            };
            if let Some(bmp) = bmp {
                if bmp.width > 0 && bmp.height > 0 {
                    let origin_x = pen_x as i32 + bmp.left;
                    // zeno's Placement::top is already a y-down offset from the glyph's
                    // (0,0) origin (negative = above baseline), so add it directly.
                    let origin_y = baseline_y + bmp.top;
                    for yy in 0..bmp.height as i32 {
                        let dy = origin_y + yy;
                        if dy < 0 || dy as usize >= height {
                            continue;
                        }
                        for xx in 0..bmp.width as i32 {
                            let dx = origin_x + xx;
                            if dx < 0 || dx as usize >= PAGE_WIDTH {
                                continue;
                            }
                            let src = bmp.pixels[(yy as usize) * bmp.width as usize + xx as usize];
                            if src > 0 {
                                let dst_idx = dy as usize * PAGE_WIDTH + dx as usize;
                                buf[dst_idx] = buf[dst_idx].max(src);
                            }
                        }
                    }
                }
                pen_x += if use_chapter {
                    chapter.advance_width(ch)
                } else {
                    fallback.advance_width(ch)
                }
                .max(1.0)
                .min(CELL_W as f32 * 1.2);
            } else {
                pen_x += CELL_W as f32;
            }
        }
    }
    (buf, PAGE_WIDTH, height)
}

fn write_pgm(path: &str, buf: &[u8], width: usize, height: usize) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    write!(f, "P5\n{} {}\n255\n", width, height)?;
    f.write_all(buf)?;
    Ok(())
}

fn main() {
    let t0 = Instant::now();

    let woff2_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "testdata/chapter1.woff2".to_string());
    let sth_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "testdata/STHeitiMedium.ttf".to_string());

    // ---- font load + decode ----
    let t_load_start = Instant::now();
    let woff2_bytes = std::fs::read(&woff2_path).expect("read woff2");
    let ttf_bytes = woff2_patched::convert_woff2_to_ttf(&mut std::io::Cursor::new(&woff2_bytes))
        .expect("woff2 -> ttf decode failed");
    let sth_bytes = std::fs::read(&sth_path).expect("read STHeiti ttf");
    let load_decode_elapsed = t_load_start.elapsed();

    let chapter_font_ref = FontRef::new(&ttf_bytes).expect("parse decoded chapter ttf");
    let sth_font_ref = FontRef::new(&sth_bytes).expect("parse STHeiti ttf");
    let mut chapter = LoadedFont::new(chapter_font_ref);
    let mut fallback = LoadedFont::new(sth_font_ref);

    println!("[spike-font] woff2 decoded: {} bytes -> {} bytes ttf", woff2_bytes.len(), ttf_bytes.len());
    println!("[spike-font] font load+decode: {:?}", load_decode_elapsed);

    // ---- cmap queries ----
    let sample = sample_text(2000);
    let t_cmap = Instant::now();
    let mut chapter_hits = 0usize;
    let mut fallback_hits = 0usize;
    for &ch in &sample {
        if chapter.cmap_has(ch) {
            chapter_hits += 1;
        }
        if fallback.cmap_has(ch) {
            fallback_hits += 1;
        }
    }
    let cmap_elapsed = t_cmap.elapsed();
    println!(
        "[spike-font] cmap queries: {:?} for {} chars x2 fonts (chapter cmap hits {}/{}, STHeiti hits {}/{})",
        cmap_elapsed,
        sample.len(),
        chapter_hits,
        sample.len(),
        fallback_hits,
        sample.len()
    );

    // ---- ad hoc visual-compare sample (same literal chars as the Pillow comparison) ----
    {
        let compare_chars: Vec<char> = "的一是在不了有和人这".chars().collect();
        let compare_lines: Vec<&[char]> = vec![compare_chars.as_slice()];
        let (cbuf, cw, ch) = render_page(&compare_lines, &mut chapter, &mut fallback);
        let out_dir = std::env::args().nth(3).unwrap_or_else(|| "/tmp".to_string());
        let cmp_path = format!("{}/spike_font_compare.pgm", out_dir);
        let _ = write_pgm(&cmp_path, &cbuf, cw, ch);
        println!("[spike-font] wrote compare sample {}", cmp_path);
    }

    // ---- layout ----
    let lines_all = wrap_lines(&sample, CHARS_PER_LINE);
    let page1: Vec<&[char]> = lines_all.iter().take(LINES_PER_PAGE).cloned().collect();
    let page2_start = LINES_PER_PAGE.min(lines_all.len());
    let page2_end = (page2_start + LINES_PER_PAGE).min(lines_all.len());
    let page2: Vec<&[char]> = lines_all[page2_start..page2_end].to_vec();

    // ---- first page render: cold (glyph cache empty) ----
    let t_cold = Instant::now();
    let (buf1, w1, h1) = render_page(&page1, &mut chapter, &mut fallback);
    let cold_elapsed = t_cold.elapsed();
    println!(
        "[spike-font] first page render (cold glyph cache): {:?} ({} lines x {} chars, {}x{})",
        cold_elapsed,
        page1.len(),
        CHARS_PER_LINE,
        w1,
        h1
    );
    println!(
        "[spike-font] glyph cache size after page1: chapter={} fallback={}",
        chapter.cache.len(),
        fallback.cache.len()
    );

    // ---- second page render: warm (mostly same glyphs already cached) ----
    let t_warm = Instant::now();
    let (buf2, w2, h2) = if !page2.is_empty() {
        render_page(&page2, &mut chapter, &mut fallback)
    } else {
        render_page(&page1, &mut chapter, &mut fallback)
    };
    let warm_elapsed = t_warm.elapsed();
    println!(
        "[spike-font] second page render (warm glyph cache): {:?} ({}x{})",
        warm_elapsed, w2, h2
    );

    // ---- RSS ----
    if let Some(hwm) = vm_hwm_kb() {
        println!("[spike-font] VmHWM: {} kB", hwm);
    } else {
        println!("[spike-font] VmHWM: unavailable (not Linux /proc)");
    }

    // ---- write PGMs for visual verification ----
    let out_dir = std::env::args().nth(3).unwrap_or_else(|| "/tmp".to_string());
    let out1 = format!("{}/spike_font_page1.pgm", out_dir);
    let out2 = format!("{}/spike_font_page2.pgm", out_dir);
    if let Err(e) = write_pgm(&out1, &buf1, w1, h1) {
        println!("[spike-font] failed to write {}: {}", out1, e);
    } else {
        println!("[spike-font] wrote {} ({} bytes)", out1, buf1.len());
    }
    if let Err(e) = write_pgm(&out2, &buf2, w2, h2) {
        println!("[spike-font] failed to write {}: {}", out2, e);
    } else {
        println!("[spike-font] wrote {} ({} bytes)", out2, buf2.len());
    }

    println!("[spike-font] total wall time: {:?}", t0.elapsed());
}
