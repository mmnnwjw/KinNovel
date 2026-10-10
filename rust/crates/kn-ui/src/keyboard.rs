//! 屏幕键盘 + 单行输入框 (搜索用)。布局与行为参照 KOReader 的 `VirtualKeyboard`
//! (`en_keyboard.lua` 的五行 QWERTY、`zh_CN_keyboard.lua` 的中文标点与拼音输入), 代码重新写:
//!
//! - 五行: 数字 / QWERTY 三行 (第三行末尾是中文逗号, 第四行两侧是 Shift 与退格) /
//!   符号 · 中英 · 句号 · 空格 · ← → · 回车。符号层两页 (中文标点 / ASCII), Shift 换页。
//! - 中文模式打字母进入拼音输入 ([`kn_ime::Composer`]); 候选在键盘上方一行, 点选上屏
//!   (KOReader 把候选写在输入框里用 ← → 轮换, 墨水屏上直接点更快)。← → 在有候选时翻候选页, 否则移动光标。
//! - KOReader 的 "separate": 空格、回车、标点、数字、切换中英时先把当前首选上屏。
//! - 在 ， 。 键上向上滑输入 ； ： (KOReader 的 north 手势); 长按退格清空, 长按 Shift 锁定大写。
//!
//! 刷新: 输入框与候选行紧贴键盘上方, 打字时变化的区域 (输入框 → 按下的键) 不超过半屏,
//! 调度器按普通局部刷新处理, 不会每次都闪。
//!
//! 使用: 页面在 `render` 里 `editor.render(...)` (底部 [`TextEditor::height`] 高的矩形),
//! 在 `on_input` 里把落在 [`TextEditor::owns`] 范围内的命中交给 [`TextEditor::on_gesture`]。

use kn_ime::Composer;
use kn_platform::GestureKind;
use kn_render::{Bitmap, Rect};

use crate::hits::{HitId, Hits};
use crate::theme::Theme;
use crate::widgets::{Ink, Metrics};

/// 输入框最多多少个字
const MAX_CHARS: usize = 60;

const KEY_SLOTS: u32 = 100;
const ID_FIELD: u32 = 100;
const ID_CLEAR: u32 = 101;
const ID_CAND_PREV: u32 = 102;
const ID_CAND_NEXT: u32 = 103;
const ID_CAND_BASE: u32 = 110;
const ID_SPAN: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    /// 直接输入的字符, 可带上滑输入的另一个字符
    Char(&'static str, Option<&'static str>),
    Letter(char),
    Shift,
    Backspace,
    Symbols,
    Lang,
    Space,
    Left,
    Right,
    Enter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shift {
    Off,
    Once,
    Lock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    None,
    /// 内容或键盘状态变了, 需要重绘
    Edited,
    /// 按了回车 (搜索)
    Submit,
}

pub struct TextEditor {
    text: Vec<char>,
    cursor: usize,
    composer: Option<Composer>,
    chinese: bool,
    symbols: bool,
    shift: Shift,
    /// 符号层的页 (0 = 中文标点, 1 = ASCII)
    sym_page: u8,
    /// 候选翻页: 当前页第一个候选的下标, 及之前各页的起点 (往回翻用)
    cand_start: usize,
    cand_back: Vec<usize>,
    /// 上次绘制时这一页放下的候选数 (翻下一页用)
    cand_shown: usize,
    base: u32,
}

const DIGITS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
const SYM_ZH: [[&str; 10]; 2] = [["！", "？", "、", "：", "；", "“", "”", "‘", "’", "…"], ["（", "）", "《", "》", "【", "】", "「", "」", "—", "·"]];
const SYM_ASCII: [[&str; 10]; 2] = [["!", "?", "@", "#", "$", "%", "^", "&", "*", "~"], ["(", ")", "[", "]", "{", "}", "<", ">", "_", "|"]];
const SYM_ROW4: [[&str; 7]; 2] = [["-", "+", "=", "/", "\"", "'", ":"], ["\\", "`", ",", ".", ";", "'", "\""]];

impl TextEditor {
    /// `base`: 命中 id 的起点, 占用 base..base+200。
    pub fn new(base: u32, text: &str) -> Self {
        kn_ime::preload();
        let text: Vec<char> = text.chars().take(MAX_CHARS).collect();
        TextEditor {
            cursor: text.len(),
            text,
            composer: None,
            chinese: true,
            symbols: false,
            shift: Shift::Off,
            sym_page: 0,
            cand_start: 0,
            cand_back: Vec::new(),
            cand_shown: 0,
            base,
        }
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.chars().take(MAX_CHARS).collect();
        self.cursor = self.text.len();
        self.composer_mut().clear();
        self.reset_candidates();
    }

    pub fn is_composing(&self) -> bool {
        self.composer.as_ref().is_some_and(|c| !c.is_empty())
    }

    /// 把正在输入的拼音按首选上屏 (收起键盘前调用, KOReader onCloseKeyboard → separate)。
    pub fn finish(&mut self) {
        if self.is_composing() {
            let s = self.composer_mut().commit_best();
            self.insert(&s);
        }
    }

    pub fn owns(&self, hit: HitId) -> bool {
        (self.base..self.base + ID_SPAN).contains(&hit.0)
    }

    /// 输入框 + 候选行 + 五行键盘的总高度。
    pub fn height(m: &Metrics) -> u32 {
        // 上下各一个间距 + 输入框/候选行/五行键之间的间距
        field_h(m) + cand_h(m) + 5 * key_h(m) + 8 * gap(m)
    }

    fn composer_mut(&mut self) -> &mut Composer {
        self.composer.get_or_insert_with(Composer::default)
    }

    fn reset_candidates(&mut self) {
        self.cand_start = 0;
        self.cand_back.clear();
        self.cand_shown = 0;
    }

    fn rows(&self) -> Vec<Vec<(Key, f32)>> {
        let one = |k: Key| (k, 1.0);
        let mut rows = vec![DIGITS.iter().map(|d| one(Key::Char(d, None))).collect::<Vec<_>>()];
        let (comma, period) = if self.chinese { (Key::Char("，", Some("；")), Key::Char("。", Some("："))) } else { (Key::Char(",", Some(";")), Key::Char(".", Some(":"))) };
        if self.symbols {
            let set = if self.sym_page == 0 { &SYM_ZH } else { &SYM_ASCII };
            rows.push(set[0].iter().map(|s| one(Key::Char(s, None))).collect());
            rows.push(set[1].iter().map(|s| one(Key::Char(s, None))).collect());
            let mut r = vec![(Key::Shift, 1.5)];
            r.extend(SYM_ROW4[self.sym_page as usize].iter().map(|s| one(Key::Char(s, None))));
            r.push((Key::Backspace, 1.5));
            rows.push(r);
        } else {
            rows.push("qwertyuiop".chars().map(|c| one(Key::Letter(c))).collect());
            let mut r: Vec<_> = "asdfghjkl".chars().map(|c| one(Key::Letter(c))).collect();
            r.push(one(comma));
            rows.push(r);
            let mut r = vec![(Key::Shift, 1.5)];
            r.extend("zxcvbnm".chars().map(|c| one(Key::Letter(c))));
            r.push((Key::Backspace, 1.5));
            rows.push(r);
        }
        rows.push(vec![(Key::Symbols, 1.5), one(Key::Lang), one(period), (Key::Space, 3.0), one(Key::Left), one(Key::Right), (Key::Enter, 1.5)]);
        rows
    }

    fn key_at(&self, slot: u32) -> Option<Key> {
        let rows = self.rows();
        rows.get((slot / 20) as usize)?.get((slot % 20) as usize).map(|k| k.0)
    }

    fn insert(&mut self, s: &str) {
        for ch in s.chars() {
            if self.text.len() >= MAX_CHARS {
                break;
            }
            self.text.insert(self.cursor, ch);
            self.cursor += 1;
        }
    }

    /// 标点 / 数字 / 空格之前: 先把拼音首选上屏 (KOReader separate)
    fn separate(&mut self) {
        if self.is_composing() {
            let s = self.composer_mut().commit_best();
            self.insert(&s);
            self.reset_candidates();
        }
    }

    pub fn on_gesture(&mut self, hit: HitId, kind: GestureKind) -> EditorEvent {
        if !self.owns(hit) {
            return EditorEvent::None;
        }
        let id = hit.0 - self.base;
        let long = kind == GestureKind::Long;
        if kind != GestureKind::Tap && !long && kind != GestureKind::SwipeUp {
            return EditorEvent::None;
        }
        match id {
            ID_FIELD => EditorEvent::None,
            ID_CLEAR => {
                self.composer_mut().clear();
                self.text.clear();
                self.cursor = 0;
                self.reset_candidates();
                EditorEvent::Edited
            }
            ID_CAND_PREV => {
                self.cand_start = self.cand_back.pop().unwrap_or(0);
                EditorEvent::Edited
            }
            ID_CAND_NEXT => {
                self.page_candidates_forward();
                EditorEvent::Edited
            }
            _ if id >= ID_CAND_BASE => {
                let i = self.cand_start + (id - ID_CAND_BASE) as usize;
                if let Some(s) = self.composer_mut().select(i) {
                    self.insert(&s);
                }
                self.reset_candidates();
                EditorEvent::Edited
            }
            _ if id < KEY_SLOTS => match self.key_at(id) {
                Some(key) => self.press(key, kind),
                None => EditorEvent::None,
            },
            _ => EditorEvent::None,
        }
    }

    fn page_candidates_forward(&mut self) {
        let total = self.composer.as_ref().map_or(0, |c| c.candidates().len());
        if self.cand_shown > 0 && self.cand_start + self.cand_shown < total {
            self.cand_back.push(self.cand_start);
            self.cand_start += self.cand_shown;
        }
    }

    fn press(&mut self, key: Key, kind: GestureKind) -> EditorEvent {
        let long = kind == GestureKind::Long;
        if kind == GestureKind::SwipeUp {
            // 上滑只对带第二字符的键有意义, 其余当作点按
            if let Key::Char(_, Some(alt)) = key {
                self.separate();
                self.insert(alt);
                return EditorEvent::Edited;
            }
        }
        match key {
            Key::Char(s, _) => {
                self.separate();
                self.insert(s);
            }
            Key::Letter(c) => {
                let upper = self.shift != Shift::Off;
                if self.chinese && !upper {
                    if self.composer_mut().push(c) {
                        self.reset_candidates();
                    }
                } else {
                    self.separate();
                    let ch = if upper { c.to_ascii_uppercase() } else { c };
                    self.insert(&ch.to_string());
                }
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
            }
            Key::Shift => {
                if self.symbols {
                    self.sym_page ^= 1;
                } else {
                    self.shift = match (self.shift, long) {
                        (_, true) => Shift::Lock,
                        (Shift::Off, false) => Shift::Once,
                        _ => Shift::Off,
                    };
                }
            }
            Key::Backspace => {
                if long {
                    self.composer_mut().clear();
                    self.text.clear();
                    self.cursor = 0;
                } else if self.is_composing() {
                    self.composer_mut().pop();
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                    self.text.remove(self.cursor);
                }
                self.reset_candidates();
            }
            Key::Symbols => {
                self.symbols = !self.symbols;
                self.sym_page = if self.chinese { 0 } else { 1 };
            }
            Key::Lang => {
                self.separate();
                self.chinese = !self.chinese;
                if self.symbols {
                    self.sym_page = if self.chinese { 0 } else { 1 };
                }
            }
            Key::Space => {
                if self.is_composing() {
                    self.separate();
                } else {
                    self.insert(" ");
                }
            }
            Key::Left => {
                if self.is_composing() {
                    self.cand_start = self.cand_back.pop().unwrap_or(0);
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                }
            }
            Key::Right => {
                if self.is_composing() {
                    self.page_candidates_forward();
                } else {
                    self.cursor = (self.cursor + 1).min(self.text.len());
                }
            }
            Key::Enter => {
                self.separate();
                return EditorEvent::Submit;
            }
        }
        EditorEvent::Edited
    }

    /// 在 `rect` (宽 = 屏宽, 高 = [`TextEditor::height`]) 里画输入框、候选行和键盘, 登记命中区域。
    /// `enter_label`: 回车键上的字 ("搜索")。
    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, rect: Rect, placeholder: &str, enter_label: &str) {
        let g = gap(m) as i32;
        let side = m.margin as i32 / 2;
        frame.fill_rect(rect, theme.light);
        let mut y = rect.y + g;

        // 输入框
        let field = Rect::new(rect.x + side, y, (rect.w as i32 - 2 * side).max(0) as u32, field_h(m));
        self.draw_field(ink, frame, hits, theme, m, field, placeholder);
        y = field.bottom() + g;

        // 候选行
        let cand = Rect::new(rect.x, y, rect.w, cand_h(m));
        self.draw_candidates(ink, frame, hits, theme, m, cand);
        y = cand.bottom() + g;

        // 键盘
        let kh = key_h(m) as i32;
        let rows = self.rows();
        let unit_w = (rect.w as i32 - 2 * side + g) as f32 / 10.0;
        for (r, row) in rows.iter().enumerate() {
            let mut x = (rect.x + side) as f32;
            for (c, (key, width)) in row.iter().enumerate() {
                let w = (unit_w * width).round() as i32 - g;
                let kr = Rect::new(x.round() as i32, y, w.max(1) as u32, kh as u32);
                self.draw_key(ink, frame, theme, m, kr, *key, enter_label);
                hits.add(HitId(self.base + r as u32 * 20 + c as u32), kr);
                x += unit_w * width;
            }
            y += kh + g;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_field(&self, ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, field: Rect, placeholder: &str) {
        frame.rounded_rect(field, m.radius, Some(theme.background), Some(theme.foreground), 2);
        hits.add(HitId(self.base + ID_FIELD), field).no_feedback();
        let pad = m.margin as i32 / 2;
        let size = m.body;
        let empty = self.text.is_empty() && !self.is_composing();
        let clear_w = if empty { 0 } else { field.h as i32 };
        let avail = (field.w as i32 - 2 * pad - clear_w).max(0) as f32;
        let line_h = (size * 1.25) as i32;
        let top = field.y + (field.h as i32 - line_h) / 2;
        if empty {
            frame.fill_rect(Rect::new(field.x + pad, top, 3, line_h as u32), theme.foreground);
            ink.text(frame, field.x + pad + 10, top, placeholder, size, theme.muted);
            return;
        }
        let preedit = self.composer.as_ref().map_or("", |c| c.preedit()).to_string();
        let after: String = self.text[self.cursor..].iter().collect();
        // 放不下时从光标前面开始省略 (光标总在可见范围里)
        let mut skip = 0;
        let fixed = ink.width(&preedit, size) + ink.width("…", size);
        while skip < self.cursor {
            let before: String = self.text[skip..self.cursor].iter().collect();
            let after_w = ink.width(&after, size).min(avail / 3.0);
            if ink.width(&before, size) + fixed + after_w <= avail {
                break;
            }
            skip += 1;
        }
        let mut x = (field.x + pad) as f32;
        if skip > 0 {
            x += ink.text(frame, x as i32, top, "…", size, theme.muted);
        }
        let before: String = self.text[skip..self.cursor].iter().collect();
        x += ink.text(frame, x as i32, top, &before, size, theme.foreground);
        if !preedit.is_empty() {
            let w = ink.text(frame, x as i32, top, &preedit, size, theme.foreground);
            frame.fill_rect(Rect::new(x as i32, top + line_h - 2, w.ceil() as u32, 2), theme.foreground);
            x += w;
        }
        frame.fill_rect(Rect::new(x as i32 + 1, top, 3, line_h as u32), theme.foreground);
        let limit = field.right() - pad - clear_w;
        let rest = ink.fit(&after, size, (limit as f32 - x - 6.0).max(0.0));
        ink.text(frame, x as i32 + 6, top, &rest, size, theme.foreground);
        // 清空按钮 (×)
        let cr = Rect::new(field.right() - clear_w, field.y, clear_w as u32, field.h);
        let (cx, cy) = ((cr.x + cr.w as i32 / 2) as f32, (cr.y + cr.h as i32 / 2) as f32);
        let s = field.h as f32 * 0.17;
        let lw = (3.0 * m.scale).max(2.0);
        frame.line(cx - s, cy - s, cx + s, cy + s, lw, theme.muted);
        frame.line(cx - s, cy + s, cx + s, cy - s, lw, theme.muted);
        hits.add(HitId(self.base + ID_CLEAR), cr);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_candidates(&mut self, ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, area: Rect) {
        self.cand_shown = 0;
        let Some(composer) = self.composer.as_ref().filter(|c| !c.is_empty()) else {
            return;
        };
        let cands = composer.candidates();
        let size = (m.body * 1.1).round();
        let arrow_w = (m.touch as f32 * 0.8) as i32;
        let pad = (m.margin as f32 * 0.45) as i32;
        let min_w = (m.touch as f32 * 0.8) as i32;
        let has_prev = self.cand_start > 0;
        let left = area.x + if has_prev { arrow_w } else { m.margin as i32 / 4 };
        let right_limit = area.right() - arrow_w;
        let mut x = left;
        let mut shown = 0;
        for (i, c) in cands.iter().enumerate().skip(self.cand_start) {
            let w = (ink.width(&c.text, size).ceil() as i32 + 2 * pad).max(min_w);
            // 最后一个候选可以占用 "下一页" 的位置
            let is_last = i + 1 == cands.len();
            let limit = if is_last { area.right() } else { right_limit };
            if x + w > limit && shown > 0 {
                break;
            }
            let w = w.min(limit - x);
            let r = Rect::new(x, area.y, w.max(1) as u32, area.h);
            let text = ink.fit(&c.text, size, (w - pad) as f32);
            ink.text_centered(frame, r, &text, size, theme.foreground);
            hits.add(HitId(self.base + ID_CAND_BASE + shown as u32), r);
            x += w;
            shown += 1;
            if shown as u32 >= ID_SPAN - ID_CAND_BASE {
                break;
            }
        }
        self.cand_shown = shown;
        let lw = (3.0 * m.scale).max(2.0);
        let cy = (area.y + area.h as i32 / 2) as f32;
        let s = area.h as f32 * 0.16;
        if has_prev {
            let r = Rect::new(area.x, area.y, arrow_w as u32, area.h);
            let cx = (r.x + r.w as i32 / 2) as f32;
            frame.line(cx + s * 0.5, cy - s, cx - s * 0.5, cy, lw, theme.foreground);
            frame.line(cx - s * 0.5, cy, cx + s * 0.5, cy + s, lw, theme.foreground);
            hits.add(HitId(self.base + ID_CAND_PREV), r);
        }
        if self.cand_start + shown < cands.len() {
            let r = Rect::new(area.right() - arrow_w, area.y, arrow_w as u32, area.h);
            let cx = (r.x + r.w as i32 / 2) as f32;
            frame.line(cx - s * 0.5, cy - s, cx + s * 0.5, cy, lw, theme.foreground);
            frame.line(cx + s * 0.5, cy, cx - s * 0.5, cy + s, lw, theme.foreground);
            hits.add(HitId(self.base + ID_CAND_NEXT), r);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_key(&self, ink: &mut Ink, frame: &mut Bitmap, theme: &Theme, m: &Metrics, r: Rect, key: Key, enter_label: &str) {
        let active = match key {
            Key::Shift => !self.symbols && self.shift != Shift::Off,
            _ => false,
        };
        let (fill, fg) = match key {
            Key::Enter => (theme.foreground, theme.background),
            _ if active => (theme.foreground, theme.background),
            _ => (theme.background, theme.foreground),
        };
        frame.rounded_rect(r, m.radius, Some(fill), None, 0);
        let lw = (3.0 * m.scale).max(2.0);
        let (cx, cy) = ((r.x + r.w as i32 / 2) as f32, (r.y + r.h as i32 / 2) as f32);
        let s = r.h as f32 * 0.2;
        let upper = self.shift != Shift::Off;
        match key {
            Key::Char(label, alt) => {
                ink.text_centered(frame, r, label, m.body, fg);
                if let Some(alt) = alt {
                    // 右上角小字: 上滑输入的字符 (KOReader alt_label)
                    let w = ink.width(alt, m.tiny);
                    ink.text(frame, r.right() - w as i32 - 4, r.y + 2, alt, m.tiny, theme.muted);
                }
            }
            Key::Letter(c) => {
                let ch = if upper { c.to_ascii_uppercase() } else { c };
                ink.text_centered(frame, r, &ch.to_string(), m.body, fg);
            }
            Key::Shift if self.symbols => {
                ink.text_centered(frame, r, if self.sym_page == 0 { "1/2" } else { "2/2" }, m.small, fg);
            }
            Key::Shift => {
                // 向上的空心箭头; 锁定时底下加一横
                let (t, b) = (cy - s * 1.1, cy + s * 0.9);
                let pts = [(cx, t), (cx + s, cy), (cx + s * 0.45, cy), (cx + s * 0.45, b), (cx - s * 0.45, b), (cx - s * 0.45, cy), (cx - s, cy), (cx, t)];
                for w in pts.windows(2) {
                    frame.line(w[0].0, w[0].1, w[1].0, w[1].1, lw, fg);
                }
                if self.shift == Shift::Lock {
                    frame.line(cx - s * 0.6, b + s * 0.45, cx + s * 0.6, b + s * 0.45, lw, fg);
                }
            }
            Key::Backspace => {
                // 左尖的标签形外框 + ×
                let (l, rr, t, b) = (cx - s * 1.4, cx + s * 1.2, cy - s * 0.8, cy + s * 0.8);
                let pts = [(l, cy), (l + s * 0.7, t), (rr, t), (rr, b), (l + s * 0.7, b), (l, cy)];
                for w in pts.windows(2) {
                    frame.line(w[0].0, w[0].1, w[1].0, w[1].1, lw, fg);
                }
                let (xc, d) = (cx + s * 0.25, s * 0.38);
                frame.line(xc - d, cy - d, xc + d, cy + d, lw, fg);
                frame.line(xc - d, cy + d, xc + d, cy - d, lw, fg);
            }
            Key::Symbols => ink.text_centered(frame, r, if self.symbols { "ABC" } else { "符号" }, m.small, fg),
            Key::Lang => {
                // 当前语言黑字, 另一个灰字: "中/英"
                let label = if self.chinese { "中" } else { "英" };
                ink.text_centered(frame, r, label, m.body, fg);
            }
            Key::Space => ink.text_centered(frame, r, "空格", m.small, theme.muted),
            Key::Left | Key::Right => {
                let dir = if key == Key::Left { -1.0 } else { 1.0 };
                frame.line(cx - dir * s, cy, cx + dir * s, cy, lw, fg);
                frame.line(cx + dir * s, cy, cx + dir * s * 0.3, cy - s * 0.7, lw, fg);
                frame.line(cx + dir * s, cy, cx + dir * s * 0.3, cy + s * 0.7, lw, fg);
            }
            Key::Enter => ink.text_centered(frame, r, enter_label, m.small, fg),
        }
    }
}

fn gap(m: &Metrics) -> u32 {
    ((m.margin as f32) / 6.0).round().max(3.0) as u32
}

fn key_h(m: &Metrics) -> u32 {
    (m.touch as f32 * 0.92) as u32
}

fn cand_h(m: &Metrics) -> u32 {
    (m.touch as f32 * 0.9) as u32
}

fn field_h(m: &Metrics) -> u32 {
    m.touch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_of(e: &TextEditor, want: Key) -> HitId {
        for (r, row) in e.rows().iter().enumerate() {
            if let Some(c) = row.iter().position(|k| k.0 == want) {
                return HitId(e.base + r as u32 * 20 + c as u32);
            }
        }
        panic!("no key {want:?}");
    }

    fn tap(e: &mut TextEditor, key: Key) -> EditorEvent {
        let id = slot_of(e, key);
        e.on_gesture(id, GestureKind::Tap)
    }

    fn type_str(e: &mut TextEditor, s: &str) {
        for c in s.chars() {
            tap(e, Key::Letter(c));
        }
    }

    #[test]
    fn pinyin_space_commits_best_and_enter_submits() {
        let mut e = TextEditor::new(5000, "");
        type_str(&mut e, "dongman");
        assert!(e.is_composing());
        assert_eq!(e.text(), "");
        tap(&mut e, Key::Space);
        assert_eq!(e.text(), "动漫");
        // 没有拼音时空格就是空格
        tap(&mut e, Key::Space);
        type_str(&mut e, "zg");
        assert_eq!(tap(&mut e, Key::Enter), EditorEvent::Submit);
        assert!(!e.is_composing());
        assert!(e.text().starts_with("动漫 "), "{}", e.text());
    }

    #[test]
    fn candidate_tap_backspace_and_english() {
        let mut e = TextEditor::new(0, "");
        type_str(&mut e, "jingling");
        // 第一个候选 = 词组
        e.on_gesture(HitId(ID_CAND_BASE), GestureKind::Tap);
        assert_eq!(e.text(), "精灵");
        type_str(&mut e, "ab");
        tap(&mut e, Key::Backspace);
        assert_eq!(e.composer.as_ref().unwrap().raw(), "a");
        tap(&mut e, Key::Backspace);
        tap(&mut e, Key::Backspace);
        assert_eq!(e.text(), "精");
        // 英文模式直接输入, Shift 只管一个字母
        tap(&mut e, Key::Lang);
        tap(&mut e, Key::Shift);
        type_str(&mut e, "re");
        assert_eq!(e.text(), "精Re");
        // 标点上滑
        let comma = slot_of(&e, Key::Char(",", Some(";")));
        e.on_gesture(comma, GestureKind::SwipeUp);
        assert_eq!(e.text(), "精Re;");
        // 光标左移后插入
        tap(&mut e, Key::Left);
        tap(&mut e, Key::Char("1", None));
        assert_eq!(e.text(), "精Re1;");
        let back = slot_of(&e, Key::Backspace);
        e.on_gesture(back, GestureKind::Long);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn punctuation_separates_and_symbol_pages() {
        let mut e = TextEditor::new(0, "");
        type_str(&mut e, "wo");
        tap(&mut e, Key::Char("，", Some("；")));
        assert_eq!(e.text(), "我，");
        tap(&mut e, Key::Symbols);
        tap(&mut e, Key::Char("《", None));
        tap(&mut e, Key::Shift);
        tap(&mut e, Key::Char("@", None));
        assert_eq!(e.text(), "我，《@");
    }
}
