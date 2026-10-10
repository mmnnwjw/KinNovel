//! 消息通知: 分页列表 + "全部已读" (对照 Python `account.py` 的通知一段)。
//! 压栈页面, 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::api::{self, Notification};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_READ_ALL: HitId = HitId(5);
const ROW_BASE: u32 = 1000;

enum Status {
    Loading,
    Offline,
    NeedLogin,
    Error(String),
    Ready,
}

struct Loaded(i64, Result<(Vec<Notification>, i64, i64), String>);
struct MarkedAll(Vec<i64>, Result<(), String>);
struct MarkedOne(i64, Result<(), String>);

pub struct NotificationsPage {
    status: Status,
    items: Vec<Notification>,
    page: i64,
    total_pages: i64,
    marking: bool,
}

impl NotificationsPage {
    pub fn new() -> Self {
        NotificationsPage { status: Status::Loading, items: Vec::new(), page: 1, total_pages: 1, marking: false }
    }

    fn online(&self, cx: &Cx<KinNovel>) -> bool {
        cx.app.net().is_some() || api::fake_mode()
    }

    fn load(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if !self.online(cx) {
            self.status = Status::Offline;
            return;
        }
        if !cx.app.signed_in() {
            self.status = Status::NeedLogin;
            return;
        }
        self.status = Status::Loading;
        self.page = page;
        let net = cx.app.net();
        let size = per_page(cx);
        cx.spawn(move || {
            let r = api::load_notifications(net.as_ref(), page, size);
            Loaded(page, r.map(|p| (p.items, p.page, p.total_pages)))
        });
    }
}

fn per_page(cx: &Cx<KinNovel>) -> i64 {
    let m = cx.app.metrics;
    let top = m.header_h() as i32 + m.touch as i32;
    let bottom = m.pager_h() as i32;
    let area = cx.height as i32 - top - bottom;
    (area / m.row_h() as i32).max(1) as i64
}

impl Page<KinNovel> for NotificationsPage {
    fn id(&self) -> PageId {
        PageId(320)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx, 1);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Loaded>() {
            Ok(l) => {
                if l.0 == self.page {
                    match l.1 {
                        Ok((items, page, total)) => {
                            self.items = items;
                            self.page = page;
                            self.total_pages = total;
                            self.status = Status::Ready;
                        }
                        Err(e) => self.status = Status::Error(e),
                    }
                    cx.request_redraw(RefreshHint::Ui);
                }
                return;
            }
            Err(other) => other,
        };
        let msg = match msg.downcast::<MarkedAll>() {
            Ok(m) => {
                self.marking = false;
                if m.1.is_ok() {
                    for item in &mut self.items {
                        if m.0.contains(&item.id) {
                            item.is_read = true;
                        }
                    }
                }
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(m) = msg.downcast::<MarkedOne>() {
            if m.1.is_ok() {
                if let Some(item) = self.items.iter_mut().find(|i| i.id == m.0) {
                    item.is_read = true;
                }
                cx.request_redraw(RefreshHint::Ui);
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "消息通知", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);

        match &self.status {
            Status::Loading => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                return;
            }
            Status::Offline => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY)));
                return;
            }
            Status::NeedLogin => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "查看通知需要登录\n在 config.json 中设置账号", None);
                return;
            }
            Status::Error(e) => {
                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                return;
            }
            Status::Ready => {}
        }

        let side = m.margin as i32;
        let has_unread = self.items.iter().any(|i| !i.is_read);
        let action_rect = Rect::new(cx.width as i32 - side - (m.small * 6.0) as i32, bar.bottom(), (m.small * 6.0) as u32, m.touch);
        widgets::button(&mut ink, frame, &theme, &m, action_rect, if self.marking { "标记中…" } else { "全部已读" }, if has_unread && !self.marking { ButtonStyle::Secondary } else { ButtonStyle::Disabled });
        cx.hits.add(HIT_READ_ALL, action_rect).enabled(has_unread && !self.marking);

        let list_top = bar.bottom() + m.touch as i32;
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        if self.items.is_empty() {
            let empty_area = Rect::new(0, list_top, cx.width, (list_bottom - list_top).max(0) as u32);
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, empty_area, "暂无通知", None);
            return;
        }
        let row_h = m.row_h();
        for (row, item) in self.items.iter().enumerate() {
            let r = Rect::new(0, list_top + row as i32 * row_h as i32, cx.width, row_h);
            if r.bottom() > list_bottom {
                break;
            }
            let title = if item.is_read { item.title.clone() } else { format!("● {}", item.title) };
            widgets::list_row(&mut ink, frame, &theme, &m, r, &title, &item.body, &item.created_at);
            cx.hits.add(HitId(ROW_BASE + row as u32), r);
        }
        if self.total_pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (self.page - 1).max(0) as usize, self.total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        match hit {
            HIT_BACK => Transition::Back,
            HIT_RETRY => {
                self.load(cx, self.page);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_PREV => {
                if self.page > 1 {
                    self.load(cx, self.page - 1);
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            HIT_NEXT => {
                if self.page < self.total_pages {
                    self.load(cx, self.page + 1);
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            HIT_READ_ALL => {
                let ids: Vec<i64> = self.items.iter().filter(|i| !i.is_read).map(|i| i.id).collect();
                if !ids.is_empty() && !self.marking {
                    self.marking = true;
                    let net = cx.app.net();
                    let ids_for_task = ids.clone();
                    cx.spawn(move || MarkedAll(ids_for_task, api::mark_notifications(net.as_ref(), &ids)));
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            HitId(id) if id >= ROW_BASE => {
                let index = (id - ROW_BASE) as usize;
                if let Some(item) = self.items.get(index) {
                    if !item.is_read {
                        let net = cx.app.net();
                        let nid = item.id;
                        cx.spawn(move || MarkedOne(nid, api::mark_notifications(net.as_ref(), &[nid])));
                    }
                }
                Transition::None
            }
            _ => Transition::None,
        }
    }
}
