//! 操作教程弹窗: 第一次用到某个页面时弹出该页的手势说明。两个按钮:
//! 「我知道了」= 不再提示 (记到 config `tips_seen`); 「感觉会忘记」= 这次先关掉, 下次启动 KinNovel 后
//! 再进这个页面时仍会提示 (只记在内存里)。设置里关掉「操作提示」则全部不弹; 重新打开会把记录清空。
//!
//! 页面接入: `render` 最后调 [`show`], `on_input` 最前面调 [`input`]。正在弹的提示记在
//! `KinNovel::tip_open` (同一时间只有一个页面在前台), 只有对应的页面会画它、被它吞掉输入。

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::Ink;
use kn_ui::{Cx, HitId, RefreshHint};

use crate::KinNovel;

pub const CONFIG_SEEN: &str = "tips_seen";
pub const CONFIG_ENABLED: &str = "show_tips";

const HIT_GOT_IT: HitId = HitId(u32::MAX - 1);
const HIT_LATER: HitId = HitId(u32::MAX - 2);

pub struct Tip {
    pub key: &'static str,
    pub title: &'static str,
    pub lines: &'static [&'static str],
}

pub const SHELF: Tip = Tip {
    key: "shelf",
    title: "书架",
    lines: &[
        "点书打开详情，点文件夹进入；左上角箭头回到上一层。",
        "长按书：移动到别的文件夹，或移出书架。",
        "长按文件夹：移动、重命名或删除（里面的书会移到上一层）。",
        "「＋ 新建文件夹」在当前这一层新建。",
        "列表上滑 / 下滑翻页，其他列表页也一样。",
    ],
};

pub const READER: Tip = Tip {
    key: "reader",
    title: "阅读",
    lines: &[
        "点屏幕右侧或左滑：下一页；点左侧或右滑：上一页。",
        "点屏幕中间或下滑：打开菜单（目录与书签、字号、亮度、设置），上滑收起。",
        "菜单顶栏的书签图标：给这一页加书签，再点一次删除。",
        "拖动菜单里的进度条：跳到本章任意一页。",
        "点插图：全屏查看并放大。",
        "点正文里的「注」图标：查看注释。",
    ],
};

pub const COMIC: Tip = Tip {
    key: "comic",
    title: "看漫画",
    lines: &[
        "点屏幕左右两侧或左右滑动翻页；从右往左读的漫画方向相反（菜单里可切换）。",
        "点屏幕中间或下滑：打开菜单（目录、放大、上一话 / 下一话、设置）。",
        "长按画面：放大查看这一页。",
    ],
};

pub const KEYBOARD: Tip = Tip {
    key: "keyboard",
    title: "屏幕键盘",
    lines: &[
        "「中 / 英」键切换拼音和英文。",
        "打拼音后点候选词输入；按空格、回车或标点输入第一个候选，候选多时按 ← → 翻看。",
        "「符号」键切换到标点符号，Shift 换一页；在 ， 。 键上向上滑输入 ； ：",
        "长按退格清空输入，长按 Shift 锁定大写。",
    ],
};

pub const BOOKMARKS: Tip = Tip {
    key: "bookmarks",
    title: "目录与书签",
    lines: &["点章节或书签直接跳过去。", "在「书签」里长按一个书签可以删除它。"],
};

pub const IMAGE: Tip = Tip {
    key: "image",
    title: "查看插图",
    lines: &["用底部的 ＋ － 放大缩小，放大后上下左右滑动查看其它部分。", "「原图」下载未压缩的图片。", "轻触图片显示 / 隐藏工具栏。"],
};

/// 这个提示现在该不该弹: 设置里开着、没点过「我知道了」、这次运行也没点过「感觉会忘记」。
fn pending(app: &KinNovel, key: &str) -> bool {
    app.config.bool(CONFIG_ENABLED, true) && !app.tips_later.contains(key) && !app.config.strings(CONFIG_SEEN).iter().any(|k| k == key)
}

/// 在 `render` 最后调用: `when` 为真且该弹时弹出, 弹出中的这个提示画在最上层。
pub fn show(cx: &mut Cx<KinNovel>, frame: &mut Bitmap, tip: &'static Tip, when: bool) {
    if when && cx.app.tip_open != Some(tip.key) && pending(cx.app, tip.key) {
        cx.app.tip_open = Some(tip.key);
    }
    if cx.app.tip_open == Some(tip.key) {
        draw(cx, frame, tip);
    }
}

/// 在 `on_input` 最前面调用: 这个提示正弹着时处理并吞掉输入 (返回 true)。
pub fn input(cx: &mut Cx<KinNovel>, event: &InputEvent, tip: &'static Tip) -> bool {
    if cx.app.tip_open != Some(tip.key) {
        return false;
    }
    if let InputEvent::Gesture(g) = event {
        if g.kind == GestureKind::Tap {
            match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
                Some(HIT_GOT_IT) => {
                    let mut seen = cx.app.config.strings(CONFIG_SEEN);
                    if !seen.iter().any(|k| k == tip.key) {
                        seen.push(tip.key.to_string());
                    }
                    cx.app.config.set(CONFIG_SEEN, serde_json::Value::from(seen));
                    close(cx);
                }
                Some(HIT_LATER) => {
                    cx.app.tips_later.insert(tip.key);
                    close(cx);
                }
                _ => {}
            }
        }
    }
    true
}

fn close(cx: &mut Cx<KinNovel>) {
    cx.app.tip_open = None;
    // 提示关闭按普通 UI 恢复, 避免关闭说明时整屏闪烁
    cx.request_ui_redraw(RefreshHint::Flash);
}

/// 居中卡片: 标题、逐条说明 (自动折行)、底部两个按钮; 先登记一个吞掉框外点击的全屏区域。
fn draw(cx: &mut Cx<KinNovel>, frame: &mut Bitmap, tip: &Tip) {
    let (theme, m) = (cx.theme, cx.app.metrics);
    let chain = cx.app.ui_fonts.clone();
    let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
    let (fw, fh) = (frame.width(), frame.height());
    let w = ((fw as f32 * 0.88) as u32).min(fw - m.margin);
    let pad = m.margin as i32;
    let inner = w as i32 - 2 * pad;
    let bullet = "·  ";
    let bullet_w = ink.width(bullet, m.body).ceil() as i32;
    let line_h = (m.body * 1.45) as i32;
    let para_gap = (m.body * 0.45) as i32;
    let wrapped: Vec<Vec<String>> = tip.lines.iter().map(|l| super::announce::wrap_all(l, &ink, m.body, (inner - bullet_w) as f32)).collect();
    let text_h: i32 = wrapped.iter().map(|p| p.len() as i32 * line_h + para_gap).sum();
    let title_h = (m.title * 1.6) as i32;
    let btn_h = m.touch as i32;
    let h = pad + title_h + pad / 3 + text_h + pad / 2 + btn_h + pad;
    let rect = Rect::new((fw - w) as i32 / 2, ((fh as i32 - h) / 2).max(0), w, h as u32);
    cx.hits.add(HitId(u32::MAX), frame.bounds()).no_feedback();
    frame.rounded_rect(rect, m.radius, Some(theme.background), Some(theme.foreground), 3);
    let mut y = rect.y + pad;
    ink.text_centered(frame, Rect::new(rect.x + pad, y, inner as u32, title_h as u32), &format!("操作提示 · {}", tip.title), m.title, theme.foreground);
    y += title_h + pad / 3;
    for para in &wrapped {
        for (i, line) in para.iter().enumerate() {
            if i == 0 {
                ink.text(frame, rect.x + pad, y, bullet, m.body, theme.foreground);
            }
            ink.text(frame, rect.x + pad + bullet_w, y, line, m.body, theme.foreground);
            y += line_h;
        }
        y += para_gap;
    }
    y += pad / 2;
    let gap = pad / 2;
    let bw = ((inner - gap) / 2).max(0) as u32;
    let buttons = [(HIT_LATER, "感觉会忘记", false), (HIT_GOT_IT, "我知道了", true)];
    for (i, (id, label, primary)) in buttons.into_iter().enumerate() {
        let r = Rect::new(rect.x + pad + i as i32 * (bw as i32 + gap), y, bw, btn_h as u32);
        let (fill, text) = if primary { (theme.foreground, theme.background) } else { (theme.background, theme.foreground) };
        frame.rounded_rect(r, m.radius, Some(fill), Some(theme.foreground), 2);
        ink.text_centered(frame, r, label, m.body, text);
        cx.hits.add(id, r).rounded(m.radius);
    }
}
