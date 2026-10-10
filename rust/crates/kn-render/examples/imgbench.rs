//! 设备上测图片解码耗时: `imgbench <file> [target_w target_h]`。
//! 依次测: 全尺寸解码、带 DCT 缩放解码 (若给了目标尺寸)、缩放到目标尺寸 (fit_within)。

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: imgbench <file> [w h]");
    let bytes = std::fs::read(path).expect("read");
    let target = match (args.get(2), args.get(3)) {
        (Some(w), Some(h)) => Some((w.parse().unwrap(), h.parse().unwrap())),
        _ => None,
    };
    let t = Instant::now();
    let full = kn_render::decode_gray(&bytes, None).expect("decode");
    println!("full decode {}x{}: {:?}", full.width(), full.height(), t.elapsed());
    if let Some((w, h)) = target {
        let t = Instant::now();
        let scaled = kn_render::decode_gray(&bytes, Some((w, h))).expect("decode");
        let decoded = t.elapsed();
        let fitted = scaled.fit_within(w, h);
        println!(
            "scaled decode {}x{}: {:?}, fit to {}x{}: {:?} total",
            scaled.width(),
            scaled.height(),
            decoded,
            fitted.width(),
            fitted.height(),
            t.elapsed()
        );
    }
}
