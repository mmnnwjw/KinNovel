//! 漫画的本地数据: 每话的图片地址清单、书目 (话列表)、阅读进度, 以及页面图片的下载与缓存。
//!
//! 接口 (对照网页版 `services/manga` 与 `pages/Manga/Reader.vue`):
//! - `GetComicContent {Cid, Skip, Take}` 分批返回一话的图片地址 (网页版每批 6 张), 附本话总页数;
//!   `Skip = 0` 时附服务器阅读位置 `{ChapterId, Position}` (Position = 1 起的页码字符串)。
//! - 阅读位置上传沿用 `SaveReadPosition {Bid, Cid, XPath}`, XPath 填 1 起的页码。
//!
//! 缓存布局 (`cache/comic/`, 独立于小说缓存的配额 `comic_cache_mb`):
//! - `chapters/<cid>.json`: 图片地址清单 (已取到的批次; 未取到的位置为 null), 离线时据此读已缓存的页。
//! - `books/<bid>.json`: 书名与话列表 (详情页加载时写入; 阅读页换话、目录用)。
//! - `pages/<sha256(url#h=档位)>.img`: CDN 按高度缩放后的原始字节 (JPEG / WebP, 不重新编码)。
//! - 阅读进度: `cache/progress/comic-<bid>.json` (不参与缓存清理)。
//!
//! 这里的下载函数都是阻塞的, 只在后台线程调用。

use std::sync::Mutex;

use kn_net::Client;
use serde_json::{json, Value};

use crate::api::{self, ChapterRef, ComicBatch};
use crate::store::{self, Paths};

/// 每次向服务器要几张图的地址 (与网页版一致)。
pub const BATCH: usize = 6;
const IMAGE_LIMIT: usize = 16 * 1024 * 1024;

/// 读-合并-写清单的串行锁: 同一话的两个批次可能在两个后台线程里同时到达。
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());

fn as_i64(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0)
}

fn as_string(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// 书目
// ---------------------------------------------------------------------------

/// 一部漫画 (一本书) 的话列表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComicBook {
    pub id: i64,
    pub title: String,
    pub chapters: Vec<ChapterRef>,
}

impl ComicBook {
    pub fn from_info(info: &api::BookInfo) -> ComicBook {
        ComicBook { id: info.book.id, title: info.book.title.clone(), chapters: info.chapters.clone() }
    }

    pub fn load(paths: &Paths, book_id: i64) -> Option<ComicBook> {
        let v: Value = serde_json::from_slice(&std::fs::read(paths.comic_book_file(book_id)).ok()?).ok()?;
        let chapters = v
            .get("chapters")?
            .as_array()?
            .iter()
            .map(|c| ChapterRef {
                id: as_i64(c.get("id")),
                sort_num: as_i64(c.get("sort_num")),
                title: as_string(c.get("title")),
                page_count: as_i64(c.get("page_count")),
            })
            .filter(|c| c.id > 0)
            .collect();
        Some(ComicBook { id: book_id, title: as_string(v.get("title")), chapters })
    }

    pub fn save(&self, paths: &Paths) {
        let chapters: Vec<Value> =
            self.chapters.iter().map(|c| json!({"id": c.id, "sort_num": c.sort_num, "title": c.title, "page_count": c.page_count})).collect();
        let v = json!({"id": self.id, "title": self.title, "chapters": chapters});
        if let Err(e) = store::atomic_write(&paths.comic_book_file(self.id), v.to_string().as_bytes()) {
            eprintln!("[comic] 书目缓存写入失败: {e}");
        }
    }

    pub fn index_of(&self, chapter_id: i64) -> Option<usize> {
        self.chapters.iter().position(|c| c.id == chapter_id)
    }
}

/// 书目: 先读缓存, 没有且在线时取 `GetBookInfo` 并写缓存。失败返回 None (阅读页仍可读当前话, 只是不能换话)。
pub fn load_book(paths: &Paths, net: Option<&Client>, book_id: i64) -> Option<ComicBook> {
    if let Some(book) = ComicBook::load(paths, book_id) {
        return Some(book);
    }
    if net.is_none() && !api::fake_mode() {
        return None;
    }
    match api::load_book_info(net, book_id) {
        Ok(info) => {
            let book = ComicBook::from_info(&info);
            book.save(paths);
            Some(book)
        }
        Err(e) => {
            eprintln!("[comic] 书目 {book_id}: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 一话的图片地址清单
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Manifest {
    pub chapter_id: i64,
    pub book_id: i64,
    pub book_name: String,
    pub title: String,
    pub sort_num: i64,
    /// 长度 = 本话总页数; None = 该批还没取过
    pub images: Vec<Option<String>>,
}

impl Manifest {
    pub fn total(&self) -> usize {
        self.images.len()
    }

    pub fn url(&self, index: usize) -> Option<&str> {
        self.images.get(index).and_then(|u| u.as_deref()).filter(|u| !u.is_empty())
    }

    /// 第 `index` 页所在批次的起点 (按 `BATCH` 对齐, 与网页版一致)。
    pub fn batch_start(index: usize) -> usize {
        index / BATCH * BATCH
    }

    /// 第 `index` 页的地址还不知道 (需要取它所在的批次)。
    pub fn needs_batch(&self, index: usize) -> bool {
        index < self.total() && self.images[index].is_none()
    }

    pub fn merge(&mut self, batch: &ComicBatch) {
        if self.chapter_id == 0 {
            self.chapter_id = batch.chapter_id;
        }
        if batch.book_id > 0 {
            self.book_id = batch.book_id;
        }
        if !batch.book_name.is_empty() {
            self.book_name = batch.book_name.clone();
        }
        if !batch.title.is_empty() {
            self.title = batch.title.clone();
        }
        if batch.sort_num > 0 {
            self.sort_num = batch.sort_num;
        }
        // 总页数以服务器最新的为准 (章节被编辑过时可能变化)
        if batch.total != self.images.len() {
            self.images.resize(batch.total, None);
        }
        for (i, url) in batch.images.iter().enumerate() {
            if let Some(slot) = self.images.get_mut(batch.skip + i) {
                *slot = Some(url.clone());
            }
        }
    }

    pub fn load(paths: &Paths, chapter_id: i64) -> Option<Manifest> {
        let file = paths.comic_chapter_file(chapter_id);
        let v: Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
        store::touch(&file);
        let images = v.get("images")?.as_array()?.iter().map(|u| u.as_str().map(str::to_string)).collect();
        Some(Manifest {
            chapter_id,
            book_id: as_i64(v.get("book_id")),
            book_name: as_string(v.get("book_name")),
            title: as_string(v.get("title")),
            sort_num: as_i64(v.get("sort_num")),
            images,
        })
    }

    fn save(&self, paths: &Paths) {
        let v = json!({
            "chapter_id": self.chapter_id, "book_id": self.book_id, "book_name": self.book_name,
            "title": self.title, "sort_num": self.sort_num, "images": self.images,
        });
        if let Err(e) = store::atomic_write(&paths.comic_chapter_file(self.chapter_id), v.to_string().as_bytes()) {
            eprintln!("[comic] 清单缓存写入失败: {e}");
        }
    }
}

/// 取一批图片地址, 合并进磁盘上的清单并返回合并后的清单与服务器阅读位置 (只有 skip = 0 才有)。
pub fn fetch_batch(paths: &Paths, net: Option<&Client>, chapter_id: i64, skip: usize, priority: i32) -> Result<(Manifest, Option<(i64, usize)>), String> {
    if net.is_none() && !api::fake_mode() {
        return Err("本话尚未缓存 (离线)".into());
    }
    let batch = api::load_comic_content(net, chapter_id, skip, BATCH, priority)?;
    let _guard = MANIFEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut manifest = Manifest::load(paths, chapter_id).unwrap_or(Manifest { chapter_id, ..Manifest::default() });
    manifest.merge(&batch);
    manifest.save(paths);
    Ok((manifest, batch.read_position))
}

// ---------------------------------------------------------------------------
// 阅读进度
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComicProgress {
    pub chapter_id: i64,
    /// 0 起
    pub page: usize,
    pub total: usize,
    /// Unix 秒
    pub time: i64,
}

pub fn load_progress(paths: &Paths, book_id: i64) -> Option<ComicProgress> {
    let v: Value = serde_json::from_slice(&std::fs::read(paths.comic_progress_file(book_id)).ok()?).ok()?;
    let p = ComicProgress {
        chapter_id: as_i64(v.get("chapter_id")),
        page: as_i64(v.get("page")).max(0) as usize,
        total: as_i64(v.get("total")).max(0) as usize,
        time: as_i64(v.get("time")),
    };
    (p.chapter_id > 0).then_some(p)
}

pub fn save_progress(paths: &Paths, book_id: i64, p: &ComicProgress) -> std::io::Result<()> {
    let v = json!({"chapter_id": p.chapter_id, "page": p.page, "total": p.total, "time": p.time});
    store::atomic_write(&paths.comic_progress_file(book_id), v.to_string().as_bytes())
}

// ---------------------------------------------------------------------------
// 页面图片
// ---------------------------------------------------------------------------

/// 向 CDN 要多高的图: 按 URL 里的 `size=WxH` 算出适配屏幕后的高度, 再取档位;
/// 没有尺寸信息时按屏幕高度。横向跨页图适配宽度后很矮, 不用要 2048 的版本。
pub fn request_height(url: &str, screen_w: u32, screen_h: u32) -> u32 {
    let size = url.split_once('?').and_then(|(_, q)| {
        q.split('&').find_map(|kv| {
            let (w, h) = kv.strip_prefix("size=")?.split_once('x')?;
            Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
        })
    });
    let height = match size {
        Some((w, h)) if w > 0.0 && h > 0.0 => {
            let scale = (screen_w as f32 / w).min(screen_h as f32 / h);
            (h * scale).ceil() as u32
        }
        _ => screen_h,
    };
    kn_render::height_bucket(height.max(1))
}

/// 页面图片字节: 先查缓存, 缺失时按缩放后的 URL 下载并写缓存。离线且未缓存返回 Ok(None)。
pub fn page_bytes(paths: &Paths, net: Option<&Client>, url: &str, height: u32) -> Result<Option<Vec<u8>>, String> {
    if let Some(bytes) = api::fixture_file(url) {
        return Ok(Some(bytes));
    }
    let file = paths.comic_image_file(url, height);
    if let Ok(bytes) = std::fs::read(&file) {
        store::touch(&file);
        return Ok(Some(bytes));
    }
    let Some(net) = net else { return Ok(None) };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(None);
    }
    let bytes = net.download(&crate::net::scaled_image_url(url, height), IMAGE_LIMIT).map_err(|e| e.to_string())?;
    if let Err(e) = store::atomic_write(&file, &bytes) {
        eprintln!("[comic] 图片缓存写入失败: {e}");
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(skip: usize, n: usize, total: usize) -> ComicBatch {
        ComicBatch {
            chapter_id: 7,
            book_id: 3,
            book_name: "书".into(),
            title: "第1话".into(),
            sort_num: 1,
            total,
            skip,
            images: (skip..skip + n).map(|i| format!("u{i}")).collect(),
            read_position: None,
        }
    }

    #[test]
    fn manifest_merges_batches_in_any_order() {
        let mut m = Manifest::default();
        m.merge(&batch(6, 4, 10));
        assert_eq!(m.total(), 10);
        assert!(m.needs_batch(0) && !m.needs_batch(6) && !m.needs_batch(10));
        assert_eq!(m.url(9), Some("u9"));
        m.merge(&batch(0, 6, 10));
        assert!((0..10).all(|i| m.url(i) == Some(&*format!("u{i}"))));
        assert_eq!((m.chapter_id, m.book_id, m.sort_num), (7, 3, 1));
        assert_eq!(Manifest::batch_start(7), 6);
        // 服务器总页数变少: 截断
        m.merge(&batch(0, 6, 8));
        assert_eq!(m.total(), 8);
    }

    #[test]
    fn manifest_and_progress_round_trip() {
        let dir = std::env::temp_dir().join(format!("kn-comic-test-{}", std::process::id()));
        let paths = Paths { app_dir: dir.clone() };
        let mut m = Manifest { chapter_id: 7, ..Manifest::default() };
        m.merge(&batch(0, 6, 9));
        m.save(&paths);
        assert_eq!(Manifest::load(&paths, 7), Some(m));
        let p = ComicProgress { chapter_id: 7, page: 4, total: 9, time: 123 };
        save_progress(&paths, 3, &p).unwrap();
        assert_eq!(load_progress(&paths, 3), Some(p));
        let book = ComicBook { id: 3, title: "书".into(), chapters: vec![ChapterRef { id: 7, sort_num: 1, title: "第1话".into(), page_count: 9 }] };
        book.save(&paths);
        assert_eq!(ComicBook::load(&paths, 3), Some(book));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn request_height_fits_screen() {
        // 竖版 1000x1500 适配 1236x1648: 高 1648 -> 档位 2048
        assert_eq!(request_height("https://x/a.jpg?size=1000x1500", 1236, 1648), 2048);
        // 横向跨页 2000x1400: 宽度受限, 高 ~866 -> 1024
        assert_eq!(request_height("https://x/a.jpg?size=2000x1400&t=1", 1236, 1648), 1024);
        // 没有尺寸: 按屏幕高
        assert_eq!(request_height("https://x/a.jpg", 1236, 1648), 2048);
        assert_eq!(request_height("https://x/a.jpg", 600, 800), 1024);
    }
}
