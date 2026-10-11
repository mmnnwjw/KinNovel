//! 阅读统计 (只在本机, 不上传): 每天的阅读时长与页数、每本书的累计。
//!
//! 计时按翻页: 两次翻页之间的时间算作读上一页, 单页最多算 [`MAX_PAGE_SECS`] (放下设备走开不会一直累计);
//! 离开阅读页/休眠时把最后一页的时间补上。小说和漫画阅读页各有一个 [`Tracker`]。
//! 存在 `data/stats.json` = `{"days": {"2026-10-11": [秒, 页]}, "books": {"123": {"name", "secs", "pages", "comic", "last"}}}`。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::store::{self, atomic_write, Paths};

/// 单页最多计入的秒数
pub const MAX_PAGE_SECS: u64 = 300;
/// 每翻这么多页落盘一次 (另外离开/休眠时落盘)
const FLUSH_EVERY_PAGES: u32 = 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookStat {
    pub name: String,
    pub secs: u64,
    pub pages: u64,
    pub comic: bool,
    /// 最近一次阅读 (Unix 秒)
    pub last: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// 本地日期 "YYYY-MM-DD" → (秒, 页)
    pub days: BTreeMap<String, (u64, u64)>,
    pub books: BTreeMap<i64, BookStat>,
}

static STATS: Mutex<Option<Stats>> = Mutex::new(None);

impl Paths {
    fn stats_file(&self) -> PathBuf {
        self.data_dir().join("stats.json")
    }
}

/// 本地时区的日期 (年, 月, 日)。
pub fn local_date(unix: i64) -> (i64, u32, u32) {
    let days = (unix + crate::tz::local_utc_offset_secs(unix)).div_euclid(86_400);
    civil_from_days(days)
}

/// 1970-01-01 起的天数 → 公历日期 (Howard Hinnant 算法)。
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn day_key(unix: i64) -> String {
    let (y, m, d) = local_date(unix);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Stats {
    fn from_json(v: &Value) -> Stats {
        let mut s = Stats::default();
        if let Some(days) = v.get("days").and_then(Value::as_object) {
            for (k, d) in days {
                let n = |i: usize| d.get(i).and_then(Value::as_u64).unwrap_or(0);
                s.days.insert(k.clone(), (n(0), n(1)));
            }
        }
        if let Some(books) = v.get("books").and_then(Value::as_object) {
            for (k, b) in books {
                let Ok(id) = k.parse::<i64>() else { continue };
                s.books.insert(
                    id,
                    BookStat {
                        name: b.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        secs: b.get("secs").and_then(Value::as_u64).unwrap_or(0),
                        pages: b.get("pages").and_then(Value::as_u64).unwrap_or(0),
                        comic: b.get("comic").and_then(Value::as_bool).unwrap_or(false),
                        last: b.get("last").and_then(Value::as_i64).unwrap_or(0),
                    },
                );
            }
        }
        s
    }

    fn to_json(&self) -> Value {
        let days: Map<String, Value> = self.days.iter().map(|(k, (secs, pages))| (k.clone(), json!([secs, pages]))).collect();
        let books: Map<String, Value> = self
            .books
            .iter()
            .map(|(id, b)| (id.to_string(), json!({"name": b.name, "secs": b.secs, "pages": b.pages, "comic": b.comic, "last": b.last})))
            .collect();
        json!({"days": days, "books": books})
    }

    /// 记一段阅读 (secs 秒, pages 页) 到 `now` 那天和这本书。
    pub fn add(&mut self, now: i64, book_id: i64, name: &str, comic: bool, secs: u64, pages: u64) {
        let day = self.days.entry(day_key(now)).or_default();
        day.0 += secs;
        day.1 += pages;
        let book = self.books.entry(book_id).or_default();
        if !name.is_empty() {
            book.name = name.to_string();
        }
        book.secs += secs;
        book.pages += pages;
        book.comic = comic;
        book.last = now;
    }

    /// 截至 `today` (含) 连续有阅读的天数。今天还没读时从昨天算起。
    pub fn streak(&self, now: i64) -> u32 {
        let today = (now + crate::tz::local_utc_offset_secs(now)).div_euclid(86_400);
        let read = |d: i64| {
            let (y, m, dd) = civil_from_days(d);
            self.days.get(&format!("{y:04}-{m:02}-{dd:02}")).is_some_and(|v| v.0 > 0)
        };
        let mut day = if read(today) { today } else { today - 1 };
        let mut n = 0;
        while read(day) {
            n += 1;
            day -= 1;
        }
        n
    }
}

/// 当前统计的副本 (首次调用时从文件读)。
pub fn snapshot(paths: &Paths) -> Stats {
    with(paths, |s| s.clone())
}

fn with<R>(paths: &Paths, f: impl FnOnce(&mut Stats) -> R) -> R {
    let mut guard = STATS.lock().unwrap_or_else(|e| e.into_inner());
    let stats = guard.get_or_insert_with(|| {
        std::fs::read(paths.stats_file()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| Stats::from_json(&v)).unwrap_or_default()
    });
    f(stats)
}

fn flush(paths: &Paths) {
    let text = with(paths, |s| s.to_json().to_string());
    if let Err(e) = atomic_write(&paths.stats_file(), text.as_bytes()) {
        eprintln!("[stats] 保存失败: {e}");
    }
}

/// 一个阅读页的计时器。
#[derive(Default)]
pub struct Tracker {
    last: Option<Instant>,
    unflushed: u32,
}

impl Tracker {
    /// 开始计时 (进入阅读页、内容就绪、唤醒后)。
    pub fn start(&mut self) {
        self.last = Some(Instant::now());
    }

    fn elapsed(&mut self) -> u64 {
        let secs = self.last.map(|t| t.elapsed().as_secs().min(MAX_PAGE_SECS)).unwrap_or(0);
        self.last = Some(Instant::now());
        secs
    }

    /// 翻了一页: 上一页的时间 + 1 页。
    pub fn page(&mut self, paths: &Paths, book_id: i64, name: &str, comic: bool) {
        let secs = self.elapsed();
        with(paths, |s| s.add(store::unix_now(), book_id, name, comic, secs, 1));
        self.unflushed += 1;
        if self.unflushed >= FLUSH_EVERY_PAGES {
            self.unflushed = 0;
            flush(paths);
        }
    }

    /// 停止计时 (离开阅读页、休眠): 补上当前页的时间并落盘。
    pub fn stop(&mut self, paths: &Paths, book_id: i64, name: &str, comic: bool) {
        if self.last.is_none() {
            return;
        }
        let secs = self.elapsed();
        self.last = None;
        with(paths, |s| s.add(store::unix_now(), book_id, name, comic, secs, 0));
        self.unflushed = 0;
        flush(paths);
    }
}

/// "35 分钟" / "1 小时 20 分" / "0 分钟"
pub fn duration_text(secs: u64) -> String {
    let minutes = secs / 60;
    if minutes < 60 {
        format!("{minutes} 分钟")
    } else if minutes % 60 == 0 {
        format!("{} 小时", minutes / 60)
    } else {
        format!("{} 小时 {} 分", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_737), (2026, 10, 11));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn add_round_trip_and_streak() {
        let mut s = Stats::default();
        let day = 86_400;
        let now = 20_737 * day + 12 * 3600; // 2026-10-11 正午 (UTC; 测试只看相对天数)
        s.add(now - 2 * day, 1, "书", false, 600, 10);
        s.add(now - day, 1, "书", false, 60, 1);
        s.add(now, 2, "漫画", true, 120, 3);
        let back = Stats::from_json(&s.to_json());
        assert_eq!(back, s);
        assert_eq!(s.books[&1].secs, 660);
        assert!(s.books[&2].comic);
        assert_eq!(s.streak(now), 3);
        assert_eq!(s.streak(now + 3 * day), 0);
    }

    #[test]
    fn durations() {
        assert_eq!(duration_text(59), "0 分钟");
        assert_eq!(duration_text(35 * 60), "35 分钟");
        assert_eq!(duration_text(3600), "1 小时");
        assert_eq!(duration_text(80 * 60), "1 小时 20 分");
    }
}
