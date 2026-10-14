//! 书架整理: 文件夹命名 (新建 / 重命名) 与"移动到"。从书架页压栈进入, 保存成功后返回,
//! 书架页回到前台时会重新拉取云端书架。写回规则见 `crate::api::shelf_create_folder` 等。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::keyboard::{EditorEvent, TextEditor};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::api::{self, FolderNode, ShelfRef};
use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_PREV: HitId = HitId(2);
const HIT_NEXT: HitId = HitId(3);
const ROW_BASE: u32 = 1000;
const EDITOR_BASE: u32 = 5000;
/// 文件夹名最多几个字
const MAX_NAME: usize = 20;

struct Saved(Result<(), String>);

/// 文件夹命名要做的事
pub enum NameAction {
    /// 在这个路径下新建 (空 = 最外层)
    Create { parents: Vec<String> },
    Rename { id: String },
}

/// 文件夹命名: 上方说明, 下方屏幕键盘, 回车键 = 保存。
pub struct FolderNamePage {
    action: NameAction,
    editor: TextEditor,
    saving: bool,
    error: Option<String>,
    /// 已保存, 返回书架
    done: bool,
}

impl FolderNamePage {
    pub fn create(parents: Vec<String>) -> Self {
        FolderNamePage { action: NameAction::Create { parents }, editor: TextEditor::new(EDITOR_BASE, ""), saving: false, error: None, done: false }
    }

    pub fn rename(id: String, title: &str) -> Self {
        FolderNamePage { action: NameAction::Rename { id }, editor: TextEditor::new(EDITOR_BASE, title), saving: false, error: None, done: false }
    }

    fn save(&mut self, cx: &mut Cx<KinNovel>) {
        self.editor.finish();
        let name: String = self.editor.text().trim().chars().take(MAX_NAME).collect();
        if name.is_empty() {
            self.error = Some("请输入文件夹名".into());
            cx.request_redraw(RefreshHint::Ui);
            return;
        }
        self.saving = true;
        self.error = None;
        let net = cx.app.net();
        let job = match &self.action {
            NameAction::Create { parents } => {
                let parents = parents.clone();
                Box::new(move || Saved(api::shelf_create_folder(net.as_ref(), &parents, &name).map(|_| ()))) as Box<dyn FnOnce() -> Saved + Send>
            }
            NameAction::Rename { id } => {
                let id = id.clone();
                Box::new(move || Saved(api::shelf_rename_folder(net.as_ref(), &id, &name)))
            }
        };
        cx.spawn(job);
        cx.request_redraw(RefreshHint::Ui);
    }
}

impl Page<KinNovel> for FolderNamePage {
    fn id(&self) -> PageId {
        PageId(102)
    }

    fn take_transition(&mut self) -> Transition<KinNovel> {
        if std::mem::take(&mut self.done) { Transition::Back } else { Transition::None }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        if let Ok(saved) = msg.downcast::<Saved>() {
            self.saving = false;
            match saved.0 {
                Ok(()) => self.done = true,
                Err(e) => {
                    self.error = Some(format!("保存失败: {e}"));
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let (theme, m) = (cx.theme, cx.app.metrics);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let title = match self.action {
            NameAction::Create { .. } => "新建文件夹",
            NameAction::Rename { .. } => "重命名文件夹",
        };
        let bar = widgets::header(&mut ink, frame, &theme, &m, title, &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let side = m.margin as i32;
        let mut y = bar.bottom() + m.margin as i32;
        let hint = match &self.action {
            NameAction::Create { parents } if parents.is_empty() => "新文件夹会放在书架最上面。".to_string(),
            NameAction::Create { .. } => "新文件夹会放在当前文件夹的最上面。".to_string(),
            NameAction::Rename { .. } => "输入新的名字。".to_string(),
        };
        ink.text(frame, side, y, &hint, m.body, theme.muted);
        y += (m.body * 1.8) as i32;
        let status = if self.saving { Some("正在保存…".to_string()) } else { self.error.clone() };
        if let Some(text) = status {
            let text = ink.fit(&text, m.body, (cx.width - 2 * m.margin) as f32);
            ink.text(frame, side, y, &text, m.body, theme.foreground);
        }
        let kb_h = TextEditor::height(&m);
        let kb = Rect::new(0, cx.height as i32 - kb_h as i32, cx.width, kb_h);
        self.editor.render(&mut ink, frame, cx.hits, &theme, &m, kb, &format!("文件夹名 (最多 {MAX_NAME} 字)"), "保存");
        super::tips::show(cx, frame, &super::tips::KEYBOARD, true);
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        if super::tips::input(cx, event, &super::tips::KEYBOARD) {
            return Transition::None;
        }
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind == GestureKind::Down || self.saving {
            return Transition::None;
        }
        let hit = cx.hits.at(Point { x: g.start.0, y: g.start.1 });
        if let Some(hit) = hit.filter(|h| self.editor.owns(*h)) {
            match self.editor.on_gesture(hit, g.kind) {
                EditorEvent::None => {}
                EditorEvent::Edited => cx.request_redraw(RefreshHint::Ui),
                EditorEvent::Submit => self.save(cx),
            }
            return Transition::None;
        }
        if g.kind == GestureKind::Tap && hit == Some(HIT_BACK) {
            return Transition::Back;
        }
        Transition::None
    }
}

/// "移动到": 列出最外层和全部文件夹 (按层级缩进), 点一项就移过去。
/// 当前所在的位置、以及移动文件夹时它自己和它的下级, 显示为灰色不可选。
pub struct MovePickerPage {
    what: ShelfRef,
    title: String,
    from: Vec<String>,
    folders: Vec<FolderNode>,
    page: usize,
    saving: bool,
    error: Option<String>,
    done: bool,
}

impl MovePickerPage {
    pub fn new(data: &api::ShelfData, what: ShelfRef, title: String) -> Self {
        let from = data.parents_of(&what);
        MovePickerPage { from, what, title, folders: data.folder_tree(), page: 0, saving: false, error: None, done: false }
    }

    /// (显示名, 目标路径, 缩进层数, 可选)
    fn targets(&self) -> Vec<(String, Vec<String>, usize, bool)> {
        let mut out = vec![("书架最外层".to_string(), Vec::new(), 0, !self.from.is_empty())];
        for f in &self.folders {
            let inside_self = matches!(&self.what, ShelfRef::Folder(id) if f.path.contains(id));
            out.push((f.title.clone(), f.path.clone(), f.path.len(), !inside_self && f.path != self.from));
        }
        out
    }
}

impl Page<KinNovel> for MovePickerPage {
    fn id(&self) -> PageId {
        PageId(103)
    }

    fn take_transition(&mut self) -> Transition<KinNovel> {
        if std::mem::take(&mut self.done) { Transition::Back } else { Transition::None }
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        if let Ok(saved) = msg.downcast::<Saved>() {
            self.saving = false;
            match saved.0 {
                Ok(()) => self.done = true,
                Err(e) => {
                    self.error = Some(format!("移动失败: {e}"));
                    cx.request_redraw(RefreshHint::Ui);
                }
            }
        }
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let (theme, m) = (cx.theme, cx.app.metrics);
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let bar = widgets::header(&mut ink, frame, &theme, &m, "移动到", &super::status_text(), true, false);
        cx.hits.add(HIT_BACK, Rect::new(0, 0, bar.h, bar.h));
        let side = m.margin as i32;
        let mut y = bar.bottom() + m.margin as i32 / 2;
        let label = match (&self.error, self.saving) {
            (_, true) => "正在移动…".to_string(),
            (Some(e), _) => e.clone(),
            _ => format!("移动「{}」到:", self.title),
        };
        let label = ink.fit(&label, m.small, (cx.width - 2 * m.margin) as f32);
        ink.text(frame, side, y, &label, m.small, theme.muted);
        y += (m.small * 1.6) as i32;
        frame.fill_rect(Rect::new(side, y - 2, cx.width - 2 * m.margin, 2), theme.foreground);

        let targets = self.targets();
        let row_h = m.row_h();
        let list_bottom = cx.height as i32 - m.pager_h() as i32;
        let per_page = ((list_bottom - y) / row_h as i32).max(1) as usize;
        let pages = targets.len().div_ceil(per_page).max(1);
        self.page = self.page.min(pages - 1);
        let indent = m.margin as i32;
        for (row, (index, (title, path, depth, enabled))) in targets.iter().enumerate().skip(self.page * per_page).take(per_page).enumerate() {
            let r = Rect::new(0, y + row as i32 * row_h as i32, cx.width, row_h);
            let shifted = Rect::new(r.x + indent * *depth as i32, r.y, r.w - (indent * *depth as i32) as u32, r.h);
            let meta = if *path == self.from { "当前位置" } else { "" };
            // 不可选的行 (当前位置、文件夹自己和它的下级) 用灰字
            let row_theme = if *enabled { theme } else { kn_ui::Theme { foreground: theme.muted, ..theme } };
            widgets::list_row(&mut ink, frame, &row_theme, &m, shifted, title, "", meta);
            if *enabled {
                cx.hits.add(HitId(ROW_BASE + index as u32), r);
            }
        }
        if pages > 1 {
            let pr = Rect::new(0, list_bottom, cx.width, m.pager_h());
            widgets::pager(&mut ink, frame, cx.hits, &theme, &m, pr, self.page, pages, HIT_PREV, HIT_NEXT);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if self.saving {
            return Transition::None;
        }
        match g.kind {
            GestureKind::Tap => match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
                Some(HIT_BACK) => Transition::Back,
                Some(HIT_PREV) => self.turn(cx, -1),
                Some(HIT_NEXT) => self.turn(cx, 1),
                Some(HitId(id)) if id >= ROW_BASE => {
                    if let Some((_, to, _, true)) = self.targets().into_iter().nth((id - ROW_BASE) as usize) {
                        self.saving = true;
                        self.error = None;
                        let net = cx.app.net();
                        let what = self.what.clone();
                        cx.spawn(move || Saved(api::shelf_move(net.as_ref(), &what, &to)));
                        cx.request_redraw(RefreshHint::Ui);
                    }
                    Transition::None
                }
                _ => Transition::None,
            },
            GestureKind::SwipeUp | GestureKind::SwipeLeft => self.turn(cx, 1),
            GestureKind::SwipeDown | GestureKind::SwipeRight => self.turn(cx, -1),
            _ => Transition::None,
        }
    }
}

impl MovePickerPage {
    fn turn(&mut self, cx: &mut Cx<KinNovel>, d: i64) -> Transition<KinNovel> {
        let n = self.page as i64 + d;
        if n >= 0 {
            self.page = n as usize;
            cx.request_redraw(RefreshHint::Ui);
        }
        Transition::None
    }
}
