//! 插图全屏预览 (从阅读页点击插图进入)。
//!
//! - 打开时显示阅读页用的那份插图 (CDN 缩放版, 已在缓存里), 整图适配全屏。
//! - 底部工具栏: 返回 / 缩小 / 放大 / 原图。放大后左右上下滑动平移半屏; 轻触图片区域显示/隐藏工具栏。
//! - "原图" 请求不带缩放参数的原始资源 (单独缓存), 解码后替换显示, 保持当前缩放与视野中心。
//!   插图 URL 不支持缩放 (没有 `size=`) 时阅读页拿到的就是原图, 按钮显示 "已是原图"。
//! - 适配全屏用区域平均缩小 (结果缓存); 放大用双线性只采样可见部分 (`Bitmap::sample_into`)。

use std::any::Any;

use kn_platform::{GestureKind, InputEvent, KeyCode};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};

use crate::KinNovel;

const HIT_BACK: HitId = HitId(1);
const HIT_ZOOM_OUT: HitId = HitId(2);
const HIT_ZOOM_IN: HitId = HitId(3);
const HIT_ORIGINAL: HitId = HitId(4);
const HIT_TOOLBAR: HitId = HitId(5);

/// 相对 "适配全屏" 的缩放档位。
const ZOOMS: [f32; 5] = [1.0, 1.5, 2.0, 3.0, 4.0];
/// 适配全屏时最多放大到 2 倍 (小图不糊成一片)。
const MAX_FIT_SCALE: f32 = 2.0;

enum Status {
    Loading,
    Failed(String),
    Ready,
}

enum Original {
    /// 阅读页那份就是原图
    Same,
    NotRequested,
    Loading,
    Loaded,
    Unavailable(String),
}

/// 后台解码结果。
struct Decoded {
    original: bool,
    result: Result<Option<Bitmap>, String>,
}

pub struct ImagePage {
    url: String,
    /// 漫画页面 (缩放版在漫画缓存里, 见 `crate::comic`)
    comic: bool,
    /// 阅读页请求插图时用的高度 (决定缓存里那份缩放版)
    height: u32,
    status: Status,
    original: Original,
    src: Option<Bitmap>,
    /// 适配全屏的缩小结果 (src 变化时作废)
    fit: Option<Bitmap>,
    zoom: usize,
    /// 视野中心 (源图坐标)
    center: (f32, f32),
    toolbar: bool,
}

impl ImagePage {
    pub fn new(url: String, height: u32) -> Self {
        let original = if crate::net::has_original_variant(&url) { Original::NotRequested } else { Original::Same };
        ImagePage { url, comic: false, height, status: Status::Loading, original, src: None, fit: None, zoom: 0, center: (0.0, 0.0), toolbar: true }
    }

    /// 漫画页面的放大预览 (从漫画阅读页长按或 "放大" 进入)。
    pub fn comic(url: String, height: u32) -> Self {
        ImagePage { comic: true, ..ImagePage::new(url, height) }
    }

    fn fit_scale(&self, w: u32, h: u32) -> f32 {
        let Some(src) = &self.src else { return 1.0 };
        (w as f32 / src.width().max(1) as f32).min(h as f32 / src.height().max(1) as f32).min(MAX_FIT_SCALE)
    }

    /// 视野中心夹在图内: 缩放后比屏幕窄/矮的方向居中, 否则不让图边离开屏幕边。
    fn clamp_center(&mut self, w: u32, h: u32) {
        let Some(src) = &self.src else { return };
        let scale = self.fit_scale(w, h) * ZOOMS[self.zoom];
        let (sw, sh) = (src.width() as f32, src.height() as f32);
        let clamp_axis = |c: f32, size: f32, view: f32| {
            let half = view / 2.0 / scale;
            if size * scale <= view { size / 2.0 } else { c.clamp(half, size - half) }
        };
        self.center = (clamp_axis(self.center.0, sw, w as f32), clamp_axis(self.center.1, sh, h as f32));
    }

    fn set_zoom(&mut self, cx: &mut Cx<KinNovel>, zoom: usize) {
        let zoom = zoom.min(ZOOMS.len() - 1);
        if zoom != self.zoom {
            self.zoom = zoom;
            self.clamp_center(cx.width, cx.height);
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn pan(&mut self, cx: &mut Cx<KinNovel>, dx: f32, dy: f32) {
        if self.zoom == 0 || self.src.is_none() {
            return;
        }
        let scale = self.fit_scale(cx.width, cx.height) * ZOOMS[self.zoom];
        let before = self.center;
        self.center.0 += dx * cx.width as f32 / 2.0 / scale;
        self.center.1 += dy * cx.height as f32 / 2.0 / scale;
        self.clamp_center(cx.width, cx.height);
        if self.center != before {
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn request_original(&mut self, cx: &mut Cx<KinNovel>) {
        if !matches!(self.original, Original::NotRequested | Original::Unavailable(_)) {
            return;
        }
        self.original = Original::Loading;
        let paths = cx.app.paths.clone();
        let net = cx.app.net();
        let url = self.url.clone();
        cx.spawn(move || Decoded {
            original: true,
            result: crate::net::original_image_bytes(&paths, net.as_ref(), &url)
                .and_then(|bytes| bytes.map(|b| kn_render::decode_gray(&b, None).map_err(|e| e.to_string())).transpose()),
        });
        cx.request_redraw(RefreshHint::Ui);
    }

    fn toolbar_rect(cx: &Cx<KinNovel>) -> Rect {
        let m = cx.app.metrics;
        let pad = m.margin as i32 / 2;
        let h = pad + (m.small * 1.6) as i32 + pad / 2 + m.touch as i32 + pad;
        Rect::new(0, cx.height as i32 - h, cx.width, h as u32)
    }

    fn draw_toolbar(&self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let panel = Self::toolbar_rect(cx);
        frame.fill_rect(panel, theme.background);
        frame.fill_rect(Rect::new(0, panel.y, cx.width, 2), theme.foreground);
        cx.hits.add(HIT_TOOLBAR, panel).no_feedback();
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let side = m.margin as i32;
        let pad = m.margin as i32 / 2;
        let info_h = (m.small * 1.6) as i32;

        // 信息行: 尺寸 · 缩放 · 原图状态
        let mut info = match &self.src {
            Some(src) => format!("{} × {}", src.width(), src.height()),
            None => String::new(),
        };
        if self.zoom > 0 {
            info.push_str(&format!(" · {}×", ZOOMS[self.zoom]));
        }
        let state = match &self.original {
            Original::Same | Original::Loaded => "原图".to_string(),
            Original::NotRequested => "预览".to_string(),
            Original::Loading => "原图加载中…".to_string(),
            Original::Unavailable(e) => format!("原图: {e}"),
        };
        if !info.is_empty() {
            info.push_str(" · ");
        }
        info.push_str(&state);
        let info_rect = Rect::new(side, panel.y + pad, cx.width - 2 * side as u32, info_h as u32);
        let info = ink.fit(&info, m.small, info_rect.w as f32);
        ink.text_centered(frame, info_rect, &info, m.small, theme.muted);

        // 返回 / 缩小 / 放大 / 原图
        let y = info_rect.bottom() + pad / 2;
        let gap = pad;
        let cols = 4;
        let inner_w = cx.width as i32 - 2 * side;
        let col_w = (inner_w - gap * (cols - 1)) / cols;
        let cell = |i: i32| Rect::new(side + i * (col_w + gap), y, col_w as u32, m.touch);
        let ready = matches!(self.status, Status::Ready);
        let can_out = ready && self.zoom > 0;
        let can_in = ready && self.zoom + 1 < ZOOMS.len();
        let (orig_label, can_orig) = match self.original {
            Original::Same | Original::Loaded => ("已是原图", false),
            Original::Loading => ("加载中…", false),
            Original::NotRequested => ("原图", ready),
            Original::Unavailable(_) => ("重试原图", ready),
        };
        let style = |on: bool| if on { ButtonStyle::Secondary } else { ButtonStyle::Disabled };
        widgets::button(&mut ink, frame, &theme, &m, cell(0), "返回", ButtonStyle::Secondary);
        cx.hits.add(HIT_BACK, cell(0)).rounded(m.radius);
        widgets::button(&mut ink, frame, &theme, &m, cell(1), "缩小", style(can_out));
        cx.hits.add(HIT_ZOOM_OUT, cell(1)).rounded(m.radius).enabled(can_out);
        widgets::button(&mut ink, frame, &theme, &m, cell(2), "放大", style(can_in));
        cx.hits.add(HIT_ZOOM_IN, cell(2)).rounded(m.radius).enabled(can_in);
        widgets::button(&mut ink, frame, &theme, &m, cell(3), orig_label, style(can_orig));
        cx.hits.add(HIT_ORIGINAL, cell(3)).rounded(m.radius).enabled(can_orig);
    }
}

impl Page<KinNovel> for ImagePage {
    fn id(&self) -> PageId {
        PageId(11)
    }

    fn enter(&mut self, cx: &mut Cx<KinNovel>, returning: bool) {
        if returning || !matches!(self.status, Status::Loading) {
            return;
        }
        let paths = cx.app.paths.clone();
        let net = cx.app.net();
        let (url, height, comic) = (self.url.clone(), self.height, self.comic);
        cx.spawn(move || Decoded {
            original: false,
            result: if comic { crate::comic::page_bytes(&paths, net.as_ref(), &url, height) } else { crate::net::image_bytes(&paths, net.as_ref(), &url, height) }
                .and_then(|bytes| bytes.map(|b| kn_render::decode_gray(&b, None).map_err(|e| e.to_string())).transpose()),
        });
    }

    fn on_message(&mut self, cx: &mut Cx<KinNovel>, msg: Box<dyn Any + Send>) {
        let Ok(d) = msg.downcast::<Decoded>() else { return };
        match (d.original, d.result) {
            (false, Ok(Some(img))) => {
                self.center = (img.width() as f32 / 2.0, img.height() as f32 / 2.0);
                self.src = Some(img);
                self.status = Status::Ready;
            }
            (false, Ok(None)) => self.status = Status::Failed("图片未缓存 (离线)".into()),
            (false, Err(e)) => self.status = Status::Failed(e),
            (true, Ok(Some(img))) => {
                // 保持视野: 中心按新旧尺寸比例换算, 缩放档位相对适配全屏不变
                if let Some(old) = &self.src {
                    let (rx, ry) = (img.width() as f32 / old.width().max(1) as f32, img.height() as f32 / old.height().max(1) as f32);
                    self.center = (self.center.0 * rx, self.center.1 * ry);
                }
                self.src = Some(img);
                self.fit = None;
                self.original = Original::Loaded;
                self.clamp_center(cx.width, cx.height);
            }
            (true, Ok(None)) => self.original = Original::Unavailable("离线".into()),
            (true, Err(e)) => {
                eprintln!("[image] 原图 {}: {e}", self.url);
                self.original = Original::Unavailable("加载失败".into());
            }
        }
        cx.request_redraw(RefreshHint::Ui);
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let (w, h) = (cx.width, cx.height);
        match &self.status {
            Status::Loading | Status::Failed(_) => {
                let text = match &self.status {
                    Status::Failed(e) => format!("插图无法显示: {e}"),
                    _ => "正在加载插图…".to_string(),
                };
                let chain = cx.app.ui_fonts.clone();
                let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
                let r = Rect::new(m.margin as i32, h as i32 / 2 - m.body as i32, w - 2 * m.margin, (m.body * 1.5) as u32);
                ink.text_centered(frame, r, &text, m.body, theme.foreground);
                self.draw_toolbar(cx, frame);
                return;
            }
            Status::Ready => {}
        }
        let scale = self.fit_scale(w, h) * ZOOMS[self.zoom];
        let Some(src) = &self.src else { return };
        if self.zoom == 0 {
            if self.fit.is_none() {
                let fw = ((src.width() as f32 * scale).round() as u32).max(1);
                let fh = ((src.height() as f32 * scale).round() as u32).max(1);
                self.fit = Some(src.resize(fw, fh));
            }
            let fit = self.fit.as_ref().expect("fit");
            let x = (w as i32 - fit.width() as i32) / 2;
            let y = (h as i32 - fit.height() as i32) / 2;
            frame.blit(fit, fit.bounds(), x, y);
        } else {
            let src_x = self.center.0 - w as f32 / 2.0 / scale;
            let src_y = self.center.1 - h as f32 / 2.0 / scale;
            src.sample_into(frame, Rect::new(0, 0, w, h), scale, src_x, src_y);
        }
        if self.toolbar {
            self.draw_toolbar(cx, frame);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else {
            return match event {
                InputEvent::Key { code: KeyCode::PageForward, pressed: true } => {
                    self.set_zoom(cx, self.zoom + 1);
                    Transition::None
                }
                InputEvent::Key { code: KeyCode::PageBack, pressed: true } => {
                    self.set_zoom(cx, self.zoom.saturating_sub(1));
                    Transition::None
                }
                _ => Transition::None,
            };
        };
        match g.kind {
            GestureKind::Tap => {
                let p = Point { x: g.start.0, y: g.start.1 };
                match cx.hits.at(p) {
                    Some(HIT_BACK) => return Transition::Back,
                    Some(HIT_ZOOM_OUT) => self.set_zoom(cx, self.zoom.saturating_sub(1)),
                    Some(HIT_ZOOM_IN) => self.set_zoom(cx, self.zoom + 1),
                    Some(HIT_ORIGINAL) => self.request_original(cx),
                    Some(_) => {}
                    None => {
                        if matches!(self.status, Status::Ready) {
                            self.toolbar = !self.toolbar;
                            cx.request_redraw(RefreshHint::Ui);
                        }
                    }
                }
            }
            // 手指向左滑 = 内容向左移 = 视野向右
            GestureKind::SwipeLeft => self.pan(cx, 1.0, 0.0),
            GestureKind::SwipeRight => self.pan(cx, -1.0, 0.0),
            GestureKind::SwipeUp => self.pan(cx, 0.0, 1.0),
            GestureKind::SwipeDown => self.pan(cx, 0.0, -1.0),
            _ => {}
        }
        Transition::None
    }
}
