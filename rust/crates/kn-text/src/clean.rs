//! Python 字符串语义的移植: str.isspace() / str.strip() / _clean_text。

/// Python `str.isspace()`: Unicode 空白类目 (Zs/Zl/Zp) 加上几个双向属性为
/// WS/B/S 的控制字符 (\x1c-\x1f 等), Rust 的 `char::is_whitespace` 不含后者。
pub fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python `str.strip()`: 两端剥除满足 `py_isspace` 的字符。
pub fn py_strip(s: &str) -> &str {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut start = 0;
    while start < chars.len() && py_isspace(chars[start].1) {
        start += 1;
    }
    if start == chars.len() {
        return "";
    }
    let mut end = chars.len();
    while end > start && py_isspace(chars[end - 1].1) {
        end -= 1;
    }
    let byte_start = chars[start].0;
    let byte_end = if end == chars.len() {
        s.len()
    } else {
        chars[end].0
    };
    &s[byte_start..byte_end]
}

/// 对应 reader.py 的 `_clean_text`: 剥除零宽/格式字符, 把连续的
/// " \t\r\f\v" (不含 \n) 折叠成一个空格, 再 strip。
pub fn clean_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_fold_space = false;
    for c in text.chars() {
        match c {
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{ad}' | '\u{2060}' => continue,
            ' ' | '\t' | '\r' | '\x0c' | '\x0b' => {
                if last_was_fold_space {
                    continue;
                }
                out.push(' ');
                last_was_fold_space = true;
            }
            _ => {
                out.push(c);
                last_was_fold_space = false;
            }
        }
    }
    py_strip(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_record_separator() {
        assert!(py_isspace('\u{1c}'));
        assert!(!char::is_whitespace('\u{1c}'));
    }

    #[test]
    fn collapses_ascii_whitespace_but_keeps_newline_and_nbsp() {
        assert_eq!(clean_text("a\t\t b\r\x0cc"), "a b c");
        assert_eq!(clean_text("a\nb"), "a\nb");
        assert_eq!(clean_text("a\u{a0}b"), "a\u{a0}b");
    }

    #[test]
    fn strips_invisible_chars() {
        assert_eq!(clean_text("a\u{200b}b\u{feff}\u{ad}c"), "abc");
    }

    #[test]
    fn strips_edges_only() {
        assert_eq!(clean_text("  hello world  "), "hello world");
        assert_eq!(clean_text("\u{3000}hi\u{3000}"), "hi");
    }
}
