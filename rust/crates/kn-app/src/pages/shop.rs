//! 积分商城: 商品列表 (本地分页) + 购买确认对话框。对照 Python `account.py` 的商城一段。
//! 压栈页面, 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::api::{self, Shop, ShopItem};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_DIALOG_CANCEL: HitId = HitId(5);
const HIT_DIALOG_CONFIRM: HitId = HitId(6);
const ROW_BASE: u32 = 1000;
const TOAST_TOKEN: u32 = 1;

enum Status {
    Loading,
    Offline,
    NeedLogin,
    Error(String),
    Ready(Shop),
}

struct Loaded(Result<Shop, String>);
struct BoughtDone(ShopItem, Result<(), String>);

pub struct ShopPage {
    status: Status,
    page: usize,
    pending_buy: Option<ShopItem>,
    buying: bool,
    toast: Option<String>,
}

impl ShopPage {
    pub fn new() -> Self {
        ShopPage { status: Status::Loading, page: 0, pending_buy: None, buying: false, toast: None }
    }

    fn online(&self, cx: &Cx<KinNovel>) -> bool {
        cx.app.net().is_some() || api::fake_mode()
    }

    fn load(&mut self, cx: &mut Cx<KinNovel>) {
        if !self.online(cx) {
            self.status = Status::Offline;
            return;
        }
        if !cx.app.signed_in() {
            self.status = Status::NeedLogin;
            return;
        }
        self.status = Status::Loading;
        let net = cx.app.net();
        cx.spawn(move || Loaded(api::load_shop(net.as_ref())));
    }

    fn show_toast(&mut self, cx: &mut Cx<KinNovel>, text: impl Into<String>) {
        self.toast = Some(text.into());
        cx.after(std::time::Duration::from_secs(2), TOAST_TOKEN);
        cx.request_redraw(RefreshHint::Ui);
    }
}

fn per_page(cx: &Cx<KinNovel>) -> usize {
    let m = cx.app.metrics;
    let top = m.header_h() as i32;
    let bottom = m.pager_h() as i32;
    let area = cx.height as i32 - top - bottom;
    (area / m.row_h() as i32).max(1) as usize
}

impl Page<KinNovel> for ShopPage {
    fn id(&self) -> PageId {
        PageId(330)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let msg = match msg.downcast::<Loaded>() {
            Ok(l) => {
                self.status = match l.0 {
                    Ok(shop) => Status::Ready(shop),
                    Err(e) => Status::Error(e),
                };
                cx.request_redraw(RefreshHint::Ui);
                return;
            }
            Err(other) => other,
        };
        if let Ok(b) = msg.downcast::<BoughtDone>() {
            self.buying = false;
            match b.1 {
                Ok(()) => {
                    self.show_toast(cx, format!("购买成功: {}", b.0.name));
                    self.load(cx);
                }
                Err(e) => self.show_toast(cx, format!("购买失败: {e}")),
            }
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<KinNovel>, token: u32) {
        if token == TOAST_TOKEN && self.toast.take().is_some() {
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let per = per_page(cx);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };

        let title = match &self.status {
            Status::Ready(shop) => format!("积分商城 · 金币 {}", shop.coin),
            _ => "积分商城".to_string(),
        };
        let bar = widgets::header(&mut ink, frame, &theme, &m, &title, &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);

        let shop = match &self.status {
            Status::Loading => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                return;
            }
            Status::Offline => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY)));
                return;
            }
            Status::NeedLogin => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "积分商城需要登录\n在 config.json 中设置账号", None);
                return;
            }
            Status::Error(e) => {
                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                return;
            }
            Status::Ready(s) => s,
        };

        if shop.items.is_empty() {
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无商品", None);
            return;
        }

        let pages = shop.items.len().div_ceil(per).max(1);
        self.page = self.page.min(pages - 1);
        let row_h = m.row_h();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        for (row, item) in shop.items.iter().skip(self.page * per).take(per).enumerate() {
            let r = Rect::new(0, bar.bottom() + row as i32 * row_h as i32, cx.width, row_h);
            if r.bottom() > list_bottom {
                break;
            }
            let meta = format!("{} 金币 · 持有 {}", item.price, item.owned);
            widgets::list_row(&mut ink, frame, &theme, &m, r, &item.name, "", &meta);
            cx.hits.add(HitId(ROW_BASE + (self.page * per + row) as u32), r);
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, HIT_PREV, HIT_NEXT);
        }

        if let Some(item) = &self.pending_buy {
            let lines = [format!("{} 金币", item.price)];
            let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            widgets::dialog(
                &mut ink,
                frame,
                cx.hits,
                &theme,
                &m,
                &format!("购买 {}？", item.name),
                &line_refs,
                &[("取消", HIT_DIALOG_CANCEL, ButtonStyle::Secondary), ("购买", HIT_DIALOG_CONFIRM, ButtonStyle::Primary)],
            );
        }
        if let Some(text) = &self.toast {
            widgets::toast(&mut ink, frame, &theme, &m, text, list_bottom - m.margin as i32 / 2);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        if self.pending_buy.is_some() {
            match hit {
                HIT_DIALOG_CANCEL => {
                    self.pending_buy = None;
                    cx.request_redraw(RefreshHint::Ui);
                }
                HIT_DIALOG_CONFIRM => {
                    if let Some(item) = self.pending_buy.take() {
                        self.buying = true;
                        let net = cx.app.net();
                        let key = item.key.clone();
                        cx.spawn(move || BoughtDone(item, api::buy_shop_item(net.as_ref(), &key)));
                    }
                    cx.request_redraw(RefreshHint::Ui);
                }
                _ => {}
            }
            return Transition::None;
        }
        match hit {
            HIT_BACK => return Transition::Back,
            HIT_RETRY => {
                self.load(cx);
                cx.request_redraw(RefreshHint::Ui);
            }
            HIT_PREV => {
                if self.page > 0 {
                    self.page -= 1;
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
            HIT_NEXT => {
                let per = per_page(cx);
                if let Status::Ready(shop) = &self.status {
                    let pages = shop.items.len().div_ceil(per).max(1);
                    if self.page + 1 < pages {
                        self.page += 1;
                        cx.request_redraw(RefreshHint::Ui);
                    }
                }
            }
            HitId(id) if id >= ROW_BASE && !self.buying => {
                let index = (id - ROW_BASE) as usize;
                if let Status::Ready(shop) = &self.status {
                    if let Some(item) = shop.items.get(index) {
                        self.pending_buy = Some(item.clone());
                        cx.request_redraw(RefreshHint::Ui);
                    }
                }
            }
            _ => {}
        }
        Transition::None
    }
}
