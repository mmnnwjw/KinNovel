//! "我的": 账号卡片 (登录状态/签到) + 入口列表 (设置、关于、退出), 对照 Python `account.py`。
//! 设置项在二级页 [`super::settings::SettingsPage`]。

use std::any::Any;
use std::time::Duration;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use super::Tab;
use crate::api::{self, MyInfo};
use crate::KinNovel;

const ROW_BASE: u32 = 100;
const HIT_DIALOG_CLOSE: HitId = HitId(60);
const HIT_DIALOG_EXIT: HitId = HitId(61);
const HIT_SIGN_IN: HitId = HitId(70);
const HIT_RETRY: HitId = HitId(71);
const TOAST_TOKEN: u32 = 1;

/// 入口列表的行
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Notifications,
    Announcements,
    Shop,
    Settings,
    About,
    Exit,
}

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Notifications => "消息通知",
            Row::Announcements => "公告",
            Row::Shop => "积分商城",
            Row::Settings => "设置",
            Row::About => "关于 KinNovel",
            Row::Exit => "退出",
        }
    }
}

fn rows(logged_in: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    if logged_in {
        rows.extend([Row::Notifications, Row::Shop]);
    }
    rows.extend([Row::Announcements, Row::Settings, Row::About, Row::Exit]);
    rows
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialog {
    About,
    Exit,
}

struct InfoLoaded(Result<MyInfo, String>);
struct SignInDone(Result<(), String>);

#[derive(Default)]
pub struct MePage {
    dialog: Option<Dialog>,
    info: Option<Result<MyInfo, String>>,
    info_loading: bool,
    signing_in: bool,
    toast: Option<String>,
}

fn logged_in(cx: &Cx<KinNovel>) -> bool {
    cx.app.signed_in()
}

impl MePage {
    fn load_info(&mut self, cx: &mut Cx<KinNovel>) {
        self.info_loading = true;
        let net = cx.app.net();
        cx.spawn(move || InfoLoaded(api::load_my_info(net.as_ref())));
    }

    fn show_toast(&mut self, cx: &mut Cx<KinNovel>, text: impl Into<String>) {
        self.toast = Some(text.into());
        cx.after(Duration::from_secs(2), TOAST_TOKEN);
        cx.request_redraw(RefreshHint::Ui);
    }
}

impl Page<KinNovel> for MePage {
    fn id(&self) -> PageId {
        PageId(130)
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let tabs_top = super::draw_tabs(cx, frame, Tab::Me);
        let logged_in = logged_in(cx);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "我的", &super::status_text(), false, false);
        let side = m.margin as i32;
        let row_h = (m.touch as f32 * 1.15) as i32;
        let mut y = bar.bottom() + m.margin as i32 / 3;

        // 账号卡片
        let has_sign_in = logged_in && matches!(self.info, Some(Ok(_)));
        let card_h = if has_sign_in { row_h + m.touch as i32 + m.margin as i32 / 3 } else { row_h };
        let card = Rect::new(side, y, cx.width - 2 * m.margin, card_h as u32);
        widgets::card(frame, &theme, &m, card);
        let pad = m.margin as i32 / 2;
        if !logged_in {
            let offline = cx.app.net().is_none();
            ink.text(frame, side + pad, card.y + (row_h - (m.body * 1.25) as i32) / 2, if offline { "离线模式" } else { "未登录" }, m.body, theme.foreground);
            let note = if offline { "联网功能已关闭" } else { "账号配置于 bin/config.json" };
            let nw = ink.width(note, m.small).ceil() as i32;
            ink.text(frame, card.right() - pad - nw, card.y + (row_h - (m.small * 1.25) as i32) / 2, note, m.small, theme.muted);
        } else {
            match &self.info {
                None | Some(Err(_)) if self.info_loading => {
                    ink.text(frame, side + pad, card.y + (row_h - (m.body * 1.25) as i32) / 2, "正在获取账号信息…", m.body, theme.muted);
                }
                Some(Err(e)) => {
                    let text = ink.fit(&format!("获取账号信息失败: {e}"), m.small, (card.w as i32 - 2 * pad) as f32);
                    ink.text(frame, side + pad, card.y + (row_h - (m.small * 1.25) as i32) / 2, &text, m.small, theme.muted);
                    let r = Rect::new(card.right() - pad - (m.small * 4.0) as i32, card.y + (row_h - m.touch as i32) / 2, (m.small * 4.0) as u32, m.touch);
                    widgets::button(&mut ink, frame, &theme, &m, r, "重试", ButtonStyle::Secondary);
                    cx.hits.add(HIT_RETRY, r);
                }
                Some(Ok(info)) => {
                    let title = format!("{} · Lv.{}", info.user_name, info.level);
                    ink.text(frame, side + pad, card.y + (row_h - (m.body * 1.25) as i32) / 2, &title, m.body, theme.foreground);
                    let meta = format!("金币 {} · 连续签到 {} 天", info.coin, info.sign_streak);
                    let nw = ink.width(&meta, m.small).ceil() as i32;
                    ink.text(frame, card.right() - pad - nw, card.y + (row_h - (m.small * 1.25) as i32) / 2, &meta, m.small, theme.muted);
                    let r = Rect::new(card.x + pad, card.y + row_h, card.w - 2 * pad as u32, m.touch);
                    let signed = info.today_signed || self.signing_in;
                    let label = if info.today_signed { "今日已签到" } else if self.signing_in { "签到中…" } else { "每日签到" };
                    widgets::button(&mut ink, frame, &theme, &m, r, label, if signed { ButtonStyle::Disabled } else { ButtonStyle::Primary });
                    cx.hits.add(HIT_SIGN_IN, r).enabled(!signed);
                }
                None => {}
            }
        }
        y = card.bottom() + m.margin as i32 / 3;

        // 入口列表
        let entry_h = m.row_h() as i32;
        for (i, row) in rows(logged_in).into_iter().enumerate() {
            if y + entry_h > tabs_top {
                break;
            }
            let meta = if row == Row::About { env!("CARGO_PKG_VERSION") } else { "" };
            let r = Rect::new(0, y, cx.width, entry_h as u32);
            widgets::list_row(&mut ink, frame, &theme, &m, r, row.label(), "", meta);
            cx.hits.add(HitId(ROW_BASE + i as u32), r);
            y += entry_h;
        }

        match self.dialog {
            Some(Dialog::About) => {
                let version = format!("KinNovel {} (Rust)", env!("CARGO_PKG_VERSION"));
                let lines = [version.as_str(), "轻书架 Kindle 客户端", "显示: FBInk (GPL-3.0)", "github.com/mmnnwjw/KinNovel"];
                widgets::dialog(&mut ink, frame, cx.hits, &theme, &m, "关于", &lines, &[("好", HIT_DIALOG_CLOSE, ButtonStyle::Primary)]);
            }
            Some(Dialog::Exit) => {
                widgets::dialog(&mut ink, frame, cx.hits, &theme, &m, "退出 KinNovel？", &["阅读进度已保存"], &[("取消", HIT_DIALOG_CLOSE, ButtonStyle::Secondary), ("退出", HIT_DIALOG_EXIT, ButtonStyle::Primary)]);
            }
            None => {}
        }

        if let Some(text) = &self.toast {
            widgets::toast(&mut ink, frame, &theme, &m, text, tabs_top - m.margin as i32 / 2);
        }
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, _returning: bool) {
        if logged_in(cx) && self.info.is_none() && !self.info_loading {
            self.load_info(cx);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<InfoLoaded>() {
            Ok(loaded) => {
                self.info_loading = false;
                self.info = Some(loaded.0);
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(done) = msg.downcast::<SignInDone>() {
            self.signing_in = false;
            match done.0 {
                Ok(()) => {
                    self.show_toast(cx, "签到成功");
                    self.load_info(cx);
                }
                Err(e) => self.show_toast(cx, format!("签到失败: {e}")),
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<KinNovel>, token: u32) {
        if token == TOAST_TOKEN && self.toast.take().is_some() {
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        if self.dialog.is_some() {
            match hit {
                HIT_DIALOG_EXIT => return Transition::Exit,
                HIT_DIALOG_CLOSE => {
                    self.dialog = None;
                    // 弹窗区域局部闪刷: 深色按钮在普通局部刷新下残留严重
                    cx.request_redraw(RefreshHint::Clean);
                }
                _ => {}
            }
            return Transition::None;
        }
        if let Some(t) = super::tab_transition(hit, Tab::Me) {
            return t;
        }
        match hit {
            HitId(id) if id >= ROW_BASE => match rows(logged_in(cx)).get((id - ROW_BASE) as usize) {
                Some(Row::Settings) => return Transition::Push(Box::new(super::settings::SettingsPage::default())),
                Some(Row::Notifications) => return Transition::Push(Box::new(super::notify::NotificationsPage::new())),
                Some(Row::Announcements) => return Transition::Push(Box::new(super::announce::AnnouncementsPage::new())),
                Some(Row::Shop) => return Transition::Push(Box::new(super::shop::ShopPage::new())),
                Some(&row @ (Row::About | Row::Exit)) => {
                    self.dialog = Some(if row == Row::About { Dialog::About } else { Dialog::Exit });
                    cx.request_redraw(RefreshHint::Ui);
                }
                None => {}
            },
            HIT_RETRY => {
                self.load_info(cx);
                cx.request_redraw(RefreshHint::Ui);
            }
            HIT_SIGN_IN => {
                if !self.signing_in && !matches!(&self.info, Some(Ok(info)) if info.today_signed) {
                    self.signing_in = true;
                    let net = cx.app.net();
                    cx.spawn(move || SignInDone(api::sign_in(net.as_ref())));
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
            _ => {}
        }
        Transition::None
    }
}
