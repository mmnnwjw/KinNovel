//! 主机上的粗略计时 (不代表 Kindle 上的数字):
//! `cargo-zigbuild test --release --target x86_64-pc-windows-gnu -p kn-text --test bench -- --ignored --nocapture`

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use kn_text::{extract_document, paginate_all, LayoutParams, Measure};

struct Fixed;
impl Measure for Fixed {
    fn char_width(&mut self, size: u32, ch: char) -> f32 {
        if ch == ' ' {
            size as f32 * 0.25
        } else if (ch as u32) < 0x2E80 {
            size as f32 * 0.5
        } else {
            size as f32
        }
    }
}

fn chapters() -> Vec<String> {
    let mut out = Vec::new();
    for dir in ["tests/fixtures", "tests/fixtures/private"] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.contains(".golden.") {
                continue;
            }
            if name.ends_with(".html") {
                out.push(fs::read_to_string(&p).unwrap());
            } else if name.ends_with(".json") {
                let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
                let c = v.get("Chapter").cloned().unwrap_or(v);
                out.push(c.get("Content").and_then(|c| c.as_str()).unwrap_or("").to_string());
            }
        }
    }
    out
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

#[test]
#[ignore]
fn extract_and_paginate_timing() {
    let chapters = chapters();
    for rich in [false, true] {
        let mut extract = Vec::new();
        let mut layout = Vec::new();
        for _ in 0..15 {
            let t = Instant::now();
            let docs: Vec<_> = chapters.iter().map(|c| extract_document(c, "https://www.lightnovel.life", rich)).collect();
            extract.push(t.elapsed());
            let t = Instant::now();
            for d in &docs {
                let params = LayoutParams { rich, strict_kinsoku: rich, ..LayoutParams::default() };
                std::hint::black_box(paginate_all(&d.blocks, params, &mut Fixed));
            }
            layout.push(t.elapsed());
        }
        println!(
            "{} 章 rich={rich}: extract {:?}, paginate {:?} (中位数, 15 次)",
            chapters.len(),
            median(extract),
            median(layout)
        );
    }
}
