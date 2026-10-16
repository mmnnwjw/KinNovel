//! 横屏软件旋转的拷贝耗时: 整屏 1648x1236 逻辑帧 → 面板方向 (15 次取中位数)。
//! 设备上: `cargo zigbuild --release --target armv7-unknown-linux-musleabihf -p kn-platform --example rotbench`,
//! 拷到 /tmp 运行。KPW5: q=1 ~22 ms, q=3 ~25 ms, q=2 ~8 ms。
use std::time::Instant;

use kn_platform::Rotation;
use kn_render::{Bitmap, Rect};

fn main() {
    let frame = Bitmap::new(1648, 1236, 0x80);
    let mut out = Vec::new();
    for q in [1u8, 3, 2] {
        let r = Rotation::new(q);
        let (w, h) = r.logical_size((1236, 1648));
        let frame = if q == 2 { Bitmap::new(w, h, 0x80) } else { frame.clone() };
        let mut times: Vec<f64> = (0..15)
            .map(|_| {
                let t = Instant::now();
                r.copy_region(&frame, Rect::new(0, 0, w, h), &mut out);
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("q={q}: median {:.1} ms", times[7]);
    }
}
