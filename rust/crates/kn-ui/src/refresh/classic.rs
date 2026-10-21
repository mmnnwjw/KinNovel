//! v1.6.4 的 UI 定点清理, 仅在选择经典模式时调用。
use kn_platform::{RefreshRequest, Waveform};
use kn_render::{Bitmap, Rect};
use super::REGION_PAD;

/// 定点清残影的格子边长 (px)。
const CLEAR_TILE: u32 = 16;
/// 格子里至少这么多像素由深色变成背景色, 才算 "实心深色块被清掉"。正文/列表文字远达不到。
const CLEAR_TILE_FILL: u32 = CLEAR_TILE * CLEAR_TILE / 2;
/// 清掉的实心块至少占整屏这个比例才单独闪刷 (KPW5: 键盘按键 ~0.45%, 筛选按钮 ~0.75%, 底部标签 ~1.3%)。
const CLEAR_MIN_RATIO: f32 = 0.006;
/// 按下反馈区域小于整屏这个比例时, 其中的反相还原不触发闪刷 (键盘按键、小按钮)。
const FEEDBACK_SMALL_RATIO: f32 = 0.02;
/// 清掉的块占主刷新区域这个比例以上: 主刷新直接改成区域闪刷, 省一次刷新。
const CLEAR_TAKEOVER_RATIO: f32 = 0.5;
/// 单独闪刷的块最多几个, 再多就合并成一个包围盒。
const CLEAR_MAX_RECTS: usize = 4;
/// 深/浅的判定阈值 (L8)。
const DARK_MAX: u8 = 96;
const LIGHT_MIN: u8 = 160;

pub(super) fn threshold(configured: f32, light_background: bool) -> f32 {
    configured.min(if light_background { 3.0 } else { 1.0 })
}

fn request(rect: Rect, waveform: Waveform, flash: bool) -> RefreshRequest {
    RefreshRequest { rect, waveform, flash, swipe: None }
}

pub(super) fn plan(old: &Bitmap, frame: &Bitmap, regions: Vec<Rect>, waveform: Waveform, light_background: bool, feedback: Option<Rect>) -> Vec<RefreshRequest> {
    let full = frame.bounds();
    let full_area = full.area().max(1) as f32;
    // Ui / Fast: 定点清残影
    let ignore = feedback.filter(|r| (r.area() as f32) < full_area * FEEDBACK_SMALL_RATIO);
    let min_area = (full_area * CLEAR_MIN_RATIO) as u64;
    let mut out = Vec::with_capacity(regions.len());
    let mut blocks = Vec::new();
    for r in regions {
        let found = cleared_blocks(old, frame, r, light_background, ignore, min_area);
        let cleared: u64 = found.iter().map(|b| b.area()).sum();
        if cleared as f32 >= r.area() as f32 * CLEAR_TAKEOVER_RATIO {
            out.push(request(r, Waveform::Gc16, true));
        } else {
            out.push(request(r, waveform, false));
            blocks.extend(found);
        }
    }
    if blocks.len() > CLEAR_MAX_RECTS {
        let union = blocks.iter().skip(1).fold(blocks[0], |acc, b| acc.union(b));
        blocks = vec![union];
    }
    out.extend(
        blocks
            .into_iter()
            .filter_map(|b| b.inflate(REGION_PAD).intersect(&full))
            .map(|b| request(b, Waveform::Gc16, true)),
    );
    out
}

/// 在 `rect` 内找 "实心深色块被清成背景色" 的连通块 (见模块说明), 返回各块包围盒 (已裁到 `rect`)。
fn cleared_blocks(old: &Bitmap, new: &Bitmap, rect: Rect, light_bg: bool, ignore: Option<Rect>, min_area: u64) -> Vec<Rect> {
    let t = CLEAR_TILE as i32;
    let (tx0, ty0) = (rect.x / t, rect.y / t);
    let (tx1, ty1) = ((rect.right() + t - 1) / t, (rect.bottom() + t - 1) / t);
    let (cols, rows) = ((tx1 - tx0) as usize, (ty1 - ty0) as usize);
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let mut counts = vec![0u32; cols * rows];
    // 逐行把命中 (0/1) 累加到按列的 u8 累加器 (整行一个长循环, 便于 NEON 向量化), 每满一行格子再按格汇总。
    // 逐格切片计数在 KPW5 上让整屏计划多花 ~15 ms。
    let (x0, x1) = (rect.x as usize, rect.right() as usize);
    let mut acc = vec![0u8; x1 - x0];
    let flush = |acc: &mut [u8], tile_row: usize, counts: &mut [u32]| {
        for (c, count) in counts[tile_row * cols..][..cols].iter_mut().enumerate() {
            let from = (((tx0 + c as i32) * t) as usize).max(x0) - x0;
            let to = (((tx0 + c as i32 + 1) * t) as usize).min(x1) - x0;
            *count += acc[from..to].iter().map(|&v| v as u32).sum::<u32>();
        }
        acc.fill(0);
    };
    let flip = if light_bg { 0u8 } else { 255u8 };
    for y in rect.y..rect.bottom() {
        let a = &old.row(y as u32)[x0..x1];
        let b = &new.row(y as u32)[x0..x1];
        // 夜间模式取反后同一判断: 浅色块变黑
        for ((v, &o), &n) in acc.iter_mut().zip(a).zip(b) {
            *v += (((o ^ flip) <= DARK_MAX) & ((n ^ flip) >= LIGHT_MIN)) as u8;
        }
        if (y + 1) % t == 0 || y + 1 == rect.bottom() {
            flush(&mut acc, (y / t - ty0) as usize, &mut counts);
        }
    }
    let mut solid: Vec<bool> = counts.iter().map(|&n| n >= CLEAR_TILE_FILL).collect();
    if let Some(ig) = ignore {
        for (i, s) in solid.iter_mut().enumerate() {
            let cx = (tx0 + (i % cols) as i32) * t + t / 2;
            let cy = (ty0 + (i / cols) as i32) * t + t / 2;
            if ig.contains(kn_render::Point { x: cx, y: cy }) {
                *s = false;
            }
        }
    }
    // 8 邻域连通块
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..solid.len() {
        if !solid[start] {
            continue;
        }
        solid[start] = false;
        stack.push(start);
        let (mut c0, mut c1, mut r0, mut r1, mut n) = (cols, 0, rows, 0, 0u64);
        while let Some(i) = stack.pop() {
            let (c, r) = (i % cols, i / cols);
            (c0, c1, r0, r1) = (c0.min(c), c1.max(c), r0.min(r), r1.max(r));
            n += 1;
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    let (nc, nr) = (c as i32 + dc, r as i32 + dr);
                    if nc < 0 || nr < 0 || nc >= cols as i32 || nr >= rows as i32 {
                        continue;
                    }
                    let j = nr as usize * cols + nc as usize;
                    if solid[j] {
                        solid[j] = false;
                        stack.push(j);
                    }
                }
            }
        }
        if n * (t * t) as u64 >= min_area {
            let block = Rect::new(
                (tx0 + c0 as i32) * t,
                (ty0 + r0 as i32) * t,
                ((c1 - c0 + 1) as i32 * t) as u32,
                ((r1 - r0 + 1) as i32 * t) as u32,
            );
            out.extend(block.intersect(&rect));
        }
    }
    out
}
