//! 设置 (从 "我的" 进入): 分三组 —— 阅读 / 显示 / 内容, 顶部分段切换, 任何屏幕尺寸都放得下。
//! 每次改动即时写回 config.json (与 Python 版共用同一份配置与键名)。

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};
use serde_json::Value;

use crate::KinNovel;

#[derive(Clone, Copy)]
enum Kind {
    /// 数值: 最小、最大、步长、小数位
    Step { min: f64, max: f64, step: f64, decimals: usize },
    Toggle { default: bool },
    /// 字符串取值循环切换: (配置值, 显示名); 配置值 "" 写成 null (Python 的 None)
    Choice(&'static [(&'static str, &'static str)]),
}

struct Setting {
    key: &'static str,
    label: &'static str,
    note: &'static str,
    kind: Kind,
    default: f64,
}

const fn step(key: &'static str, label: &'static str, note: &'static str, min: f64, max: f64, step: f64, decimals: usize, default: f64) -> Setting {
    Setting { key, label, note, kind: Kind::Step { min, max, step, decimals }, default }
}

const fn toggle(key: &'static str, label: &'static str, note: &'static str, default: bool) -> Setting {
    Setting { key, label, note, kind: Kind::Toggle { default }, default: 0.0 }
}

const CONVERT: &[(&str, &str)] = &[("", "关闭"), ("t2s", "繁转简"), ("s2t", "简转繁")];

const GROUPS: &[(&str, &[Setting])] = &[
    (
        "阅读",
        &[
            step("font_size", "正文字号", "", 28.0, 80.0, 4.0, 0, 48.0),
            step("line_spacing", "行距", "", 1.2, 2.2, 0.1, 1, 1.42),
            step("reader_margin", "页边距", "", 16.0, 96.0, 8.0, 0, 34.0),
            toggle("first_line_indent", "首行缩进", "", true),
            Setting { key: "convert", label: "简繁转换", note: "", kind: Kind::Choice(CONVERT), default: 0.0 },
            toggle("prefetch_chapters", "预加载前后章节", "", false),
        ],
    ),
    (
        "显示",
        &[
            toggle("night_mode", "夜间模式", "", false),
            toggle("page_turn_animation", "翻页动画", "PW5 及更新机型", true),
            toggle("page_flash", "翻页全屏刷新", "重启后生效", false),
            step("full_refresh_every", "残影清理间隔 (屏)", "重启后生效", 2.0, 12.0, 1.0, 0, 6.0),
        ],
    ),
    (
        "内容",
        &[
            toggle("ignore_japanese", "忽略日文书籍", "", false),
            toggle("ignore_ai", "忽略 AI 翻译", "", false),
            step("cache_limit_mb", "缓存上限 (MB)", "启动时清理", 64.0, 1024.0, 64.0, 0, 192.0),
        ],
    ),
];

const HIT_BACK: HitId = HitId(1);
const GROUP_BASE: u32 = 10;
const SET_BASE: u32 = 100;

#[derive(Default)]
pub struct SettingsPage {
    group: usize,
}

fn number(config: &crate::store::Config, s: &Setting) -> f64 {
    match s.kind {
        Kind::Toggle { default } => config.bool(s.key, default) as u8 as f64,
        Kind::Step { .. } => config.float(s.key, s.default),
        Kind::Choice(options) => {
            let v = config.string(s.key, "");
            options.iter().position(|(value, _)| *value == v).unwrap_or(0) as f64
        }
    }
}

impl SettingsPage {
    fn change(&mut self, cx: &mut Cx<KinNovel>, s: &Setting, delta: i32) {
        let v = number(&cx.app.config, s);
        match s.kind {
            Kind::Toggle { .. } => {
                let on = v == 0.0;
                cx.app.config.set(s.key, Value::from(on));
                if s.key == "night_mode" {
                    cx.app.night = on;
                    cx.request_redraw(RefreshHint::Flash);
                    return;
                }
            }
            Kind::Step { min, max, step, decimals } => {
                let scale = 10f64.powi(decimals as i32);
                let next = (((v + step * delta as f64) * scale).round() / scale).clamp(min, max);
                if next == v {
                    return;
                }
                let value = if decimals == 0 { Value::from(next as i64) } else { Value::from(next) };
                cx.app.config.set(s.key, value);
            }
            Kind::Choice(options) => {
                let (value, _) = options[(v as usize + 1) % options.len()];
                cx.app.config.set(s.key, if value.is_empty() { Value::Null } else { Value::from(value) });
            }
        }
        cx.request_redraw(RefreshHint::Ui);
    }
}

impl Page<KinNovel> for SettingsPage {
    fn id(&self) -> PageId {
        PageId(131)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "设置", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let side = m.margin as i32;
        let mut y = bar.bottom() + side / 3;
        let names: Vec<&str> = GROUPS.iter().map(|(name, _)| *name).collect();
        let seg = Rect::new(side, y, cx.width - 2 * m.margin, m.touch);
        widgets::segmented(&mut ink, frame, cx.hits, &theme, &m, seg, &names, self.group, GROUP_BASE);
        y = seg.bottom() + side / 2;

        let row_h = (m.touch as f32 * 1.15) as i32;
        let btn = m.touch as i32;
        for (i, s) in GROUPS[self.group].1.iter().enumerate() {
            let r = Rect::new(0, y, cx.width, row_h as u32);
            let label_y = r.y + (row_h - (m.body * 1.25) as i32) / 2;
            let label_w = ink.width(s.label, m.body).ceil() as i32;
            ink.text(frame, side, label_y, s.label, m.body, theme.foreground);
            if !s.note.is_empty() {
                ink.text(frame, side + label_w + side / 3, r.y + (row_h - (m.tiny * 1.25) as i32) / 2, s.note, m.tiny, theme.muted);
            }
            let v = number(&cx.app.config, s);
            let id = |k: u32| HitId(SET_BASE + i as u32 * 4 + k);
            // 开关/选项: 右侧一个宽按钮, 整行可点
            let wide = |label: &str, on: bool, ink: &mut Ink, frame: &mut Bitmap| {
                let sw = Rect::new(cx.width as i32 - side - btn * 2, r.y + (row_h - btn * 2 / 3) / 2, (btn * 2) as u32, (btn * 2 / 3) as u32);
                widgets::button(ink, frame, &theme, &m, sw, label, if on { ButtonStyle::Primary } else { ButtonStyle::Secondary });
            };
            match s.kind {
                Kind::Step { min, max, decimals, .. } => {
                    let plus = Rect::new(cx.width as i32 - side - btn, r.y + (row_h - btn) / 2, btn as u32, btn as u32);
                    let value_w = (m.body * 2.6) as i32;
                    let value_r = Rect::new(plus.x - value_w, r.y, value_w as u32, row_h as u32);
                    let minus = Rect::new(value_r.x - btn, plus.y, btn as u32, btn as u32);
                    let style = |enabled: bool| if enabled { ButtonStyle::Secondary } else { ButtonStyle::Disabled };
                    widgets::button(&mut ink, frame, &theme, &m, minus, "－", style(v > min));
                    ink.text_centered(frame, value_r, &format!("{v:.decimals$}"), m.body, theme.foreground);
                    widgets::button(&mut ink, frame, &theme, &m, plus, "＋", style(v < max));
                    cx.hits.add(id(0), minus).enabled(v > min);
                    cx.hits.add(id(1), plus).enabled(v < max);
                }
                Kind::Toggle { .. } => {
                    let on = v != 0.0;
                    wide(if on { "开" } else { "关" }, on, &mut ink, frame);
                    cx.hits.add(id(2), r);
                }
                Kind::Choice(options) => {
                    let index = v as usize;
                    wide(options[index].1, index != 0, &mut ink, frame);
                    cx.hits.add(id(2), r);
                }
            }
            frame.fill_rect(Rect::new(side, r.bottom() - 1, cx.width - 2 * m.margin, 1), theme.mid);
            y += row_h;
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        match hit {
            HIT_BACK => return Transition::Back,
            HitId(id) if (GROUP_BASE..GROUP_BASE + GROUPS.len() as u32).contains(&id) => {
                let group = (id - GROUP_BASE) as usize;
                if group != self.group {
                    self.group = group;
                    // 换分组相当于换页: 闪刷, 否则下方空白处的旧残影不会被刷到 (差分只刷变化的像素)
                    cx.request_redraw(RefreshHint::Flash);
                }
            }
            HitId(id) if id >= SET_BASE => {
                let settings = GROUPS[self.group].1;
                let index = ((id - SET_BASE) / 4) as usize;
                if let Some(s) = settings.get(index) {
                    let delta = if (id - SET_BASE) % 4 == 0 { -1 } else { 1 };
                    self.change(cx, s, delta);
                }
            }
            _ => {}
        }
        Transition::None
    }
}
