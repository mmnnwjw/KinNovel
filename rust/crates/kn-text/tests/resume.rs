//! 有阅读进度时只排到锚点所在的页 (`AnchorIndex`): 得到的页码与页面内容必须与整章排完的结果完全相同。

use std::fs;
use std::path::Path;

use serde_json::Value;

use kn_text::{
    extract_blocks, extract_document, first_anchor_on_page, page_for_path, paginate_all, AnchorIndex, Block, BlockKind, LayoutParams, Measure, Page,
    Paginator,
};

const BASE: &str = "https://www.lightnovel.life";

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

fn chapters() -> Vec<(String, String)> {
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
            let content = if name.ends_with(".html") {
                fs::read_to_string(&p).unwrap()
            } else if name.ends_with(".json") {
                let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
                let c = v.get("Chapter").cloned().unwrap_or(v);
                c.get("Content").and_then(|c| c.as_str()).unwrap_or("").to_string()
            } else {
                continue;
            };
            out.push((name, content));
        }
    }
    assert!(out.len() >= 2, "fixtures missing");
    out
}

/// 阅读页的做法: 排到所有锚点都确定为止 (至少一页), 多个锚点取最远页。返回 (已排的页, 目标页)。
fn resume(blocks: &[Block], params: &LayoutParams, anchors: &[(String, Option<i64>)]) -> (Vec<Page>, usize) {
    let mut paginator = Paginator::new(blocks, params.clone());
    let mut index = AnchorIndex::new(blocks);
    let mut pages = Vec::new();
    let mut done = false;
    while !done && (pages.is_empty() || !anchors.iter().all(|(p, _)| index.settled(p, done))) {
        match paginator.next_page(&mut Fixed) {
            Some(page) => {
                index.add_page(&page);
                pages.push(page);
            }
            None => done = true,
        }
    }
    let target = anchors
        .iter()
        .map(|(path, offset)| page_for_path(&pages, path, *offset, usize::MAX))
        .filter(|&p| p != usize::MAX)
        .max()
        .unwrap_or(0);
    (pages, target)
}

fn full_target(pages: &[Page], anchors: &[(String, Option<i64>)]) -> usize {
    anchors
        .iter()
        .map(|(path, offset)| page_for_path(pages, path, *offset, usize::MAX))
        .filter(|&p| p != usize::MAX)
        .max()
        .unwrap_or(0)
}

fn layouts() -> Vec<(bool, LayoutParams)> {
    vec![
        (false, LayoutParams::default()),
        // 阅读页的参数 (网页样式模式)
        (
            true,
            LayoutParams { rich: true, real_image_size: true, strict_kinsoku: true, cover_full_page: true, paragraph_spacing: 0.2, ..LayoutParams::default() },
        ),
        // 大字号、窄屏: 页数多
        (true, LayoutParams { rich: true, width: 600, height: 760, font_size: 40, ..LayoutParams::default() }),
    ]
}

#[test]
fn resumed_page_matches_full_pagination() {
    let mut partial_seen = false;
    for (name, content) in chapters() {
        for (rich, params) in layouts() {
            let blocks = if rich { extract_document(&content, BASE, true).blocks } else { extract_blocks(&content, BASE) };
            let full = paginate_all(&blocks, params.clone(), &mut Fixed);
            let n = full.len();
            // 锚点: 开头 / 中间 / 结尾页的首个锚点, 页中间某行的锚点, 块的开头, 脚注, 不带偏移 (服务器进度), 找不到的 XPath
            let mut anchors: Vec<Vec<(String, Option<i64>)>> = Vec::new();
            let step = (n / 12).max(1);
            for i in (0..n).step_by(step).chain([n / 2, n - 1]) {
                let (path, offset) = first_anchor_on_page(&full, i as i64);
                anchors.push(vec![(path.clone(), Some(offset as i64))]);
                anchors.push(vec![(path, None)]);
                // 页里最后一行 (段落跨页时偏移大于页首)
                if let Some(item) = full[i].iter().rev().find(|it| !it.path().is_empty()) {
                    anchors.push(vec![(item.path().to_string(), Some(item.offset() as i64 + 1))]);
                }
            }
            for b in blocks.iter().step_by((blocks.len() / 10).max(1)).chain(blocks.iter().filter(|b| b.kind == BlockKind::Footnote).take(2)) {
                anchors.push(vec![(b.path.clone(), Some(b.offset as i64))]);
            }
            anchors.push(vec![("/body/nope[9]".into(), Some(3))]);
            anchors.push(vec![(String::new(), None)]);
            // 多端进度: 一个靠前一个靠后, 取最远
            let (a, _) = first_anchor_on_page(&full, (n / 4) as i64);
            let (b, bo) = first_anchor_on_page(&full, (n * 3 / 4) as i64);
            anchors.push(vec![(b, Some(bo as i64)), (a, None)]);

            for anchor in &anchors {
                let (pages, target) = resume(&blocks, &params, anchor);
                assert_eq!(target, full_target(&full, anchor), "{name} rich={rich}: {anchor:?}");
                assert!(target < pages.len(), "{name}: 目标页已排好");
                assert_eq!(pages[..], full[..pages.len()], "{name}: 页边界与整章一致");
                if pages.len() < n {
                    partial_seen = true;
                }
            }
        }
    }
    assert!(partial_seen, "至少有锚点不必排完整章");
}

#[test]
fn early_anchor_does_not_paginate_whole_chapter() {
    // 合成章节: 400 段, 进度在第 3 段
    let html: String = (0..400).map(|i| format!("<p>第{i}段。{}</p>", "正文内容".repeat(30))).collect();
    let blocks = extract_document(&html, BASE, true).blocks;
    let params = LayoutParams { rich: true, ..LayoutParams::default() };
    let full = paginate_all(&blocks, params.clone(), &mut Fixed);
    assert!(full.len() > 50);
    let anchor = vec![(blocks[3].path.clone(), Some(10))];
    let (pages, target) = resume(&blocks, &params, &anchor);
    assert_eq!(target, full_target(&full, &anchor));
    assert!(pages.len() <= target + 2, "排了 {} 页, 目标第 {} 页", pages.len(), target + 1);
}
