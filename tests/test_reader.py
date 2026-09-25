import unittest

from PIL import Image, ImageDraw, ImageFont

from kinnovel.config import Config
from kinnovel.reader import ReaderDocument, extract_blocks, sanitize_html


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


if __name__ == "__main__":
    unittest.main()
