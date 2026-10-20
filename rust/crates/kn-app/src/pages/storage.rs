//! 「我的 → 存储空间」: 各类缓存占用多少, 可以分别清理。
//!
//! 阅读进度、书签、登录会话、配置不在这里 (不是缓存, 不能清)。大小在后台统计 (vfat 上遍历几千个文件要几百毫秒)。

use std::any::Any;
use std::path::PathBuf;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::store::Paths;
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_CLEAR_ALL: HitId = HitId(2);
const HIT_CONFIRM: HitId = HitId(3);
const HIT_CANCEL: HitId = HitId(4);
const CLEAR_BASE: u32 = 100;

/// 缓存类别: (名称, 说明, 目录 [相对 cache/])
const KINDS: &[(&str, &str, &[&str])] = &[
    ("章节", "清理后离线时读不了这些章节", &["content"]),
    ("章节字体", "清理后打开章节会重新下载", &["fonts"]),
    ("插图与封面", "", &["covers", "images"]),
    ("漫画", "漫画页面图片与目录", &["comic"]),
];

struct Sized(Vec<u64>);
struct Cleared(Vec<u64>);

#[derive(Default)]
pub struct StoragePage {
    sizes: Option<Vec<u64>>,
    busy: bool,
    /// 等待确认的清理: Some(Some(i)) = 第 i 类, Some(None) = 全部
    confirm: Option<Option<usize>>,
}

fn dirs(paths: &Paths, kind: usize) -> Vec<PathBuf> {
    KINDS[kind].2.iter().map(|d| paths.cache_dir().join(d)).collect()
}

fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn measure(paths: &Paths) -> Vec<u64> {
    (0..KINDS.len()).map(|k| dirs(paths, k).iter().map(|d| dir_size(d)).sum()).collect()
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 * 1024 => format!("{:.1} GB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{} KB", b / 1024),
        b => format!("{b} B"),
    }
}

impl StoragePage {
    fn measure(&mut self, cx: &mut Cx<KinNovel>) {
        self.busy = true;
        let paths = cx.app.paths.clone();
        cx.spawn_local_droppable(move || Sized(measure(&paths)));
    }

    fn clear(&mut self, cx: &mut Cx<KinNovel>, which: Option<usize>) {
        self.busy = true;
        let paths = cx.app.paths.clone();
        cx.spawn_local(move || {
            let kinds: Vec<usize> = match which {
                Some(k) => vec![k],
                None => (0..KINDS.len()).collect(),
            };
            for k in kinds {
                for d in dirs(&paths, k) {
                    if let Err(e) = std::fs::remove_dir_all(&d) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            eprintln!("[storage] 清理 {} 失败: {e}", d.display());
                        }
                    }
                }
            }
            Cleared(measure(&paths))
        });
    }
}

impl Page<KinNovel> for StoragePage {
    fn id(&self) -> PageId {
        PageId(135)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        self.measure(cx);
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let sizes = match msg.downcast::<Sized>() {
            Ok(s) => s.0,
            Err(other) => match other.downcast::<Cleared>() {
                Ok(c) => {
                    eprintln!("[storage] 已清理");
                    c.0
                }
                Err(_) => return,
            },
        };
        self.sizes = Some(sizes);
        self.busy = false;
        cx.request_redraw(RefreshHint::Ui);
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "存储空间", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let side = m.margin as i32;
        let mut y = bar.bottom() + side / 2;
        let Some(sizes) = &self.sizes else {
            let area = Rect::new(0, y, cx.width, (cx.height as i32 - y) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "正在统计…", None);
            return;
        };
        let total: u64 = sizes.iter().sum();
        let limit = cx.app.config.int("cache_limit_mb", 192);
        let comic = cx.app.config.int("comic_cache_mb", 256);
        ink.text(frame, side, y, &format!("缓存共 {}", human(total)), m.title, theme.foreground);
        y += (m.title * 1.5) as i32;
        let note = format!("启动时自动清理到上限: 小说 {limit} MB · 漫画 {comic} MB");
        let note = ink.fit(&note, m.small, (cx.width as i32 - 2 * side) as f32);
        ink.text(frame, side, y, &note, m.small, theme.muted);
        y += (m.small * 1.8) as i32;
        frame.fill_rect(Rect::new(side, y, cx.width - 2 * m.margin, 2), theme.foreground);

        let row_h = m.row_h() as i32;
        let btn_w = (m.small * 5.0) as i32;
        let btn_h = (m.touch as f32 * 0.8) as i32;
        for (i, ((name, desc, _), size)) in KINDS.iter().zip(sizes).enumerate() {
            let r = Rect::new(0, y, cx.width, row_h as u32);
            let b = Rect::new(cx.width as i32 - side - btn_w, y + (row_h - btn_h) / 2, btn_w as u32, btn_h as u32);
            let row = Rect::new(0, y, (b.x - side / 2).max(0) as u32, row_h as u32);
            widgets::list_row(&mut ink, frame, &theme, &m, row, name, desc, &human(*size));
            frame.fill_rect(Rect::new(side, r.bottom() - 1, cx.width - 2 * m.margin, 1), theme.mid);
            let enabled = *size > 0 && !self.busy;
            widgets::button(&mut ink, frame, &theme, &m, b, "清理", if enabled { ButtonStyle::Secondary } else { ButtonStyle::Disabled });
            cx.hits.add(HitId(CLEAR_BASE + i as u32), b).rounded(m.radius).enabled(enabled);
            y += row_h;
        }
        y += side;
        let all = Rect::new(side, y, cx.width - 2 * m.margin, m.touch);
        let enabled = total > 0 && !self.busy;
        widgets::button(&mut ink, frame, &theme, &m, all, if self.busy { "处理中…" } else { "全部清理" }, if enabled { ButtonStyle::Primary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_CLEAR_ALL, all).rounded(m.radius).enabled(enabled);
        y = all.bottom() + side / 2;
        let keep = "阅读进度、书签、登录状态和设置不会被清理";
        ink.text_centered(frame, Rect::new(side, y, cx.width - 2 * m.margin, (m.small * 1.6) as u32), keep, m.small, theme.muted);

        if let Some(which) = self.confirm {
            let (title, line) = match which {
                Some(k) => (format!("清理「{}」？", KINDS[k].0), KINDS[k].1),
                None => ("清理全部缓存？".to_string(), "离线时将无法阅读已缓存的章节"),
            };
            let lines: Vec<&str> = if line.is_empty() { Vec::new() } else { vec![line] };
            widgets::dialog(&mut ink, frame, cx.hits, &theme, &m, &title, &lines, &[("取消", HIT_CANCEL, ButtonStyle::Secondary), ("清理", HIT_CONFIRM, ButtonStyle::Primary)]);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let hit = cx.hits.at(Point { x: g.start.0, y: g.start.1 });
        if let Some(which) = self.confirm {
            match hit {
                Some(HIT_CONFIRM) => {
                    self.confirm = None;
                    self.clear(cx, which);
                    cx.request_redraw(RefreshHint::Clean);
                }
                Some(HIT_CANCEL) => {
                    self.confirm = None;
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        match hit {
            Some(HIT_BACK) => Transition::Back,
            Some(HIT_CLEAR_ALL) => {
                self.confirm = Some(None);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            Some(HitId(id)) if (CLEAR_BASE..CLEAR_BASE + KINDS.len() as u32).contains(&id) => {
                self.confirm = Some(Some((id - CLEAR_BASE) as usize));
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            _ => Transition::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(2048), "2 KB");
        assert_eq!(human(5 * 1024 * 1024 + 512 * 1024), "5.5 MB");
    }
}
