//! 休眠屏保: 显示最近在读的书的封面 (设置「休眠显示封面」, 重启后生效)。
//!
//! 做法同 KOReader (frontend/device/kindle/device.lua): 启动脚本在暂停框架前卸载系统屏保模块
//! (`lipc-set-prop com.lab126.blanket unload screensaver`, 有广告的机型不卸载), 退出时再加载回来,
//! 并用环境变量 `KN_SLEEP_COVER=1` 告诉程序; 进入屏保时运行时渲染 [`SleepPage`] 并整屏闪刷。
//!
//! 封面地址: 书目接口解析时顺手记下 (书 id → 封面 URL, 只在内存); 打开阅读页时把当前书的地址写进
//! `data/sleep_cover.json`, 并在后台把大尺寸封面下载进缓存 (休眠时不联网)。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use kn_platform::InputEvent;
use kn_render::{Bitmap, Rect, HEIGHT_BUCKETS};
use kn_ui::widgets::Ink;
use kn_ui::{Cx, Page, PageId, Transition};
use serde_json::{json, Value};

use crate::store::{self, atomic_write, Paths};
use crate::KinNovel;

static COVERS: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();

fn covers() -> &'static Mutex<HashMap<i64, String>> {
    COVERS.get_or_init(Default::default)
}

/// 启动脚本卸载了系统屏保。
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("KN_SLEEP_COVER").as_deref() == Ok("1"))
}

/// 记下一本书的封面地址 (书目接口解析时调用)。
pub fn note_cover(book_id: i64, url: &str) {
    if book_id > 0 && !url.is_empty() {
        covers().lock().unwrap_or_else(|e| e.into_inner()).insert(book_id, url.to_string());
    }
}

/// 书目接口里见过的封面地址。
pub fn known_cover(book_id: i64) -> Option<String> {
    covers().lock().unwrap_or_else(|e| e.into_inner()).get(&book_id).cloned()
}

fn cover_file(paths: &Paths) -> std::path::PathBuf {
    paths.data_dir().join("sleep_cover.json")
}

/// 封面在屏保里请求的高度 (缓存档位按它取)。
fn cover_height(screen_h: u32) -> u32 {
    screen_h * 4 / 5
}

/// 保存的 (书 id, 封面 URL)。
fn saved(paths: &Paths) -> Option<(i64, String)> {
    let v: Value = serde_json::from_slice(&std::fs::read(cover_file(paths)).ok()?).ok()?;
    Some((v.get("book_id")?.as_i64()?, v.get("url")?.as_str()?.to_string()))
}

/// 打开一本书时调用: 记下它的封面并在后台下载大图 (未开启封面屏保时什么都不做)。
pub fn on_book_opened(paths: &Paths, net: Option<kn_net::Client>, book_id: i64, screen_h: u32) {
    if !enabled() {
        return;
    }
    let url = covers().lock().unwrap_or_else(|e| e.into_inner()).get(&book_id).cloned();
    let url = match url {
        Some(u) => u,
        None => match saved(paths) {
            Some((id, u)) if id == book_id => u,
            _ => return,
        },
    };
    if saved(paths).as_ref() != Some(&(book_id, url.clone())) {
        if let Err(e) = atomic_write(&cover_file(paths), json!({"book_id": book_id, "url": url}).to_string().as_bytes()) {
            eprintln!("[sleep] 无法保存封面地址: {e}");
        }
    }
    let paths = paths.clone();
    std::thread::spawn(move || {
        if let Err(e) = crate::net::image_bytes(&paths, net.as_ref(), &url, cover_height(screen_h)) {
            eprintln!("[sleep] 封面下载失败: {e}");
        }
    });
}

/// 缓存里最大的一份封面 (先试屏保档位, 再往小找)。
fn cached_cover(paths: &Paths, url: &str, screen_h: u32) -> Option<Vec<u8>> {
    let want = kn_render::height_bucket(cover_height(screen_h));
    let mut buckets: Vec<u32> = HEIGHT_BUCKETS.iter().copied().filter(|&b| b <= want).collect();
    buckets.reverse();
    buckets.into_iter().find_map(|h| crate::net::image_bytes(paths, None, url, h).ok().flatten())
}

/// 屏保页: 封面 (没有就是书名卡片) + 书名、章节、进度。
#[derive(Default)]
pub struct SleepPage;

impl Page<KinNovel> for SleepPage {
    fn id(&self) -> PageId {
        PageId(900)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let (w, h) = (cx.width as i32, cx.height as i32);
        let side = m.margin as i32;
        let last = store::load_last_read(&cx.app.paths);
        let cover = last.as_ref().and_then(|l| {
            let (id, url) = saved(&cx.app.paths)?;
            (id == l.book_id).then_some(url)
        });
        let image = cover.and_then(|url| cached_cover(&cx.app.paths, &url, cx.height)).and_then(|bytes| {
            let (bw, bh) = ((w - 2 * side) as u32, cover_height(cx.height));
            kn_render::decode_gray(&bytes, Some((bw, bh))).ok().map(|img| img.fit_within(bw, bh).dithered())
        });
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };

        // 底部信息区
        let info_h = (m.title * 1.5 + m.small * 1.6 * 2.0) as i32 + side;
        let info_top = h - info_h;
        let area = Rect::new(side, side, (w - 2 * side) as u32, (info_top - side * 3 / 2).max(0) as u32);
        match &image {
            Some(img) => {
                let x = area.x + (area.w as i32 - img.width() as i32) / 2;
                let y = area.y + (area.h as i32 - img.height() as i32) / 2;
                frame.blit(img, img.bounds(), x, y);
                frame.rounded_rect(Rect::new(x, y, img.width(), img.height()).inflate(1), 0, None, Some(theme.foreground), 1);
            }
            None => {
                // 没有封面: 书名卡片 (或者只有应用名)
                let card = Rect::new(area.x + area.w as i32 / 6, area.y + area.h as i32 / 8, area.w * 2 / 3, area.h * 3 / 4);
                frame.rounded_rect(card, m.radius, Some(theme.background), Some(theme.foreground), 3);
                let title = last.as_ref().map(|l| l.book_name.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "KinNovel".into());
                let size = m.title * 1.3;
                let mut lines = Vec::new();
                let mut rest: Vec<char> = title.chars().collect();
                let max_w = (card.w as i32 - 2 * side) as f32;
                while !rest.is_empty() && lines.len() < 5 {
                    let mut n = rest.len();
                    while n > 1 && ink.width(&rest[..n].iter().collect::<String>(), size) > max_w {
                        n -= 1;
                    }
                    lines.push(rest.drain(..n).collect::<String>());
                }
                let line_h = (size * 1.5) as i32;
                let mut y = card.y + (card.h as i32 - line_h * lines.len() as i32) / 2;
                for line in &lines {
                    ink.text_centered(frame, Rect::new(card.x, y, card.w, line_h as u32), line, size, theme.foreground);
                    y += line_h;
                }
            }
        }

        frame.fill_rect(Rect::new(side, info_top, (w - 2 * side) as u32, 2), theme.foreground);
        let mut y = info_top + side / 2;
        if let Some(l) = &last {
            let name = ink.fit(&l.book_name, m.title, (w - 2 * side) as f32);
            ink.text(frame, side, y, &name, m.title, theme.foreground);
            y += (m.title * 1.5) as i32;
            let chapter = if l.comic {
                let t = if l.chapter_title.is_empty() { format!("第 {} 话", l.sort_num) } else { l.chapter_title.clone() };
                format!("{t} · 第 {} / {} 页", l.page + 1, l.pages.max(1))
            } else {
                format!("第 {} 章 · {}", l.sort_num, l.chapter_title)
            };
            let chapter = ink.fit(&chapter, m.small, (w - 2 * side) as f32);
            ink.text(frame, side, y, &chapter, m.small, theme.foreground);
        }
        let label = "KinNovel · 休眠中";
        let lw = ink.width(label, m.small).ceil() as i32;
        ink.text(frame, w - side - lw, h - side / 2 - (m.small * 1.3) as i32, label, m.small, theme.muted);
    }

    fn on_input(&mut self, _cx: &mut Cx<KinNovel>, _event: &InputEvent) -> Transition<KinNovel> {
        Transition::None
    }
}
