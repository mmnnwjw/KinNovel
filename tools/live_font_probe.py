#!/usr/bin/env python3
"""Fetch one chapter and prove whether the chapter font affects glyph output."""

import sys
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))

from fontTools.ttLib import TTFont

from kinnovel.api import ApiClient, SessionStore
from kinnovel.config import Config
from kinnovel.reader import extract_blocks, ensure_font
from kinnovel.utils import sha256_text


DELAY = 10
BOOK_ID = 20439
CHAPTER_SORT = 2


def wait():
    print("[timer] waiting %ss" % DELAY, flush=True)
    time.sleep(DELAY)


def request(name, operation):
    print("[request] " + name, flush=True)
    result = operation()
    print("[ok] " + name, flush=True)
    wait()
    return result


def render_sample(text, font, output):
    image = Image.new("L", (1000, 240), 255)
    draw = ImageDraw.Draw(image)
    draw.text((20, 20), text[:160], font=font, fill=0)
    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(output)
    return image


def local_system_font():
    candidates = [
        Path(r"C:\Windows\Fonts\msyh.ttc"),
        Path(r"C:\Windows\Fonts\simhei.ttf"),
        Path("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
        Path("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return Path(ROOT / "bin" / "vendor" / "PIL" / "DejaVuSans.ttf")


def main():
    session_path = ROOT / "build" / "font-probe-session.json"
    try:
        session_path.unlink()
    except OSError:
        pass
    config_data = Config()
    api = ApiClient(config_data, SessionStore(session_path))
    try:
        lines = [line.strip() for line in (ROOT / "TESTACCOUNT.txt").read_text(
            encoding="utf-8-sig").splitlines() if line.strip()]
        credentials = request("POST /api/user/login", lambda: api._http(
            "/api/user/login", {"email": lines[0], "password": sha256_text(lines[1])}))
        api._store_credentials(credentials)
        api.session.set_many({"TokenUpdatedAt": time.time() + 3600})

        info = request("GetBookInfo", lambda: api.get_book_info(BOOK_ID))
        chapters = ((info.get("Book") or {}).get("Chapters") or [])
        if not chapters:
            raise RuntimeError("book has no chapters")
        sort_num = int(chapters[min(CHAPTER_SORT - 1, len(chapters) - 1)].get("SortNum") or 1)

        novel = request("GetNovelContent", lambda: api.get_novel_content(BOOK_ID, sort_num))
        chapter = novel.get("Chapter") or {}
        content = chapter.get("Content") or ""
        blocks = extract_blocks(content)
        text = "\n".join(block.text for block in blocks if block.kind != "image")
        if not text:
            raise RuntimeError("chapter has no extracted text")

        font_url = chapter.get("Font")
        chapter_font_path = ensure_font(font_url, api.server, strict_tls=True) if font_url else ""
        if not chapter_font_path:
            raise RuntimeError("chapter font is unavailable")

        size = 34
        chapter_font = ImageFont.truetype(chapter_font_path, size)
        system_font = ImageFont.truetype(str(local_system_font()), size)
        custom = render_sample(text, chapter_font, ROOT / "build" / "font-custom.png")
        system = render_sample(text, system_font, ROOT / "build" / "font-system.png")
        difference = ImageChops.difference(custom, system)
        changed = sum(1 for value in difference.getdata() if value)
        total = difference.width * difference.height

        cmap_font = TTFont(chapter_font_path)
        cmap = cmap_font.getBestCmap()
        chars = {ord(char) for char in text if not char.isspace()}
        covered = sum(1 for codepoint in chars if codepoint in cmap)
        pua = sum(1 for codepoint in chars if 0xE000 <= codepoint <= 0xF8FF)
        print("[summary] chars=%s covered_by_font=%s/%s pua=%s" % (
            len(chars), covered, len(chars), pua), flush=True)
        print("[summary] custom-vs-system changed_pixels=%s/%s (%.2f%%)" % (
            changed, total, changed * 100.0 / total), flush=True)
        print("[summary] previews=%s,%s" % (
            ROOT / "build" / "font-custom.png",
            ROOT / "build" / "font-system.png"), flush=True)
    finally:
        try:
            api.hub.close()
        except Exception:
            pass
        try:
            session_path.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    main()
