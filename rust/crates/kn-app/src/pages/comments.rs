//! 评论: 只读, 对照 Python `announcements.py` 的 `render_comments`
//! (`{Users, Commentaries, Data}`, 支持回复)。压栈页面, 有返回箭头, 没有标签栏。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::api::{self, CommentsPage as CommentsData};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const HIT_RETRY: HitId = HitId(4);
const HIT_LOGIN: HitId = HitId(40);

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
    local_page: usize,
    lines: Vec<(u32, bool, String)>,
    layout: Option<(u32, u32)>,
    local_total: usize,
    open_last: bool,
}

impl CommentsPage {
    /// `comment_type` 对照服务端的 `Type` 参数 (`"Announcement"` / `"Book"`)。
    pub fn new(comment_type: &'static str, target_id: i64) -> Self {
        CommentsPage { comment_type, target_id, status: Status::Loading, page: 1, local_page: 0, lines: Vec::new(), layout: None, local_total: 1, open_last: false }
    }

    fn turn(&mut self, cx: &mut Cx<KinNovel>, forward: bool) {
        let Status::Ready(data) = &self.status else { return };
        if forward && self.local_page + 1 < self.local_total {
            self.local_page += 1;
        } else if !forward && self.local_page > 0 {
            self.local_page -= 1;
        } else if forward && self.page < data.total_pages {
            self.open_last = false;
            self.load(cx, self.page + 1);
        } else if !forward && self.page > 1 {
            self.open_last = true;
            self.load(cx, self.page - 1);
        }
        cx.request_redraw(RefreshHint::Ui);
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
        self.local_page = 0;
        self.layout = None;
        let net = cx.app.net();
        let comment_type = self.comment_type;
        let target_id = self.target_id;
        cx.spawn_droppable(move || Loaded(page, api::load_comments(net.as_ref(), comment_type, target_id, page)));
    }
}

impl Page<KinNovel> for CommentsPage {
    fn id(&self) -> PageId {
        PageId(310)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        // 从登录页回来也重新加载
        if !returning || matches!(self.status, Status::NeedLogin) {
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
                widgets::state_message(&mut ink, frame, cx.hits, &theme, &m, area, "登录后可以查看评论", Some(("登录", HIT_LOGIN)));
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
        let line_h = (m.small * 1.4).ceil() as i32;
        let top = bar.bottom() + m.margin as i32 / 3;
        let per = ((list_bottom - top) / line_h).max(1) as usize;
        if self.layout != Some((cx.width, cx.height)) {
            self.lines.clear();
            let mut pending: Vec<_> = data.items.iter().rev().map(|c| (c, 0)).collect();
            while let Some((comment, depth)) = pending.pop() {
                let indent = depth.min(2) * (m.margin / 2 + m.touch / 3);
                let width = cx.width.saturating_sub(2 * m.margin + indent).max(1) as f32;
                let header = if comment.created_at.is_empty() { comment.user_name.clone() } else { format!("{} · {}", comment.user_name, comment.created_at) };
                for line in super::announce::wrap_all(&header, &ink, m.small, width) {
                    self.lines.push((indent, true, line));
                }
                for line in super::announce::wrap_all(&comment.content, &ink, m.tiny, width) {
                    self.lines.push((indent, false, line));
                }
                self.lines.push((indent, false, String::new()));
                pending.extend(comment.replies.iter().rev().map(|c| (c, depth + 1)));
            }
            self.local_total = self.lines.len().div_ceil(per).max(1);
            self.local_page = if self.open_last { self.local_total - 1 } else { self.local_page.min(self.local_total - 1) };
            self.open_last = false;
            self.layout = Some((cx.width, cx.height));
        }
        for (row, (indent, header, text)) in self.lines.iter().skip(self.local_page * per).take(per).enumerate() {
            ink.text(frame, side + *indent as i32, top + row as i32 * line_h, text,
                if *header { m.small } else { m.tiny }, if *header { theme.foreground } else { theme.muted });
        }
        let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
        // 本地分页保留服务器一页中的全部内容, 到边界再请求上一/下一页。
        if data.total_pages > 1 || self.local_total > 1 {
            let label = if data.total_pages > 1 {
                format!("{} / {} · {}屏", data.page, data.total_pages, self.local_page + 1)
            } else { format!("{} / {}", self.local_page + 1, self.local_total) };
            widgets::pager_with_label(&mut ink, frame, cx.hits, &theme, &m, pr, &label,
                self.local_page > 0 || self.page > 1,
                self.local_page + 1 < self.local_total || self.page < data.total_pages, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        match g.kind {
            GestureKind::SwipeUp | GestureKind::SwipeLeft => { self.turn(cx, true); return Transition::None; }
            GestureKind::SwipeDown | GestureKind::SwipeRight => { self.turn(cx, false); return Transition::None; }
            GestureKind::Tap => {}
            _ => return Transition::None,
        }
        let Some(hit) = cx.hits.at(Point { x: g.start.0, y: g.start.1 }) else { return Transition::None };
        match hit {
            HIT_BACK => Transition::Back,
            HIT_LOGIN => Transition::Push(Box::new(super::login::LoginPage::new())),
            HIT_RETRY => {
                self.load(cx, self.page);
                cx.request_redraw(RefreshHint::Ui);
                Transition::None
            }
            HIT_PREV => { self.turn(cx, false); Transition::None }
            HIT_NEXT => { self.turn(cx, true); Transition::None }
            _ => Transition::None,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Comment;

    #[test]
    fn local_pages_reach_every_comment_and_nested_reply() {
        crate::test_support::with_cx(|cx, frame| {
            let mut page = CommentsPage::new("Book", 1);
            let content = "long comment ".repeat(120);
            let data = CommentsData { page: 1, total_pages: 1, items: vec![Comment {
                user_name: "first".into(), content: content.clone(), replies: vec![Comment {
                    user_name: "reply".into(), content: "reply body".into(), replies: vec![Comment {
                        user_name: "nested".into(), content: "nested tail".into(), ..Default::default()
                    }], ..Default::default()
                }], ..Default::default()
            }, Comment { user_name: "last".into(), content: "last comment".into(), ..Default::default() }] };
            page.status = Status::Ready(data);
            page.render(cx, frame);
            assert!(page.local_total > 1);
            let mut reached = String::new();
            let m = cx.app.metrics;
            let top = m.header_h() as i32 + m.margin as i32 / 3;
            let per = ((cx.height as i32 - m.pager_h() as i32 - top) / (m.small * 1.4).ceil() as i32).max(1) as usize;
            for expected in 0..page.local_total {
                assert_eq!(page.local_page, expected);
                for (_, _, line) in page.lines.iter().skip(page.local_page * per).take(per) { reached.push_str(line); }
                page.turn(cx, true);
            }
            assert!(reached.contains("nested tail") && reached.contains("last comment"));
            assert!(reached.contains(&content));
            assert_eq!(page.page, 1, "本地翻页不请求服务器下一页");
            page.turn(cx, false);
            assert_eq!(page.local_page, page.local_total - 2);
        });
    }
}
