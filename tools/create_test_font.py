#!/usr/bin/env python3
import os
import sys
from fontTools import subset

def main():
    system_fonts = [
        "/system/fonts/NotoSansCJK-Regular.ttc",
        "/system/fonts/NotoSerifCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "C:/Windows/Fonts/simhei.ttf",
        "C:/Windows/Fonts/msyh.ttc"
    ]
    font_path = None
    for sf in system_fonts:
        if os.path.exists(sf):
            font_path = sf
            break
    if not font_path:
        print("No CJK font found to subset from.")
        return 1

    out_dir = "cpp/tests/fixtures"
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "testfont.ttf")

    chars = (
        " 0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
        "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"
        "，。、；：？！“”‘’（）《》【】「」『』…—·　"
        "第一章中文正文标题前文封面说明后一二三测试破折号文本注释汉字"
    )

    options = subset.Options()
    from fontTools.ttLib import TTFont
    font = TTFont(font_path, fontNumber=0 if font_path.endswith(".ttc") else 0)
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(text=chars)
    subsetter.subset(font)
    subset.save_font(font, out_path, options)
    print("Generated test font:", out_path, "size:", os.path.getsize(out_path))
    return 0

if __name__ == "__main__":
    sys.exit(main())
