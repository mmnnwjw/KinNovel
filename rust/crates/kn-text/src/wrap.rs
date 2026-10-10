//! 行内分行与标点禁则, 对应 reader.py 的 `_wrap_line_parts`。

use crate::clean::py_isspace;

// 标点禁则:闭标点不允许出现在行首,开标点不允许出现在行尾
const LINE_START_FORBIDDEN: &str = "，。、；：？！,.!?;:'\")]】》”’%…—·";
const LINE_END_FORBIDDEN: &str = "（《【「『“‘([{";

fn is_line_start_forbidden(c: char) -> bool {
    LINE_START_FORBIDDEN.contains(c)
}

fn is_line_end_forbidden(c: char) -> bool {
    LINE_END_FORBIDDEN.contains(c)
}

/// 行内字符测宽, 由调用方(真实字体表或测试里的合成宽度函数)实现。
pub trait Measure {
    fn char_width(&mut self, size: u32, ch: char) -> f32;
}

fn rstrip_py(chars: &[char]) -> String {
    let mut end = chars.len();
    while end > 0 && py_isspace(chars[end - 1]) {
        end -= 1;
    }
    chars[..end].iter().collect()
}

/// 对应 `_wrap_line_parts`: 返回 (该行渲染文本, 该行在 `text` 中的起始字符偏移)。
/// `remove_trailing_spaces` 在 reader.py 中的调用点始终为 true, 这里直接内置。
pub fn wrap(text: &str, size: u32, max_width: f64, m: &mut impl Measure) -> Vec<(String, usize)> {
    let chars: Vec<char> = text.replace('\u{a0}', " ").chars().collect();
    if chars.is_empty() {
        return vec![(String::new(), 0)];
    }

    let mut lines: Vec<(String, usize)> = Vec::new();
    let mut current: Vec<char> = Vec::new();
    let mut current_start: usize = 0;
    let mut current_width: f64 = 0.0;

    let char_width = |m: &mut dyn Measure, c: char| -> f64 { m.char_width(size, c) as f64 };
    let measure = |m: &mut dyn Measure, value: &[char]| -> f64 {
        value.iter().map(|&c| char_width(m, c)).sum()
    };

    let n = chars.len();
    let mut index = 0usize;
    while index < n {
        let c = chars[index];
        if c == '\n' {
            lines.push((rstrip_py(&current), current_start));
            current.clear();
            current_width = 0.0;
            index += 1;
            current_start = index;
            continue;
        }
        if is_line_start_forbidden(c) {
            let mut run_end = index + 1;
            while run_end < n && is_line_start_forbidden(chars[run_end]) {
                run_end += 1;
            }
            let run = &chars[index..run_end];
            let run_width = measure(m, run);
            if !current.is_empty() && current_width + run_width > max_width {
                if run.len() == 1 {
                    // 保留单字符闭标点的悬挂语义
                    current.extend_from_slice(run);
                    lines.push((rstrip_py(&current), current_start));
                    current.clear();
                    current_width = 0.0;
                    index = run_end;
                    continue;
                }
                let cut_full = current.clone();
                let mut carry_start = cut_full.len();
                while carry_start > 0 && is_line_end_forbidden(cut_full[carry_start - 1]) {
                    carry_start -= 1;
                }
                let carry = &cut_full[carry_start..];
                let cut = &cut_full[..carry_start];
                if !cut.is_empty() {
                    lines.push((rstrip_py(cut), current_start));
                }
                current_start += carry_start;
                current = carry.to_vec();
                current.extend_from_slice(run);
                current_width = measure(m, &current);
            } else {
                if current.is_empty() {
                    current_start = index;
                }
                current.extend_from_slice(run);
                current_width += run_width;
            }
            index = run_end;
            continue;
        }
        let w = char_width(m, c);
        if !current.is_empty() && current_width + w > max_width {
            let cut_full = current.clone();
            let mut carry_start = cut_full.len();
            while carry_start > 0 && is_line_end_forbidden(cut_full[carry_start - 1]) {
                carry_start -= 1;
            }
            let carry = &cut_full[carry_start..];
            let cut = &cut_full[..carry_start];
            if !cut.is_empty() {
                lines.push((rstrip_py(cut), current_start));
            }
            current_start += carry_start;
            current = carry.to_vec();
            current.push(c);
            current_width = measure(m, &current);
        } else {
            if current.is_empty() {
                current_start = index;
            }
            current.push(c);
            current_width += w;
        }
        index += 1;
    }
    lines.push((rstrip_py(&current), current_start));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl Measure for Fixed {
        fn char_width(&mut self, size: u32, ch: char) -> f32 {
            if ch == ' ' {
                size as f32 * 0.25
            } else if (ch as u32) < 0x2E80 {
                size as f32 * 0.5
            } else {
                size as f32
            }
        }
    }

    #[test]
    fn empty_text_yields_single_empty_line() {
        let mut m = Fixed;
        assert_eq!(wrap("", 48, 1000.0, &mut m), vec![(String::new(), 0)]);
    }

    #[test]
    fn breaks_on_newline() {
        let mut m = Fixed;
        let out = wrap("ab\ncd", 48, 1000.0, &mut m);
        assert_eq!(out, vec![("ab".to_string(), 0), ("cd".to_string(), 3)]);
    }

    #[test]
    fn closing_punct_not_at_line_start_single_char_hangs() {
        let mut m = Fixed;
        // 宽度只够放 "。" 之前的部分, 单字符闭标点悬挂到当前行尾。
        let out = wrap("甲。", 48, 48.0, &mut m);
        assert_eq!(out[0].0, "甲。");
    }

    #[test]
    fn opening_punct_not_at_line_end_carries_forward() {
        let mut m = Fixed;
        // "（" 在行尾禁则内, 应被带到下一行而不是留在上一行结尾。
        let max_width = 48.0 * 3.0; // 容纳 3 个全角字符
        let out = wrap("甲乙（丙", 48, max_width, &mut m);
        assert!(!out[0].0.ends_with('（'));
    }

    #[test]
    fn nbsp_is_treated_as_space() {
        let mut m = Fixed;
        let out = wrap("a\u{a0}b", 48, 1000.0, &mut m);
        assert_eq!(out[0].0, "a b");
    }
}
