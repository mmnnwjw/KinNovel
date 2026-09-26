#!/usr/bin/env python3
"""Inspect one chapter's missing glyphs without printing chapter text."""

import os
import sys
import time
from pathlib import Path

from fontTools.ttLib import TTFont
from PIL import ImageFont


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))

from kinnovel.api import ApiClient, SessionStore
from kinnovel.config import Config
from kinnovel.reader import ensure_font, extract_blocks, split_font_runs
from kinnovel.utils import sha256_text


DELAY = 10
BOOK_ID = int(os.environ.get("BOOK_ID", "9990"))
CHAPTER_SORT = int(os.environ.get("CHAPTER_SORT", "5"))


def fetch(name, operation):
    print("[request]", name, flush=True)
    result = operation()
    print("[ok]", name, flush=True)
    time.sleep(DELAY)
    return result


def visible(font, character):
    if character.isspace():
        return True
    try:
        return bool(font.getmask(character).getbbox())
    except (AttributeError, OSError, ValueError):
        return False


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
    lines = [
        line.strip()
        for line in (ROOT / "TESTACCOUNT.txt").read_text(
            encoding="utf-8-sig").splitlines()
        if line.strip()
    ]
    session_path = ROOT / "build" / "chapter-font-session.json"
    try:
        session_path.unlink()
    except OSError:
        pass
    config = Config()
    api = ApiClient(config, SessionStore(session_path))
    try:
        credentials = fetch("login", lambda: api._http("/api/user/login", {
            "email": lines[0],
            "password": sha256_text(lines[1]),
        }))
        api._store_credentials(credentials)
        api.session.set_many({"TokenUpdatedAt": time.time() + 3600})
        info = fetch("GetBookInfo", lambda: api.get_book_info(BOOK_ID))
        chapters = ((info.get("Book") or {}).get("Chapters") or [])
        index = min(max(CHAPTER_SORT - 1, 0), len(chapters) - 1)
        sort_num = int(chapters[index].get("SortNum") or CHAPTER_SORT)
        novel = fetch("GetNovelContent",
                      lambda: api.get_novel_content(BOOK_ID, sort_num))
        chapter = novel.get("Chapter") or {}
        font_url = chapter.get("Font") or ""
        font_path = fetch("download font",
                          lambda: ensure_font(font_url, api.server, strict_tls=True))
        if not font_path:
            raise RuntimeError("chapter font unavailable")

        chapter_font = ImageFont.truetype(font_path, 48)
        system_font = ImageFont.truetype(str(local_system_font()), 48)
        cmap = TTFont(font_path).getBestCmap() or {}
        blocks = extract_blocks(chapter.get("Content") or "")
        text = "".join(block.text for block in blocks if block.kind != "image")
        chars = sorted({character for character in text if not character.isspace()})
        empty_chapter = [character for character in chars
                         if not visible(chapter_font, character)]
        missing_cmap = [character for character in chars if ord(character) not in cmap]
        fallback_available = [character for character in empty_chapter
                              if visible(system_font, character)]
        runs = split_font_runs(text, chapter_font, system_font)
        fallback_runs = sum(1 for _run, font in runs if font is system_font)

        print("[summary] book=%s sort=%s chapter_chars=%s unique=%s" % (
            BOOK_ID, sort_num, len(text), len(chars)), flush=True)
        print("[summary] missing_cmap=%s empty_glyph=%s fallback_available=%s" % (
            len(missing_cmap), len(empty_chapter), len(fallback_available)),
            flush=True)
        print("[summary] missing_codepoints=%s" % [
            hex(ord(character)) for character in empty_chapter
        ], flush=True)
        print("[summary] fallback_codepoints=%s" % [
            hex(ord(character)) for character in fallback_available
        ], flush=True)
        print("[summary] fallback_runs=%s/%s" % (
            fallback_runs, len(runs)), flush=True)
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
