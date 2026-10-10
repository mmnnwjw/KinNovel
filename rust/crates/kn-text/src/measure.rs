//! 基于 kn-render FontStore 的真实 `Measure` 实现。

use kn_render::{FontId, FontStore, FxMap};

use crate::wrap::Measure;

/// 字宽缓存: (字号, 字符) → 宽度。由调用方持有, 跨多次排版复用 (同一章节/同一字体链)。
pub type WidthCache = FxMap<(u32, char), f32>;

/// 用字体回退链(`chain`)和 FontStore 计算字宽, 结果写入调用方的 [`WidthCache`],
/// 对应 reader.py `_wrap_line_parts` 里每 (字体,回退字体) 一份的宽度表。
/// 字体链变化时调用方必须换一份新的缓存。
pub struct FontMeasure<'a> {
    fonts: &'a FontStore,
    chain: &'a [FontId],
    cache: &'a mut WidthCache,
}

impl<'a> FontMeasure<'a> {
    pub fn new(fonts: &'a FontStore, chain: &'a [FontId], cache: &'a mut WidthCache) -> Self {
        FontMeasure { fonts, chain, cache }
    }
}

impl Measure for FontMeasure<'_> {
    fn char_width(&mut self, size: u32, ch: char) -> f32 {
        if let Some(&w) = self.cache.get(&(size, ch)) {
            return w;
        }
        let font = self.fonts.pick_font(self.chain, ch);
        let w = self.fonts.advance(font, ch, size as f32);
        self.cache.insert((size, ch), w);
        w
    }
}
