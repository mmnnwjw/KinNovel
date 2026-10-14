//! 行内分行与标点禁则, 对应 reader.py 的 `_wrap_line_parts`。

use crate::clean::py_isspace;

// 标点禁则:闭标点不允许出现在行首,开标点不允许出现在行尾
const LINE_START_FORBIDDEN: &str = "，。、；：？！,.!?;:'\")]】》”’%…—·";
const LINE_END_FORBIDDEN: &str = "（《【「『“‘([{";
/// 严格禁则额外的行首禁止字符: Python 版漏掉的中日文闭括号和波浪线。
const LINE_START_FORBIDDEN_STRICT: &str = "」』）〉〕］｝〗〙～";

fn is_line_start_forbidden(c: char, strict: bool) -> bool {
    LINE_START_FORBIDDEN.contains(c) || (strict && LINE_START_FORBIDDEN_STRICT.contains(c))
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
///
/// `strict` = false 与 Python 逐字一致 (金标准测试)。true 时:
/// - 」』）等闭括号也不放行首 (Python 漏了, 「…了。」换行后 」 会独占行首);
/// - 两个以上闭标点连在一起 (。」、……) 放不下时, 把行尾最后一个字一起带到下一行 (追い出し),
///   不让整串标点出现在下一行行首; 带不走 (整行只有它) 时整串悬挂在行尾。
pub fn wrap(text: &str, size: u32, max_width: f64, strict: bool, m: &mut impl Measure) -> Vec<(String, usize)> {
    let chars: Vec<char> = text.replace('\u{a0}', " ").chars().collect();
    let widths: Vec<f64> = chars.iter().map(|&c| m.char_width(size, c) as f64).collect();
    wrap_widths(&chars, &widths, max_width, 0.0, strict)
}

/// [`wrap`] 的通用形式: 每个字符的宽度由调用方给出 (同一行可混排不同字号; 宽度 0 的字符不占位),
/// 第一行另有 `first_indent` 像素的缩进。`chars` 里的不换行空格应已换成普通空格。
pub fn wrap_widths(chars: &[char], widths: &[f64], max_width: f64, first_indent: f64, strict: bool) -> Vec<(String, usize)> {
    debug_assert_eq!(chars.len(), widths.len());
    if chars.is_empty() {
        return vec![(String::new(), 0)];
    }

    let mut lines: Vec<(String, usize)> = Vec::new();
    let mut current: Vec<char> = Vec::new();
    let mut current_start: usize = 0;
    let mut current_width: f64 = first_indent;

    // 与 Python 版一致: 带走若干字之后从头重新累加这一行的宽度 (求和顺序不变, 结果逐位相同)
    let measure = |from: usize, to: usize| -> f64 { widths[from..to].iter().sum() };

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
        if is_line_start_forbidden(c, strict) {
            let mut run_end = index + 1;
            while run_end < n && is_line_start_forbidden(chars[run_end], strict) {
                run_end += 1;
            }
            let run = &chars[index..run_end];
            let run_width = measure(index, run_end);
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
                // 严格模式: 行尾最后一个字随标点串一起换行
                let mut carry_start = if strict { cut_full.len() - 1 } else { cut_full.len() };
                while carry_start > 0 && is_line_end_forbidden(cut_full[carry_start - 1]) {
                    carry_start -= 1;
                }
                if strict && carry_start == 0 {
                    // 整行都得带走: 退回悬挂
                    current.extend_from_slice(run);
                    lines.push((rstrip_py(&current), current_start));
                    current.clear();
                    current_width = 0.0;
                    index = run_end;
                    continue;
                }
                let carry = &cut_full[carry_start..];
                let cut = &cut_full[..carry_start];
                if !cut.is_empty() {
                    lines.push((rstrip_py(cut), current_start));
                }
                current_start += carry_start;
                current = carry.to_vec();
                current.extend_from_slice(run);
                current_width = measure(current_start, run_end);
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
        let w = widths[index];
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
            current_width = measure(current_start, index + 1);
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
        assert_eq!(wrap("", 48, 1000.0, false, &mut m), vec![(String::new(), 0)]);
    }

    #[test]
    fn breaks_on_newline() {
        let mut m = Fixed;
        let out = wrap("ab\ncd", 48, 1000.0, false, &mut m);
        assert_eq!(out, vec![("ab".to_string(), 0), ("cd".to_string(), 3)]);
    }

    #[test]
    fn closing_punct_not_at_line_start_single_char_hangs() {
        let mut m = Fixed;
        // 宽度只够放 "。" 之前的部分, 单字符闭标点悬挂到当前行尾。
        let out = wrap("甲。", 48, 48.0, false, &mut m);
        assert_eq!(out[0].0, "甲。");
    }

    #[test]
    fn opening_punct_not_at_line_end_carries_forward() {
        let mut m = Fixed;
        // "（" 在行尾禁则内, 应被带到下一行而不是留在上一行结尾。
        let max_width = 48.0 * 3.0; // 容纳 3 个全角字符
        let out = wrap("甲乙（丙", 48, max_width, false, &mut m);
        assert!(!out[0].0.ends_with('（'));
    }

    #[test]
    fn strict_keeps_closing_brackets_off_the_line_start() {
        let mut m = Fixed;
        // 一行 4 个全角字: 「甲乙丙。」 -> Python 规则: 。悬挂, 」独占下一行行首
        let width = 48.0 * 4.0;
        let loose = wrap("「甲乙丙。」丁", 48, width, false, &mut m);
        assert_eq!(loose[1].0.chars().next(), Some('」'));
        // 严格: 丙 随 。」 一起换行
        let strict = wrap("「甲乙丙。」丁", 48, width, true, &mut m);
        assert_eq!(strict.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), vec!["「甲乙", "丙。」丁"]);
        assert_eq!(strict[1].1, 3);
        // 单个 」 也悬挂而不是放到行首
        let single = wrap("甲乙丙丁」戊", 48, width, true, &mut m);
        assert_eq!(single[0].0, "甲乙丙丁」");
    }

    #[test]
    fn strict_hangs_when_nothing_can_be_carried() {
        let mut m = Fixed;
        let out = wrap("甲。」", 48, 48.0, true, &mut m);
        assert_eq!(out[0].0, "甲。」");
    }

    #[test]
    fn nbsp_is_treated_as_space() {
        let mut m = Fixed;
        let out = wrap("a\u{a0}b", 48, 1000.0, false, &mut m);
        assert_eq!(out[0].0, "a b");
    }
}
