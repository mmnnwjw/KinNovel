//! 拼音输入法引擎 (屏幕键盘用), 思路参照 KOReader 的拼音键盘
//! (`frontend/ui/data/keyboardlayouts/zh_CN_keyboard.lua` + `generic_ime.lua`), 代码是重新写的:
//!
//! - 词库与 KOReader 同源 (rime-pinyin-simp, Apache-2.0, 见 `data/README.md`), 但除了单字还保留了词组,
//!   连续输入 "zhongguo" 首选就是 "中国"; KOReader 只有单字, 每个音节要单独选。
//! - 与 KOReader 一样按前缀匹配: 最后 (或任何) 一个音节没打完也能出字 ("zhongg" → 中国, "zg" → 中国)。
//! - 空格 / 回车 / 标点按 KOReader 的 "separate": 把当前首选上屏 ([`Composer::commit_best`])。
//!
//! 词库约 1.3 MB 文本 (Brotli 压缩后 415 KB 嵌在二进制里), 第一次用时解压并建行索引
//! (设备上几十毫秒, 可以先 [`preload`] 到后台线程)。

use std::collections::HashSet;
use std::sync::OnceLock;

static DATA: &[u8] = include_bytes!("../data/pinyin.br");

/// 单个音节最长的字母数 (zhuang / chuang / shuang)
const MAX_SYLLABLE: usize = 6;
/// 输入串上限 (字母数)
pub const MAX_RAW: usize = 40;
/// 词组候选最多试几个音节
const MAX_PHRASE: usize = 8;

/// 词库: 按键排序的行 `key\tword\tweight`。
pub struct Dict {
    text: String,
    /// 每行起始偏移
    lines: Vec<u32>,
    /// 完整音节 (单字条目的 key), 排序
    syllables: Vec<String>,
}

/// 全局词库 (第一次调用时解压)。
pub fn dict() -> &'static Dict {
    static DICT: OnceLock<Dict> = OnceLock::new();
    DICT.get_or_init(|| {
        let mut out = Vec::with_capacity(1 << 21);
        let mut input = DATA;
        if let Err(e) = brotli_decompressor::BrotliDecompress(&mut input, &mut out) {
            eprintln!("[ime] 词库解压失败: {e}");
            out.clear();
        }
        Dict::from_text(String::from_utf8(out).unwrap_or_default())
    })
}

/// 在后台线程解压词库, 打开键盘时调用, 第一次打字就不用等。
pub fn preload() {
    let _ = std::thread::Builder::new().name("ime-load".into()).spawn(|| {
        dict();
    });
}

/// 输入串切成的一段: 完整音节, 或某些音节的开头 (没打完 / 简拼)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seg {
    pub text: String,
    pub full: bool,
    /// 这一段在输入串中的结束位置 (字节, 输入串只有 ASCII)
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    /// 选中后消耗输入串的前多少字节
    pub consumed: usize,
}

impl Dict {
    pub fn from_text(text: String) -> Dict {
        let mut lines = Vec::new();
        let mut start = 0usize;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                if i > start {
                    lines.push(start as u32);
                }
                start = i + 1;
            }
        }
        if start < text.len() {
            lines.push(start as u32);
        }
        let mut dict = Dict { text, lines, syllables: Vec::new() };
        let mut syllables: Vec<String> = (0..dict.lines.len()).map(|i| dict.key(i)).filter(|k| !k.contains('\'')).map(str::to_string).collect();
        syllables.dedup();
        dict.syllables = syllables;
        dict
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    fn line(&self, i: usize) -> &str {
        let start = self.lines[i] as usize;
        let end = self.text[start..].find('\n').map_or(self.text.len(), |n| start + n);
        &self.text[start..end]
    }

    fn key(&self, i: usize) -> &str {
        let line = self.line(i);
        line.split('\t').next().unwrap_or("")
    }

    /// (key, word, weight)
    fn entry(&self, i: usize) -> (&str, &str, u32) {
        let mut parts = self.line(i).split('\t');
        let key = parts.next().unwrap_or("");
        let word = parts.next().unwrap_or("");
        let weight = parts.next().and_then(|w| w.parse().ok()).unwrap_or(0);
        (key, word, weight)
    }

    /// 第一个 key >= prefix 的行
    fn lower_bound(&self, prefix: &str) -> usize {
        let (mut lo, mut hi) = (0, self.lines.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.key(mid) < prefix {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    pub fn is_syllable(&self, s: &str) -> bool {
        self.syllables.binary_search_by(|x| x.as_str().cmp(s)).is_ok()
    }

    /// 有音节以 s 开头
    pub fn is_syllable_prefix(&self, s: &str) -> bool {
        let i = self.syllables.partition_point(|x| x.as_str() < s);
        self.syllables.get(i).is_some_and(|x| x.starts_with(s))
    }

    /// 把输入串切成音节: 从左到右取最长的完整音节, 取不到就取最长的音节开头。
    /// `'` 是用户输入的分隔符。不认识的字母自成一段 (`full = false`, 没有候选)。
    pub fn segment(&self, raw: &str) -> Vec<Seg> {
        let bytes = raw.as_bytes();
        let mut segs = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                i += 1;
                continue;
            }
            let run_end = raw[i..].find('\'').map_or(raw.len(), |n| i + n);
            let max = (run_end - i).min(MAX_SYLLABLE);
            let full = (1..=max).rev().find(|&n| self.is_syllable(&raw[i..i + n]));
            let (n, full) = match full {
                Some(n) => (n, true),
                None => ((1..=max).rev().find(|&n| self.is_syllable_prefix(&raw[i..i + n])).unwrap_or(1), false),
            };
            segs.push(Seg { text: raw[i..i + n].to_string(), full, end: i + n });
            i += n;
        }
        segs
    }

    /// 与前 k 段完全对上 (音节数相同, 完整段相等, 不完整段是前缀) 的词, 按权重排序。
    fn lookup(&self, segs: &[Seg], limit: usize) -> Vec<(&str, u32)> {
        // 二分查找用的前缀: 到第一个不完整段为止
        let mut prefix = String::new();
        for (i, s) in segs.iter().enumerate() {
            if i > 0 {
                prefix.push('\'');
            }
            prefix.push_str(&s.text);
            if !s.full {
                break;
            }
        }
        let mut found = Vec::new();
        let mut i = self.lower_bound(&prefix);
        while i < self.lines.len() {
            let (key, word, weight) = self.entry(i);
            if !key.starts_with(&prefix) {
                break;
            }
            if key_matches(key, segs) {
                found.push((word, weight));
            }
            i += 1;
        }
        found.sort_by(|a, b| b.1.cmp(&a.1));
        found.truncate(limit);
        found
    }

    /// 候选: 先是覆盖全部音节的词, 再依次少一个音节, 最后是单字; 末尾附上原样字母 (打英文用)。
    pub fn candidates(&self, raw: &str) -> Vec<Candidate> {
        let segs = self.segment(raw);
        let mut out: Vec<Candidate> = Vec::new();
        let mut seen = HashSet::new();
        let longest = segs.len().min(MAX_PHRASE);
        for k in (1..=longest).rev() {
            let limit = if k == 1 { 300 } else { 20 };
            for (word, _) in self.lookup(&segs[..k], limit) {
                if seen.insert(word.to_string()) {
                    out.push(Candidate { text: word.to_string(), consumed: segs[k - 1].end });
                }
            }
        }
        if out.is_empty() {
            if let Some(first) = segs.first() {
                // 不认识的字母 (i/u/v 开头之类): 原样上屏这一段
                out.push(Candidate { text: first.text.clone(), consumed: first.end });
            }
        }
        let letters: String = raw.chars().filter(|c| *c != '\'').collect();
        if !letters.is_empty() && seen.insert(letters.clone()) && out.iter().all(|c| c.text != letters) {
            out.push(Candidate { text: letters, consumed: raw.len() });
        }
        out
    }
}

fn key_matches(key: &str, segs: &[Seg]) -> bool {
    let mut parts = key.split('\'');
    for s in segs {
        let Some(p) = parts.next() else { return false };
        let ok = if s.full { p == s.text } else { p.starts_with(&s.text) };
        if !ok {
            return false;
        }
    }
    parts.next().is_none()
}

/// 正在输入的拼音串与它的候选。
pub struct Composer {
    dict: &'static Dict,
    raw: String,
    candidates: Vec<Candidate>,
    preedit: String,
}

impl Default for Composer {
    fn default() -> Self {
        Composer::with_dict(dict())
    }
}

impl Composer {
    pub fn with_dict(dict: &'static Dict) -> Self {
        Composer { dict, raw: String::new(), candidates: Vec::new(), preedit: String::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// 显示在输入框里的拼音, 音节之间加 `'` ("zhong'guo")
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// 只接受小写字母和 `'`。返回是否接受。
    pub fn push(&mut self, c: char) -> bool {
        if !(c.is_ascii_lowercase() || (c == '\'' && !self.raw.is_empty() && !self.raw.ends_with('\''))) {
            return false;
        }
        if self.raw.len() >= MAX_RAW {
            return true;
        }
        self.raw.push(c);
        self.update();
        true
    }

    /// 删掉最后一个字母 (KOReader 的逐键删除)。
    pub fn pop(&mut self) {
        self.raw.pop();
        self.update();
    }

    pub fn clear(&mut self) {
        self.raw.clear();
        self.update();
    }

    /// 选中第 i 个候选: 返回上屏的文字, 输入串去掉它消耗的部分。
    pub fn select(&mut self, i: usize) -> Option<String> {
        let c = self.candidates.get(i)?.clone();
        self.raw = self.raw[c.consumed.min(self.raw.len())..].trim_start_matches('\'').to_string();
        self.update();
        Some(c.text)
    }

    /// 依次选首选直到输入串用完 (空格、回车、标点、切换语言时)。
    pub fn commit_best(&mut self) -> String {
        let mut out = String::new();
        while !self.raw.is_empty() {
            match self.select(0) {
                Some(t) => out.push_str(&t),
                None => {
                    out.push_str(&self.raw.replace('\'', ""));
                    self.raw.clear();
                    self.update();
                }
            }
        }
        out
    }

    fn update(&mut self) {
        if self.raw.is_empty() {
            self.candidates.clear();
            self.preedit.clear();
            return;
        }
        self.candidates = self.dict.candidates(&self.raw);
        let segs = self.dict.segment(&self.raw);
        self.preedit = segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("'");
        if self.raw.ends_with('\'') {
            self.preedit.push('\'');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> &'static Dict {
        static D: OnceLock<Dict> = OnceLock::new();
        D.get_or_init(|| {
            let mut lines = vec![
                "zhong\t中\t100",
                "zhong\t种\t50",
                "zhong'guo\t中国\t90",
                "zhong'guo'ren\t中国人\t30",
                "guo\t国\t80",
                "guo\t过\t95",
                "gao\t高\t60",
                "ren\t人\t70",
                "xian\t先\t40",
                "xi\t西\t30",
                "an\t安\t20",
                "xi'an\t西安\t25",
            ];
            lines.sort_by(|a, b| a.split('\t').next().unwrap().cmp(b.split('\t').next().unwrap()));
            Dict::from_text(lines.join("\n") + "\n")
        })
    }

    fn texts(c: &[Candidate]) -> Vec<&str> {
        c.iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn segments_longest_syllable_then_prefix() {
        let d = tiny();
        let s = d.segment("zhongguo");
        assert_eq!(s.iter().map(|s| (s.text.as_str(), s.full)).collect::<Vec<_>>(), vec![("zhong", true), ("guo", true)]);
        let s = d.segment("zhongg");
        assert_eq!(s.iter().map(|s| (s.text.as_str(), s.full)).collect::<Vec<_>>(), vec![("zhong", true), ("g", false)]);
        let s = d.segment("xi'an");
        assert_eq!(s.iter().map(|s| (s.text.as_str(), s.end)).collect::<Vec<_>>(), vec![("xi", 2), ("an", 5)]);
        // 不认识的字母自成一段
        assert_eq!(d.segment("v")[0], Seg { text: "v".into(), full: false, end: 1 });
    }

    #[test]
    fn phrases_first_then_shorter_then_letters() {
        let d = tiny();
        let c = d.candidates("zhongguo");
        assert_eq!(texts(&c), vec!["中国", "中", "种", "zhongguo"]);
        assert_eq!(c[1].consumed, 5);
        // 最后一个音节没打完, 按前缀匹配, 不同音节之间按权重排
        assert_eq!(texts(&d.candidates("g")), vec!["过", "国", "高", "g"]);
        assert_eq!(texts(&d.candidates("zg")), vec!["中国", "中", "种", "zg"]);
        assert_eq!(texts(&d.candidates("xian")), vec!["先", "xian"]);
        assert_eq!(texts(&d.candidates("xi'an")), vec!["西安", "西", "xian"]);
        assert_eq!(texts(&d.candidates("v")), vec!["v"]);
    }

    #[test]
    fn composer_selects_in_steps() {
        let mut c = Composer::with_dict(tiny());
        for ch in "zhongguoren".chars() {
            assert!(c.push(ch));
        }
        assert_eq!(c.preedit(), "zhong'guo'ren");
        assert_eq!(c.candidates()[0].text, "中国人");
        // 选单字只消耗第一个音节, 剩下的继续出候选
        let i = c.candidates().iter().position(|x| x.text == "种").unwrap();
        assert_eq!(c.select(i).as_deref(), Some("种"));
        assert_eq!(c.raw(), "guoren");
        assert_eq!(c.commit_best(), "过人");
        assert!(c.is_empty());
        assert!(c.candidates().is_empty());
        // 逐键删除
        c.push('g');
        c.push('u');
        c.pop();
        assert_eq!(c.raw(), "g");
        assert!(!c.push('A'));
        assert!(!c.push('1'));
    }

    #[test]
    fn embedded_dictionary_loads() {
        let d = dict();
        assert!(d.len() > 60_000, "{}", d.len());
        assert!(d.is_syllable("zhuang") && d.is_syllable("lv") && !d.is_syllable("zh"));
        let c = d.candidates("zhongguo");
        assert_eq!(c[0].text, "中国");
        let c = d.candidates("jingling");
        assert!(texts(&c).contains(&"精灵"), "{:?}", &texts(&c)[..5]);
        let c = d.candidates("mofashaonv");
        assert!(texts(&c)[..3].iter().any(|t| t.starts_with("魔法")), "{:?}", &texts(&c)[..5]);
    }
}
