//! 字体加载、逐字回退、度量与字形缓存。
//!
//! 行为必须与 Python 版 `bin/src/kinnovel/reader.py` 一致:
//! - 字体链 (TextStyle::fonts) 按顺序查 cmap, 第一个含该字符的字体负责它 (对应 `char_font`/`split_font_runs`);
//!   都没有则用链上最后一个字体 (通常是系统字体) 画 .notdef。
//! - 宽度为各字形 advance 之和 (对应 `text_width`), 单位像素, f32。
//! - 零宽/不可见字符 (见 reader.py `_INVISIBLE_RE`) 由上层清洗, 这里不特殊处理。
//!
//! 实现: `woff2-patched` 解码 WOFF2 → TTF; `skrifa` 解析 cmap/度量/轮廓; `zeno` 光栅化为覆盖率蒙版
//! (注意 skrifa 轮廓 Y 向上, 位图 Y 向下; zeno Placement.top 已是相对基线的 Y 向下偏移)。

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::Path;

use skrifa::{
    outline::{DrawSettings, OutlinePen},
    prelude::{LocationRef, Size},
    FontRef, GlyphId, MetadataProvider,
};
use zeno::{Command, Fill, Mask, PathBuilder};

use crate::bitmap::Bitmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub u32);

#[derive(Debug)]
pub enum TextError {
    Io(std::io::Error),
    /// WOFF2 解码失败或不是可识别的 TTF/OTF/WOFF2
    BadFont(String),
}

impl From<std::io::Error> for TextError {
    fn from(e: std::io::Error) -> Self {
        TextError::Io(e)
    }
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextError::Io(e) => write!(f, "io error: {e}"),
            TextError::BadFont(msg) => write!(f, "bad font: {msg}"),
        }
    }
}

impl std::error::Error for TextError {}

/// 合成字形 (字体本身没有粗体/斜体字重时模拟网页版的 `font-weight: bold` / `font-style: italic`)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Synth {
    /// 加粗: 同一字形向右错开 1~2 像素再画一遍 (宽度不变, 不影响排版)
    pub bold: bool,
    /// 斜体: 光栅化时把轮廓向右倾斜 (约 12°)
    pub italic: bool,
}

/// 斜体的倾斜系数 (x += y * k, 约 12°)。
const ITALIC_SKEW: f32 = 0.21;

/// 合成粗体的额外描画次数 (每次右移 1 像素): 48 px 正文约 2 像素。
pub fn bold_strokes(size_px: f32) -> i32 {
    ((size_px * 0.04).round() as i32).max(1)
}

/// 一段文字的样式: 字体回退链 + 像素字号。借用字体链, 每段文字建一个不用分配。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle<'a> {
    pub fonts: &'a [FontId],
    pub size_px: f32,
}

struct LoadedFont {
    /// 空 = 已卸载 (FontId 不复用, 避免缓存串号)
    data: Vec<u8>,
}

impl LoadedFont {
    fn font_ref(&self) -> FontRef<'_> {
        // TTC 使用第一个字体; 字节在加载时已用同样的入口验证。
        FontRef::from_index(&self.data, 0).expect("font bytes validated at load time")
    }
}

/// 已加载字体的集合。字体数据常驻内存 (TTF 字节), FontId 是其下标。
pub struct FontStore {
    fonts: Vec<LoadedFont>,
}

impl Default for FontStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FontStore {
    pub fn new() -> Self {
        FontStore { fonts: Vec::new() }
    }

    /// 从文件加载 TTF/OTF/TTC/WOFF2 (TTC 取第一个字体, WOFF2 按文件头 `wOF2` 判断)。WOFF2 解码后的 TTF 字节可由调用方缓存,
    /// 见 `decode_woff2`。
    pub fn load_file(&mut self, path: &Path) -> Result<FontId, TextError> {
        let bytes = std::fs::read(path)?;
        self.load_bytes(bytes)
    }

    /// 从内存加载 (TTF/OTF/TTC/WOFF2)。
    pub fn load_bytes(&mut self, bytes: Vec<u8>) -> Result<FontId, TextError> {
        let ttf_bytes = if woff2_patched::decode::is_woff2(&bytes) {
            decode_woff2(&bytes)?
        } else {
            bytes
        };
        // Validate it parses before storing; FontRef borrows, so just check and drop.
        FontRef::from_index(&ttf_bytes, 0).map_err(|e| TextError::BadFont(format!("{e:?}")))?;
        let id = FontId(self.fonts.len() as u32);
        self.fonts.push(LoadedFont { data: ttf_bytes });
        Ok(id)
    }

    fn loaded(&self, font: FontId) -> &LoadedFont {
        &self.fonts[font.0 as usize]
    }

    /// Internal: the parsed font for `font`, used by the glyph cache for rasterization.
    pub(crate) fn font_ref(&self, font: FontId) -> FontRef<'_> {
        self.loaded(font).font_ref()
    }

    /// Internal: glyph id for `ch` in `font`'s cmap, or `.notdef` (id 0) if absent.
    pub(crate) fn glyph_id(&self, font: FontId, ch: char) -> GlyphId {
        self.font_ref(font)
            .charmap()
            .map(ch)
            .unwrap_or(GlyphId::new(0))
    }

    /// 该字体 cmap 是否包含字符 (O(1) 级, 不光栅化)。
    pub fn has_char(&self, font: FontId, ch: char) -> bool {
        if !self.is_loaded(font) {
            return false;
        }
        // Mirrors reader.py's glyph_available(): whitespace is always considered
        // available on the primary font, regardless of actual cmap coverage.
        if ch.is_whitespace() {
            return true;
        }
        self.font_ref(font).charmap().map(ch).is_some()
    }

    /// 释放字体数据 (章节字体解码后有几 MB, 换章时卸载旧的)。之后该 FontId 不能再用于绘制;
    /// 调用方应同时 `GlyphCache::forget_font`。
    pub fn unload(&mut self, font: FontId) {
        if let Some(f) = self.fonts.get_mut(font.0 as usize) {
            f.data = Vec::new();
        }
    }

    pub fn is_loaded(&self, font: FontId) -> bool {
        self.fonts.get(font.0 as usize).is_some_and(|f| !f.data.is_empty())
    }

    /// 字体链中负责该字符的字体 (见模块文档)。
    pub fn pick_font(&self, chain: &[FontId], ch: char) -> FontId {
        for &f in chain {
            if self.has_char(f, ch) {
                return f;
            }
        }
        *chain
            .last()
            .expect("TextStyle.fonts must contain at least one font")
    }

    /// 单字符 advance (像素)。
    pub fn advance(&self, font: FontId, ch: char, size_px: f32) -> f32 {
        if is_invisible(ch) {
            return 0.0;
        }
        let font_ref = self.font_ref(font);
        let gid = font_ref.charmap().map(ch);
        match gid {
            Some(gid) => {
                let metrics = font_ref.glyph_metrics(Size::new(size_px), LocationRef::default());
                metrics.advance_width(gid).unwrap_or(size_px)
            }
            None => size_px,
        }
    }

    /// 整段宽度 = 逐字按回退链选字体后的 advance 之和。
    pub fn measure(&self, text: &str, style: &TextStyle) -> f32 {
        let mut total = 0.0f32;
        // Per-call memo so repeated chars don't re-run cmap/fallback lookups.
        let mut memo: HashMap<char, f32> = HashMap::new();
        for ch in text.chars() {
            let w = *memo.entry(ch).or_insert_with(|| {
                let font = self.pick_font(&style.fonts, ch);
                self.advance(font, ch, style.size_px)
            });
            total += w;
        }
        total
    }

    /// 最长可容纳前缀 + 省略号。宽度按 measure 的顺序累加, 每个字符只查一次字体。
    pub fn fit(&self, text: &str, style: &TextStyle, max_width: f32) -> String {
        let mut memo = HashMap::new();
        let mut prefixes = vec![(0, 0.0f32)];
        let mut total = 0.0;
        for (i, ch) in text.char_indices() {
            total += *memo.entry(ch).or_insert_with(|| self.advance(self.pick_font(&style.fonts, ch), ch, style.size_px));
            prefixes.push((i + ch.len_utf8(), total));
        }
        if total <= max_width {
            return text.to_string();
        }
        let ellipsis = self.measure("…", style);
        for &(end, width) in prefixes.iter().rev().skip(1) {
            if width + ellipsis <= max_width {
                return format!("{}…", &text[..end]);
            }
        }
        "…".to_string()
    }

    /// 字号对应的 ascent/descent/行高 (像素), 取链上第一个字体。
    pub fn line_metrics(&self, style: &TextStyle) -> LineMetrics {
        let font = *style
            .fonts
            .first()
            .expect("TextStyle.fonts must contain at least one font");
        let font_ref = self.font_ref(font);
        let m = font_ref.metrics(Size::new(style.size_px), LocationRef::default());
        LineMetrics {
            ascent: m.ascent,
            descent: m.descent,
            line_gap: m.leading,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

/// WOFF2 → TTF 字节, 供调用方落盘缓存。
/// 不占位、不绘制的字符: 不做字形组合 (shaping), emoji 序列里的这些单独画只会是方框或色块。
/// 零宽连接符、变体选择符 (FE0E/FE0F 等)、标签字符 (旗帜子区域)、肤色修饰符、组合键帽符。
pub fn is_invisible(ch: char) -> bool {
    matches!(ch, '\u{200D}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0020}'..='\u{E007F}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{20E3}')
}

pub fn decode_woff2(bytes: &[u8]) -> Result<Vec<u8>, TextError> {
    woff2_patched::convert_woff2_to_ttf(&mut std::io::Cursor::new(bytes))
        .map_err(|e| TextError::BadFont(format!("{e:?}")))
}

/// Collects glyph outline commands via skrifa's `OutlinePen` trait into a zeno path.
/// Font outlines are Y-up (em space); raster images are Y-down, so flip Y here.
/// `skew` != 0 时同时做斜体倾斜 (y 越高越往右)。
struct PenCollector {
    cmds: Vec<Command>,
    skew: f32,
}

impl PenCollector {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        (x + y * self.skew, -y)
    }
}

impl OutlinePen for PenCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.cmds.move_to(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.cmds.line_to(p);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (c, p) = (self.p(cx0, cy0), self.p(x, y));
        self.cmds.quad_to(c, p);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (c0, c1, p) = (self.p(cx0, cy0), self.p(cx1, cy1), self.p(x, y));
        self.cmds.curve_to(c0, c1, p);
    }
    fn close(&mut self) {
        self.cmds.close();
    }
}

/// One rasterized, tightly-cropped glyph: coverage mask + placement + advance.
struct GlyphEntry {
    width: u32,
    height: u32,
    /// Offset from pen x to bitmap left.
    left: i32,
    /// Offset from baseline to bitmap top (zeno's `Placement::top`, already y-down).
    top: i32,
    advance: f32,
    /// `width * height` bytes of coverage (0..=255), empty for zero-size glyphs (e.g. space).
    pixels: Vec<u8>,
    last_used: u64,
}

impl GlyphEntry {
    fn byte_size(&self) -> usize {
        self.pixels.len()
    }
}

/// FxHash (rustc 用的乘法哈希): 键都是小整数, 默认 SipHash 在 armv7 上明显更慢。
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u32);

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(b as u32);
        }
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x9e37_79b9);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.write_u32(i as u32);
        self.write_u32((i >> 32) as u32);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0 as u64
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

fn quantize_size(size_px: f32) -> u32 {
    (size_px.max(0.0) * 64.0).round() as u32
}

/// 字形覆盖率位图缓存, 键 = (FontId, glyph id, 字号的 1/64 像素量化)。
/// 以字节数为上限做 LRU 淘汰 (默认 4 MB)。
pub struct GlyphCache {
    max_bytes: usize,
    bytes_used: usize,
    tick: u64,
    map: FxMap<(FontId, u32, u32), GlyphEntry>,
    /// 字体链 → 编号 (按整条链查, 不会串号; 编号不复用)。
    chains: FxMap<Vec<FontId>, u32>,
    next_chain: u32,
    /// (字体链编号, 字符) → (负责的字体, 字形 id): 跨调用复用, 省去每次 cmap 查找与回退。
    chars: FxMap<(u32, char), (FontId, GlyphId)>,
    stats: GlyphStats,
}

/// 字形缓存的命中统计 (供 KN_DEBUG 打印, 用来定缓存容量)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphStats {
    pub hits: u64,
    pub misses: u64,
    /// 被淘汰的字形数
    pub evicted: u64,
    /// 当前缓存的字形数与字节数
    pub entries: usize,
    pub bytes: usize,
}

impl GlyphCache {
    pub fn new(max_bytes: usize) -> Self {
        GlyphCache {
            max_bytes,
            bytes_used: 0,
            tick: 0,
            map: FxMap::default(),
            chains: FxMap::default(),
            next_chain: 0,
            chars: FxMap::default(),
            stats: GlyphStats::default(),
        }
    }

    /// 字体链的编号 (第一次见到时分配)。
    fn chain_id(&mut self, chain: &[FontId]) -> u32 {
        if let Some(&id) = self.chains.get(chain) {
            return id;
        }
        let id = self.next_chain;
        self.next_chain += 1;
        self.chains.insert(chain.to_vec(), id);
        id
    }

    fn rasterize(fonts: &FontStore, font_id: FontId, gid: GlyphId, size_px: f32, italic: bool) -> GlyphEntry {
        let font_ref = fonts.font_ref(font_id);
        let metrics = font_ref.glyph_metrics(Size::new(size_px), LocationRef::default());
        let advance = metrics.advance_width(gid).unwrap_or(size_px);

        let outlines = font_ref.outline_glyphs();
        if let Some(outline) = outlines.get(gid) {
            let mut pen = PenCollector { cmds: Vec::new(), skew: if italic { ITALIC_SKEW } else { 0.0 } };
            let settings = DrawSettings::unhinted(Size::new(size_px), LocationRef::default());
            if outline.draw(settings, &mut pen).is_ok() && !pen.cmds.is_empty() {
                let (pixels, placement) = Mask::new(&pen.cmds).style(Fill::NonZero).render();
                return GlyphEntry {
                    width: placement.width,
                    height: placement.height,
                    left: placement.left,
                    top: placement.top,
                    advance,
                    pixels,
                    last_used: 0,
                };
            }
        }
        // Space, empty contours, or an unrenderable .notdef: zero-size bitmap.
        GlyphEntry {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            advance,
            pixels: Vec::new(),
            last_used: 0,
        }
    }

    /// 超限时一次淘汰最久没用的一批, 降到上限的 75%: 每批 O(n), 之后要再插入约 n/4 个字形才会再淘汰,
    /// 均摊 O(1) (以前每插一个字形都全表扫描找最旧的)。
    fn evict_if_needed(&mut self) {
        if self.bytes_used <= self.max_bytes {
            return;
        }
        let goal = self.max_bytes / 4 * 3;
        let mut ages: Vec<(u64, (FontId, u32, u32))> = self.map.iter().map(|(k, e)| (e.last_used, *k)).collect();
        let mut start = 0;
        while self.bytes_used > goal && start < ages.len() {
            // 按字节比例估计要淘汰几个, 字形大小不一时不够就再来一轮
            let rest = &mut ages[start..];
            let excess = (self.bytes_used - goal) as u64;
            let k = ((rest.len() as u64 * excess).div_ceil(self.bytes_used.max(1) as u64) as usize).clamp(1, rest.len());
            if k < rest.len() {
                rest.select_nth_unstable(k);
            }
            for &(_, key) in &rest[..k] {
                if let Some(removed) = self.map.remove(&key) {
                    self.bytes_used -= removed.byte_size();
                    self.stats.evicted += 1;
                }
            }
            start += k;
        }
    }

    /// 命中统计与当前占用。
    pub fn stats(&self) -> GlyphStats {
        GlyphStats { entries: self.map.len(), bytes: self.bytes_used, ..self.stats }
    }

    /// 在 (x, baseline_y) 处按样式绘制文字, 颜色 `color`; 返回绘制宽度 (像素)。
    /// x 以 1/64 像素累加 advance 再取整定位每个字形, 与 measure 结果一致。
    pub fn draw_text(
        &mut self,
        fonts: &FontStore,
        target: &mut Bitmap,
        x: f32,
        baseline_y: f32,
        text: &str,
        style: &TextStyle,
        color: u8,
    ) -> f32 {
        self.draw_text_synth(fonts, target, x, baseline_y, text, style, color, Synth::default())
    }

    /// 同 [`Self::draw_text`], 可合成粗体/斜体 (advance 不变)。
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_synth(
        &mut self,
        fonts: &FontStore,
        target: &mut Bitmap,
        x: f32,
        baseline_y: f32,
        text: &str,
        style: &TextStyle,
        color: u8,
        synth: Synth,
    ) -> f32 {
        if style.fonts.is_empty() {
            return 0.0;
        }
        // 斜体字形单独缓存 (最高位做标记)
        let size_key = quantize_size(style.size_px) | if synth.italic { 1 << 31 } else { 0 };
        let strokes = if synth.bold { bold_strokes(style.size_px) } else { 0 };
        let base_y = baseline_y.round() as i32;
        let start_x_64 = (x as f64 * 64.0).round() as i64;
        let mut pen_x_64 = start_x_64;

        let ck = self.chain_id(&style.fonts);

        for ch in text.chars() {
            if is_invisible(ch) {
                continue;
            }
            let (font_id, gid) = *self.chars.entry((ck, ch)).or_insert_with(|| {
                let font_id = fonts.pick_font(&style.fonts, ch);
                (font_id, fonts.glyph_id(font_id, ch))
            });

            let key = (font_id, gid.to_u32(), size_key);
            self.tick += 1;
            let tick = self.tick;
            if self.map.contains_key(&key) {
                self.stats.hits += 1;
            } else {
                self.stats.misses += 1;
                let mut entry = Self::rasterize(fonts, font_id, gid, style.size_px, synth.italic);
                entry.last_used = tick; // 先标记为最新, 免得插入后立刻被淘汰
                self.bytes_used += entry.byte_size();
                self.map.insert(key, entry);
                self.evict_if_needed();
            }
            let Some(entry) = self.map.get_mut(&key) else {
                continue; // max_bytes 小于单个字形时会被立即淘汰
            };
            entry.last_used = tick;
            let entry = &*entry;

            if entry.width > 0 && entry.height > 0 {
                let px = (pen_x_64 as f64 / 64.0) as f32;
                let origin_x = px.round() as i32 + entry.left;
                let origin_y = base_y + entry.top;
                for dx in 0..=strokes {
                    target.blend_mask(&entry.pixels, entry.width, entry.height, origin_x + dx, origin_y, color);
                }
            }
            pen_x_64 += (entry.advance as f64 * 64.0).round() as i64;
        }

        ((pen_x_64 - start_x_64) as f64 / 64.0) as f32
    }

    /// 丢弃某个字体的全部字形与字符映射 (字体卸载后调用)。
    pub fn forget_font(&mut self, font: FontId) {
        let mut freed = 0;
        self.map.retain(|k, e| {
            let keep = k.0 != font;
            if !keep {
                freed += e.byte_size();
            }
            keep
        });
        self.bytes_used -= freed;
        // 含这个字体的链不会再用 (FontId 不复用), 连同它们的字符映射一起丢掉
        let mut dead = Vec::new();
        self.chains.retain(|chain, id| {
            let keep = !chain.contains(&font);
            if !keep {
                dead.push(*id);
            }
            keep
        });
        self.chars.retain(|k, v| v.0 != font && !dead.contains(&k.0));
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.chains.clear();
        self.chars.clear();
        self.bytes_used = 0;
    }

    pub fn bytes_used(&self) -> usize {
        self.bytes_used
    }
}

#[cfg(test)]
mod tests {
    use super::is_invisible;

    #[test]
    fn emoji_joiners_and_modifiers_are_invisible() {
        for ch in ['\u{200D}', '\u{FE0F}', '\u{FE0E}', '\u{1F3FD}', '\u{E0067}', '\u{20E3}'] {
            assert!(is_invisible(ch), "{:X}", ch as u32);
        }
        for ch in ['a', '中', '\u{1F44D}', '\u{2764}', ' '] {
            assert!(!is_invisible(ch), "{:X}", ch as u32);
        }
    }
}
