#!/usr/bin/env python3
import glob
from pathlib import Path

from fontTools.ttLib import TTFont
from PIL import ImageFont
from kinnovel.reader import split_font_runs


TESTS = [0x2014, 0x300C, 0x300D, 0x30FB, 0xFF5E, 0x2500, 0x2015, 0x2026,
         0x300A, 0x300B, 0x002D, 0x2010]


def main():
    print("codes", " ".join(hex(code) for code in TESTS))
    for raw_path in glob.glob("cache/fonts/*.woff2"):
        path = Path(raw_path)
        cmap = TTFont(path).getBestCmap() or {}
        covered = [1 if code in cmap else 0 for code in TESTS]
        font = ImageFont.truetype(str(path), 36)
        metrics = []
        for code in TESTS:
            char = chr(code)
            mask = font.getmask(char)
            metrics.append((round(font.getlength(char), 1), mask.size, bool(mask.getbbox())))
        print(path.name, covered)
        print("  metrics", metrics)
        fallback = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", 36)
        sample = "\u300e\u30c9\u30e9\u30b4\u30f3\u30fb\u30c6\u30b9\u30c8\u300f\u300c\u2500\u2500\u300d"
        print("  runs", [(ascii(run), selected is fallback)
                         for run, selected in split_font_runs(sample, font, fallback)])


if __name__ == "__main__":
    main()
