//! 发现 / 历史 / 书架 / 书籍详情用到的响应解析与加载函数。
//!
//! 容错解析: 缺字段用默认值, 绝不 panic (字段来自 `serde_json::Value`, 服务器可能变化)。
//! 对照 Python: `bin/src/kinnovel/pages/{shelf,history,browse,rank,book,account}.py`,
//! `bin/src/kinnovel/api.py` 的 `dict_items` / `novel_items` / `_novel_data` 等助手。
//!
//! Fixture 模式: 设置环境变量 `KN_FAKE_DIR` 时, 每个 `load_*` 函数改为读取
//! `<KN_FAKE_DIR>/<name>.json` 而不访问网络 (主机预览、测试用)。

use std::path::PathBuf;

use kn_net::Client;
use serde_json::{json, Value};

fn fake_dir() -> Option<PathBuf> {
    std::env::var_os("KN_FAKE_DIR").map(PathBuf::from)
}

/// 是否处于 fixture 模式 (页面用它判断"离线但有数据可读"与"真离线"的区别)。
pub fn fake_mode() -> bool {
    fake_dir().is_some()
}

/// fixture 模式下的图片: `fixture:<相对路径>` 读 `<KN_FAKE_DIR>/<相对路径>` (封面、漫画页面)。
pub fn fixture_file(url: &str) -> Option<Vec<u8>> {
    let rel = url.strip_prefix("fixture:")?;
    if rel.contains("..") {
        return None;
    }
    std::fs::read(fake_dir()?.join(rel)).ok()
}

fn load_fake(name: &str) -> Result<Value, String> {
    let dir = fake_dir().ok_or_else(|| "fixture 模式未开启".to_string())?;
    let path = dir.join(format!("{name}.json"));
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// 基础取值助手
// ---------------------------------------------------------------------------

fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
}

fn str_of(v: &Value, key: &str) -> String {
    get(v, key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn i64_of(v: &Value, key: &str) -> i64 {
    get(v, key).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)).or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0)
}

fn obj_of<'a>(v: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    get(v, key).unwrap_or(&NULL)
}

/// 书目条目的 `Category: {ShortName, Name, Color}` → 角标文字 (简称优先, 缩略图很窄)。
fn category_label(v: &Value) -> String {
    let c = obj_of(v, "Category");
    let short = str_of(c, "ShortName");
    if short.is_empty() { str_of(c, "Name") } else { short }
}

fn arr_of<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    get(v, key).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

/// 把 "2024-01-02T03:04:05" / "2024-01-02 03:04:05" 之类的时间字符串截成 "2024-01-02"
/// 之外的部分一并显示用不到, 列表行里只需要日期。空输入原样返回空串。
fn date_part(s: &str) -> String {
    let s = s.trim();
    match s.get(..10) {
        Some(date) if s.as_bytes()[4] == b'-' => date.to_string(),
        _ => s.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 书目列表项 (最新/排行/分类/书架/历史都用这个), 对照 `GetBookList` 等接口的条目字段。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookItem {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub cover: String,
    /// "YYYY-MM-DD" 或原始字符串; 空表示未知
    pub last_update: String,
    pub last_chapter: String,
    pub views: i64,
    pub favorite: i64,
    /// 漫画 (`Type: "Comic"`; 漫画列表接口的条目一律是漫画)
    pub comic: bool,
    /// 漫画列表条目的 `Count` (话数); 其它接口为 0
    pub count: i64,
    /// 封面角标 (网页 BookCard): 分类简称 (`Category.ShortName`, 没有时用 `Name`); 空 = 不显示
    pub category: String,
    /// 阅读等级 `Level` / 书籍内部等级 `InteriorLevel`; 0 = 无
    pub level: i64,
    pub interior_level: i64,
}

impl BookItem {
    pub fn parse(v: &Value) -> BookItem {
        let id = i64_of(v, "Id");
        let title = str_of(v, "Title");
        crate::sleep::note_cover(id, &str_of(v, "Cover"));
        BookItem {
            id,
            title: if title.is_empty() { format!("书籍 #{id}") } else { title },
            author: str_of(v, "UserName"),
            cover: str_of(v, "Cover"),
            last_update: date_part(&str_of(v, "LastUpdatedAt")),
            last_chapter: str_of(v, "LastUpdatedChapter"),
            views: i64_of(v, "Views"),
            favorite: i64_of(v, "Favorite"),
            comic: is_kind(v, "comic"),
            count: i64_of(v, "Count"),
            category: category_label(v),
            level: i64_of(v, "Level"),
            interior_level: i64_of(v, "InteriorLevel"),
        }
    }

    /// 漫画列表条目 (`GetComicList` / `GetBookListByIds{Type: Comic}`):
    /// `{Id, Title, Cover, Count, LastUpdatedAt}`, 按系列聚合, `Id` 是系列里的一本。
    pub fn parse_comic(v: &Value) -> BookItem {
        BookItem { comic: true, ..BookItem::parse(v) }
    }

    /// 书名前的小框: 分类简称 · Level N (网页 BookCard 的封面角标; 有书籍内部等级时显示它, 与网页一致)。
    pub fn badge(&self) -> String {
        let level = if self.interior_level > 0 { self.interior_level } else { self.level };
        match (self.category.is_empty(), level > 0) {
            (false, true) => format!("{} · Level {level}", self.category),
            (false, false) => self.category.clone(),
            (true, true) => format!("Level {level}"),
            (true, false) => String::new(),
        }
    }

    /// 列表行的副标题: 漫画显示话数, 小说显示作者 (没有作者时显示最新章节)。
    pub fn subtitle(&self) -> String {
        if self.comic {
            return if self.count > 0 { format!("漫画 · {} 话", self.count) } else { "漫画".to_string() };
        }
        if self.author.is_empty() { self.last_chapter.clone() } else { self.author.clone() }
    }

    pub fn list_from(items: &[Value]) -> Vec<BookItem> {
        items.iter().map(BookItem::parse).collect()
    }
}

/// 分页列表响应: `{Data, Page, TotalPages}` 或裸数组 (对照 `browse.py` `_response_parts`)。
#[derive(Clone, Debug, Default)]
pub struct ListPage {
    pub items: Vec<BookItem>,
    pub page: i64,
    pub total_pages: i64,
}

fn parse_list_page(v: &Value, requested_page: i64) -> ListPage {
    parse_list_page_with(v, requested_page, BookItem::parse)
}

fn parse_list_page_with(v: &Value, requested_page: i64, parse: fn(&Value) -> BookItem) -> ListPage {
    match v {
        Value::Object(_) => {
            let data = arr_of(v, "Data");
            let data = if data.is_empty() { arr_of(v, "data") } else { data };
            let page = {
                let p = i64_of(v, "Page");
                if p > 0 { p } else { requested_page.max(1) }
            };
            let total = {
                let t = i64_of(v, "TotalPages");
                if t > 0 { t } else { page }
            };
            ListPage { items: data.into_iter().map(parse).collect(), page: page.max(1), total_pages: total.max(1) }
        }
        Value::Array(items) => ListPage { items: items.iter().map(parse).collect(), page: requested_page.max(1), total_pages: requested_page.max(1) },
        _ => ListPage { items: Vec::new(), page: requested_page.max(1), total_pages: requested_page.max(1) },
    }
}

/// 分类 (对照 `GetBookCategories`)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Category {
    pub id: i64,
    pub name: String,
}

fn parse_categories(v: &Value) -> Vec<Category> {
    let items = match v {
        Value::Object(_) => arr_of(v, "Data"),
        Value::Array(items) => items.iter().collect(),
        _ => Vec::new(),
    };
    items
        .into_iter()
        .map(|c| Category { id: i64_of(c, "Id"), name: { let n = str_of(c, "Name"); if n.is_empty() { "未命名".to_string() } else { n } } })
        .collect()
}

/// 章节目录项 (对照 `(Book).Chapters`)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChapterRef {
    pub id: i64,
    pub sort_num: i64,
    pub title: String,
    /// 漫画: 本话图片数 (`PageCount`); 小说为 0
    pub page_count: i64,
}

/// 书籍详情 (对照 `GetBookInfo` 的 `Book` + `Series`/`SeriesTitle` + `ReadPosition`)。
#[derive(Clone, Debug, Default)]
pub struct BookInfo {
    pub book: BookItem,
    pub intro: String,
    pub tags: Vec<String>,
    pub series_name: String,
    pub chapters: Vec<ChapterRef>,
    /// 服务器记录的阅读位置所在章节 id (0 = 无)
    pub read_position_chapter_id: i64,
    /// 漫画 (`Book.Type == "Comic"`): 章节是 "话", 用漫画阅读页打开
    pub is_comic: bool,
    /// 同系列的书 (`Series`: `{Id, Title, Cover}`), 漫画详情用它列出其它卷
    pub series_books: Vec<BookItem>,
}

impl BookInfo {
    pub fn parse(v: &Value) -> BookInfo {
        let book_v = obj_of(v, "Book");
        let id = i64_of(book_v, "Id");
        let title = str_of(book_v, "Title");
        crate::sleep::note_cover(id, &str_of(book_v, "Cover"));
        let classification = obj_of(obj_of(book_v, "Extra"), "classification");
        let author = {
            let a = str_of(book_v, "Author");
            if !a.is_empty() { a } else { str_of(classification, "author") }
        };
        let book = BookItem {
            id,
            title: if title.is_empty() { format!("书籍 #{id}") } else { title },
            author,
            cover: str_of(book_v, "Cover"),
            last_update: date_part(&str_of(book_v, "LastUpdatedAt")),
            last_chapter: str_of(book_v, "LastUpdatedChapter"),
            views: i64_of(book_v, "Views"),
            favorite: i64_of(book_v, "Favorite"),
            comic: is_kind(book_v, "comic"),
            count: 0,
            category: category_label(book_v),
            level: i64_of(book_v, "Level"),
            interior_level: i64_of(book_v, "InteriorLevel"),
        };
        let tags = arr_of(classification, "tags").into_iter().filter_map(Value::as_str).map(str::to_string).collect();
        let mut series_name = str_of(v, "SeriesTitle");
        if series_name.is_empty() {
            series_name = str_of(classification, "series_name_cn");
        }
        if series_name.is_empty() {
            series_name = str_of(classification, "series_name");
        }
        let chapters = arr_of(book_v, "Chapters")
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let sort_num = i64_of(c, "SortNum");
                ChapterRef {
                    id: i64_of(c, "Id"),
                    sort_num: if sort_num > 0 { sort_num } else { i as i64 + 1 },
                    title: str_of(c, "Title"),
                    page_count: i64_of(c, "PageCount"),
                }
            })
            .collect();
        let is_comic = book.comic;
        let series_books = arr_of(v, "Series").into_iter().map(|s| BookItem { comic: is_comic, ..BookItem::parse(s) }).filter(|b| b.id > 0).collect();
        BookInfo {
            book,
            intro: html_to_paragraphs(&str_of(book_v, "Introduction")).join("\n"),
            tags,
            series_name,
            chapters,
            read_position_chapter_id: i64_of(obj_of(v, "ReadPosition"), "ChapterId"),
            is_comic,
            series_books,
        }
    }
}

/// 本地日期 "YYYY-MM-DD" (对照 `pages::relative_time` 的日期计算)。
fn today_date_string() -> String {
    let now = crate::store::unix_now();
    let day = (now + crate::tz::local_utc_offset_secs(now)).div_euclid(86_400);
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 当前用户 (对照 `GetMyInfo` / `User.Growth`)。
#[derive(Clone, Debug, Default)]
pub struct MyInfo {
    pub user_name: String,
    pub level: i64,
    pub coin: i64,
    pub sign_streak: i64,
    pub today_signed: bool,
}

impl MyInfo {
    pub fn parse(v: &Value) -> MyInfo {
        let growth = obj_of(v, "Growth");
        let today_signed = growth.get("TodaySigned").and_then(Value::as_bool).unwrap_or(false)
            || {
                let text = {
                    let a = str_of(growth, "LastSignAt");
                    if !a.is_empty() { a } else { str_of(growth, "LastSignTime") }
                };
                date_part(&text) == today_date_string()
            };
        MyInfo {
            user_name: { let n = str_of(v, "UserName"); if n.is_empty() { "未知用户".to_string() } else { n } },
            level: i64_of(v, "Level"),
            coin: i64_of(growth, "Coin"),
            sign_streak: i64_of(growth, "SignStreak"),
            today_signed,
        }
    }
}

// ---------------------------------------------------------------------------
// 书架 (折叠文件夹嵌套: 只取顶层未被折叠的书籍, 对照 `shelf.py` 的 `shelf_book_id` / `is_folder`)
// ---------------------------------------------------------------------------

fn is_kind(item: &Value, kind: &str) -> bool {
    let value = get(item, "type").or_else(|| get(item, "Type"));
    value.and_then(Value::as_str).is_some_and(|s| s.trim().eq_ignore_ascii_case(kind))
}

fn index_key(item: &Value) -> i64 {
    i64_of(item, "index")
}

// ---------------------------------------------------------------------------
// 加载函数 (fixture 优先, 否则走 `net`; `net` 为 None 时离线返回错误)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 书架的文件夹层级与写回 (对照 Python `pages/shelf.py` / `pages/book.py::_set_shelf`)
//
// 书架是一张表: 条目 `{id, type: NOVEL|COMIC|FOLDER, index, parents: [文件夹 id...], title?, updateAt?}`,
// 条目所在的文件夹 = parents 的最后一个。`SaveBookShelf` 整表覆盖, 所以每次写之前都重新拉取最新的表,
// 只改目标条目, 其余条目 (含不认识的字段) 原样写回。
// ---------------------------------------------------------------------------

/// 书架当前层的一项。
#[derive(Clone, Debug, PartialEq)]
pub enum ShelfEntry {
    Folder { id: String, title: String, count: usize },
    Book(BookItem),
}

/// 云端书架: 原始条目 + 书籍元数据。
#[derive(Clone, Debug, Default)]
pub struct ShelfData {
    pub raw: Vec<Value>,
    pub books: std::collections::HashMap<i64, BookItem>,
}

fn shelf_items(envelope: &Value) -> Vec<Value> {
    match envelope {
        Value::Object(_) => {
            let d = arr_of(envelope, "data");
            if d.is_empty() { arr_of(envelope, "Data") } else { d }
        }
        Value::Array(items) => items.iter().collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .cloned()
    .collect()
}

/// 文件夹 id 是 nanoid 字符串 (也容忍数字)。
fn folder_id(item: &Value) -> Option<String> {
    is_kind(item, "folder").then(|| match get(item, "id") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    })
}

fn shelf_book_id(item: &Value) -> Option<i64> {
    if !item.is_object() || is_kind(item, "folder") {
        return None;
    }
    Some(i64_of(item, "id").max(i64_of(item, "Id"))).filter(|&id| id > 0)
}

fn last_parent(item: &Value) -> Option<String> {
    let parents = arr_of(item, "parents");
    parents.last().map(|p| match p {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

impl ShelfData {
    /// 某一层 (None = 根) 的条目, 按 index 排序; 元数据缺失的书跳过。
    pub fn level(&self, folder: Option<&str>) -> Vec<ShelfEntry> {
        // 一遍算出: 这一层的条目, 以及每个文件夹直接装了几项 (以前每个文件夹各扫一遍全表)
        let mut items: Vec<&Value> = Vec::new();
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for it in &self.raw {
            let parent = last_parent(it);
            if it.is_object() && parent.as_deref() == folder {
                items.push(it);
            }
            if let Some(p) = parent {
                *counts.entry(p).or_default() += 1;
            }
        }
        items.sort_by_key(|it| index_key(it));
        let mut seen = std::collections::HashSet::new();
        items
            .into_iter()
            .filter_map(|it| match folder_id(it) {
                Some(id) => {
                    let count = counts.get(&id).copied().unwrap_or(0);
                    let title = str_of(it, "title");
                    Some(ShelfEntry::Folder { title: if title.is_empty() { "文件夹".into() } else { title }, id, count })
                }
                None => {
                    let id = shelf_book_id(it)?;
                    seen.insert(id).then(|| self.books.get(&id).cloned().map(ShelfEntry::Book)).flatten()
                }
            })
            .collect()
    }

    pub fn folder_title(&self, id: &str) -> String {
        self.raw
            .iter()
            .find(|it| folder_id(it).as_deref() == Some(id))
            .map(|it| str_of(it, "title"))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "文件夹".into())
    }

    pub fn has_folder(&self, id: &str) -> bool {
        self.raw.iter().any(|it| folder_id(it).as_deref() == Some(id))
    }

    /// 全部文件夹, 深度优先 (同层按 index), 选择"移动到"的目标用。
    pub fn folder_tree(&self) -> Vec<FolderNode> {
        fn walk(raw: &[Value], parent: Option<&str>, out: &mut Vec<FolderNode>, depth: usize) {
            let mut level: Vec<&Value> = raw.iter().filter(|it| folder_id(it).is_some() && last_parent(it).as_deref() == parent).collect();
            level.sort_by_key(|it| index_key(it));
            for it in level {
                let id = folder_id(it).unwrap_or_default();
                // 防御性: 数据里的环或过深的层级
                if depth > 16 || out.iter().any(|n| n.id == id) {
                    continue;
                }
                let mut path = parents_of(it);
                path.push(id.clone());
                let title = str_of(it, "title");
                out.push(FolderNode { id: id.clone(), title: if title.is_empty() { "文件夹".into() } else { title }, path });
                walk(raw, Some(&id), out, depth + 1);
            }
        }
        let mut out = Vec::new();
        walk(&self.raw, None, &mut out, 0);
        out
    }

    /// 某一项现在所在的文件夹路径。
    pub fn parents_of(&self, what: &ShelfRef) -> Vec<String> {
        self.raw.iter().find(|it| what.matches(it)).map(parents_of).unwrap_or_default()
    }
}

/// fixture 模式下的书架写回 (只在内存里, 预览用)。
static FAKE_SHELF: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

fn fetch_shelf(net: Option<&Client>) -> Result<Value, String> {
    if fake_mode() {
        if let Some(v) = FAKE_SHELF.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(v);
        }
        return load_fake("book_shelf");
    }
    let net = net.ok_or("离线，无法加载")?;
    crate::net::ensure_login(net)?;
    net.get_book_shelf().map_err(|e| e.to_string())
}

fn save_shelf(net: Option<&Client>, items: Vec<Value>) -> Result<(), String> {
    // 写回后书架页必须重新拉取 (书架页、书籍详情页、文件夹编辑页的写回都经过这里);
    // 写回前后各作废一次: 写回期间完成的加载拿到的可能还是旧表
    forget_shelf();
    let result = save_shelf_now(net, items);
    forget_shelf();
    result
}

fn save_shelf_now(net: Option<&Client>, items: Vec<Value>) -> Result<(), String> {
    if fake_mode() {
        *FAKE_SHELF.lock().unwrap_or_else(|e| e.into_inner()) = Some(json!({ "data": items }));
        return Ok(());
    }
    let net = net.ok_or("离线，无法保存")?;
    // `ver` 与 Python 版 / 网页版写回时带的格式版本一致
    net.save_book_shelf(Value::Array(items), "20260921").map(|_| ()).map_err(|e| e.to_string())
}

/// 书目元数据缓存多久: 进入书架时只给缓存里没有 (或过期) 的书请求 `GetBookListByIds`。
const BOOK_META_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// 书架的内存缓存 (进程内, 不落盘)。
#[derive(Default)]
struct ShelfCache {
    /// 书 id → (取到的时间, 元数据; None = 请求过但服务器没返回, 例如已下架)
    books: std::collections::HashMap<i64, (std::time::Instant, Option<BookItem>)>,
    /// 最近一次加载成功的书架与时间 (从别的页面返回书架时短时间内直接复用)
    last: Option<(std::time::Instant, ShelfData)>,
    /// 每次写回 / 换账号加一: 写回之前发出的加载不能再记成「最近一次」
    generation: u64,
}

type CacheLock = std::sync::Mutex<ShelfCache>;

fn shelf_cache() -> &'static CacheLock {
    static CACHE: std::sync::OnceLock<CacheLock> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn lock_cache(cache: &CacheLock) -> std::sync::MutexGuard<'_, ShelfCache> {
    cache.lock().unwrap_or_else(|e| e.into_inner())
}

/// 书架改过 (写回、登录、退出登录): 下次进入书架重新拉取。书目元数据不受影响, 照样复用。
pub fn forget_shelf() {
    forget_shelf_in(shelf_cache());
}

fn forget_shelf_in(cache: &CacheLock) {
    let mut c = lock_cache(cache);
    c.last = None;
    c.generation += 1;
}

/// 不到 `max_age` 之前加载过、之后没写回过的书架。
pub fn recent_shelf(max_age: std::time::Duration) -> Option<ShelfData> {
    recent_shelf_in(shelf_cache(), std::time::Instant::now(), max_age)
}

fn recent_shelf_in(cache: &CacheLock, now: std::time::Instant, max_age: std::time::Duration) -> Option<ShelfData> {
    let c = lock_cache(cache);
    c.last.as_ref().filter(|(at, _)| now.saturating_duration_since(*at) < max_age).map(|(_, data)| data.clone())
}

/// 云端书架 (含文件夹)。每次都拉 `GetBookShelf`; 书籍元数据只请求缓存里没有 (或过期) 的。
pub fn load_shelf(net: Option<&Client>) -> Result<ShelfData, String> {
    load_shelf_in(shelf_cache(), std::time::Instant::now(), || fetch_shelf(net), |ids| load_books_by_ids_any(net, ids))
}

/// [`load_shelf`] 的实现 (缓存与两个请求由调用方给出, 测试用)。请求期间不持有锁。
fn load_shelf_in(
    cache: &CacheLock,
    now: std::time::Instant,
    fetch: impl FnOnce() -> Result<Value, String>,
    fetch_books: impl FnOnce(&[i64]) -> Result<Vec<BookItem>, String>,
) -> Result<ShelfData, String> {
    let generation = lock_cache(cache).generation;
    let raw = shelf_items(&fetch()?);
    let ids: Vec<i64> = {
        let mut seen = std::collections::HashSet::new();
        raw.iter().filter_map(shelf_book_id).filter(|id| seen.insert(*id)).collect()
    };
    let fresh = |at: &std::time::Instant| now.saturating_duration_since(*at) < BOOK_META_TTL;
    let missing: Vec<i64> = {
        let c = lock_cache(cache);
        ids.iter().copied().filter(|id| !c.books.get(id).is_some_and(|(at, _)| fresh(at))).collect()
    };
    let fetched = if missing.is_empty() { Vec::new() } else { fetch_books(&missing)? };
    let mut c = lock_cache(cache);
    c.books.retain(|_, (at, _)| fresh(at));
    for id in &missing {
        c.books.insert(*id, (now, None));
    }
    for b in fetched {
        c.books.insert(b.id, (now, Some(b)));
    }
    let mut books = std::collections::HashMap::new();
    for id in &ids {
        if let Some((_, Some(b))) = c.books.get(id) {
            // 缓存里的书这次没有重新解析: 补记封面地址 (休眠封面用, 那边的表会淘汰旧项)
            crate::sleep::note_cover(b.id, &b.cover);
            books.insert(*id, b.clone());
        }
    }
    let data = ShelfData { raw, books };
    if c.generation == generation {
        c.last = Some((now, data.clone()));
    }
    Ok(data)
}

/// "2026-10-11T03:40:00.000Z" (UTC)
fn iso_now() -> String {
    let now = crate::store::unix_now();
    let (y, m, d) = crate::stats::civil_from_days(now.div_euclid(86_400));
    let s = now.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", s / 3600, s / 60 % 60, s % 60)
}

/// 移出一本书 (网页版 removeFromShelf: 之后每层重新编号)。
fn without_book(items: Vec<Value>, book_id: i64) -> Vec<Value> {
    squeezed(items.into_iter().filter(|it| shelf_book_id(it) != Some(book_id)).collect())
}

/// 加入一本书 (网页版 addToShelf: 排在最外层第一个, 最外层其余条目的 index 都加 1)。
fn with_book(mut items: Vec<Value>, book_id: i64, comic: bool, now: &str) -> Vec<Value> {
    if !items.iter().any(|it| shelf_book_id(it) == Some(book_id)) {
        for it in &mut items {
            if parents_of(it).is_empty() {
                let i = index_key(it);
                set_index(it, i + 1);
            }
        }
        let kind = if comic { "COMIC" } else { "NOVEL" };
        items.insert(0, json!({"id": book_id, "type": kind, "parents": [], "index": 0, "updateAt": now}));
    }
    items
}

/// 删除文件夹 (网页版 deleteFolder): 去掉文件夹条目, 并把它从其余条目的 parents 里拿掉;
/// 直接放在里面的条目接到上一层末尾, 更深的层级关系不变; 之后每层重新编号。
fn without_folder(items: Vec<Value>, id: &str) -> Vec<Value> {
    let Some(parent) = items.iter().find(|it| folder_id(it).as_deref() == Some(id)).map(parents_of) else { return items };
    let parent_last = parent.last().cloned();
    let mut next = items.iter().filter(|it| last_parent(it) == parent_last).map(index_key).max().unwrap_or(-1) + 1;
    let mut items: Vec<Value> = items.into_iter().filter(|it| folder_id(it).as_deref() != Some(id)).collect();
    for it in &mut items {
        let parents = parents_of(it);
        let Some(depth) = parents.iter().position(|p| p == id) else { continue };
        let direct = depth == parents.len() - 1;
        let rest: Vec<String> = parents.iter().enumerate().filter(|(i, _)| *i != depth).map(|(_, p)| p.clone()).collect();
        set_parents(it, &rest);
        if direct {
            set_index(it, next);
            next += 1;
        }
    }
    squeezed(items)
}

/// 这本书是否在书架上 (书籍详情页的按钮状态)。
pub fn shelf_contains(net: Option<&Client>, book_id: i64) -> Result<bool, String> {
    Ok(shelf_items(&fetch_shelf(net)?).iter().any(|it| shelf_book_id(it) == Some(book_id)))
}

/// 加入/移出书架 (拉取最新的表、只改这一本、整表写回)。返回操作后是否在书架上。
pub fn shelf_set_book(net: Option<&Client>, book_id: i64, comic: bool, on_shelf: bool) -> Result<bool, String> {
    let items = shelf_items(&fetch_shelf(net)?);
    let items = if on_shelf { with_book(items, book_id, comic, &iso_now()) } else { without_book(items, book_id) };
    save_shelf(net, items)?;
    Ok(on_shelf)
}

pub fn shelf_delete_folder(net: Option<&Client>, folder: &str) -> Result<(), String> {
    let items = shelf_items(&fetch_shelf(net)?);
    save_shelf(net, without_folder(items, folder))
}

// 新建 / 重命名文件夹、移动条目: 规则照网页版 `stores/shelf.ts` (createFolder / renameFolder / moveItems),
// 写回前和网页版一样把每层的 index 挤压成 0, 1, 2, ...

/// 书架上的一项 (书的 id 是数字, 文件夹的是字符串)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfRef {
    Book(i64),
    Folder(String),
}

impl ShelfRef {
    fn matches(&self, item: &Value) -> bool {
        match self {
            ShelfRef::Book(id) => shelf_book_id(item) == Some(*id),
            ShelfRef::Folder(id) => folder_id(item).as_deref() == Some(id.as_str()),
        }
    }
}

/// 网页版根文件夹的显示名, 不能用作文件夹名。
const ROOT_FOLDER_NAME: &str = "根文件夹";

/// 选择移动目标时列出的一个文件夹。
#[derive(Clone, Debug, PartialEq)]
pub struct FolderNode {
    pub id: String,
    pub title: String,
    /// 从根到它自己的完整路径 (= 移进去之后条目的 parents)
    pub path: Vec<String>,
}

fn parents_of(item: &Value) -> Vec<String> {
    arr_of(item, "parents")
        .into_iter()
        .map(|p| match p {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}

fn set_parents(item: &mut Value, parents: &[String]) {
    if let Some(obj) = item.as_object_mut() {
        obj.insert("parents".into(), json!(parents));
    }
}

fn set_index(item: &mut Value, index: i64) {
    if let Some(obj) = item.as_object_mut() {
        obj.insert("index".into(), json!(index));
    }
}

/// 网页版 `squeezeShelfItemIndex`: 按 (index, 层深) 稳定排序后, 每层从 0 重新编号。
fn squeezed(mut items: Vec<Value>) -> Vec<Value> {
    items.sort_by_key(|it| (index_key(it), parents_of(it).len()));
    let mut next: std::collections::HashMap<Option<String>, i64> = std::collections::HashMap::new();
    for it in &mut items {
        let n = next.entry(last_parent(it)).or_insert(0);
        set_index(it, *n);
        *n += 1;
    }
    items
}

/// 文件夹名的校验: 非空、不是 "根文件夹"、同一层没有同名的 (`except` = 重命名时的它自己)。
fn check_folder_name(items: &[Value], name: &str, parents: &[String], except: Option<&str>) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name == ROOT_FOLDER_NAME {
        return Err("这个文件夹名不能用".into());
    }
    let clash = items.iter().any(|it| {
        folder_id(it).is_some_and(|id| Some(id.as_str()) != except) && str_of(it, "title") == name && parents_of(it) == parents
    });
    if clash {
        return Err("这一层已有同名文件夹".into());
    }
    Ok(name.to_string())
}

/// 21 位 nanoid 风格的文件夹 id (字母表同 nanoid)。
fn new_folder_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    const ALPHABET: &[u8] = b"useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";
    let state = std::collections::hash_map::RandomState::new();
    let mut seed = 0u64;
    (0..21)
        .map(|i| {
            let mut h = state.build_hasher();
            h.write_u64(seed ^ i);
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
            seed = h.finish();
            ALPHABET[(seed % 64) as usize] as char
        })
        .collect()
}

/// 在 `parents` 这一层最前面新建文件夹。
fn with_new_folder(mut items: Vec<Value>, parents: &[String], name: &str, id: &str, now: &str) -> Result<Vec<Value>, String> {
    let name = check_folder_name(&items, name, parents, None)?;
    for it in &mut items {
        if parents_of(it) == parents {
            let i = index_key(it);
            set_index(it, i + 1);
        }
    }
    items.insert(0, json!({"index": 0, "type": "FOLDER", "parents": parents, "id": id, "title": name, "updateAt": now}));
    Ok(squeezed(items))
}

fn renamed_folder(mut items: Vec<Value>, id: &str, name: &str) -> Result<Vec<Value>, String> {
    let target = ShelfRef::Folder(id.to_string());
    let parents = items.iter().find(|it| target.matches(it)).map(parents_of).ok_or("文件夹已不存在")?;
    let name = check_folder_name(&items, name, &parents, Some(id))?;
    if let Some(obj) = items.iter_mut().find(|it| target.matches(it)).and_then(Value::as_object_mut) {
        obj.insert("title".into(), json!(name));
    }
    Ok(items)
}

/// 把一项移到 `to` (目标文件夹的完整路径, 空 = 最外层), 排在那一层最前面;
/// 移动文件夹时, 它下面所有条目的路径前缀一起改写。
fn moved(mut items: Vec<Value>, what: &ShelfRef, to: &[String]) -> Result<Vec<Value>, String> {
    let from = items.iter().find(|it| what.matches(it)).map(parents_of).ok_or("条目已不在书架上")?;
    if from == to {
        return Ok(items);
    }
    if let ShelfRef::Folder(id) = what {
        if to.contains(id) {
            return Err("不能把文件夹移到它自己或它的下级里".into());
        }
    }
    for it in &mut items {
        if what.matches(it) {
            set_parents(it, to);
            set_index(it, -1);
        } else if parents_of(it) == to {
            let i = index_key(it);
            set_index(it, i + 1);
        }
    }
    if let ShelfRef::Folder(id) = what {
        for it in &mut items {
            let parents = parents_of(it);
            if let Some(anchor) = parents.iter().position(|p| p == id) {
                let rewritten: Vec<String> = to.iter().cloned().chain(parents[anchor..].iter().cloned()).collect();
                set_parents(it, &rewritten);
            }
        }
    }
    Ok(squeezed(items))
}

/// 新建文件夹, 返回它的 id。
pub fn shelf_create_folder(net: Option<&Client>, parents: &[String], name: &str) -> Result<String, String> {
    let items = shelf_items(&fetch_shelf(net)?);
    let id = new_folder_id();
    save_shelf(net, with_new_folder(items, parents, name, &id, &iso_now())?)?;
    Ok(id)
}

pub fn shelf_rename_folder(net: Option<&Client>, id: &str, name: &str) -> Result<(), String> {
    let items = shelf_items(&fetch_shelf(net)?);
    save_shelf(net, renamed_folder(items, id, name)?)
}

pub fn shelf_move(net: Option<&Client>, what: &ShelfRef, to: &[String]) -> Result<(), String> {
    let items = shelf_items(&fetch_shelf(net)?);
    save_shelf(net, moved(items, what, to)?)
}

/// 阅读历史的全部 id (最新在前, 对照 `GetReadHistory()`): 小说是书 id, 漫画是分卷 (书) id。
#[derive(Clone, Debug, Default)]
pub struct HistoryIds {
    pub novel: Vec<i64>,
    pub comic: Vec<i64>,
}

pub fn load_history_ids(net: Option<&Client>) -> Result<HistoryIds, String> {
    let envelope = if fake_mode() {
        load_fake("read_history")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_read_history().map_err(|e| e.to_string())?
    };
    let ids = |key: &str| arr_of(&envelope, key).into_iter().filter_map(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).collect();
    Ok(HistoryIds { novel: ids("Novel"), comic: ids("Comic") })
}

/// 按 id 批量取元数据 (服务器单次最多 24 本, 由 `kn-net` 分块; fixture 模式下按
/// `books_by_ids.json` 里出现的顺序过滤)。只要小说。
pub fn load_books_by_ids(net: Option<&Client>, ids: &[i64]) -> Result<Vec<BookItem>, String> {
    load_by_ids(net, ids, Some("Novel"))
}

/// 同上, 小说与漫画都要 (书架)。
pub fn load_books_by_ids_any(net: Option<&Client>, ids: &[i64]) -> Result<Vec<BookItem>, String> {
    load_by_ids(net, ids, None)
}

fn load_by_ids(net: Option<&Client>, ids: &[i64], book_type: Option<&str>) -> Result<Vec<BookItem>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if fake_mode() {
        let all = load_fake("books_by_ids")?;
        let by_id: std::collections::HashMap<i64, BookItem> =
            all.as_array().into_iter().flatten().map(BookItem::parse).filter(|b| book_type.is_none() || !b.comic).map(|b| (b.id, b)).collect();
        return Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect());
    }
    let net = net.ok_or("离线，无法加载")?;
    let values = net.get_book_list_by_ids_chunked(ids, book_type, 24).map_err(|e| e.to_string())?;
    Ok(BookItem::list_from(&values))
}

/// 漫画阅读历史的一页: 分卷 id -> 按系列聚合的条目 (`GetBookListByIds{Type: Comic}`, 与网页版一致)。
/// 同一系列跨页重复出现由调用方去重。
pub fn load_comic_series_by_ids(net: Option<&Client>, ids: &[i64]) -> Result<Vec<BookItem>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let v = if fake_mode() {
        load_fake("comic_series_by_ids")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_book_list_by_ids(&ids[..ids.len().min(24)], Some("Comic")).map_err(|e| e.to_string())?
    };
    Ok(parse_list_page_with(&v, 1, BookItem::parse_comic).items)
}

/// 书目过滤 (设置页的 "忽略日文" / "忽略 AI", 对照 Python `browse.py` / `series.py`)。
#[derive(Clone, Copy, Debug, Default)]
pub struct Filters {
    pub ignore_japanese: bool,
    pub ignore_ai: bool,
}

impl Filters {
    pub fn from_config(config: &crate::store::Config) -> Filters {
        Filters { ignore_japanese: config.bool("ignore_japanese", false), ignore_ai: config.bool("ignore_ai", false) }
    }
}

/// 最新/分类书目 (对照 `GetBookList`): `category_id` 为 `None` 时是"最新"。
pub fn load_book_list(net: Option<&Client>, page: i64, size: i64, category_id: Option<i64>, filters: Filters) -> Result<ListPage, String> {
    let name = match category_id {
        Some(id) => format!("category_{id}_p{page}"),
        None => format!("latest_p{page}"),
    };
    let v = if fake_mode() {
        load_fake(&name)?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_book_list(page, size, None, "latest", filters.ignore_japanese, filters.ignore_ai, category_id).map_err(|e| e.to_string())?
    };
    Ok(parse_list_page(&v, page))
}

/// 排行榜 (日/周/月, 对照 `GetRank`; 一次取全部, 翻页在本地做)。
pub fn load_rank(net: Option<&Client>, days: i64) -> Result<Vec<BookItem>, String> {
    let v = if fake_mode() {
        load_fake(&format!("rank_{days}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_rank(days).map_err(|e| e.to_string())?
    };
    let items: Vec<&Value> = match &v {
        Value::Object(_) => arr_of(&v, "Data"),
        Value::Array(items) => items.iter().collect(),
        _ => Vec::new(),
    };
    Ok(items.into_iter().map(BookItem::parse).collect())
}

/// 分类列表 (对照 `GetBookCategories`)。
pub fn load_categories(net: Option<&Client>) -> Result<Vec<Category>, String> {
    let v = if fake_mode() {
        load_fake("categories")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_book_categories("Novel").map_err(|e| e.to_string())?
    };
    Ok(parse_categories(&v))
}

/// 漫画列表 (发现 · 漫画, 对照网页版 `GetComicList`)。`order`: latest | new | view。
pub fn load_comic_list(net: Option<&Client>, page: i64, size: i64, order: &str) -> Result<ListPage, String> {
    let v = if fake_mode() {
        load_fake(&format!("comic_list_{order}_p{page}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_comic_list(page, size, order).map_err(|e| e.to_string())?
    };
    Ok(parse_list_page_with(&v, page, BookItem::parse_comic))
}

/// 搜索维度, 对照网页版 `services/book/types.ts` 的 `SearchMode` 与 `components/SearchInput.vue` 的菜单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    /// 标题模糊 (默认, `GetBookList`)
    Fuzzy,
    /// 精确 (标题/作者包含, `GetBookList` + 关键词加引号)
    Exact,
    /// 按书名 (卷标题)
    Title,
    Author,
    /// 按作品名/系列名
    Name,
    /// 按标签 (逗号分隔多个, 同时满足)
    Tags,
}

impl SearchMode {
    pub const ALL: [SearchMode; 6] = [SearchMode::Fuzzy, SearchMode::Exact, SearchMode::Title, SearchMode::Author, SearchMode::Name, SearchMode::Tags];

    /// 网页版 URL / `SearchComicSeries.Mode` 用的值
    pub fn key(self) -> &'static str {
        match self {
            SearchMode::Fuzzy => "fuzzy",
            SearchMode::Exact => "exact",
            SearchMode::Title => "title",
            SearchMode::Author => "author",
            SearchMode::Name => "name",
            SearchMode::Tags => "tags",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SearchMode::Fuzzy => "模糊",
            SearchMode::Exact => "精确",
            SearchMode::Title => "书名",
            SearchMode::Author => "作者",
            SearchMode::Name => "系列",
            SearchMode::Tags => "标签",
        }
    }

    /// 小说搜索的接口名与实际发送的关键词
    fn book_request(self, keywords: &str) -> (&'static str, String) {
        match self {
            SearchMode::Fuzzy => ("GetBookList", keywords.to_string()),
            SearchMode::Exact => ("GetBookList", format!("\"{keywords}\"")),
            SearchMode::Title => ("GetBookListByTitle", keywords.to_string()),
            SearchMode::Author => ("GetBookListByAuthor", keywords.to_string()),
            SearchMode::Name => ("GetBookListByName", keywords.to_string()),
            SearchMode::Tags => ("GetBookListByTags", keywords.to_string()),
        }
    }
}

/// fixture 模式: 先找 `<name>_<mode>_p<page>`, 没有就用 `<name>_p<page>` (关键词不参与)。
fn load_fake_search(name: &str, mode: SearchMode, page: i64) -> Result<Value, String> {
    load_fake(&format!("{name}_{}_p{page}", mode.key())).or_else(|_| load_fake(&format!("{name}_p{page}")))
}

/// 搜索小说 (网页版 `pages/Search.vue` 的 Book 标签)。
pub fn search_books(net: Option<&Client>, keywords: &str, mode: SearchMode, page: i64, size: i64, filters: Filters) -> Result<ListPage, String> {
    let v = if fake_mode() {
        load_fake_search("search", mode, page)?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        let (method, keywords) = mode.book_request(keywords);
        net.search_books(method, &keywords, page, size, filters.ignore_japanese, filters.ignore_ai).map_err(|e| e.to_string())?
    };
    Ok(parse_list_page(&v, page))
}

/// 搜索漫画 (`SearchComicSeries`, 按系列聚合, 条目与漫画列表相同)。
pub fn search_comics(net: Option<&Client>, keywords: &str, mode: SearchMode, page: i64, size: i64, filters: Filters) -> Result<ListPage, String> {
    let v = if fake_mode() {
        load_fake_search("search_comic", mode, page)?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.search_comic_series(keywords, mode.key(), page, size, filters.ignore_japanese, filters.ignore_ai).map_err(|e| e.to_string())?
    };
    Ok(parse_list_page_with(&v, page, BookItem::parse_comic))
}

/// `GetComicContent` 的一批 (网页版每批 6 页)。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComicBatch {
    pub chapter_id: i64,
    pub book_id: i64,
    pub book_name: String,
    pub title: String,
    pub sort_num: i64,
    /// 本话总页数 (与 skip/take 无关)
    pub total: usize,
    /// 本批第一张图在整话中的下标 (0 起)
    pub skip: usize,
    pub images: Vec<String>,
    /// 服务器阅读位置 (章节 id, 1 起的页码); 只有 skip = 0 的请求才带
    pub read_position: Option<(i64, usize)>,
}

impl ComicBatch {
    pub fn parse(v: &Value) -> ComicBatch {
        let ch = obj_of(v, "Chapter");
        let images: Vec<String> = arr_of(ch, "Images").into_iter().map(|u| u.as_str().unwrap_or("").to_string()).collect();
        let skip = i64_of(ch, "Skip").max(0) as usize;
        let total = (i64_of(ch, "Total").max(0) as usize).max(skip + images.len());
        let pos = obj_of(v, "ReadPosition");
        let read_position = (i64_of(pos, "ChapterId") > 0).then(|| (i64_of(pos, "ChapterId"), i64_of(pos, "Position").max(1) as usize));
        ComicBatch {
            chapter_id: i64_of(ch, "Id"),
            book_id: i64_of(ch, "BookId"),
            book_name: str_of(ch, "BookName"),
            title: str_of(ch, "Title"),
            sort_num: i64_of(ch, "SortNum"),
            total,
            skip,
            images,
            read_position,
        }
    }
}

/// 漫画一话的一批图片地址 (阻塞, 后台线程用)。fixture: `comic_content_{cid}_{skip}.json`。
pub fn load_comic_content(net: Option<&Client>, chapter_id: i64, skip: usize, take: usize, priority: i32) -> Result<ComicBatch, String> {
    let v = if fake_mode() {
        load_fake(&format!("comic_content_{chapter_id}_{skip}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_comic_content(chapter_id, skip as i64, take as i64, priority).map_err(|e| e.to_string())?
    };
    let batch = ComicBatch::parse(&v);
    if batch.chapter_id != 0 && batch.chapter_id != chapter_id {
        return Err("漫画数据与请求的章节不符".into());
    }
    Ok(batch)
}

/// 书籍详情 (对照 `GetBookInfo`)。
pub fn load_book_info(net: Option<&Client>, book_id: i64) -> Result<BookInfo, String> {
    let v = if fake_mode() {
        load_fake(&format!("book_info_{book_id}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_book_info(book_id).map_err(|e| e.to_string())?
    };
    Ok(BookInfo::parse(&v))
}

/// 当前用户信息 (对照 `GetMyInfo`)。
pub fn load_my_info(net: Option<&Client>) -> Result<MyInfo, String> {
    let v = if fake_mode() {
        load_fake("my_info")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_my_info().map_err(|e| e.to_string())?
    };
    Ok(MyInfo::parse(&v))
}

/// 签到 (对照 `SignIn`); fixture 模式下直接当作成功, 不落盘任何状态。
pub fn sign_in(net: Option<&Client>) -> Result<(), String> {
    if fake_mode() {
        return Ok(());
    }
    let net = net.ok_or("离线，无法签到")?;
    crate::net::ensure_login(net)?;
    net.sign_in().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 公告 (对照 Python `announcements.py`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Announcement {
    pub id: i64,
    pub title: String,
    pub created_at: String,
}

impl Announcement {
    fn parse(v: &Value) -> Announcement {
        let id = i64_of(v, "Id");
        let title = str_of(v, "Title");
        Announcement { id, title: if title.is_empty() { format!("公告 #{id}") } else { title }, created_at: date_part(&str_of(v, "CreatedAt")) }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnnouncementListPage {
    pub items: Vec<Announcement>,
    pub page: i64,
    pub total_pages: i64,
}

fn parse_announcement_list(v: &Value, requested_page: i64) -> AnnouncementListPage {
    match v {
        Value::Object(_) => {
            let data = arr_of(v, "Data");
            let page = { let p = i64_of(v, "Page"); if p > 0 { p } else { requested_page.max(1) } };
            let total = { let t = i64_of(v, "TotalPages"); if t > 0 { t } else { page } };
            AnnouncementListPage { items: data.into_iter().map(Announcement::parse).collect(), page: page.max(1), total_pages: total.max(1) }
        }
        Value::Array(items) => AnnouncementListPage { items: items.iter().map(Announcement::parse).collect(), page: requested_page.max(1), total_pages: requested_page.max(1) },
        _ => AnnouncementListPage { items: Vec::new(), page: requested_page.max(1), total_pages: requested_page.max(1) },
    }
}

/// 公告列表 (对照 `GetAnnouncementList`), 公开接口不需要登录。
pub fn load_announcement_list(net: Option<&Client>, page: i64, size: i64) -> Result<AnnouncementListPage, String> {
    let v = if fake_mode() {
        load_fake(&format!("announcements_p{page}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_announcement_list(page, size).map_err(|e| e.to_string())?
    };
    Ok(parse_announcement_list(&v, page))
}

#[derive(Clone, Debug, Default)]
pub struct AnnouncementDetail {
    #[allow(dead_code)]
    pub id: i64,
    pub title: String,
    pub date: String,
    /// HTML 已剥除标签、按段落切分 (空行分段)。
    pub paragraphs: Vec<String>,
}

/// HTML → 纯文本段落 (公告、简介、评论)。块级标签 (`p`/`div`/`h1`–`h6`/`li`/...) 分段,
/// `<br>` 是段内换行 (结果里的 `\n`); 源码里的换行和缩进按 HTML 规则只算空白, 连续空白合并;
/// `<li>` 前加 "• "; 解码常见实体。服务器的公告正文是 `<div>` + `<b>` + `<br />` 排版的。
fn html_to_paragraphs(html: &str) -> Vec<String> {
    const BLOCKS: &[&str] = &[
        "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "ul", "ol", "blockquote", "tr", "table", "section", "article", "hr", "pre",
    ];
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let closing = tag.trim_start().starts_with('/');
                let name: String = tag.trim_start().trim_start_matches('/').chars().take_while(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
                if name == "br" {
                    text.push('\n');
                } else if BLOCKS.contains(&name.as_str()) {
                    text.push_str("\n\n");
                    if name == "li" && !closing {
                        text.push_str("• ");
                    }
                }
            }
            _ if in_tag => tag.push(ch),
            '\r' | '\n' | '\t' => text.push(' '),
            _ => text.push(ch),
        }
    }
    let decode = |s: &str| {
        s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&amp;", "&")
    };
    text.split("\n\n")
        .map(|para| {
            para.split('\n')
                .map(|line| decode(&line.split_whitespace().collect::<Vec<_>>().join(" ")))
                .filter(|line| !line.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// 公告详情 (对照 `GetAnnouncementDetail`)。
pub fn load_announcement_detail(net: Option<&Client>, id: i64) -> Result<AnnouncementDetail, String> {
    let v = if fake_mode() {
        load_fake(&format!("announcement_detail_{id}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_announcement_detail(id).map_err(|e| e.to_string())?
    };
    let title = str_of(&v, "Title");
    Ok(AnnouncementDetail {
        id: { let i = i64_of(&v, "Id"); if i > 0 { i } else { id } },
        title: if title.is_empty() { format!("公告 #{id}") } else { title },
        date: date_part(&str_of(&v, "CreatedAt")),
        paragraphs: html_to_paragraphs(&str_of(&v, "Content")),
    })
}

// ---------------------------------------------------------------------------
// 消息通知 (对照 Python `account.py` `enter_notifications` 一段)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notification {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub is_read: bool,
    pub created_at: String,
}

impl Notification {
    fn parse(v: &Value) -> Notification {
        let title = str_of(v, "Title");
        Notification {
            id: i64_of(v, "Id"),
            title: if title.is_empty() { "通知".to_string() } else { title },
            body: str_of(v, "Body"),
            is_read: v.get("IsRead").and_then(Value::as_bool).unwrap_or(false),
            created_at: date_part(&str_of(v, "CreatedAt")),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NotificationListPage {
    pub items: Vec<Notification>,
    pub page: i64,
    pub total_pages: i64,
}

fn parse_notification_list(v: &Value, requested_page: i64) -> NotificationListPage {
    match v {
        Value::Object(_) => {
            let data = arr_of(v, "Data");
            let page = { let p = i64_of(v, "Page"); if p > 0 { p } else { requested_page.max(1) } };
            let total = { let t = i64_of(v, "TotalPages"); if t > 0 { t } else { page } };
            NotificationListPage { items: data.into_iter().map(Notification::parse).collect(), page: page.max(1), total_pages: total.max(1) }
        }
        Value::Array(items) => NotificationListPage { items: items.iter().map(Notification::parse).collect(), page: requested_page.max(1), total_pages: requested_page.max(1) },
        _ => NotificationListPage { items: Vec::new(), page: requested_page.max(1), total_pages: requested_page.max(1) },
    }
}

/// 消息通知列表 (对照 `GetNotifications`); 需要登录。
pub fn load_notifications(net: Option<&Client>, page: i64, size: i64) -> Result<NotificationListPage, String> {
    let v = if fake_mode() {
        load_fake(&format!("notifications_p{page}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_notifications(page, size).map_err(|e| e.to_string())?
    };
    Ok(parse_notification_list(&v, page))
}

/// 标记通知已读 (对照 `MarkNotifications`); 需要登录。fixture 模式下直接当作成功。
pub fn mark_notifications(net: Option<&Client>, ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    if fake_mode() {
        return Ok(());
    }
    let net = net.ok_or("离线，无法操作")?;
    crate::net::ensure_login(net)?;
    net.mark_notifications(ids).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 积分商城 (对照 Python `account.py` `enter_shop` / `handle_shop`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShopItem {
    pub key: String,
    pub name: String,
    pub price: i64,
    pub owned: i64,
}

impl ShopItem {
    fn parse(v: &Value) -> ShopItem {
        let name = str_of(v, "Name");
        let key = str_of(v, "Key");
        ShopItem { key: key.clone(), name: if name.is_empty() { key } else { name }, price: i64_of(v, "Price"), owned: i64_of(v, "Owned") }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Shop {
    pub coin: i64,
    pub items: Vec<ShopItem>,
}

/// `GetShop` 的响应把 `Items`/`Coin` 放在顶层 (不是 `Data`), 兼容裸数组。
fn parse_shop(v: &Value) -> Shop {
    match v {
        Value::Object(_) => {
            let items = { let a = arr_of(v, "Items"); if a.is_empty() { arr_of(v, "Data") } else { a } };
            Shop { coin: i64_of(v, "Coin"), items: items.into_iter().map(ShopItem::parse).collect() }
        }
        Value::Array(items) => Shop { coin: 0, items: items.iter().map(ShopItem::parse).collect() },
        _ => Shop::default(),
    }
}

/// 商城商品 + 当前金币 (对照 `GetShop`); 需要登录。
pub fn load_shop(net: Option<&Client>) -> Result<Shop, String> {
    let v = if fake_mode() {
        load_fake("shop")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_shop().map_err(|e| e.to_string())?
    };
    Ok(parse_shop(&v))
}

/// 购买商品 (对照 `BuyShopItem`); 需要登录。
pub fn buy_shop_item(net: Option<&Client>, key: &str) -> Result<(), String> {
    if fake_mode() {
        return Ok(());
    }
    let net = net.ok_or("离线，无法购买")?;
    crate::net::ensure_login(net)?;
    net.buy_shop_item(key, 1).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 评论 (对照 Python `announcements.py` `render_comments`: `{Users, Commentaries, Data}`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comment {
    pub id: i64,
    pub user_name: String,
    pub content: String,
    pub created_at: String,
    pub replies: Vec<Comment>,
}

#[derive(Clone, Debug, Default)]
pub struct CommentsPage {
    pub items: Vec<Comment>,
    pub page: i64,
    pub total_pages: i64,
}

fn parse_comments(v: &Value, requested_page: i64) -> CommentsPage {
    let users = obj_of(v, "Users");
    let commentaries = obj_of(v, "Commentaries");
    let rows = arr_of(v, "Data");
    let items = rows
        .into_iter()
        .filter_map(|row| {
            let id = i64_of(row, "Id");
            let commentary = commentaries.get(id.to_string().as_str()).unwrap_or(row);
            // 每条顶层评论一个 seen 集合: 回复引用自己或互相引用 (服务器数据) 时不会无限递归
            let mut seen = std::collections::HashSet::from([id]);
            Some(comment_from_commentary_full(id, commentary, users, commentaries, 0, &mut seen))
        })
        .collect();
    let page = { let p = i64_of(v, "Page"); if p > 0 { p } else { requested_page.max(1) } };
    let total = { let t = i64_of(v, "TotalPages"); if t > 0 { t } else { page } };
    CommentsPage { items, page: page.max(1), total_pages: total.max(1) }
}

/// 回复的最大嵌套层数 (界面只画一层回复; 防止服务器数据里极深的回复链撑爆工作线程栈)。
const MAX_REPLY_DEPTH: usize = 8;

/// 同 [`comment_from_commentary`], 但 `Replies` 引用的子评论从 `commentaries` 映射解析 (支持递归)。
/// `seen` 记录这棵评论树里已出现的 id: 每条评论最多出现一次, 环引用被跳过。
fn comment_from_commentary_full(
    id: i64,
    commentary: &Value,
    users: &Value,
    commentaries: &Value,
    depth: usize,
    seen: &mut std::collections::HashSet<i64>,
) -> Comment {
    let user_id = i64_of(commentary, "UserId");
    let user = users.get(user_id.to_string().as_str());
    let user_name = user.map(|u| str_of(u, "UserName")).filter(|s| !s.is_empty()).unwrap_or_else(|| "用户".to_string());
    let mut replies = Vec::new();
    if depth < MAX_REPLY_DEPTH {
        for rid in arr_of(commentary, "Replies") {
            let Some(rid) = rid.as_i64().or_else(|| rid.as_str().and_then(|s| s.parse().ok())) else { continue };
            let Some(rc) = commentaries.get(rid.to_string().as_str()) else { continue };
            if seen.insert(rid) {
                replies.push(comment_from_commentary_full(rid, rc, users, commentaries, depth + 1, seen));
            }
        }
    }
    Comment { id, user_name, content: html_to_paragraphs(&str_of(commentary, "Content")).join("\n"), created_at: date_part(&str_of(commentary, "CreatedAt")), replies }
}

/// 评论列表 (对照 `GetComments`); Python 在跳转前要求已登录, 这里同样要求。
pub fn load_comments(net: Option<&Client>, comment_type: &str, target_id: i64, page: i64) -> Result<CommentsPage, String> {
    let v = if fake_mode() {
        load_fake(&format!("comments_{}_{}_p{}", comment_type.to_ascii_lowercase(), target_id, page))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_comments(comment_type, target_id, page).map_err(|e| e.to_string())?
    };
    Ok(parse_comments(&v, page))
}

// ---------------------------------------------------------------------------
// 系列 (对照 Python `series.py` 的接口分页分支)
// ---------------------------------------------------------------------------

/// 系列内的其它书籍 (对照 `GetBooksBySeries`)。
pub fn load_books_by_series(net: Option<&Client>, series_name: &str, page: i64, size: i64, filters: Filters) -> Result<ListPage, String> {
    let safe_name: String = series_name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let v = if fake_mode() {
        load_fake(&format!("series_{safe_name}_p{page}"))?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        net.get_books_by_series(series_name, page, size, "latest", filters.ignore_japanese, filters.ignore_ai).map_err(|e| e.to_string())?
    };
    Ok(parse_list_page(&v, page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_modes_follow_web_client() {
        assert_eq!(SearchMode::Exact.book_request("魔法"), ("GetBookList", "\"魔法\"".to_string()));
        assert_eq!(SearchMode::Fuzzy.book_request("魔法").0, "GetBookList");
        assert_eq!(SearchMode::Tags.book_request("百合,校园"), ("GetBookListByTags", "百合,校园".to_string()));
        assert_eq!(SearchMode::ALL.map(SearchMode::key), ["fuzzy", "exact", "title", "author", "name", "tags"]);
    }

    #[test]
    fn book_item_defaults_title_from_id() {
        let b = BookItem::parse(&json!({"Id": 7}));
        assert_eq!(b.title, "书籍 #7");
        assert_eq!(b.author, "");
        assert_eq!(b.views, 0);
    }

    #[test]
    fn book_item_reads_known_fields() {
        let b = BookItem::parse(&json!({
            "Id": 20644, "Title": "测试小说", "UserName": "某作者",
            "Cover": "https://x/a.jpg", "LastUpdatedAt": "2024-05-01T12:00:00",
            "LastUpdatedChapter": "第十章", "Views": 100, "Favorite": 5,
        }));
        assert_eq!(b.id, 20644);
        assert_eq!(b.title, "测试小说");
        assert_eq!(b.last_update, "2024-05-01");
        assert_eq!(b.last_chapter, "第十章");
        assert_eq!(b.views, 100);
    }

    #[test]
    fn list_page_parses_envelope_and_bare_array() {
        let envelope = json!({"Data": [{"Id": 1}, {"Id": 2}], "Page": 2, "TotalPages": 5});
        let p = parse_list_page(&envelope, 1);
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.page, 2);
        assert_eq!(p.total_pages, 5);

        let bare = json!([{"Id": 1}]);
        let p2 = parse_list_page(&bare, 3);
        assert_eq!(p2.items.len(), 1);
        assert_eq!(p2.page, 3);
    }

    #[test]
    fn book_item_badges() {
        let v = json!({"Id": 1, "Title": "t", "Level": 2, "InteriorLevel": 0,
            "Category": {"ShortName": "轻小说", "Name": "轻小说文库", "Color": "#fff"}});
        let b = BookItem::parse(&v);
        assert_eq!((b.category.as_str(), b.level, b.interior_level), ("轻小说", 2, 0));
        let b = BookItem::parse(&json!({"Id": 2, "Category": {"Name": "原创"}}));
        assert_eq!(b.category, "原创");
        assert_eq!(BookItem::parse(&json!({"Id": 3})).category, "");
        assert_eq!(BookItem::parse(&v).badge(), "轻小说 · Level 2");
        assert_eq!(BookItem::parse(&json!({"Id": 4, "Level": 1, "InteriorLevel": 5})).badge(), "Level 5");
        assert_eq!(BookItem::parse(&json!({"Id": 3})).badge(), "");
    }

    #[test]
    fn categories_parse_both_shapes() {
        let envelope = json!({"Data": [{"Id": 1, "Name": "玄幻"}]});
        assert_eq!(parse_categories(&envelope), vec![Category { id: 1, name: "玄幻".into() }]);
        let bare = json!([{"Id": 2, "Name": "都市"}]);
        assert_eq!(parse_categories(&bare), vec![Category { id: 2, name: "都市".into() }]);
    }

    fn shelf_fixture() -> ShelfData {
        let v = json!({"data": [
            {"id": 3, "type": "NOVEL", "index": 2, "parents": []},
            {"id": "f1", "type": "FOLDER", "index": 0, "parents": [], "title": "存档"},
            {"id": 5, "type": "COMIC", "index": 1, "parents": []},
            {"id": 9, "type": "NOVEL", "index": 0, "parents": ["f1"], "extra": "keep"},
            {"id": 7, "type": "NOVEL", "index": 3, "parents": []},
        ]});
        let book = |id: i64| (id, BookItem { id, title: format!("b{id}"), ..Default::default() });
        // 7 没有元数据 (例如已下架): 不显示, 但写回时保留
        ShelfData { raw: shelf_items(&v), books: [book(3), book(5), book(9)].into_iter().collect() }
    }

    #[test]
    fn shelf_levels_follow_last_parent_and_index() {
        let s = shelf_fixture();
        let root = s.level(None);
        assert_eq!(root.len(), 3);
        assert!(matches!(&root[0], ShelfEntry::Folder { id, title, count: 1 } if id == "f1" && title == "存档"));
        assert!(matches!(&root[1], ShelfEntry::Book(b) if b.id == 5));
        assert!(matches!(&root[2], ShelfEntry::Book(b) if b.id == 3));
        let inner = s.level(Some("f1"));
        assert!(matches!(inner.as_slice(), [ShelfEntry::Book(b)] if b.id == 9));
        assert_eq!(s.folder_title("f1"), "存档");
    }

    /// 嵌套文件夹的计数只算直接装着的项 (一遍计数与以前逐个扫描的结果一致)。
    #[test]
    fn shelf_folder_counts_are_direct_children() {
        let v = json!({"data": [
            {"id": "a", "type": "FOLDER", "index": 0, "parents": []},
            {"id": "b", "type": "FOLDER", "index": 0, "parents": ["a"]},
            {"id": 1, "type": "NOVEL", "index": 1, "parents": ["a"]},
            {"id": 2, "type": "NOVEL", "index": 0, "parents": ["a", "b"]},
            {"id": 3, "type": "NOVEL", "index": 1, "parents": ["a", "b"]},
            {"id": "c", "type": "FOLDER", "index": 1, "parents": []},
        ]});
        let s = ShelfData { raw: shelf_items(&v), books: Default::default() };
        let counts = |level: Vec<ShelfEntry>| level.into_iter().filter_map(|e| if let ShelfEntry::Folder { id, count, .. } = e { Some((id, count)) } else { None }).collect::<Vec<_>>();
        assert_eq!(counts(s.level(None)), vec![("a".to_string(), 2), ("c".to_string(), 0)]);
        assert_eq!(counts(s.level(Some("a"))), vec![("b".to_string(), 2)]);
    }

    /// 书目元数据按 id 缓存: 第二次进入书架只给新出现的书请求; 过期后重新请求; 写回后不再复用整张书架。
    #[test]
    fn shelf_metadata_is_cached_by_id() {
        use std::cell::RefCell;
        use std::time::{Duration, Instant};
        let cache = CacheLock::default();
        let t0 = Instant::now();
        let shelf = |ids: &[i64]| json!({"data": ids.iter().enumerate().map(|(i, id)| json!({"id": id, "type": "NOVEL", "index": i, "parents": []})).collect::<Vec<_>>()});
        let requested: RefCell<Vec<Vec<i64>>> = RefCell::new(Vec::new());
        // 服务器: 99 已下架 (不返回)
        let books = |ids: &[i64]| {
            requested.borrow_mut().push(ids.to_vec());
            Ok(ids.iter().filter(|&&id| id != 99).map(|&id| BookItem { id, title: format!("b{id}"), ..Default::default() }).collect())
        };
        let titles = |d: &ShelfData| d.level(None).into_iter().map(|e| if let ShelfEntry::Book(b) = e { b.title } else { String::new() }).collect::<Vec<_>>();

        let d = load_shelf_in(&cache, t0, || Ok(shelf(&[1, 2, 99])), books).unwrap();
        assert_eq!(titles(&d), ["b1", "b2"]);
        assert!(recent_shelf_in(&cache, t0 + Duration::from_secs(30), Duration::from_secs(60)).is_some());
        assert!(recent_shelf_in(&cache, t0 + Duration::from_secs(61), Duration::from_secs(60)).is_none());

        // 一分钟后: 新加了 3, 只请求 3 (99 记着没有, 不再请求)
        let d = load_shelf_in(&cache, t0 + Duration::from_secs(60), || Ok(shelf(&[3, 1, 2, 99])), books).unwrap();
        assert_eq!(titles(&d), ["b3", "b1", "b2"]);
        // 全都在缓存里: 不请求元数据
        load_shelf_in(&cache, t0 + Duration::from_secs(90), || Ok(shelf(&[3, 1])), books).unwrap();
        assert_eq!(*requested.borrow(), vec![vec![1, 2, 99], vec![3]]);

        // 过期: 1、2、99 重新请求, 3 还新鲜
        load_shelf_in(&cache, t0 + BOOK_META_TTL + Duration::from_secs(1), || Ok(shelf(&[1, 2, 3, 99])), books).unwrap();
        assert_eq!(requested.borrow().last().unwrap(), &vec![1, 2, 99]);

        // 写回之后: 不复用整张书架; 写回前发出、之后才完成的加载也不算数
        forget_shelf_in(&cache);
        assert!(recent_shelf_in(&cache, t0 + BOOK_META_TTL + Duration::from_secs(2), Duration::from_secs(60)).is_none());
        let now = t0 + BOOK_META_TTL + Duration::from_secs(3);
        load_shelf_in(&cache, now, || {
            forget_shelf_in(&cache);
            Ok(shelf(&[1]))
        }, books)
        .unwrap();
        assert!(recent_shelf_in(&cache, now, Duration::from_secs(60)).is_none());

        // 元数据请求失败: 整个加载失败 (与以前一样), 缓存不变
        let n = requested.borrow().len();
        assert!(load_shelf_in(&cache, now, || Ok(shelf(&[1, 4])), |_| Err("断网".to_string())).is_err());
        assert_eq!(requested.borrow().len(), n);
        assert!(load_shelf_in(&cache, now, || Err("断网".to_string()), books).is_err());
    }

    #[test]
    fn shelf_writes_touch_only_the_target() {
        let raw = shelf_fixture().raw;
        let removed = without_book(raw.clone(), 9);
        assert_eq!(removed.len(), 4);
        assert!(removed.iter().any(|it| it["id"] == 7));
        let added = with_book(removed.clone(), 11, true, "T");
        assert_eq!(added[0], json!({"id": 11, "type": "COMIC", "parents": [], "index": 0, "updateAt": "T"}));
        // 最外层其余条目往后挪一位, 文件夹里的不动
        let root_index = |v: &[Value], id: Value| v.iter().find(|it| it["id"] == id).unwrap()["index"].as_i64().unwrap();
        assert_eq!(root_index(&added, json!("f1")), root_index(&removed, json!("f1")) + 1);
        assert_eq!(with_book(raw.clone(), 9, false, "T"), raw);
        // 加入再移出: 每层编号回到 0..n
        let back = without_book(added, 11);
        assert_eq!(back.iter().filter(|it| parents_of(it).is_empty()).map(index_key).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        let unfoldered = without_folder(raw, "f1");
        assert_eq!(unfoldered.len(), 4);
        let nine = unfoldered.iter().find(|it| it["id"] == 9).unwrap();
        assert_eq!(nine["parents"], json!([]));
        assert_eq!(nine["extra"], "keep");
        // 文件夹里的书接到上一层末尾
        assert_eq!(nine["index"], 3);
        assert!(iso_now().ends_with(".000Z") && iso_now().len() == 24);
    }

    #[test]
    fn shelf_folder_create_rename_move_follow_the_web_client() {
        let raw = shelf_fixture().raw;
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // 新建: 排在这一层最前, 其余挤后; 同层重名 / 根文件夹 被拒绝
        let created = with_new_folder(raw.clone(), &[], "新的", "f2", "T").unwrap();
        let root: Vec<_> = created.iter().filter(|it| parents_of(it).is_empty()).map(|it| (it["id"].clone(), it["index"].as_i64().unwrap())).collect();
        assert_eq!(root[0], (json!("f2"), 0));
        assert_eq!(root.iter().map(|r| r.1).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert!(with_new_folder(raw.clone(), &[], " 存档 ", "x", "T").is_err());
        assert!(with_new_folder(raw.clone(), &[], "根文件夹", "x", "T").is_err());
        assert!(with_new_folder(raw.clone(), &p(&["f1"]), "存档", "x", "T").is_ok());
        // 重命名
        let renamed = renamed_folder(raw.clone(), "f1", "旧书").unwrap();
        assert_eq!(renamed.iter().find(|it| it["id"] == "f1").unwrap()["title"], "旧书");
        assert!(renamed_folder(raw.clone(), "f1", "存档").is_ok());
        // 移动书: 进入文件夹排第一, 原来的挤后
        let moved_book = moved(raw.clone(), &ShelfRef::Book(3), &p(&["f1"])).unwrap();
        let three = moved_book.iter().find(|it| it["id"] == 3).unwrap();
        assert_eq!((parents_of(three), three["index"].as_i64()), (p(&["f1"]), Some(0)));
        assert_eq!(moved_book.iter().find(|it| it["id"] == 9).unwrap()["index"], 1);
        // 移动文件夹: 下级路径一起改写; 不能移进自己
        let nested = with_new_folder(raw.clone(), &[], "外层", "f0", "T").unwrap();
        let moved_folder = moved(nested.clone(), &ShelfRef::Folder("f1".into()), &p(&["f0"])).unwrap();
        assert_eq!(parents_of(moved_folder.iter().find(|it| it["id"] == "f1").unwrap()), p(&["f0"]));
        assert_eq!(parents_of(moved_folder.iter().find(|it| it["id"] == 9).unwrap()), p(&["f0", "f1"]));
        assert!(moved(moved_folder.clone(), &ShelfRef::Folder("f0".into()), &p(&["f0", "f1"])).is_err());
        let data = ShelfData { raw: moved_folder, books: Default::default() };
        let tree: Vec<_> = data.folder_tree().into_iter().map(|n| n.path).collect();
        assert_eq!(tree, vec![p(&["f0"]), p(&["f0", "f1"])]);
        assert_eq!(data.parents_of(&ShelfRef::Book(9)), p(&["f0", "f1"]));
        // 不认识的字段原样保留
        assert_eq!(data.raw.iter().find(|it| it["id"] == 9).unwrap()["extra"], "keep");
        let id = new_folder_id();
        assert_eq!(id.len(), 21);
        assert_ne!(id, new_folder_id());
    }

    #[test]
    fn comic_list_and_content_parse() {
        let list = json!({"Data": [{"Id": 40001, "Title": "漫画甲", "Cover": "c", "Count": 12, "LastUpdatedAt": "2026-10-01T08:00:00"}], "TotalPages": 3});
        let page = parse_list_page_with(&list, 1, BookItem::parse_comic);
        assert_eq!(page.total_pages, 3);
        let item = &page.items[0];
        assert!(item.comic && item.count == 12 && item.last_update == "2026-10-01");
        assert_eq!(item.subtitle(), "漫画 · 12 话");

        let content = json!({
            "Chapter": {"Id": 7, "BookId": 40001, "BookName": "漫画甲 1", "Title": "第1话", "SortNum": 1, "Total": 9, "Skip": 6, "Images": ["a", "b", "c"]},
            "ReadPosition": {"ChapterId": 7, "Position": "4"}
        });
        let b = ComicBatch::parse(&content);
        assert_eq!((b.chapter_id, b.book_id, b.total, b.skip, b.images.len()), (7, 40001, 9, 6, 3));
        assert_eq!(b.read_position, Some((7, 4)));
        // 没有阅读位置; Total 缺失时至少覆盖本批
        let b = ComicBatch::parse(&json!({"Chapter": {"Id": 7, "Skip": 0, "Images": ["a", "b"]}, "ReadPosition": null}));
        assert_eq!((b.total, b.read_position), (2, None));
    }

    #[test]
    fn comic_book_info_parses_type_page_counts_and_series() {
        let v = json!({
            "SeriesTitle": "系列",
            "Series": [{"Id": 40001, "Title": "卷一", "Cover": "c1"}, {"Id": 40002, "Title": "卷二", "Cover": "c2"}],
            "Book": {"Id": 40001, "Type": "Comic", "Title": "卷一", "Chapters": [{"Id": 7, "SortNum": 1, "Title": "第1话", "PageCount": 9}]},
            "ReadPosition": {"ChapterId": 7, "Position": "3"}
        });
        let info = BookInfo::parse(&v);
        assert!(info.is_comic && info.book.comic);
        assert_eq!(info.chapters[0].page_count, 9);
        assert_eq!(info.series_books.len(), 2);
        assert!(info.series_books.iter().all(|b| b.comic));
        assert_eq!(info.read_position_chapter_id, 7);
        assert!(!BookInfo::parse(&json!({"Book": {"Id": 1, "Type": "Novel"}})).is_comic);
    }

    #[test]
    fn book_info_parses_book_series_and_chapters() {
        let v = json!({
            "Book": {
                "Id": 1, "Title": "书名", "Author": "作者甲",
                "Introduction": "<p>简介&nbsp;正文</p>",
                "Extra": {"classification": {"tags": ["玄幻", "热血"], "series_name_cn": "系列名"}},
                "Chapters": [{"Id": 11, "Title": "第一章", "SortNum": 1}, {"Id": 12, "Title": "第二章", "SortNum": 0}],
            },
            "SeriesTitle": "",
            "ReadPosition": {"ChapterId": 11},
        });
        let info = BookInfo::parse(&v);
        assert_eq!(info.book.title, "书名");
        assert_eq!(info.book.author, "作者甲");
        assert_eq!(info.intro, "简介 正文");
        assert_eq!(info.tags, vec!["玄幻".to_string(), "热血".to_string()]);
        assert_eq!(info.series_name, "系列名");
        assert_eq!(info.chapters.len(), 2);
        assert_eq!(info.chapters[1].sort_num, 2); // 缺 SortNum 时退回序号
        assert_eq!(info.read_position_chapter_id, 11);
    }

    #[test]
    fn my_info_parses_growth() {
        let v = json!({"UserName": "小明", "Level": 3, "Growth": {"Coin": 10, "SignStreak": 2, "TodaySigned": true}});
        let m = MyInfo::parse(&v);
        assert_eq!(m.user_name, "小明");
        assert_eq!(m.level, 3);
        assert_eq!(m.coin, 10);
        assert!(m.today_signed);
    }

    #[test]
    fn announcement_list_parses_envelope() {
        let v = json!({"Data": [{"Id": 1, "Title": "公告甲", "CreatedAt": "2024-05-01T00:00:00"}], "Page": 1, "TotalPages": 3});
        let p = parse_announcement_list(&v, 1);
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].title, "公告甲");
        assert_eq!(p.items[0].created_at, "2024-05-01");
        assert_eq!(p.total_pages, 3);
    }

    #[test]
    fn html_to_paragraphs_splits_on_br_and_p() {
        let html = "<p>第一段 &nbsp;文字</p><p>第二段<br/>换行</p>";
        let paras = html_to_paragraphs(html);
        assert_eq!(paras, vec!["第一段  文字".to_string(), "第二段\n换行".to_string()]);
    }

    /// 真实公告正文的结构 (2026-10): `<div>` 分块, `<b>` 小标题 + `<br />`, 源码带缩进换行。
    #[test]
    fn html_to_paragraphs_handles_div_blocks() {
        let html = "<div style=\"gap: 12px\">\r\n  <div>开头说明。</div>\r\n\r\n  <div>\r\n    <b>小标题</b><br />\r\n    正文第一行，\r\n    接着写。<br />\r\n    第二行 &amp; 更多\r\n  </div>\r\n<ul><li>甲</li><li>乙</li></ul></div>";
        assert_eq!(
            html_to_paragraphs(html),
            vec!["开头说明。", "小标题\n正文第一行， 接着写。\n第二行 & 更多", "• 甲", "• 乙"].into_iter().map(String::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn notification_parses_is_read() {
        let v = json!({"Id": 5, "Title": "标题", "Body": "正文", "IsRead": false, "CreatedAt": "2024-01-02"});
        let n = Notification::parse(&v);
        assert_eq!(n.id, 5);
        assert!(!n.is_read);
        assert_eq!(n.body, "正文");
    }

    #[test]
    fn shop_parses_items_and_coin_at_top_level() {
        let v = json!({"Coin": 42, "Items": [{"Key": "k1", "Name": "道具甲", "Price": 10, "Owned": 1}]});
        let shop = parse_shop(&v);
        assert_eq!(shop.coin, 42);
        assert_eq!(shop.items.len(), 1);
        assert_eq!(shop.items[0].name, "道具甲");
    }

    #[test]
    fn comments_resolve_user_names_and_nested_replies() {
        let v = json!({
            "Users": {"9": {"UserName": "甲"}, "10": {"UserName": "乙"}},
            "Commentaries": {
                "1": {"UserId": 9, "Content": "<p>评论内容</p>", "CreatedAt": "2024-01-01", "Replies": [2]},
                "2": {"UserId": 10, "Content": "回复内容", "CreatedAt": "2024-01-02"},
            },
            "Data": [{"Id": 1}],
        });
        let page = parse_comments(&v, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].user_name, "甲");
        assert_eq!(page.items[0].content, "评论内容");
        assert_eq!(page.items[0].replies.len(), 1);
        assert_eq!(page.items[0].replies[0].user_name, "乙");
    }

    #[test]
    fn comments_reply_to_itself_is_skipped() {
        let v = json!({
            "Users": {},
            "Commentaries": {"1": {"UserId": 1, "Content": "x", "Replies": [1, 2]}, "2": {"UserId": 1, "Content": "y", "Replies": ["2"]}},
            "Data": [{"Id": 1}],
        });
        let page = parse_comments(&v, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].replies.len(), 1);
        assert_eq!(page.items[0].replies[0].id, 2);
        assert!(page.items[0].replies[0].replies.is_empty());
    }

    #[test]
    fn comments_reply_cycle_terminates() {
        // A -> B -> A 互相引用: 每棵树里每条只出现一次
        let v = json!({
            "Users": {},
            "Commentaries": {"1": {"UserId": 1, "Content": "a", "Replies": [2]}, "2": {"UserId": 1, "Content": "b", "Replies": [1]}},
            "Data": [{"Id": 1}, {"Id": 2}],
        });
        let page = parse_comments(&v, 1);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].replies.len(), 1);
        assert!(page.items[0].replies[0].replies.is_empty());
        assert_eq!(page.items[1].id, 2);
        assert_eq!(page.items[1].replies[0].id, 1);
        assert!(page.items[1].replies[0].replies.is_empty());
    }

    #[test]
    fn comments_fallback_user_name_when_missing() {
        let v = json!({"Users": {}, "Commentaries": {"1": {"UserId": 1, "Content": "x"}}, "Data": [{"Id": 1}]});
        let page = parse_comments(&v, 1);
        assert_eq!(page.items[0].user_name, "用户");
    }
}
