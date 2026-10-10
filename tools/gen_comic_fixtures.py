"""Generate the synthetic comic fixtures used by host previews / device tests (KN_FAKE_DIR).

Writes JSON responses (GetComicList, GetBookInfo, GetComicContent, ...) and procedurally drawn
grayscale PNG pages (panels, screentone, speech bubbles, page numbers) into
rust/crates/kn-app/tests/fixtures/fake/. Image URLs use the `fixture:` scheme, which the app
resolves against KN_FAKE_DIR in fixture mode only. No real comic content is involved.

    python tools/gen_comic_fixtures.py
"""
import json
import random
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "rust/crates/kn-app/tests/fixtures/fake"
IMG = OUT / "comic"

W, H = 840, 1200


class Canvas:
    def __init__(self, w, h, value=255):
        self.w, self.h = w, h
        self.px = bytearray([value]) * (w * h)

    def fill(self, x0, y0, x1, y1, v):
        x0, y0, x1, y1 = max(0, x0), max(0, y0), min(self.w, x1), min(self.h, y1)
        if x1 <= x0:
            return
        row = bytes([v]) * (x1 - x0)
        for y in range(y0, y1):
            self.px[y * self.w + x0:y * self.w + x1] = row

    def frame(self, x0, y0, x1, y1, t, v=0):
        self.fill(x0, y0, x1, y0 + t, v)
        self.fill(x0, y1 - t, x1, y1, v)
        self.fill(x0, y0, x0 + t, y1, v)
        self.fill(x1 - t, y0, x1, y1, v)

    def tone(self, x0, y0, x1, y1, step, r):
        """Screentone: dots of radius r on a step grid."""
        for cy in range(y0 + step // 2, y1, step):
            for cx in range(x0 + step // 2, x1, step):
                self.ellipse(cx, cy, r, r, 40)

    def gradient(self, x0, y0, x1, y1, top, bottom):
        for y in range(max(0, y0), min(self.h, y1)):
            v = int(top + (bottom - top) * (y - y0) / max(1, y1 - y0))
            self.fill(x0, y, x1, y + 1, v)

    def ellipse(self, cx, cy, rx, ry, v, outline=0, ov=0):
        for y in range(max(0, cy - ry), min(self.h, cy + ry + 1)):
            dy = (y - cy) / ry
            if abs(dy) > 1:
                continue
            half = int(rx * (1 - dy * dy) ** 0.5)
            self.fill(cx - half, y, cx + half + 1, y + 1, v)
        if outline:
            inner_rx, inner_ry = rx - outline, ry - outline
            for y in range(max(0, cy - ry), min(self.h, cy + ry + 1)):
                dy = (y - cy) / ry
                if abs(dy) > 1:
                    continue
                half = int(rx * (1 - dy * dy) ** 0.5)
                ih = -1
                if inner_ry > 0 and abs(y - cy) < inner_ry:
                    idy = (y - cy) / inner_ry
                    ih = int(inner_rx * (1 - idy * idy) ** 0.5)
                if ih < 0:
                    self.fill(cx - half, y, cx + half + 1, y + 1, ov)
                else:
                    self.fill(cx - half, y, cx - ih, y + 1, ov)
                    self.fill(cx + ih + 1, y, cx + half + 1, y + 1, ov)

    def digits(self, x, y, text, size, v=0):
        """Seven-segment digits (and '-') of height 2*size."""
        segs = {
            "0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc", "5": "afgcd",
            "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcdfg", "-": "g",
        }
        t = max(3, size // 5)
        for ch in text:
            on = segs.get(ch, "")
            s = size
            boxes = {
                "a": (x, y, x + s, y + t), "g": (x, y + s - t // 2, x + s, y + s + t - t // 2),
                "d": (x, y + 2 * s - t, x + s, y + 2 * s), "f": (x, y, x + t, y + s),
                "b": (x + s - t, y, x + s, y + s), "e": (x, y + s, x + t, y + 2 * s),
                "c": (x + s - t, y + s, x + s, y + 2 * s),
            }
            for k in on:
                self.fill(*boxes[k], v)
            x += s + t * 2

    def png(self, path):
        raw = b"".join(b"\x00" + bytes(self.px[y * self.w:(y + 1) * self.w]) for y in range(self.h))

        def chunk(kind, data):
            return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

        ihdr = struct.pack(">IIBBBBB", self.w, self.h, 8, 0, 0, 0, 0)
        path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def panel_content(c, rng, x0, y0, x1, y1):
    w, h = x1 - x0, y1 - y0
    kind = rng.randrange(3)
    if kind == 0:
        c.tone(x0, y0, x1, y1, 14, 3)
    elif kind == 1:
        c.gradient(x0, y0, x1, y1, 235, 150)
    # a figure: head + shoulders
    cx = x0 + int(w * rng.uniform(0.3, 0.7))
    r = int(min(w, h) * rng.uniform(0.16, 0.24))
    base = y1 - int(r * 1.1) - 4
    c.ellipse(cx, base, int(r * 1.8), int(r * 1.1), 255, 6, 0)
    c.ellipse(cx, base - int(r * 1.6), r, int(r * 1.15), 255, 6, 0)
    c.ellipse(cx - r // 3, base - int(r * 1.7), r // 9 + 2, r // 6 + 2, 0)
    c.ellipse(cx + r // 3, base - int(r * 1.7), r // 9 + 2, r // 6 + 2, 0)
    # speech bubble with "text" lines
    if rng.random() < 0.8 and w > 220:
        bw, bh = int(w * 0.28), int(h * 0.22)
        bx = x0 + bw + 12 if cx > x0 + w // 2 else x1 - bw - 12
        by = y0 + bh + 12
        c.ellipse(bx, by, bw, bh, 255, 5, 0)
        for i in range(3):
            ly = by - bh // 2 + i * bh // 3
            c.fill(bx - bw // 2, ly, bx + bw // 2 - rng.randrange(0, bw // 2), ly + max(6, bh // 9), 60)


def draw_page(path, seed, label, w=W, h=H):
    rng = random.Random(seed)
    c = Canvas(w, h)
    m, g, t = 36, 18, 6
    layout = seed % 3
    if w > h:  # spread: two big panels
        panels = [(m, m, w // 2 - g // 2, h - m), (w // 2 + g // 2, m, w - m, h - m)]
    elif layout == 0:
        a, b = int(h * 0.3), int(h * 0.62)
        panels = [(m, m, w - m, a), (m, a + g, w // 2 - g // 2, b), (w // 2 + g // 2, a + g, w - m, b), (m, b + g, w - m, h - m - 60)]
    elif layout == 1:
        a = int(w * 0.55)
        panels = [(m, m, a, h - m - 60), (a + g, m, w - m, h // 2 - g // 2), (a + g, h // 2 + g // 2, w - m, h - m - 60)]
    else:
        a, b = int(h * 0.33), int(h * 0.64)
        panels = [(m, m, w - m, a), (m, a + g, w - m, b), (m, b + g, w - m, h - m - 60)]
    for x0, y0, x1, y1 in panels:
        panel_content(c, rng, x0 + t, y0 + t, x1 - t, y1 - t)
        c.frame(x0, y0, x1, y1, t)
    # page label bottom centre (chapter-page)
    size = 22
    text_w = len(label) * (size + 2 * max(3, size // 5))
    c.digits((w - text_w) // 2, h - m - 46, label, size)
    c.png(path)


def draw_cover(path, seed, w=600, h=850):
    rng = random.Random(seed)
    c = Canvas(w, h)
    c.gradient(0, 0, w, h, 250, 120)
    c.tone(0, int(h * 0.55), w, h, 16, 4)
    c.ellipse(w // 2, int(h * 0.45), int(w * 0.3), int(w * 0.33), 255, 8, 0)
    c.fill(0, 40, w, 150, 0)
    c.digits(40, 60, str(seed), 30, 255)
    c.frame(0, 0, w, h, 10)
    c.png(path)


def write(name, obj):
    (OUT / f"{name}.json").write_text(json.dumps(obj, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def main():
    IMG.mkdir(parents=True, exist_ok=True)
    pages = []
    for i in range(8):
        name = f"p{i + 1:02d}.png"
        draw_page(IMG / name, i, f"{i + 1}")
        pages.append(f"fixture:comic/{name}")
    draw_page(IMG / "spread.png", 99, "5-6", w=1680, h=1200)
    spread = "fixture:comic/spread.png"
    for i in (1, 2):
        draw_cover(IMG / f"cover{i}.png", i)
    cover = ["fixture:comic/cover1.png", "fixture:comic/cover2.png"]

    comics = [
        {"Id": 40001, "Title": "灯塔守望者", "Cover": cover[0], "Count": 3, "LastUpdatedAt": "2026-10-09T20:00:00"},
        {"Id": 40011, "Title": "雨季的邮差", "Cover": cover[1], "Count": 12, "LastUpdatedAt": "2026-10-07T12:00:00"},
        {"Id": 40021, "Title": "第七号站台", "Cover": cover[0], "Count": 5, "LastUpdatedAt": "2026-10-02T08:30:00"},
    ]
    for order, items in (("latest", comics), ("new", comics[::-1]), ("view", [comics[1], comics[0], comics[2]])):
        write(f"comic_list_{order}_p1", {"Page": 1, "TotalPages": 1, "Data": items})
    write("comic_series_by_ids", {"Page": 1, "TotalPages": 1, "Data": [comics[0]]})

    # 40001: three chapters (8 pages incl. a spread, 5, 7)
    chapters = [
        (41001, 1, "第1话 雾中的光", [pages[0], pages[1], pages[2], pages[3], spread, pages[4], pages[5], pages[6]]),
        (41002, 2, "第2话 旧航线", [pages[7], pages[0], pages[1], pages[2], pages[3]]),
        (41003, 3, "第3话 回声", [pages[4], pages[5], pages[6], pages[7], pages[0], pages[1], pages[2]]),
    ]
    book = {
        "SeriesTitle": "灯塔守望者",
        "Series": [{"Id": 40001, "Title": "灯塔守望者 第1卷", "Cover": cover[0]}, {"Id": 40002, "Title": "灯塔守望者 第2卷", "Cover": cover[1]}],
        "Book": {
            "Id": 40001, "Type": "Comic", "Title": "灯塔守望者 第1卷", "Author": "海鸥工作室", "Cover": cover[0],
            "Introduction": "<p>（测试数据）偏远海岛上的灯塔守望者，每晚记录经过的船只。某天，一艘不在任何航海图上的船靠了岸。</p>",
            "LastUpdatedAt": "2026-10-09T20:00:00", "LastUpdatedChapter": chapters[-1][2], "Views": 52011, "Favorite": 1830,
            "Extra": {"classification": {"tags": ["悬疑", "治愈"], "series_name_cn": "灯塔守望者"}},
            "Chapters": [{"Id": cid, "SortNum": s, "Title": t, "PageCount": len(imgs), "CreatedAt": "2026-10-01T00:00:00"} for cid, s, t, imgs in chapters],
        },
        "ReadPosition": {"ChapterId": 41001, "Position": "3"},
    }
    write("book_info_40001", book)
    book2 = json.loads(json.dumps(book))
    book2["Book"].update({"Id": 40002, "Title": "灯塔守望者 第2卷", "Cover": cover[1], "Chapters": [], "LastUpdatedChapter": ""})
    book2["ReadPosition"] = None
    write("book_info_40002", book2)
    for cid, sort_num, title, imgs in chapters:
        for skip in range(0, len(imgs), 6):
            content = {
                "Chapter": {"Id": cid, "BookId": 40001, "BookName": "灯塔守望者 第1卷", "Title": title, "SortNum": sort_num,
                            "Total": len(imgs), "Skip": skip, "Images": imgs[skip:skip + 6]},
                "ReadPosition": {"ChapterId": 41001, "Position": "3"} if skip == 0 else None,
            }
            write(f"comic_content_{cid}_{skip}", content)
    print("written to", OUT)


if __name__ == "__main__":
    main()
