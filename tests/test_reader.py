import unittest

from PIL import Image, ImageDraw, ImageFont

from kinnovel.config import Config
from kinnovel.reader import (
    ReaderDocument,
    extract_blocks,
    sanitize_html,
    split_font_runs,
    wrap_line,
)


class MemoryConfig:
    def __init__(self):
        self.values = {
            "font_size": 34,
            "line_spacing": 1.42,
            "reader_margin": 34,
            "first_line_indent": True,
            "strict_tls": False,
        }

    def get(self, key, default=None):
        return self.values.get(key, default)


class ReaderTests(unittest.TestCase):
    def test_utf8_content_is_not_mojibake(self):
        blocks = extract_blocks("<p>第一章 中文正文</p>")
        self.assertEqual(blocks[0].text, "第一章 中文正文")

    def test_parent_block_is_not_duplicated(self):
        blocks = extract_blocks("<div><h1>标题</h1><p>正文</p></div>")
        self.assertEqual([block.text for block in blocks], ["标题", "正文"])

    def test_image_alt_is_not_added_to_text(self):
        blocks = extract_blocks('<p>前文</p><img src="cover.jpg" alt="封面说明"><p>后文</p>')
        self.assertEqual([block.kind for block in blocks],
                         ["text", "image", "text"])
        self.assertNotIn("封面说明", "".join(block.text for block in blocks))

    def test_xpath_preserves_order(self):
        blocks = extract_blocks("<p>一</p><p>二</p><h2>三</h2>")
        self.assertEqual(blocks[0].path, "./p[1]")
        self.assertEqual(blocks[1].path, "./p[2]")
        self.assertEqual(blocks[2].path, "./h2[1]")

    def test_pagination_outputs_text(self):
        chapter = {
            "Title": "测试",
            "Font": None,
            "Chapters": ["测试"],
            "Content": "<p>" + ("测试正文。" * 200) + "</p>",
        }
        document = ReaderDocument(chapter, "https://example.test",
                                  "C:/Windows/Fonts/simhei.ttf", MemoryConfig())
        image = Image.new("L", (8, 8), 255)
        draw = ImageDraw.Draw(image)
        pages = document.prepare(draw, 800, 1000)
        self.assertGreater(len(pages), 1)
        self.assertTrue(any(item["type"] == "text" for item in pages[0]))

    def test_closing_punctuation_stays_on_previous_line(self):
        image = Image.new("L", (8, 8), 255)
        draw = ImageDraw.Draw(image)
        font = ImageFont.truetype("C:/Windows/Fonts/simhei.ttf", 20)
        max_width = font.getlength("汉汉汉汉汉") + 1
        lines = wrap_line(draw, "汉汉汉汉汉。汉汉汉", font, max_width)
        self.assertEqual(lines[0], "汉汉汉汉汉。")
        self.assertFalse(lines[1].startswith("。"))

    def test_opening_punctuation_moves_to_next_line(self):
        image = Image.new("L", (8, 8), 255)
        draw = ImageDraw.Draw(image)
        font = ImageFont.truetype("C:/Windows/Fonts/simhei.ttf", 20)
        max_width = font.getlength("汉汉汉汉“") + 1
        lines = wrap_line(draw, "汉汉汉汉“汉汉汉汉", font, max_width)
        self.assertEqual(lines[0], "汉汉汉汉")
        self.assertTrue(lines[1].startswith("“"))

    def test_invisible_format_chars_are_stripped(self):
        blocks = extract_blocks("<p>破折​号﻿测试­文本⁠。</p>")
        self.assertEqual(blocks[0].text, "破折号测试文本。")

    def test_notdef_box_is_not_treated_as_available(self):
        from kinnovel.reader import glyph_available
        font = ImageFont.truetype("C:/Windows/Fonts/simhei.ttf", 32)
        self.assertTrue(glyph_available(font, "中"))
        # 超出 Unicode 范围的探测字符必然映射到 .notdef
        self.assertFalse(glyph_available(font, "\U0010FFFF"))

    def test_missing_glyph_falls_back_per_character(self):
        class Mask:
            def __init__(self, visible):
                self.visible = visible

            def getbbox(self):
                return (0, 0, 10, 10) if self.visible else None

        class Font:
            def __init__(self, available):
                self.available = available

            def getmask(self, character):
                return Mask(character in self.available)

        primary = Font({"A", "B"})
        fallback = Font({"\u30fb"})
        runs = split_font_runs("A\u30fbB", primary, fallback)
        self.assertEqual(runs[0], ("A", primary))
        self.assertEqual(runs[1], ("\u30fb", fallback))
        self.assertEqual(runs[2], ("B", primary))


if __name__ == "__main__":
    unittest.main()
