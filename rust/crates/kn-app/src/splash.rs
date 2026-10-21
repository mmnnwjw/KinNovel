//! 启动画面: FBInk 初始化后立刻画一张像素画 (仿轻书架网页图标的扁平书本: 两本立着、一本斜靠 + 像素字 "KinNovel" 和版本号),
//! 然后才加载字体、配置等; 首页的第一帧 (整屏闪刷) 把它换掉。KOReader 启动时同样先在屏幕上打一张图。
//!
//! 这时还没有字体, 所以画和字都是编进程序的像素点阵: 图 44x30 格 (`#` 黑, `o` 深灰, `+` 浅灰),
//! 字是 5x7 点阵。格子按屏宽取整数像素, 墨水屏上边缘清楚。`ART` 由 `tools/gen_splash_art.py` 生成
//! (改图改脚本, 再把输出贴回来)。

use kn_render::{Bitmap, Rect};

const ART: [&str; 30] = [
    ".........ooooooooo.................",
    ".........oooooooooo................",
    "........ooooooooooo................",
    "........ooooooooooo................",
    "........oo+++++++oo................",
    "........oo+++++++oo................",
    ".#####..ooooooooooo................",
    "#######.ooooooooooo................",
    "#######.ooooooooooo.....++.........",
    "##ooo##.ooooooooooo...+++++........",
    "#######.ooooooooooo.++++++++.......",
    "#######.ooooooooooo.++++++++.......",
    "#######.ooooooooooo.+++++++++......",
    "#######.ooooooooooo..++++++++......",
    "#######.ooooooooooo..+++++++++.....",
    "#######.ooooooooooo...++++++++.....",
    "#######.ooooooooooo...+++++++++....",
    "#######.ooooooooooo....++++++++....",
    "#######.ooooooooooo....+++++++++...",
    "#######.ooooooooooo.....++++++++...",
    "#######.ooooooooooo.....+++++++++..",
    "#######.ooooooooooo......++++++++..",
    "#######.ooooooooooo......+++++++++.",
    "#######.ooooooooooo.......++++++++.",
    "#######.oo+++++++oo.......+++++++++",
    "#######.oo+++++++oo........++++++++",
    "#######.ooooooooooo........++++++++",
    "#######.ooooooooooo.........+++++++",
    "#######..oooooooooo.........+++++..",
    ".#####...ooooooooo...........++....",
];

/// 5x7 点阵 (每行低 5 位, 高位在左); 没有的字符画成空白。
fn glyph(c: char) -> [u8; 7] {
    match c {
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'i' => [0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'n' => [0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        'o' => [0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
        'v' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'e' => [0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110],
        'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        _ => [0; 7],
    }
}

const BLACK: u8 = 0x00;
const DARK: u8 = 0x55;
const LIGHT: u8 = 0xAA;
const WHITE: u8 = 0xFF;

/// 一行像素字的宽度 (格): 每字 5 格 + 1 格间距, 最后一个字不带间距。
fn text_cells(text: &str) -> i32 {
    (text.chars().count() as i32 * 6 - 1).max(0)
}

fn draw_text(frame: &mut Bitmap, text: &str, x: i32, y: i32, cell: i32, color: u8) {
    for (i, c) in text.chars().enumerate() {
        let gx = x + i as i32 * 6 * cell;
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    frame.fill_rect(Rect::new(gx + col * cell, y + row as i32 * cell, cell as u32, cell as u32), color);
                }
            }
        }
    }
}

/// 整屏的启动画面。
pub fn render(width: u32, height: u32) -> Bitmap {
    let mut frame = Bitmap::new(width, height, WHITE);
    let art_w = ART[0].len() as i32;
    let art_h = ART.len() as i32;
    // 图占屏宽约 60%, 每格整数像素 (KPW5: 20 px)
    let cell = ((width as f32 * 0.6) / art_w as f32).floor().max(2.0) as i32;
    let title_cell = (cell * 2 / 3).max(2);
    let version_cell = (cell / 3).max(1);
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let gap = cell * 3;
    let version_gap = title_cell * 2;
    let total_h = art_h * cell + gap + 7 * title_cell + version_gap + 7 * version_cell;
    // 整体略高于正中 (视觉中心)
    let top = ((height as i32 - total_h) * 9 / 20).max(0);

    let left = (width as i32 - art_w * cell) / 2;
    for (r, row) in ART.iter().enumerate() {
        for (c, ch) in row.bytes().enumerate() {
            let color = match ch {
                b'#' => BLACK,
                b'o' => DARK,
                b'+' => LIGHT,
                _ => continue,
            };
            frame.fill_rect(Rect::new(left + c as i32 * cell, top + r as i32 * cell, cell as u32, cell as u32), color);
        }
    }

    let mut y = top + art_h * cell + gap;
    let tx = (width as i32 - text_cells("KinNovel") * title_cell) / 2;
    // 浅灰投影 + 黑字
    draw_text(&mut frame, "KinNovel", tx + title_cell, y + title_cell, title_cell, LIGHT);
    draw_text(&mut frame, "KinNovel", tx, y, title_cell, BLACK);
    y += 7 * title_cell + version_gap;
    let vx = (width as i32 - text_cells(&version) * version_cell) / 2;
    draw_text(&mut frame, &version, vx, y, version_cell, DARK);
    frame
}
