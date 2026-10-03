#!/usr/bin/env python3
"""在线图片管线基准测试(低请求频率)。

只做少量 Hub 调用(受 ApiClient 内置 9/5.5s 限流保护),图片最多取 3 张,
不会打印账号或 token。
"""

import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "src"))
sys.path.insert(0, str(ROOT / "bin" / "vendor"))

from kinnovel.api import ApiClient  # noqa: E402
from kinnovel.config import Config  # noqa: E402
from kinnovel.reader import extract_blocks  # noqa: E402
from kinnovel.ui import ImageCache  # noqa: E402

from PIL import Image, ImageOps  # noqa: E402


def rss_mb():
    try:
        for line in Path("/proc/self/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) / 1024.0
    except OSError:
        pass
    return 0.0


def wait_cached(cache, url, height, timeout=40.0):
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        image = cache.get(url, height)
        if image is not None:
            return time.monotonic() - start, image
        time.sleep(0.05)
    return None, None


def bench_one(cache, label, url, height):
    if not url:
        print("[%s] 无 URL" % label)
        return
    before = rss_mb()
    started = time.monotonic()
    scheduled = cache.prefetch(url, True, height=height)
    elapsed, image = wait_cached(cache, url, height)
    if image is None:
        print("[%s] 失败(60s 超时) scheduled=%s" % (label, scheduled))
        return
    path = cache._path(url, height)
    size_kb = path.stat().st_size / 1024.0 if path.exists() else 0.0
    print("[%s] %.2fs 解码完成 %dx%d 磁盘=%.0fKB RSS=%.1f->%.1fMB %s" % (
        label, time.monotonic() - started, image.width, image.height,
        size_kb, before, rss_mb(), "scheduled" if scheduled else "cached"))
    reread = time.monotonic()
    cache.clear_memory()
    image2 = cache.get(url, height)
    print("[%s] 二次解码 %.2fs %s" % (
        label, time.monotonic() - reread, "OK" if image2 else "FAIL"))


def main():
    config = Config()
    email = str(config.get("account_email") or "").strip()
    password = str(config.get("account_password") or "")
    if not email or not password:
        print("config.json 未配置账号")
        return 2
    api = ApiClient(config)
    print("[api] 登录…")
    api.login(email, password)
    print("[api] 书籍列表…")
    listing = api.get_book_list(page=1, size=6)
    books = listing.get("Data") or listing.get("data") or []
    if not books:
        print("[api] 列表为空")
        return 3
    book = next((b for b in books if b.get("Cover")), books[0])
    book_id = int(book.get("Id") or 0)
    print("[api] 书籍 %s" % book_id)

    cache = ImageCache(workers=2)
    bench_one(cache, "封面512", book.get("Cover"), 512)

    info = api.get_book_info(book_id)
    chapters = ((info or {}).get("Book") or {}).get("Chapters") or []
    sort_num = 1
    position = (info or {}).get("ReadPosition") or {}
    if chapters:
        first = chapters[0]
        if isinstance(first, dict):
            try:
                sort_num = int(first.get("SortNum") or 1)
            except (TypeError, ValueError):
                sort_num = 1
    elif position.get("ChapterId"):
        sort_num = 1
    print("[api] 第一章 sort_num=%s" % sort_num)

    content = api.get_novel_content(book_id, sort_num)
    chapter = (content or {}).get("Chapter") or {}
    blocks = extract_blocks(chapter.get("Content") or "", api.server)
    urls = []
    for block in blocks:
        if block.kind == "image" and block.source_url:
            urls.append(block.source_url)
        if len(urls) >= 3:
            break
    print("[api] 本章插图 %d 张(测试取 %d)" % (
        sum(1 for b in blocks if b.kind == "image"), len(urls)))
    if urls:
        started = time.monotonic()
        before = rss_mb()
        for url in urls:
            cache.prefetch(url, True, height=1024)
        for url in urls:
            wait_cached(cache, url, 1024)
        print("[插图] %d 张并发预取总耗时 %.2fs RSS=%.1f->%.1fMB" % (
            len(urls), time.monotonic() - started, before, rss_mb()))
        canvas = Image.new("L", (1236, 1648), 255)
        cover = cache.get(book.get("Cover"), 512)
        if cover is not None:
            canvas.paste(cover, (60, 120))
        illustration = cache.get(urls[0], 1024)
        if illustration is not None:
            fitted = ImageOps.contain(illustration, (1000, 1000))
            canvas.paste(fitted, (60, 720))
        canvas.save("/tmp/live_render.png")
        print("[render] /tmp/live_render.png 封面=%s 插图=%s" % (
            bool(cover), bool(illustration)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
