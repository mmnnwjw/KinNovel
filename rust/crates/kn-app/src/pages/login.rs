//! 登录页。两种入口:
//! - 欢迎页: 联网启动、没有登录、也没点过「先逛逛」时代替书架作为首页 (`KinNovel::home_page`)。
//!   登录成功或「先逛逛」后以书架为根。
//! - 普通登录页: 「我的」账号卡片、需要登录的页面 (历史、评论、通知、商城) 的「登录」按钮压栈进入, 成功后返回。
//!
//! 版面固定: 标题、说明、邮箱、密码、登录按钮始终在上半屏; 点输入框时下半屏换成键盘
//! (`TextEditor::render_keys`), 收起时那里是「先逛逛」和注册说明。打字只刷新输入框和按下的键。
//! 邮箱、密码默认英文键盘; 密码默认显示成圆点, 旁边的「显示」可切换。
//! 登录成功后只保存密码的 SHA-256 (见 `crate::account`)。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::keyboard::{EditorEvent, TextEditor};
use kn_ui::widgets::{self, ButtonStyle, Ink, Metrics};
use kn_ui::{Cx, HitId, Hits, Page, PageId, RefreshHint, Theme, Transition};
use serde_json::Value;

use crate::account::Account;
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_EMAIL: HitId = HitId(2);
const HIT_PASSWORD: HitId = HitId(3);
const HIT_SHOW: HitId = HitId(4);
const HIT_LOGIN: HitId = HitId(5);
const HIT_SKIP: HitId = HitId(6);
const EMAIL_BASE: u32 = 5000;
const PASSWORD_BASE: u32 = 5300;

/// 注册只能在网页上做 (需要邮箱验证码)
const SITE: &str = "www.lightnovel.life";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Email,
    Password,
}

struct LoginDone(Result<Account, String>);

pub struct LoginPage {
    welcome: bool,
    email: TextEditor,
    password: TextEditor,
    /// 正在输入的框 (键盘打开)
    focus: Option<Field>,
    logging_in: bool,
    message: Option<String>,
    /// 键盘顶边 (上次绘制); 点它上面的空白处收起键盘
    keys_top: i32,
    /// 登录成功或「先逛逛」后的导航 (`take_transition`)
    done: Option<Transition<KinNovel>>,
}

impl LoginPage {
    fn build(welcome: bool) -> Self {
        let mut email = TextEditor::new(EMAIL_BASE, "");
        email.set_chinese(false);
        let mut password = TextEditor::new(PASSWORD_BASE, "");
        password.set_chinese(false);
        password.set_masked(true);
        LoginPage { welcome, email, password, focus: None, logging_in: false, message: None, keys_top: i32::MAX, done: None }
    }

    /// 启动时的欢迎页 (栈底)。
    pub fn welcome() -> Self {
        Self::build(true)
    }

    /// 从其它页面进入, 登录后返回。
    pub fn new() -> Self {
        Self::build(false)
    }

    /// 保存的账号被服务器拒绝 (密码改了等) 后弹出: 邮箱已填好。
    pub fn rejected(email: &str) -> Self {
        let mut page = Self::build(false);
        page.email.set_text(email);
        page.message = Some("保存的账号无法登录（密码可能已修改），请重新登录".into());
        page
    }

    fn editor(&mut self, field: Field) -> &mut TextEditor {
        match field {
            Field::Email => &mut self.email,
            Field::Password => &mut self.password,
        }
    }

    fn focus(&mut self, cx: &mut Cx<KinNovel>, field: Option<Field>) {
        if self.focus == field {
            return;
        }
        if let Some(old) = self.focus {
            self.editor(old).finish();
        }
        let hint = if self.focus.is_some() != field.is_some() { RefreshHint::Clean } else { RefreshHint::Ui };
        self.focus = field;
        cx.request_ui_redraw(hint);
    }

    fn submit(&mut self, cx: &mut Cx<KinNovel>) {
        if self.logging_in {
            return;
        }
        self.email.finish();
        self.password.finish();
        let email = self.email.text().trim().to_string();
        let password = self.password.text();
        self.message = None;
        if email.is_empty() || !email.contains('@') {
            self.message = Some("请输入注册时用的邮箱".into());
            self.focus(cx, Some(Field::Email));
        } else if password.is_empty() {
            self.message = Some("请输入密码".into());
            self.focus(cx, Some(Field::Password));
        } else if let Some(net) = cx.app.net() {
            self.logging_in = true;
            self.focus(cx, None);
            cx.spawn(move || {
                let hash = kn_net::password_hash(&password);
                LoginDone(net.login_hashed(&email, &hash).map(|_| Account { email, password_hash: hash }).map_err(|e| e.to_string()))
            });
        } else {
            self.message = Some("离线模式，无法登录".into());
        }
        cx.request_redraw(RefreshHint::Ui);
    }

    fn finish(&mut self, cx: &mut Cx<KinNovel>) {
        self.done = Some(if self.welcome { Transition::Root(super::tab_page(super::Tab::Shelf)) } else { Transition::Back });
        cx.request_redraw(RefreshHint::Ui);
    }
}

/// 不在输入时的输入框 (与 `TextEditor::render_field` 同样的外形, 没有光标)。
#[allow(clippy::too_many_arguments)]
fn draw_idle_field(ink: &mut Ink, frame: &mut Bitmap, hits: &mut Hits, theme: &Theme, m: &Metrics, r: Rect, editor: &TextEditor, placeholder: &str, id: HitId) {
    frame.rounded_rect(r, m.radius, Some(theme.background), Some(theme.foreground), 2);
    let pad = m.margin as i32 / 2;
    let text = editor.text();
    let (shown, color) = if text.is_empty() {
        (placeholder.to_string(), theme.muted)
    } else if editor.is_masked() {
        ("●".repeat(text.chars().count()), theme.foreground)
    } else {
        (text, theme.foreground)
    };
    let shown = ink.fit(&shown, m.body, (r.w as i32 - 2 * pad).max(0) as f32);
    ink.text(frame, r.x + pad, r.y + (r.h as i32 - (m.body * 1.25) as i32) / 2, &shown, m.body, color);
    hits.add(id, r).rounded(m.radius);
}

impl Page<KinNovel> for LoginPage {
    fn id(&self) -> PageId {
        PageId(190)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let back = !self.welcome || self.focus.is_some();
        let bar = widgets::header(&mut ink, frame, &theme, &m, if self.welcome { "欢迎" } else { "登录" }, &super::status_text(), back, false);
        if back {
            cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        }
        let side = m.margin as i32;
        let width = cx.width as i32 - 2 * side;
        let gap = m.margin as i32 / 3;
        let mut y = bar.bottom() + m.margin as i32;

        let title = if self.welcome { "欢迎使用 KinNovel" } else { "登录轻书架账号" };
        ink.text(frame, side, y, title, m.title, theme.foreground);
        y += (m.title * 1.4) as i32;
        let intro = ink.fit("登录后同步书架、阅读进度和历史记录", m.small, width as f32);
        ink.text(frame, side, y, &intro, m.small, theme.muted);
        y += (m.small * 1.4) as i32 + m.margin as i32;

        // 邮箱 / 密码
        let label_h = (m.small * 1.5) as i32;
        let field_h = TextEditor::field_height(&m);
        let show_w = (m.touch as f32 * 1.3) as i32;
        let fields = [(Field::Email, "邮箱", "注册时用的邮箱", HIT_EMAIL), (Field::Password, "密码", "密码", HIT_PASSWORD)];
        for (field, label, placeholder, id) in fields {
            ink.text(frame, side, y, label, m.small, theme.muted);
            y += label_h;
            let w = if field == Field::Password { width - show_w - gap } else { width };
            let r = Rect::new(side, y, w.max(0) as u32, field_h);
            if self.focus == Some(field) {
                let editor = match field {
                    Field::Email => &self.email,
                    Field::Password => &self.password,
                };
                editor.render_field(&mut ink, frame, cx.hits, &theme, &m, r, placeholder);
            } else {
                let editor = match field {
                    Field::Email => &self.email,
                    Field::Password => &self.password,
                };
                draw_idle_field(&mut ink, frame, cx.hits, &theme, &m, r, editor, placeholder, id);
            }
            if field == Field::Password {
                let sr = Rect::new(r.right() + gap, y, show_w as u32, field_h);
                widgets::button(&mut ink, frame, &theme, &m, sr, if self.password.is_masked() { "显示" } else { "隐藏" }, ButtonStyle::Secondary);
                cx.hits.add(HIT_SHOW, sr).rounded(m.radius);
            }
            y += field_h as i32 + gap;
        }

        // 错误提示 (一行) + 登录按钮
        let msg_h = (m.small * 1.6) as i32;
        if let Some(msg) = &self.message {
            let text = ink.fit(msg, m.small, width as f32);
            ink.text(frame, side, y + (msg_h - (m.small * 1.25) as i32) / 2, &text, m.small, theme.foreground);
        }
        y += msg_h;
        let br = Rect::new(side, y, width as u32, m.touch);
        let (label, style) = if self.logging_in { ("登录中…", ButtonStyle::Disabled) } else { ("登录", ButtonStyle::Primary) };
        widgets::button(&mut ink, frame, &theme, &m, br, label, style);
        cx.hits.add(HIT_LOGIN, br).rounded(m.radius).enabled(!self.logging_in);
        let y = br.bottom() + m.margin as i32;

        if let Some(field) = self.focus {
            let kh = TextEditor::keys_height(&m);
            let kr = Rect::new(0, cx.height as i32 - kh as i32, cx.width, kh);
            self.keys_top = kr.y;
            let enter = if field == Field::Email { "下一项" } else { "登录" };
            self.editor(field).render_keys(&mut ink, frame, cx.hits, &theme, &m, kr, enter);
        } else {
            // 次要操作贴在屏幕底部 (键盘打开时这里是键盘)
            let line = (m.small * 1.6) as i32;
            let notes = if self.welcome { 2 } else { 1 };
            let mut y = (cx.height as i32 - m.margin as i32 - notes * line - if self.welcome { m.touch as i32 + gap } else { 0 }).max(y);
            if self.welcome {
                let sr = Rect::new(side, y, width as u32, m.touch);
                widgets::button(&mut ink, frame, &theme, &m, sr, "先逛逛", ButtonStyle::Secondary);
                cx.hits.add(HIT_SKIP, sr).rounded(m.radius);
                y = sr.bottom() + gap;
                let note = ink.fit("不登录也能浏览最新、排行和分类，之后可在「我的」里登录", m.small, width as f32);
                ink.text_centered(frame, Rect::new(side, y, width as u32, line as u32), &note, m.small, theme.muted);
                y += line;
            }
            let reg = format!("还没有账号？请在手机或电脑上访问 {SITE} 注册");
            let reg = ink.fit(&reg, m.small, width as f32);
            ink.text_centered(frame, Rect::new(side, y, width as u32, line as u32), &reg, m.small, theme.muted);
        }
        super::tips::show(cx, frame, &super::tips::KEYBOARD, self.focus.is_some());
    }

    fn take_transition(&mut self) -> Transition<KinNovel> {
        self.done.take().unwrap_or(Transition::None)
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        if let Ok(done) = msg.downcast::<LoginDone>() {
            self.logging_in = false;
            match done.0 {
                Ok(account) => {
                    eprintln!("[account] 已登录");
                    cx.app.remember_account(account);
                    self.finish(cx);
                }
                Err(e) => {
                    self.message = Some(format!("登录失败: {e}"));
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        if super::tips::input(cx, event, &super::tips::KEYBOARD) {
            return Transition::None;
        }
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind == GestureKind::Down {
            return Transition::None;
        }
        let hit = cx.hits.at(Point { x: g.start.0, y: g.start.1 });
        if let Some(field) = self.focus {
            if let Some(hit) = hit.filter(|h| self.editor(field).owns(*h)) {
                match self.editor(field).on_gesture(hit, g.kind) {
                    EditorEvent::None => {}
                    EditorEvent::Edited => cx.request_redraw(RefreshHint::Ui),
                    EditorEvent::Submit if field == Field::Email => self.focus(cx, Some(Field::Password)),
                    EditorEvent::Submit => self.submit(cx),
                }
                return Transition::None;
            }
        }
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        match hit {
            Some(HIT_BACK) if self.focus.is_some() => self.focus(cx, None),
            Some(HIT_BACK) => return Transition::Back,
            Some(HIT_EMAIL) => self.focus(cx, Some(Field::Email)),
            Some(HIT_PASSWORD) => self.focus(cx, Some(Field::Password)),
            Some(HIT_SHOW) => {
                let masked = self.password.is_masked();
                self.password.set_masked(!masked);
                cx.request_redraw(RefreshHint::Ui);
            }
            Some(HIT_LOGIN) => self.submit(cx),
            Some(HIT_SKIP) => {
                cx.app.config.set("welcome_skipped", Value::Bool(true));
                return Transition::Root(super::tab_page(super::Tab::Shelf));
            }
            // 点键盘上方的空白处收起键盘 (键盘里键与键之间的空隙不算)
            None if self.focus.is_some() && g.start.1 < self.keys_top => self.focus(cx, None),
            _ => {}
        }
        Transition::None
    }
}
