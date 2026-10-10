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
use serde_json::Value;

fn fake_dir() -> Option<PathBuf> {
    std::env::var_os("KN_FAKE_DIR").map(PathBuf::from)
}

/// 是否处于 fixture 模式 (页面用它判断"离线但有数据可读"与"真离线"的区别)。
pub fn fake_mode() -> bool {
    fake_dir().is_some()
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

fn arr_of<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    get(v, key).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

/// 把 "2024-01-02T03:04:05" / "2024-01-02 03:04:05" 之类的时间字符串截成 "2024-01-02"
/// 之外的部分一并显示用不到, 列表行里只需要日期。空输入原样返回空串。
fn date_part(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 10 && s.as_bytes()[4] == b'-' {
        s[..10].to_string()
    } else {
        s.to_string()
    }
}

/// 粗暴的 HTML 标签剥除 (简介文字), 对照 Python `book.py` 的 `_clean_intro`
/// (`re.sub(r"<[^>]+>", " ", value)` + 替换 `&nbsp;`)。
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").split_whitespace().collect::<Vec<_>>().join(" ")
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
}

impl BookItem {
    pub fn parse(v: &Value) -> BookItem {
        let id = i64_of(v, "Id");
        let title = str_of(v, "Title");
        BookItem {
            id,
            title: if title.is_empty() { format!("书籍 #{id}") } else { title },
            author: str_of(v, "UserName"),
            cover: str_of(v, "Cover"),
            last_update: date_part(&str_of(v, "LastUpdatedAt")),
            last_chapter: str_of(v, "LastUpdatedChapter"),
            views: i64_of(v, "Views"),
            favorite: i64_of(v, "Favorite"),
        }
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
            ListPage { items: data.into_iter().map(BookItem::parse).collect(), page: page.max(1), total_pages: total.max(1) }
        }
        Value::Array(items) => ListPage { items: items.iter().map(BookItem::parse).collect(), page: requested_page.max(1), total_pages: requested_page.max(1) },
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
}

impl BookInfo {
    pub fn parse(v: &Value) -> BookInfo {
        let book_v = obj_of(v, "Book");
        let id = i64_of(book_v, "Id");
        let title = str_of(book_v, "Title");
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
                ChapterRef { id: i64_of(c, "Id"), sort_num: if sort_num > 0 { sort_num } else { i as i64 + 1 }, title: str_of(c, "Title") }
            })
            .collect();
        BookInfo {
            book,
            intro: strip_html(&str_of(book_v, "Introduction")),
            tags,
            series_name,
            chapters,
            read_position_chapter_id: i64_of(obj_of(v, "ReadPosition"), "ChapterId"),
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

/// 书架原始条目 -> 按 index 排序、去掉文件夹与漫画、去重后的书籍 id 列表
/// (不支持文件夹嵌套导航 —— 1.0 的书架只展示一层, 文件夹内的书一并列出)。
fn shelf_book_ids(v: &Value) -> Vec<i64> {
    let items: Vec<&Value> = match v {
        Value::Object(_) => {
            let d = arr_of(v, "data");
            if d.is_empty() { arr_of(v, "Data") } else { d }
        }
        Value::Array(items) => items.iter().collect(),
        _ => Vec::new(),
    };
    let mut entries: Vec<(i64, i64)> = Vec::new(); // (index, id)
    let mut seen = std::collections::HashSet::new();
    for item in items {
        if !item.is_object() || is_kind(item, "folder") || is_kind(item, "comic") {
            continue;
        }
        let id = i64_of(item, "id").max(i64_of(item, "Id"));
        if id <= 0 || !seen.insert(id) {
            continue;
        }
        entries.push((index_key(item), id));
    }
    entries.sort_by_key(|(i, _)| *i);
    entries.into_iter().map(|(_, id)| id).collect()
}

// ---------------------------------------------------------------------------
// 加载函数 (fixture 优先, 否则走 `net`; `net` 为 None 时离线返回错误)
// ---------------------------------------------------------------------------

/// 书架 (已与书籍元数据合并; 顺序 = 书架里的顺序)。只请求当前实际存在的 id。
pub fn load_book_shelf(net: Option<&Client>) -> Result<Vec<BookItem>, String> {
    let envelope = if fake_mode() {
        load_fake("book_shelf")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_book_shelf().map_err(|e| e.to_string())?
    };
    let ids = shelf_book_ids(&envelope);
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let books = load_books_by_ids(net, &ids)?;
    let by_id: std::collections::HashMap<i64, BookItem> = books.into_iter().map(|b| (b.id, b)).collect();
    Ok(ids.into_iter().filter_map(|id| by_id.get(&id).cloned()).collect())
}

/// 阅读历史的全部 id (最新在前, 对照 `GetReadHistory().Novel`)。
pub fn load_history_ids(net: Option<&Client>) -> Result<Vec<i64>, String> {
    let envelope = if fake_mode() {
        load_fake("read_history")?
    } else {
        let net = net.ok_or("离线，无法加载")?;
        crate::net::ensure_login(net)?;
        net.get_read_history().map_err(|e| e.to_string())?
    };
    Ok(arr_of(&envelope, "Novel").into_iter().filter_map(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).collect())
}

/// 按 id 批量取元数据 (服务器单次最多 24 本, 由 `kn-net` 分块; fixture 模式下按
/// `books_by_ids.json` 里出现的顺序过滤)。
pub fn load_books_by_ids(net: Option<&Client>, ids: &[i64]) -> Result<Vec<BookItem>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if fake_mode() {
        let all = load_fake("books_by_ids")?;
        let by_id: std::collections::HashMap<i64, BookItem> =
            all.as_array().into_iter().flatten().map(BookItem::parse).map(|b| (b.id, b)).collect();
        return Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect());
    }
    let net = net.ok_or("离线，无法加载")?;
    let values = net.get_book_list_by_ids_chunked(ids, Some("Novel"), 24).map_err(|e| e.to_string())?;
    Ok(BookItem::list_from(&values))
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

/// 把 HTML 粗暴转成纯文本段落: `<br>` -> 换行, `</p>` -> 段落分隔, 其它标签去掉,
/// 解码 `&nbsp;`/`&amp;` (对照 Python `announcements.py` `render_detail`)。
fn html_to_paragraphs(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' => {
                in_tag = false;
                let t = tag.to_ascii_lowercase();
                if t.starts_with("br") {
                    text.push('\n');
                } else if t.starts_with("/p") || t.starts_with("p ") || t == "p" {
                    text.push_str("\n\n");
                }
            }
            _ if in_tag => tag.push(ch),
            _ => text.push(ch),
        }
    }
    let text = text.replace("&nbsp;", " ").replace("&amp;", "&");
    text.split("\n\n")
        .map(|p| p.split('\n').map(str::trim).collect::<Vec<_>>().join(" ").trim().to_string())
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
            Some(comment_from_commentary_full(id, commentary, users, commentaries))
        })
        .collect();
    let page = { let p = i64_of(v, "Page"); if p > 0 { p } else { requested_page.max(1) } };
    let total = { let t = i64_of(v, "TotalPages"); if t > 0 { t } else { page } };
    CommentsPage { items, page: page.max(1), total_pages: total.max(1) }
}

/// 同 [`comment_from_commentary`], 但 `Replies` 引用的子评论从 `commentaries` 映射解析 (支持递归)。
fn comment_from_commentary_full(id: i64, commentary: &Value, users: &Value, commentaries: &Value) -> Comment {
    let user_id = i64_of(commentary, "UserId");
    let user = users.get(user_id.to_string().as_str());
    let user_name = user.map(|u| str_of(u, "UserName")).filter(|s| !s.is_empty()).unwrap_or_else(|| "用户".to_string());
    let replies = arr_of(commentary, "Replies")
        .into_iter()
        .filter_map(|rid| rid.as_i64().or_else(|| rid.as_str().and_then(|s| s.parse().ok())))
        .filter_map(|rid| commentaries.get(rid.to_string().as_str()).map(|rc| comment_from_commentary_full(rid, rc, users, commentaries)))
        .collect();
    Comment { id, user_name, content: strip_html(&str_of(commentary, "Content")), created_at: date_part(&str_of(commentary, "CreatedAt")), replies }
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
    fn categories_parse_both_shapes() {
        let envelope = json!({"Data": [{"Id": 1, "Name": "玄幻"}]});
        assert_eq!(parse_categories(&envelope), vec![Category { id: 1, name: "玄幻".into() }]);
        let bare = json!([{"Id": 2, "Name": "都市"}]);
        assert_eq!(parse_categories(&bare), vec![Category { id: 2, name: "都市".into() }]);
    }

    #[test]
    fn shelf_book_ids_skip_folders_comics_and_sort_by_index() {
        let v = json!({"data": [
            {"id": 3, "type": "NOVEL", "index": 2},
            {"id": "folder1", "type": "FOLDER", "index": 0},
            {"id": 5, "type": "COMIC", "index": 1},
            {"id": 9, "type": "NOVEL", "index": 0},
        ]});
        assert_eq!(shelf_book_ids(&v), vec![9, 3]);
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
        assert_eq!(paras, vec!["第一段  文字".to_string(), "第二段 换行".to_string()]);
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
    fn comments_fallback_user_name_when_missing() {
        let v = json!({"Users": {}, "Commentaries": {"1": {"UserId": 1, "Content": "x"}}, "Data": [{"Id": 1}]});
        let page = parse_comments(&v, 1);
        assert_eq!(page.items[0].user_name, "用户");
    }
}
