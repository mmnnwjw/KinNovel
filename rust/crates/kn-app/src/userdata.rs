//! 用户数据 (Rust 版自己的格式, Python 版不读): 书签。
//!
//! 放在扩展目录的 `data/` 下而不是 `cache/`: 清理缓存不会删掉它们。
//! - 书签 `data/bookmarks/{book}.json` = `[{"sort_num", "chapter", "path", "offset", "excerpt", "font", "time"}, ...]`

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::store::{atomic_write, Paths};

impl Paths {
    pub fn data_dir(&self) -> PathBuf {
        self.app_dir.join("data")
    }

    pub fn bookmarks_file(&self, book_id: i64) -> PathBuf {
        self.data_dir().join("bookmarks").join(format!("{book_id}.json"))
    }
}

/// 一条书签: 章节序号 + 页首锚点 (XPath + 码点偏移, 与阅读进度同一种定位, 改字号后仍然准确)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub sort_num: i64,
    pub chapter: String,
    pub path: String,
    pub offset: usize,
    /// 页首几十个字, 列表里显示。正文是反爬字体的混淆码位, 只有用章节字体画才是原文
    pub excerpt: String,
    /// 章节字体在 `cache/fonts/` 里的文件名 (空 = 没有章节字体)
    pub font: String,
    /// Unix 秒
    pub time: i64,
}

/// 读取一本书的书签 (按章节、位置排序)。文件不存在或损坏时为空。
pub fn load_bookmarks(paths: &Paths, book_id: i64) -> Vec<Bookmark> {
    let Some(Value::Array(items)) = std::fs::read(paths.bookmarks_file(book_id)).ok().and_then(|b| serde_json::from_slice(&b).ok()) else {
        return Vec::new();
    };
    let mut out: Vec<Bookmark> = items
        .iter()
        .filter_map(|v| {
            Some(Bookmark {
                sort_num: v.get("sort_num")?.as_i64()?,
                chapter: v.get("chapter").and_then(Value::as_str).unwrap_or("").to_string(),
                path: v.get("path")?.as_str()?.to_string(),
                offset: v.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
                excerpt: v.get("excerpt").and_then(Value::as_str).unwrap_or("").to_string(),
                font: v.get("font").and_then(Value::as_str).unwrap_or("").to_string(),
                time: v.get("time").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .filter(|b| b.sort_num > 0 && !b.path.is_empty())
        .collect();
    sort_bookmarks(&mut out);
    out
}

/// 按章节, 再按添加时间排序 (同一章里的先后顺序要排版后才知道, 这里不需要精确)。
fn sort_bookmarks(list: &mut [Bookmark]) {
    list.sort_by(|a, b| (a.sort_num, a.time).cmp(&(b.sort_num, b.time)));
}

/// 写回一本书的全部书签 (空列表删除文件)。
pub fn save_bookmarks(paths: &Paths, book_id: i64, list: &[Bookmark]) -> std::io::Result<()> {
    let file = paths.bookmarks_file(book_id);
    if list.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let mut sorted = list.to_vec();
    sort_bookmarks(&mut sorted);
    let items: Vec<Value> = sorted
        .iter()
        .map(|b| json!({"sort_num": b.sort_num, "chapter": b.chapter, "path": b.path, "offset": b.offset, "excerpt": b.excerpt, "font": b.font, "time": b.time}))
        .collect();
    atomic_write(&file, Value::Array(items).to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_round_trip_and_empty_removes_file() {
        let dir = std::env::temp_dir().join(format!("kn-bookmarks-{}", std::process::id()));
        let paths = Paths { app_dir: dir.clone() };
        assert!(load_bookmarks(&paths, 7).is_empty());
        let a = Bookmark { sort_num: 3, chapter: "第三章".into(), path: "./p[2]".into(), offset: 5, excerpt: "开头".into(), font: "f.woff2".into(), time: 20 };
        let b = Bookmark { sort_num: 1, chapter: "第一章".into(), path: "./p[9]".into(), offset: 0, excerpt: String::new(), font: String::new(), time: 30 };
        save_bookmarks(&paths, 7, &[a.clone(), b.clone()]).unwrap();
        assert_eq!(load_bookmarks(&paths, 7), vec![b, a]);
        save_bookmarks(&paths, 7, &[]).unwrap();
        assert!(!paths.bookmarks_file(7).exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
