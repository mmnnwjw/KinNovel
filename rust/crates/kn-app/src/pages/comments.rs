//! 评论: 只读, 对照 Python `announcements.py` 的 `render_comments`
//! (`{Users, Commentaries, Data}`, 支持回复)。压栈页面, 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::api::{self, Comment, CommentsPage as CommentsData};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);

enum Status {
    Loading,
    Offline,
    NeedLogin,
    Error(String),
    Ready(CommentsData),
}

struct Loaded(i64, Result<CommentsData, String>);

pub struct CommentsPage {
    comment_type: &'static str,
    target_id: i64,
    status: Status,
    page: i64,
}

impl CommentsPage {
    /// `comment_type` 对照服务端的 `Type` 参数 (`"Announcement"` / `"Book"`)。
    pub fn new(comment_type: &'static str, target_id: i64) -> Self {
        CommentsPage { comment_type, target_id, status: Status::Loading, page: 1 }
    }

    fn load(&mut self, cx: &mut Cx<KinNovel>, page: i64) {
        if cx.app.net().is_none() && !api::fake_mode() {
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
        let comment_type = self.comment_type;
        let target_id = self.target_id;
        cx.spawn(move || Loaded(page, api::load_comments(net.as_ref(), comment_type, target_id, page)));
    }
}

impl Page<KinNovel> for CommentsPage {
    fn id(&self) -> PageId {
        PageId(310)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if !returning {
            self.load(cx, 1);
        }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        if let Ok(l) = msg.downcast::<Loaded>() {
            if l.0 == self.page {
                self.status = match l.1 {
                    Ok(data) => Status::Ready(data),
                    Err(e) => Status::Error(e),
                };
                cx.request_redraw(RefreshHint::Ui);
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "评论", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let area = Rect::new(0, bar.bottom(), cx.width, cx.height - bar.h);

        let data = match &self.status {
            Status::Loading => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "加载中…", None);
                return;
            }
            Status::Offline => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "离线，无法加载", Some(("重试", HIT_RETRY)));
                return;
            }
            Status::NeedLogin => {
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "查看评论需要登录\n在 config.json 中设置账号", None);
                return;
            }
            Status::Error(e) => {
                let text = format!("加载失败\n{}", ink.fit(e, m.small, (cx.width - 2 * m.margin) as f32));
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, &text, Some(("重试", HIT_RETRY)));
                return;
            }
            Status::Ready(d) => d,
        };

        if data.items.is_empty() {
            widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "暂无评论", None);
            return;
        }

        let side = m.margin as i32;
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        let line_h = (m.tiny * 1.4) as i32;
        let mut y = bar.bottom() + m.margin as i32 / 3;
        'outer: for comment in &data.items {
            y = draw_comment(&mut ink, frame, &theme, &m, comment, 0, side, y, cx.width, line_h, list_bottom);
            if y >= list_bottom {
                break 'outer;
            }
            for reply in &comment.replies {
                y = draw_comment(&mut ink, frame, &theme, &m, reply, 1, side, y, cx.width, line_h, list_bottom);
                if y >= list_bottom {
                    break 'outer;
                }
            }
            y += m.margin as i32 / 4;
        }
        if data.total_pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, (data.page - 1).max(0) as usize, data.total_pages.max(1) as usize, HIT_PREV, HIT_NEXT);
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
                let total = if let Status::Ready(d) = &self.status { d.total_pages } else { 1 };
                if self.page < total {
                    self.load(cx, self.page + 1);
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            _ => Transition::None,
        }
    }
}

/// 画一条评论 (或缩进的回复), 返回画完之后的 y。超出 `list_bottom` 时提前停画但仍返回新 y
/// (调用方据此判断是否继续)。
#[allow(clippy::too_many_arguments)]
fn draw_comment(
    ink: &mut Ink,
    frame: &mut Bitmap,
    theme: &kn_ui::Theme,
    m: &widgets::Metrics,
    comment: &Comment,
    indent: u32,
    side: i32,
    y: i32,
    width: u32,
    line_h: i32,
    list_bottom: i32,
) -> i32 {
    if y >= list_bottom {
        return y;
    }
    let x = side + indent as i32 * (m.margin as i32 / 2 + m.touch as i32 / 3);
    let text_w = (width as i32 - x - side) as f32;
    let header = if comment.created_at.is_empty() { comment.user_name.clone() } else { format!("{} · {}", comment.user_name, comment.created_at) };
    ink.text(frame, x, y, &header, m.small, theme.foreground);
    let mut yy = y + (m.small * 1.3) as i32;
    for line in super::announce::wrap_all(&comment.content, ink, m.tiny, text_w) {
        if yy >= list_bottom {
            break;
        }
        ink.text(frame, x, yy, &line, m.tiny, theme.muted);
        yy += line_h;
    }
    yy + m.margin as i32 / 6
}
