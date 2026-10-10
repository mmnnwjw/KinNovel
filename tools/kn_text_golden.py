"""Golden output of the Python reader layout, for diffing the Rust `kn-text` port.

Font metrics are replaced by a deterministic width function so the comparison
checks only the algorithms (HTML -> blocks, line breaking with kinsoku rules,
pagination, anchors), not Pillow vs. skrifa rasterizer differences:

    width(ch, size) = size * (0.5 if ord(ch) < 0x2E80 else 1.0)   (space = size * 0.25)

The Python reader lives on the `legacy` branch (0.8.x); check out its bin/src first, e.g.
    git worktree add ../kn-legacy legacy

Usage:
    PYTHONPATH=../kn-legacy/bin/src python tools/kn_text_golden.py <chapter.json|.html> ... --out DIR
        [--width 1236] [--height 1400] [--font-size 48]

For every input writes DIR/<stem>.golden.json:
    {"blocks": [{kind, text, level, path, offset, source_url, width, height}],
     "pages": [[{type, text?, x, y, size?, path, offset, url?, width?, height?}]]}

Chapter JSON is either the app cache format ({"Chapter": {...}}) or a bare
chapter dict with "Content"; .html files are treated as chapter content.
"""
import argparse
import json
import os
import sys

from kinnovel import reader


def char_width(ch, size):
    if ch == " ":
        return size * 0.25
    return size * (0.5 if ord(ch) < 0x2E80 else 1.0)


class FakeFont:
    def __init__(self, size):
        self.size = size


class WidthTable:
    """Stands in for the per-font width dict in reader._wrap_line_parts."""

    def __init__(self, size):
        self.size = size

    def get(self, ch):
        return char_width(ch, self.size)

    def __setitem__(self, key, value):
        raise AssertionError("width table must never miss")


class FakeResolver:
    def resolve(self, _font, _base, size, strict_tls=False):
        return FakeFont(size)

    def system_font(self, size):
        return FakeFont(size)


def paginate(chapter, width, height, config):
    doc = reader.ReaderDocument.__new__(reader.ReaderDocument)
    doc.chapter = chapter
    doc.base_url = "https://www.lightnovel.app"
    doc.config = config
    doc.blocks = reader.extract_blocks(chapter.get("Content") or "", doc.base_url)
    doc.font_resolver = FakeResolver()
    size = int(config["font_size"])
    doc.body_font = FakeFont(size)
    doc.body_fallback = FakeFont(size)
    small = max(20, int(size * 0.82))
    doc.small_font = FakeFont(small)
    doc.small_fallback = FakeFont(small)

    original = reader._wrap_line_parts

    def wrap(draw, text, font, max_width, fallback_font=None,
             remove_trailing_spaces=True, metrics=None):
        return original(draw, text, font, max_width, fallback_font=fallback_font,
                        remove_trailing_spaces=remove_trailing_spaces,
                        metrics=WidthTable(font.size))

    reader._wrap_line_parts = wrap
    try:
        doc.pages = doc._paginate(None, width, height)
    finally:
        reader._wrap_line_parts = original
    return doc


def page_item(item):
    out = {k: item[k] for k in ("type", "x", "y", "path", "offset")}
    if item["type"] == "text":
        out["text"] = item["text"]
        out["size"] = item["size"]
    else:
        out.update(url=item["url"], width=item["width"], height=item["height"])
    return out


def load_chapter(path):
    if path.lower().endswith((".html", ".htm")):
        with open(path, encoding="utf-8") as f:
            return {"Content": f.read(), "Title": os.path.basename(path)}
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    return data.get("Chapter", data)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("inputs", nargs="+")
    ap.add_argument("--out", required=True)
    ap.add_argument("--width", type=int, default=1236)
    ap.add_argument("--height", type=int, default=1400)
    ap.add_argument("--font-size", type=int, default=48)
    args = ap.parse_args()
    config = {"font_size": args.font_size, "line_spacing": 1.42, "reader_margin": 34,
              "first_line_indent": True}
    os.makedirs(args.out, exist_ok=True)
    for path in args.inputs:
        chapter = load_chapter(path)
        doc = paginate(chapter, args.width, args.height, config)
        result = {
            "params": {"width": args.width, "height": args.height, **config},
            "blocks": [
                {"kind": b.kind, "text": b.text, "level": b.level, "path": b.path,
                 "offset": b.offset, "source_url": b.source_url,
                 "width": b.width, "height": b.height}
                for b in doc.blocks
            ],
            "pages": [[page_item(i) for i in page] for page in doc.pages],
        }
        stem = os.path.splitext(os.path.basename(path))[0]
        out = os.path.join(args.out, stem + ".golden.json")
        with open(out, "w", encoding="utf-8", newline="") as f:
            json.dump(result, f, ensure_ascii=False, indent=1)
            f.write("\n")
        print("%s: %d blocks, %d pages" % (out, len(doc.blocks), len(doc.pages)), file=sys.stderr)


if __name__ == "__main__":
    main()
