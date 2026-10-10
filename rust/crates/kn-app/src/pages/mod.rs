//! 应用页面。页面状态都在各自结构体里 (见 kn-ui 文档), 版式规范见 rust/UI-DESIGN.md。
//!
//! 顶层是四个标签页 (书架 / 历史 / 发现 / 我的), 切换标签 = 以新页面为根 (`Transition::Root`);
//! 书籍、章节列表、阅读页等都是压栈进入的二级页面 (有返回、无标签栏)。

pub mod announce;
pub mod book;
pub mod comic;
pub mod comments;
pub mod discover;
pub mod history;
pub mod image;
pub mod light;
pub mod me;
pub mod notify;
pub mod reader;
pub mod series;
pub mod settings;
pub mod shelf;
pub mod shop;

use kn_render::Bitmap;
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, Transition};

use crate::KinNovel;

/// 标签页
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Shelf,
    History,
    Discover,
    Me,
}

const TABS: [(Tab, &str); 4] = [(Tab::Shelf, "书架"), (Tab::History, "历史"), (Tab::Discover, "发现"), (Tab::Me, "我的")];
/// 标签栏命中 id 起点 (页面自己的 id 都小于它)
pub const TAB_HIT_BASE: u32 = 900_000;

pub fn tab_page(tab: Tab) -> Box<dyn Page<KinNovel>> {
    match tab {
        Tab::Shelf => Box::new(shelf::ShelfPage::default()),
        Tab::History => Box::new(history::HistoryPage::default()),
        Tab::Discover => Box::new(discover::DiscoverPage::default()),
        Tab::Me => Box::new(me::MePage::default()),
    }
}

/// 画标签栏, 返回它的顶边 y。
pub fn draw_tabs(cx: &mut Cx<KinNovel>, frame: &mut Bitmap, active: Tab) -> i32 {
    let theme = cx.theme;
    let m = cx.app.metrics;
    let chain = cx.app.ui_fonts.clone();
    let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
    let labels: Vec<&str> = TABS.iter().map(|(_, l)| *l).collect();
    let index = TABS.iter().position(|(t, _)| *t == active).unwrap_or(0);
    widgets::tab_bar(&mut ink, frame, cx.hits, &theme, &m, &labels, index, TAB_HIT_BASE).y
}

/// 标签栏点击: 切到别的标签页; 点当前标签不动。
pub fn tab_transition(hit: HitId, active: Tab) -> Option<Transition<KinNovel>> {
    let i = hit.0.checked_sub(TAB_HIT_BASE)? as usize;
    let (tab, _) = *TABS.get(i)?;
    Some(if tab == active { Transition::None } else { Transition::Root(tab_page(tab)) })
}

/// 状态栏文字: 时间 · 电量。
pub fn status_text() -> String {
    let now = crate::store::unix_now();
    let local = now + crate::tz::local_utc_offset_secs(now);
    let minutes = local.rem_euclid(86_400) / 60;
    let mut s = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    if let Some(level) = battery_level() {
        s.push_str(&format!(" · {}%", level));
    }
    s
}

/// "刚刚 / 5 分钟前 / 3 小时前 / 昨天 / 10-08"
pub fn relative_time(then: i64) -> String {
    let now = crate::store::unix_now();
    let delta = (now - then).max(0);
    if delta < 60 {
        return "刚刚".into();
    }
    if delta < 3600 {
        return format!("{} 分钟前", delta / 60);
    }
    let offset = crate::tz::local_utc_offset_secs(now);
    let day = |t: i64| (t + offset).div_euclid(86_400);
    match day(now) - day(then) {
        0 => format!("{} 小时前", delta / 3600),
        1 => "昨天".into(),
        _ => {
            // 年月日 (civil from days, Howard Hinnant 算法)
            let z = day(then) + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            format!("{m:02}-{d:02}")
        }
    }
}

fn battery_level() -> Option<u32> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in dir.flatten() {
        if let Ok(text) = std::fs::read_to_string(entry.path().join("capacity")) {
            if let Ok(v) = text.trim().parse::<u32>() {
                if v <= 100 {
                    return Some(v);
                }
            }
        }
    }
    None
}
