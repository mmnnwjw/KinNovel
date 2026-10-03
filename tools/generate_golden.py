#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

# Add bin/src to sys.path
BASE_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(BASE_DIR / "bin" / "src"))

from kinnovel.reader import ReaderDocument, _wrap_line_parts, wrap_line

class ConfigMock:
    def __init__(self, data=None):
        self.data = {
            "font_size": 34,
            "line_spacing": 1.42,
            "reader_margin": 34,
            "first_line_indent": True,
            "strict_tls": False,
        }
        if data:
            self.data.update(data)

    def get(self, key, default=None):
        return self.data.get(key, default)

def main():
    fixtures_dir = BASE_DIR / "cpp" / "tests" / "fixtures"
    golden_dir = BASE_DIR / "cpp" / "tests" / "golden"
    golden_dir.mkdir(parents=True, exist_ok=True)

    font_path = str(fixtures_dir / "testfont.ttf")
    if not os.path.exists(font_path):
        print("Error: test font fixture not found:", font_path)
        return 1

    font = ImageFont.truetype(font_path, 34)
    image = Image.new("L", (8, 8), 255)
    draw = ImageDraw.Draw(image)

    # 1. Line wrapping test cases (punctuation rules, group moves, hang)
    wrap_tests = []
    cases = [
        ("汉汉汉汉汉。汉汉汉", font.getlength("汉汉汉汉汉") + 1, "Single closing punctuation hangs"),
        ("汉汉汉汉汉。。”。汉汉", font.getlength("汉汉汉汉汉") + 1, "Consecutive closing punctuation group move"),
        ("汉汉汉汉汉。。”。汉汉", font.getlength("汉汉汉汉汉。。”。") + 1, "Consecutive closing punctuation fits"),
        ("汉汉汉汉“汉汉汉汉", font.getlength("汉汉汉汉“") + 1, "Opening punctuation moves to next line"),
        ("汉汉汉汉““汉汉汉汉", font.getlength("汉汉汉汉““") + 1, "Consecutive opening punctuation moves to next line"),
        ("第一章 中文正文测试文本。\n第二段正文测试。", 300, "Multline with newline"),
    ]

    for text, max_w, desc in cases:
        parts = _wrap_line_parts(draw, text, font, max_w)
        wrap_tests.append({
            "description": desc,
            "text": text,
            "max_width": float(max_w),
            "expected_parts": [{"text": line, "line_start": start} for line, start in parts]
        })

    with open(golden_dir / "golden_wrap.json", "w", encoding="utf-8") as f:
        json.dump(wrap_tests, f, ensure_ascii=False, indent=2)

    # 2. Complete Chapter Pagination Golden Test
    chapter_content = (
        "<h1>第一章 标题测试</h1>"
        "<p>这是第一段中文测试文本，测试首行缩进与标点禁则“开标点以及闭标点”。</p>"
        "<p>" + ("长正文测试段落，用于验证多行自动断行与分页计算是否逐字对齐。" * 12) + "</p>"
        '<img src="https://example.com/cover.jpg">'
        "<p>这是图片后的测试正文，包含破折​号﻿测试­文本⁠零宽字符剥除验证。</p>"
        "<h2>第二节 节标题测试</h2>"
        "<p>" + ("第二节长正文段落。" * 25) + "</p>"
        "<aside>这是第一条测试注释。</aside>"
        "<aside>这是第二条测试注释。</aside>"
    )

    chapter = {
        "Title": "测试章节",
        "Font": None,
        "Chapters": ["第一章 标题测试", "第二节 节标题测试"],
        "Content": chapter_content
    }

    doc = ReaderDocument(chapter, "https://example.com", font_path, ConfigMock())
    pages = doc.prepare(draw, 800, 1000)

    golden_pages = []
    for page_idx, page in enumerate(pages):
        items = []
        for it in page:
            item_data = {
                "type": it["type"],
                "x": it["x"],
                "y": it["y"],
                "path": it["path"],
                "offset": it["offset"]
            }
            if it["type"] == "text":
                item_data["text"] = it["text"]
                item_data["size"] = it["size"]
            elif it["type"] == "image":
                item_data["url"] = it["url"]
                item_data["width"] = it["width"]
                item_data["height"] = it["height"]
            items.append(item_data)
        golden_pages.append({
            "page_index": page_idx,
            "item_count": len(items),
            "items": items
        })

    golden_output = {
        "chapter_input": chapter,
        "page_count": len(pages),
        "pages": golden_pages,
        "anchor_tests": [
            {"page": 0, "anchor": list(doc.first_anchor_on_page(0))},
            {"page": 1, "anchor": list(doc.first_anchor_on_page(1))},
        ]
    }

    with open(golden_dir / "golden_chapter.json", "w", encoding="utf-8") as f:
        json.dump(golden_output, f, ensure_ascii=False, indent=2)

    print("Golden outputs generated successfully:")
    print(" -", golden_dir / "golden_wrap.json")
    print(" -", golden_dir / "golden_chapter.json")
    return 0

if __name__ == "__main__":
    sys.exit(main())
