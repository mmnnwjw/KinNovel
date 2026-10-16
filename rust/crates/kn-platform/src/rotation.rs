//! 软件旋转 (横屏阅读、倒拿)。帧缓冲始终保持开机时的竖屏方向, 上层按逻辑画面 (横屏时宽高互换) 画图,
//! 显示层把区域转好再写进帧缓冲, 输入层把触摸坐标转回逻辑画面。KOReader 在 Kindle 上同样是软件旋转。
//!
//! `Rotation(q)`: 逻辑画面相对竖屏顺时针转了 q 个 90°。
//! - 0: 竖屏; 2: 倒置
//! - 1: 逻辑画面的上边在面板右侧 = 用户把设备向左转 (逆时针) 横拿
//! - 3: 逻辑画面的上边在面板左侧 = 向右转 (顺时针) 横拿
//!
//! 记号: 面板 (物理) 尺寸 W x H, 逻辑尺寸 q 为奇数时是 H x W。

use kn_render::{Bitmap, Rect};

use crate::gesture::{Gesture, GestureKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rotation(u8);

impl Rotation {
    pub const UPRIGHT: Rotation = Rotation(0);

    pub fn new(quarter_turns: u8) -> Self {
        Rotation(quarter_turns % 4)
    }

    pub fn quarter_turns(self) -> u8 {
        self.0
    }

    /// 两个旋转叠加。
    pub fn then(self, other: Rotation) -> Rotation {
        Rotation::new(self.0 + other.0)
    }

    /// 反向旋转。
    pub fn inverse(self) -> Rotation {
        Rotation::new(4 - self.0)
    }

    /// 横屏 (逻辑宽高与面板互换)。
    pub fn swaps_axes(self) -> bool {
        self.0 % 2 == 1
    }

    /// 面板尺寸 → 逻辑尺寸。
    pub fn logical_size(self, panel: (u32, u32)) -> (u32, u32) {
        if self.swaps_axes() {
            (panel.1, panel.0)
        } else {
            panel
        }
    }

    /// 逻辑区域 → 面板区域 (`panel` = 面板宽高)。
    pub fn to_panel_rect(self, r: Rect, panel: (u32, u32)) -> Rect {
        let (w, h) = (panel.0 as i32, panel.1 as i32);
        match self.0 {
            0 => r,
            1 => Rect::new(w - r.y - r.h as i32, r.x, r.h, r.w),
            2 => Rect::new(w - r.x - r.w as i32, h - r.y - r.h as i32, r.w, r.h),
            _ => Rect::new(r.y, h - r.x - r.w as i32, r.h, r.w),
        }
    }

    /// 面板坐标 → 逻辑坐标。
    pub fn from_panel_point(self, p: (i32, i32), panel: (u32, u32)) -> (i32, i32) {
        let (w, h) = (panel.0 as i32, panel.1 as i32);
        let (px, py) = p;
        match self.0 {
            0 => p,
            1 => (py, w - 1 - px),
            2 => (w - 1 - px, h - 1 - py),
            _ => (h - 1 - py, px),
        }
    }

    /// 逻辑坐标 → 面板坐标。
    pub fn to_panel_point(self, p: (i32, i32), panel: (u32, u32)) -> (i32, i32) {
        let logical = self.logical_size(panel);
        self.inverse().from_panel_point(p, logical)
    }

    /// 面板上识别出的手势 → 逻辑画面里的手势 (坐标与滑动方向)。
    pub fn from_panel_gesture(self, g: Gesture, panel: (u32, u32)) -> Gesture {
        if self.0 == 0 {
            return g;
        }
        // 滑动方向按 90° 一步转: 面板上向右滑, q=1 时在逻辑画面里是向上
        let mut kind = g.kind;
        for _ in 0..self.0 {
            kind = match kind {
                GestureKind::SwipeRight => GestureKind::SwipeUp,
                GestureKind::SwipeUp => GestureKind::SwipeLeft,
                GestureKind::SwipeLeft => GestureKind::SwipeDown,
                GestureKind::SwipeDown => GestureKind::SwipeRight,
                k => k,
            };
        }
        Gesture { kind, start: self.from_panel_point(g.start, panel), end: self.from_panel_point(g.end, panel), ..g }
    }

    /// 把逻辑帧 `frame` 的区域 `rect` (逻辑坐标, 已裁剪) 转成面板方向, 按行写进 `out`
    /// (`to_panel_rect(rect).w * h` 个字节)。
    ///
    /// 横屏按 8x8 字节块转置: 顺序读源图相邻 8 行各 8 字节, 在寄存器里转置, 写到输出的 8 行。
    /// KPW5 (Cortex-A53) 上跨行跳着读很慢 (整屏只读就 ~14 ms, 顺序读 1.6 ms), 跳着写却快 (~1.4 ms),
    /// 所以让读顺序进行。逐像素实现整屏要 ~28 ms。
    pub fn copy_region(self, frame: &Bitmap, rect: Rect, out: &mut Vec<u8>) {
        let panel = self.inverse().logical_size((frame.width(), frame.height()));
        let pr = self.to_panel_rect(rect, panel);
        let (pw, ph) = (pr.w as usize, pr.h as usize);
        out.clear();
        out.resize(pw * ph, 0);
        let stride = frame.stride();
        let src = frame.data();
        // 逻辑像素 (lx, ly) 在输出里的下标
        let dst = |lx: usize, ly: usize| -> usize {
            let (px, py) = self.to_panel_point((lx as i32, ly as i32), panel);
            (py - pr.y) as usize * pw + (px - pr.x) as usize
        };
        let (x0, y0) = (rect.x as usize, rect.y as usize);
        let (rw, rh) = (rect.w as usize, rect.h as usize);
        if !self.swaps_axes() {
            // 竖屏 / 倒置: 每行是源图一行的一段 (倒置时倒序)
            for ly in y0..y0 + rh {
                let row = &src[ly * stride + x0..ly * stride + x0 + rw];
                let o = dst(if self.0 == 0 { x0 } else { x0 + rw - 1 }, ly);
                let out_row = &mut out[o..o + rw];
                out_row.copy_from_slice(row);
                if self.0 != 0 {
                    out_row.reverse();
                }
            }
            return;
        }
        let (full_w, full_h) = (rw / 8 * 8, rh / 8 * 8);
        let q1 = self.0 == 1;
        for by in (0..full_h).step_by(8) {
            for bx in (0..full_w).step_by(8) {
                let (lx, ly) = (x0 + bx, y0 + by);
                let mut m = [0u64; 8];
                for (r, v) in m.iter_mut().enumerate() {
                    let i = (ly + r) * stride + lx;
                    *v = u64::from_le_bytes(src[i..i + 8].try_into().unwrap());
                }
                // 转置后 m[c] 的第 r 字节 = 源图 (lx + c, ly + r), 也就是输出里的一行 8 个像素:
                // q=1 时这一行从右往左是 r = 0..8 (要倒序), q=3 时从左往右
                transpose8(&mut m);
                for (c, v) in m.iter().enumerate() {
                    let (bytes, first) = if q1 { (v.swap_bytes(), ly + 7) } else { (*v, ly) };
                    let o = dst(lx + c, first);
                    out[o..o + 8].copy_from_slice(&bytes.to_le_bytes());
                }
            }
        }
        // 右边与下边不足 8 的部分逐点
        for ly in y0..y0 + rh {
            let from = if ly - y0 < full_h { x0 + full_w } else { x0 };
            for lx in from..x0 + rw {
                out[dst(lx, ly)] = src[ly * stride + lx];
            }
        }
    }
}

/// 8x8 字节矩阵转置: `m[i]` 的第 b 个字节 (小端) 是第 i 行第 b 列; 逐级交换 4x4、2x2、1x1 的对角外块。
fn transpose8(m: &mut [u64; 8]) {
    for i in 0..4 {
        let t = ((m[i] >> 32) ^ m[i + 4]) & 0x0000_0000_FFFF_FFFF;
        m[i] ^= t << 32;
        m[i + 4] ^= t;
    }
    for i in [0, 1, 4, 5] {
        let t = ((m[i] >> 16) ^ m[i + 2]) & 0x0000_FFFF_0000_FFFF;
        m[i] ^= t << 16;
        m[i + 2] ^= t;
    }
    for i in [0, 2, 4, 6] {
        let t = ((m[i] >> 8) ^ m[i + 1]) & 0x00FF_00FF_00FF_00FF;
        m[i] ^= t << 8;
        m[i + 1] ^= t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个像素的值都和邻居不同 (行列方向都不周期性重复)
    fn numbered(w: u32, h: u32) -> Bitmap {
        let mut b = Bitmap::new(w, h, 0);
        let stride = b.stride();
        for y in 0..h as usize {
            for x in 0..w as usize {
                b.data_mut()[y * stride + x] = (x * 31 + y * 17 + x * y / 7) as u8;
            }
        }
        b
    }

    #[test]
    fn points_and_rects_round_trip() {
        let panel = (6, 10);
        for q in 0..4 {
            let r = Rotation::new(q);
            let (lw, lh) = r.logical_size(panel);
            for (x, y) in [(0, 0), (lw as i32 - 1, 0), (2, lh as i32 - 1), (3, 4)] {
                let p = r.to_panel_point((x, y), panel);
                assert!(p.0 >= 0 && p.0 < 6 && p.1 >= 0 && p.1 < 10, "q={q} {p:?}");
                assert_eq!(r.from_panel_point(p, panel), (x, y), "q={q}");
                // 1x1 区域与点的变换一致
                let pr = r.to_panel_rect(Rect::new(x, y, 1, 1), panel);
                assert_eq!((pr.x, pr.y), p, "q={q}");
            }
        }
        // q=1: 逻辑左上角在面板右上角
        assert_eq!(Rotation::new(1).to_panel_point((0, 0), panel), (5, 0));
        assert_eq!(Rotation::new(3).to_panel_point((0, 0), panel), (0, 9));
    }

    #[test]
    fn copy_region_matches_point_mapping() {
        // 小图走逐点路径; 大图横屏时走 8 行一组的路径 (含剩余行)
        for (panel, q) in [(6u32, 10u32), (83, 150)].into_iter().flat_map(|p| (0..4).map(move |q| (p, q))) {
            let r = Rotation::new(q);
            let (lw, lh) = r.logical_size(panel);
            let frame = numbered(lw, lh);
            let rect = Rect::new(1, 2, lw - 3, lh - 4);
            let mut out = Vec::new();
            r.copy_region(&frame, rect, &mut out);
            let pr = r.to_panel_rect(rect, panel);
            assert_eq!(out.len(), (pr.w * pr.h) as usize);
            for y in 0..pr.h as i32 {
                for x in 0..pr.w as i32 {
                    let (lx, ly) = r.from_panel_point((pr.x + x, pr.y + y), panel);
                    assert_eq!(out[(y * pr.w as i32 + x) as usize], frame.get(lx as u32, ly as u32), "q={q} ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn swipes_follow_the_rotation() {
        let g = |kind| Gesture { kind, start: (0, 0), end: (0, 0), duration_ms: 0 };
        // 向左转横拿 (q=1): 逻辑 x = 面板 y, 面板上向下滑 (y 增大) 在用户眼中是向右滑
        assert_eq!(Rotation::new(1).from_panel_gesture(g(GestureKind::SwipeDown), (6, 10)).kind, GestureKind::SwipeRight);
        assert_eq!(Rotation::new(1).from_panel_gesture(g(GestureKind::SwipeRight), (6, 10)).kind, GestureKind::SwipeUp);
        assert_eq!(Rotation::new(3).from_panel_gesture(g(GestureKind::SwipeDown), (6, 10)).kind, GestureKind::SwipeLeft);
        assert_eq!(Rotation::new(2).from_panel_gesture(g(GestureKind::SwipeLeft), (6, 10)).kind, GestureKind::SwipeRight);
        assert_eq!(Rotation::new(1).from_panel_gesture(g(GestureKind::Tap), (6, 10)).kind, GestureKind::Tap);
    }
}
