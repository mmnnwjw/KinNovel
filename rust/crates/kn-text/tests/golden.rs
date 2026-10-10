//! 对照 `tools/kn_text_golden.py` 产出的 golden 文件, 校验 extract_blocks +
//! 分页的行为与 Python `reader.py` 一致。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use kn_text::{extract_blocks, paginate_all, wrap::Measure, Block, BlockKind, LayoutParams, Page, PageItem};

const BASE_URL: &str = "https://www.lightnovel.app";

/// 对应 tools/kn_text_golden.py 的合成宽度函数, 不依赖真实字体即可比较算法。
struct GoldenMeasure;
impl Measure for GoldenMeasure {
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

#[derive(Deserialize)]
struct GoldenParams {
    width: i64,
    height: i64,
    font_size: u32,
    line_spacing: f64,
    reader_margin: i64,
    first_line_indent: bool,
}

#[derive(Deserialize)]
struct GoldenFile {
    params: GoldenParams,
    blocks: Vec<Value>,
    pages: Vec<Vec<Value>>,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_chapter_content(stem: &str, dir: &Path) -> String {
    let html_path = dir.join(format!("{}.html", stem));
    if html_path.exists() {
        return fs::read_to_string(html_path).unwrap();
    }
    let json_path = dir.join(format!("{}.json", stem));
    let raw = fs::read_to_string(&json_path).unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    let chapter = v.get("Chapter").cloned().unwrap_or(v);
    chapter
        .get("Content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

fn block_kind_str(k: BlockKind) -> &'static str {
    k.as_str()
}

fn check_blocks(name: &str, blocks: &[Block], golden: &[Value]) {
    assert_eq!(
        blocks.len(),
        golden.len(),
        "{name}: block count differs: got {}, want {}",
        blocks.len(),
        golden.len()
    );
    for (i, (b, g)) in blocks.iter().zip(golden.iter()).enumerate() {
        let got = (
            block_kind_str(b.kind).to_string(),
            b.text.clone(),
            b.level,
            b.path.clone(),
            b.offset,
            b.source_url.clone(),
            b.width,
            b.height,
        );
        let want = (
            g["kind"].as_str().unwrap().to_string(),
            g["text"].as_str().unwrap().to_string(),
            g["level"].as_u64().unwrap() as u8,
            g["path"].as_str().unwrap().to_string(),
            g["offset"].as_u64().unwrap() as usize,
            g["source_url"].as_str().unwrap().to_string(),
            g["width"].as_u64().unwrap() as u32,
            g["height"].as_u64().unwrap() as u32,
        );
        assert_eq!(
            got, want,
            "{name}: block[{i}] differs\n  got:  {:?}\n  want: {:?}",
            got, want
        );
    }
}

fn page_item_tuple(item: &PageItem) -> (String, i64, i64, String, usize, String, String, i64, i64) {
    match item {
        PageItem::Text { text, x, y, size, path, offset } => (
            "text".to_string(),
            *x,
            *y,
            path.clone(),
            *offset,
            text.clone(),
            String::new(),
            *size as i64,
            0,
        ),
        PageItem::Image { url, x, y, width, height, path, offset } => (
            "image".to_string(),
            *x,
            *y,
            path.clone(),
            *offset,
            String::new(),
            url.clone(),
            *width,
            *height,
        ),
    }
}

fn golden_item_tuple(g: &Value) -> (String, i64, i64, String, usize, String, String, i64, i64) {
    let kind = g["type"].as_str().unwrap().to_string();
    let x = g["x"].as_i64().unwrap();
    let y = g["y"].as_i64().unwrap();
    let path = g["path"].as_str().unwrap().to_string();
    let offset = g["offset"].as_u64().unwrap() as usize;
    if kind == "text" {
        (
            kind,
            x,
            y,
            path,
            offset,
            g["text"].as_str().unwrap().to_string(),
            String::new(),
            g["size"].as_i64().unwrap(),
            0,
        )
    } else {
        (
            kind,
            x,
            y,
            path,
            offset,
            String::new(),
            g["url"].as_str().unwrap().to_string(),
            g["width"].as_i64().unwrap(),
            g["height"].as_i64().unwrap(),
        )
    }
}

fn check_pages(name: &str, pages: &[Page], golden: &[Vec<Value>]) {
    assert_eq!(
        pages.len(),
        golden.len(),
        "{name}: page count differs: got {}, want {}",
        pages.len(),
        golden.len()
    );
    for (pi, (page, gpage)) in pages.iter().zip(golden.iter()).enumerate() {
        assert_eq!(
            page.len(),
            gpage.len(),
            "{name}: page[{pi}] item count differs: got {}, want {}",
            page.len(),
            gpage.len()
        );
        for (ii, (item, gitem)) in page.iter().zip(gpage.iter()).enumerate() {
            let got = page_item_tuple(item);
            let want = golden_item_tuple(gitem);
            assert_eq!(
                got, want,
                "{name}: page[{pi}] item[{ii}] differs\n  got:  {:?}\n  want: {:?}",
                got, want
            );
        }
    }
}

fn run_golden(stem: &str, dir: &Path) {
    let golden_path = dir.join(format!("{}.golden.json", stem));
    let golden: GoldenFile = serde_json::from_str(&fs::read_to_string(&golden_path).unwrap()).unwrap();

    let content = load_chapter_content(stem, dir);
    let blocks = extract_blocks(&content, BASE_URL);
    check_blocks(stem, &blocks, &golden.blocks);

    let params = LayoutParams {
        width: golden.params.width,
        height: golden.params.height,
        font_size: golden.params.font_size,
        line_spacing: golden.params.line_spacing,
        margin: golden.params.reader_margin,
        first_line_indent: golden.params.first_line_indent,
    };
    let mut m = GoldenMeasure;
    let pages = paginate_all(&blocks, params, &mut m);
    check_pages(stem, &pages, &golden.pages);
}

#[test]
fn synthetic1_matches_python_golden() {
    run_golden("synthetic1", &fixtures_dir());
}

#[test]
fn synthetic2_matches_python_golden() {
    run_golden("synthetic2", &fixtures_dir());
}

#[test]
fn private_chapters_match_python_golden_if_present() {
    let dir = fixtures_dir().join("private");
    if !dir.exists() {
        eprintln!("skipping private fixtures: {} not present", dir.display());
        return;
    }
    let mut stems: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".golden.json").map(|s| s.to_string())
        })
        .collect();
    stems.sort();
    assert!(!stems.is_empty(), "private fixtures dir exists but has no .golden.json files");
    for stem in stems {
        run_golden(&stem, &dir);
    }
}

#[test]
fn progressive_paginator_matches_paginate_all() {
    for (stem, dir) in [
        ("synthetic1".to_string(), fixtures_dir()),
        ("synthetic2".to_string(), fixtures_dir()),
    ] {
        let content = load_chapter_content(&stem, &dir);
        let blocks = extract_blocks(&content, BASE_URL);
        let params = LayoutParams::default();
        let mut m1 = GoldenMeasure;
        let all = paginate_all(&blocks, params.clone(), &mut m1);

        let mut m2 = GoldenMeasure;
        let mut paginator = kn_text::Paginator::new(blocks.clone(), params);
        let mut progressive = Vec::new();
        while let Some(p) = paginator.next_page(&mut m2) {
            progressive.push(p);
        }
        assert_eq!(all, progressive, "{stem}: progressive paginator diverges from paginate_all");
    }
}
