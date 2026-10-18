//! 检查更新 / 卸载 (KUAL 菜单「KinNovel 工具箱」的两项, 「检查更新」也可以从「我的 → 关于」进来)。
//!
//! 更新: 检查 → 显示新版本的说明 → 下载 (多个来源, 见 [`crate::update`]) → 装进扩展目录 → 「立即重启」
//! (程序退出, 启动脚本看到 /tmp/kinnovel_restart 后用新版重新启动)。
//! 卸载: 说明会删掉什么, 再确认一次 → 写 /tmp/kinnovel_uninstall 后退出, 启动脚本恢复系统界面后删除整个扩展目录。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Backdrop, Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::update::{self, Release};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PRIMARY: HitId = HitId(2);
const HIT_SECONDARY: HitId = HitId(3);
const HIT_CONFIRM: HitId = HitId(4);
const HIT_CANCEL: HitId = HitId(5);
const HIT_NOTES_PREV: HitId = HitId(6);
const HIT_NOTES_NEXT: HitId = HitId(7);

/// 从哪里打开的: KUAL 菜单 (这是首页, 「退出」回系统) 还是应用内 (「返回」)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Kual,
    App,
}

enum State {
    Checking,
    Failed(String),
    Latest(Release),
    Available(Release),
    /// 正在下载
    Downloading(Release, update::DownloadProgress),
    Installed(String),
}

struct Checked(Result<Release, String>);
struct Progress(update::DownloadProgress);
struct Done(Result<String, String>);

pub struct UpdatePage {
    origin: Origin,
    state: State,
    notes_page: usize,
}

impl UpdatePage {
    pub fn new(origin: Origin) -> Self {
        UpdatePage { origin, state: State::Checking, notes_page: 0 }
    }

    fn check(&mut self, cx: &mut Cx<KinNovel>) {
        self.state = State::Checking;
        let (paths, net) = (cx.app.paths.clone(), cx.app.net());
        cx.spawn(move || Checked(update::check_release(&paths, net.as_ref())));
    }

    fn download(&mut self, cx: &mut Cx<KinNovel>, release: Release) {
        let Some(net) = cx.app.net() else { return };
        let dir = update::program_dir(&cx.app.paths);
        let poster = cx.page_poster();
        self.state = State::Downloading(release.clone(), update::DownloadProgress::default());
        cx.spawn(move || {
            let result = update::download(&net, &release, |p| poster.post(Progress(p.clone())))
                .and_then(|zip| update::install(&zip, &release.version, &dir))
                .map(|n| {
                    eprintln!("[update] 已安装 {} ({n} 个文件) 到 {}", release.version, dir.display());
                    release.version.clone()
                });
            Done(result)
        });
    }

    fn leave_label(&self) -> &'static str {
        match self.origin {
            Origin::Kual => "退出",
            Origin::App => "返回",
        }
    }

    fn leave_transition(&self) -> Transition<KinNovel> {
        match self.origin {
            Origin::Kual => Transition::Exit,
            Origin::App => Transition::Back,
        }
    }
}

/// 发行说明 (Markdown) → 纯文本行 (文字, 是否小标题): 去掉井号与粗体星号, 列表项用「·」; 标题后不空行。
/// 「安装」「卸载」小节 (写给手动安装的人) 在程序里不显示。
fn plain_notes(notes: &str) -> Vec<(String, bool)> {
    let mut after_heading = false;
    let mut in_install = false;
    notes
        .lines()
        .filter(|l| {
            if l.starts_with('#') {
                let title = l.trim_start_matches('#').trim();
                in_install = title.starts_with("安装") || title.starts_with("卸载");
            }
            !in_install
        })
        .filter(|l| {
            let skip = after_heading && l.trim().is_empty();
            if !skip {
                after_heading = l.starts_with('#');
            }
            !skip
        })
        .map(str::trim_end)
        .filter(|l| !l.to_ascii_lowercase().starts_with("sha256"))
        .map(|l| {
            let heading = l.starts_with('#');
            let l = l.trim_start_matches('#').trim_start();
            let l = match l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
                Some(rest) => format!("· {rest}"),
                None => l.to_string(),
            };
            (l.replace("**", "").replace('`', ""), heading)
        })
        .fold(Vec::new(), |mut out: Vec<(String, bool)>, l| {
            // 连续空行只留一个
            if !(l.0.is_empty() && out.last().is_none_or(|p| p.0.is_empty())) {
                out.push(l);
            }
            out
        })
}

impl Page<KinNovel> for UpdatePage {
    fn id(&self) -> PageId {
        PageId(190)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let back = self.origin == Origin::App;
        let bar = widgets::header(&mut ink, frame, &theme, &m, "检查更新", &super::status_text(), back, false);
        if back {
            cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        }
        let side = m.margin as i32;
        let width = cx.width as i32 - 2 * side;
        let mut y = bar.bottom() + m.margin as i32 / 2;

        // 版本卡片
        let row_h = (m.body * 1.7) as i32;
        let card = Rect::new(side, y, width as u32, (row_h * 2 + m.margin as i32 / 2) as u32);
        widgets::card(frame, &theme, &m, card);
        let pad = m.margin as i32 / 2;
        let text_y = |i: i32| card.y + pad / 2 + i * row_h + (row_h - (m.body * 1.25) as i32) / 2;
        ink.text(frame, card.x + pad, text_y(0), "当前版本", m.body, theme.muted);
        let current = env!("CARGO_PKG_VERSION");
        let cw = ink.width(current, m.body).ceil() as i32;
        ink.text(frame, card.right() - pad - cw, text_y(0), current, m.body, theme.foreground);
        ink.text(frame, card.x + pad, text_y(1), "最新版本", m.body, theme.muted);
        let latest = match &self.state {
            State::Checking => "正在检查…".to_string(),
            State::Failed(_) => "检查失败".to_string(),
            State::Latest(r) | State::Available(r) | State::Downloading(r, _) => format!("{} · 来自 {}", r.version, r.source),
            State::Installed(v) => v.clone(),
        };
        let lw = ink.width(&latest, m.body).ceil() as i32;
        ink.text(frame, card.right() - pad - lw, text_y(1), &latest, m.body, theme.foreground);
        y = card.bottom() + m.margin as i32;

        // 底部按钮 (一到两个)
        let btn_h = m.touch as i32;
        let btn_y = cx.height as i32 - m.margin as i32 - btn_h;
        let leave = self.leave_label();
        let buttons: Vec<(String, HitId, ButtonStyle)> = match &self.state {
            State::Checking => vec![(leave.into(), HIT_SECONDARY, ButtonStyle::Secondary)],
            State::Failed(_) => vec![(leave.into(), HIT_SECONDARY, ButtonStyle::Secondary), ("重试".into(), HIT_PRIMARY, ButtonStyle::Primary)],
            State::Latest(_) => match self.origin {
                Origin::Kual => vec![("退出".into(), HIT_SECONDARY, ButtonStyle::Secondary), ("打开 KinNovel".into(), HIT_PRIMARY, ButtonStyle::Primary)],
                Origin::App => vec![("返回".into(), HIT_SECONDARY, ButtonStyle::Primary)],
            },
            State::Available(_) => vec![("以后再说".into(), HIT_SECONDARY, ButtonStyle::Secondary), ("下载并安装".into(), HIT_PRIMARY, ButtonStyle::Primary)],
            State::Downloading(..) => vec![("正在下载…".into(), HitId(0), ButtonStyle::Disabled)],
            State::Installed(_) => vec![("立即重启".into(), HIT_PRIMARY, ButtonStyle::Primary)],
        };
        let gap = m.margin as i32 / 2;
        let n = buttons.len() as i32;
        let bw = (width - gap * (n - 1)) / n;
        for (i, (label, id, style)) in buttons.iter().enumerate() {
            let r = Rect::new(side + i as i32 * (bw + gap), btn_y, bw as u32, btn_h as u32);
            widgets::button(&mut ink, frame, &theme, &m, r, label, *style);
            if id.0 != 0 {
                cx.hits.add(*id, r).rounded(m.radius);
            }
        }

        // 中间: 状态说明 / 发行说明
        let area_bottom = btn_y - m.margin as i32 / 2;
        let line_h = (m.small * 1.6) as i32;
        let mut lines: Vec<String> = Vec::new();
        match &self.state {
            State::Checking => lines.push("正在获取版本信息…".into()),
            State::Failed(e) => {
                lines.push("连不上更新服务器, 请检查网络后重试。".into());
                lines.extend(super::announce::wrap_all(e, &ink, m.small, width as f32));
            }
            State::Latest(_) => lines.push("已经是最新版本。".into()),
            State::Installed(v) => {
                lines.push(format!("已安装 {v}, 重启后生效。"));
                lines.push("设置、账号、缓存和阅读进度都保留。".into());
            }
            State::Downloading(r, p) => {
                let mb = |b: usize| b as f32 / (1024.0 * 1024.0);
                lines.push(match (p.host.is_empty(), p.total) {
                    (true, _) => "正在下载…".to_string(),
                    (false, Some(total)) if total > 0 => format!("正在从 {} 下载… {:.1} / {:.1} MB", p.host, mb(p.received), mb(total)),
                    (false, _) if p.received > 0 => format!("正在从 {} 下载… {:.1} MB", p.host, mb(p.received)),
                    (false, _) => format!("正在连接 {}…", p.host),
                });
                if r.sha256.is_some() {
                    lines.push("下载后会校验文件完整性。".into());
                }
            }
            State::Available(_) => {}
        }
        if let State::Available(r) = &self.state {
            // (文字, 样式): 0 = 正文, 1 = 版本标题, 2 = 灰字
            let mut wrapped: Vec<(String, u8)> = Vec::new();
            if r.changes.len() > 1 {
                let text = format!("从 {} 更新到 {}, 包含 {} 个版本的改动:", env!("CARGO_PKG_VERSION"), r.version, r.changes.len());
                wrapped.extend(super::announce::wrap_all(&text, &ink, m.small, width as f32).into_iter().map(|l| (l, 2)));
                wrapped.push((String::new(), 0));
            }
            for (i, change) in r.changes.iter().enumerate() {
                if i > 0 {
                    wrapped.push((String::new(), 0));
                }
                let title = if change.date.is_empty() { format!("v{}", change.version) } else { format!("v{} · {}", change.version, change.date) };
                wrapped.push((title, 1));
                for (l, heading) in plain_notes(&change.notes) {
                    if l.is_empty() {
                        wrapped.push((String::new(), 0));
                    } else {
                        let kind = if heading { 2 } else { 0 };
                        wrapped.extend(super::announce::wrap_all(&l, &ink, m.small, width as f32).into_iter().map(|w| (w, kind)));
                    }
                }
            }
            // 分页时页首不留空行
            let wrapped: Vec<(String, u8)> = wrapped.into_iter().fold(Vec::new(), |mut out, l| {
                if !(l.0.is_empty() && out.last().is_none_or(|p: &(String, u8)| p.0.is_empty())) {
                    out.push(l);
                }
                out
            });
            let per_page = ((area_bottom - y - m.touch as i32) / line_h).max(1) as usize;
            let pages = wrapped.len().div_ceil(per_page).max(1);
            self.notes_page = self.notes_page.min(pages - 1);
            for (l, kind) in wrapped.iter().skip(self.notes_page * per_page).take(per_page) {
                match kind {
                    1 => ink.text(frame, side, y + ((m.small - m.body) * 0.6) as i32, l, m.body, theme.foreground),
                    2 => ink.text(frame, side, y, l, m.small, theme.muted),
                    _ => ink.text(frame, side, y, l, m.small, theme.foreground),
                };
                y += line_h;
            }
            if pages > 1 {
                let r = Rect::new(side, area_bottom - m.touch as i32, width as u32, m.touch);
                widgets::pager(&mut ink, frame, cx.hits, &theme, &m, r, self.notes_page, pages, HIT_NOTES_PREV, HIT_NOTES_NEXT);
            }
        } else {
            for l in &lines {
                let t = ink.fit(l, m.small, width as f32);
                ink.text(frame, side, y, &t, m.small, if l.starts_with('·') { theme.foreground } else { theme.muted });
                y += line_h;
            }
        }
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning && matches!(self.state, State::Checking) {
            self.check(cx);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Checked>() {
            Ok(checked) => {
                self.state = match checked.0 {
                    Ok(r) if update::newer_than_current(&r.version).is_some() => State::Available(r),
                    Ok(r) => State::Latest(r),
                    Err(e) => State::Failed(e),
                };
                self.notes_page = 0;
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        let msg = match msg.downcast::<Progress>() {
            Ok(p) => {
                if let State::Downloading(_, progress) = &mut self.state {
                    *progress = p.0;
                    cx.request_redraw(RefreshHint::Ui);
                }
                return;
            }
            Err(other) => other,
        };
        if let Ok(done) = msg.downcast::<Done>() {
            self.state = match done.0 {
                Ok(v) => State::Installed(v),
                Err(e) => State::Failed(format!("安装失败: {e}")),
            };
            cx.request_redraw(RefreshHint::Clean);
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
            HIT_NOTES_PREV | HIT_NOTES_NEXT => {
                self.notes_page = if hit == HIT_NOTES_PREV { self.notes_page.saturating_sub(1) } else { self.notes_page + 1 };
                cx.request_redraw(RefreshHint::Ui);
            }
            HIT_SECONDARY => {
                if matches!(self.state, State::Downloading(..) | State::Installed(_)) {
                    return Transition::None;
                }
                return self.leave_transition();
            }
            HIT_PRIMARY => match &self.state {
                State::Failed(_) => {
                    self.check(cx);
                    cx.request_redraw(RefreshHint::Ui);
                }
                State::Latest(_) => return Transition::Root(super::tab_page(super::Tab::Shelf)),
                State::Available(r) => {
                    let r = r.clone();
                    self.download(cx, r);
                    cx.request_redraw(RefreshHint::Ui);
                }
                State::Installed(_) => {
                    if let Err(e) = std::fs::write(update::RESTART_MARKER, b"1") {
                        eprintln!("[update] {}: {e}", update::RESTART_MARKER);
                    }
                    return Transition::Exit;
                }
                _ => {}
            },
            _ => {}
        }
        Transition::None
    }
}

/// 卸载确认页。
#[derive(Default)]
pub struct UninstallPage {
    confirming: bool,
}

impl Page<KinNovel> for UninstallPage {
    fn id(&self) -> PageId {
        PageId(191)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "卸载 KinNovel", &super::status_text(), false, false);
        let side = m.margin as i32;
        let width = cx.width as i32 - 2 * side;
        let mut y = bar.bottom() + m.margin as i32;
        let line_h = (m.body * 1.7) as i32;
        let dir = update::program_dir(&cx.app.paths);
        let text = [
            "卸载会删除 KinNovel 的程序和它保存在本机的全部数据:".to_string(),
            "· 设置、登录的账号".to_string(),
            "· 章节、字体、图片和漫画缓存".to_string(),
            "· 本机的阅读进度、书签和阅读统计".to_string(),
            String::new(),
            "轻书架服务器上的书架和阅读进度不受影响。".to_string(),
            format!("目录: {}", dir.display()),
        ];
        for (i, l) in text.iter().enumerate() {
            if l.is_empty() {
                y += line_h / 2;
                continue;
            }
            for w in super::announce::wrap_all(l, &ink, m.body, width as f32) {
                ink.text(frame, side, y, &w, m.body, if i == text.len() - 1 { theme.muted } else { theme.foreground });
                y += line_h;
            }
        }
        let btn_h = m.touch as i32;
        let btn_y = cx.height as i32 - m.margin as i32 - btn_h;
        let gap = m.margin as i32 / 2;
        let bw = (width - gap) / 2;
        let cancel = Rect::new(side, btn_y, bw as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, cancel, "取消", ButtonStyle::Secondary);
        cx.hits.add(HIT_CANCEL, cancel).rounded(m.radius);
        let go = Rect::new(side + bw + gap, btn_y, bw as u32, btn_h as u32);
        widgets::button(&mut ink, frame, &theme, &m, go, "卸载", ButtonStyle::Primary);
        cx.hits.add(HIT_PRIMARY, go).rounded(m.radius);
        if self.confirming {
            widgets::dialog(
                &mut ink,
                frame,
                cx.hits,
                &theme,
                &m,
                "确定卸载？",
                &["删除后无法恢复"],
                &[("取消", HIT_CANCEL, ButtonStyle::Secondary), ("卸载", HIT_CONFIRM, ButtonStyle::Primary)],
            );
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        match hit {
            HIT_CANCEL if self.confirming => {
                self.confirming = false;
                cx.request_redraw(RefreshHint::Clean);
            }
            HIT_CANCEL => return Transition::Exit,
            HIT_PRIMARY => {
                self.confirming = true;
                cx.request_redraw(RefreshHint::Ui);
            }
            HIT_CONFIRM => {
                let dir = update::program_dir(&cx.app.paths);
                match std::fs::write(update::UNINSTALL_MARKER, dir.to_string_lossy().as_bytes()) {
                    Ok(()) => {
                        eprintln!("[uninstall] 退出后删除 {}", dir.display());
                        return Transition::Exit;
                    }
                    Err(e) => eprintln!("[uninstall] {}: {e}", update::UNINSTALL_MARKER),
                }
            }
            _ => {}
        }
        Transition::None
    }
}

const HIT_PROMPT_LATER: HitId = HitId(8);
const HIT_PROMPT_VIEW: HitId = HitId(9);

/// 启动检查发现新版本时弹的提示 (盖在书架等首页类页面上, 背景就是下面那页, 只刷新弹窗)。每个版本只弹一次。
pub struct UpdatePrompt {
    version: String,
    backdrop: Option<Bitmap>,
    backdrop_set: bool,
}

impl UpdatePrompt {
    pub fn new(version: String) -> Self {
        UpdatePrompt { version, backdrop: None, backdrop_set: false }
    }
}

impl Page<KinNovel> for UpdatePrompt {
    fn id(&self) -> PageId {
        PageId(192)
    }

    fn flash_on_enter(&self) -> bool {
        false
    }

    fn wants_backdrop(&self) -> Backdrop {
        if self.backdrop_set { Backdrop::None } else { Backdrop::Below }
    }

    fn set_backdrop(&mut self, backdrop: Option<Bitmap>) {
        self.backdrop = backdrop;
        self.backdrop_set = true;
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        if let Some(bg) = self.backdrop.as_ref().filter(|bg| bg.width() == cx.width && bg.height() == cx.height) {
            frame.blit(bg, bg.bounds(), 0, 0);
        }
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let current = format!("当前版本 {}", env!("CARGO_PKG_VERSION"));
        widgets::dialog(
            &mut ink,
            frame,
            cx.hits,
            &theme,
            &m,
            &format!("KinNovel {} 已发布", self.version),
            &[current.as_str(), "以后也可在「我的 → 关于」里更新"],
            &[("以后再说", HIT_PROMPT_LATER, ButtonStyle::Secondary), ("查看更新", HIT_PROMPT_VIEW, ButtonStyle::Primary)],
        );
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
            Some(HIT_PROMPT_LATER) => {
                // 下面那页重画后弹窗区域局部闪刷, 不留深色按钮的残影
                cx.request_redraw(RefreshHint::Clean);
                Transition::Back
            }
            // 更新页会重新检查一次, 装的是那时最新的版本
            Some(HIT_PROMPT_VIEW) => Transition::Replace(Box::new(UpdatePage::new(Origin::App))),
            _ => Transition::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_drop_install_sections() {
        let notes = "## 新功能

- **徽章**: 灰框

## 安装

下载 zip

卸载时删除

## 改进
- 更快
SHA256: abc
";
        let lines: Vec<(String, bool)> = plain_notes(notes);
        let texts: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, vec!["新功能", "· 徽章: 灰框", "", "改进", "· 更快"]);
        assert!(lines[0].1 && !lines[1].1 && lines[3].1);
    }
}
