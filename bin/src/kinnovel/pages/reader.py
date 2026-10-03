import json
import os
import threading
import time

from PIL import Image, ImageDraw, ImageOps

from ..config import CACHE_DIR
from ..reader import ReaderDocument, ensure_font
from ..ui import height_bucket
from ..utils import atomic_write, read_json, stable_cache_name, touch


_CHAPTER_CACHE_TTL = 12 * 3600
_CHAPTER_LOCKS = {}
_CHAPTER_LOCKS_GUARD = threading.Lock()


def _chapter_cache_path(book_id, sort_num, convert):
    key = "%s:%s:%s" % (int(book_id), int(sort_num), convert or "")
    return CACHE_DIR / "content" / (stable_cache_name(key) + ".json")


def _chapter_lock(book_id, sort_num, convert):
    key = (int(book_id), int(sort_num), convert or "")
    with _CHAPTER_LOCKS_GUARD:
        lock = _CHAPTER_LOCKS.get(key)
        if lock is None:
            lock = threading.Lock()
            _CHAPTER_LOCKS[key] = lock
        return lock


def _progress_path(book_id, sort_num):
    return CACHE_DIR / "progress" / ("%s-%s.json" % (
        int(book_id), int(sort_num)))


def _load_progress(book_id, sort_num):
    progress = read_json(_progress_path(book_id, sort_num))
    if not isinstance(progress, dict):
        return None
    path = progress.get("path")
    if not isinstance(path, str) or not path:
        return None
    try:
        offset = max(0, int(progress.get("offset") or 0))
        page = max(0, int(progress.get("page") or 0))
    except (TypeError, ValueError):
        return None
    return {"path": path, "offset": offset, "page": page}


def _load_chapter(ctx, book_id, sort_num):
    convert = ctx.config.get("convert")
    path = _chapter_cache_path(book_id, sort_num, convert)
    cached = read_json(path)
    fresh = False
    if cached:
        try:
            fresh = (time.time() - path.stat().st_mtime) < _CHAPTER_CACHE_TTL
        except OSError:
            fresh = False
    if cached and fresh:
        touch(path)
        return cached
    lock = _chapter_lock(book_id, sort_num, convert)
    with lock:
        cached = read_json(path)
        if cached:
            try:
                if (time.time() - path.stat().st_mtime) < _CHAPTER_CACHE_TTL:
                    touch(path)
                    return cached
            except OSError:
                pass
        try:
            response = ctx.api.get_novel_content(
                book_id, sort_num, convert=convert)
        except Exception:
            if cached:
                return cached
            raise
        atomic_write(path, json.dumps(response, ensure_ascii=False))
        return response


def prefetch_chapter(ctx, book_id, sort_num):
    try:
        response = _load_chapter(ctx, book_id, sort_num)
    except Exception:
        return False
    font_url = (response.get("Chapter") or {}).get("Font")
    if font_url:
        ensure_font(font_url, ctx.api.server,
                    strict_tls=bool(ctx.config.get("strict_tls")))
    return True


def _image_ready(ctx, url, ok=True):
    """图片到达后只刷新当前页上该插图所在矩形，避免整屏重绘。"""
    if not ok:
        return

    def apply():
        if ctx.page_name != "reader" or STATE.get("fullscreen_image"):
            return
        region = _visible_image_region(ctx, url)
        if region is not None:
            ctx.show(region=region)
    ctx.post(apply)


def _fullscreen_ready(ctx, url, ok=True):
    if not ok:
        return

    def apply():
        if ctx.page_name == "reader" and STATE.get("fullscreen_image") == url:
            ctx.show()
    ctx.post(apply)


def _visible_image_region(ctx, url):
    doc = STATE.get("doc")
    pages = getattr(doc, "pages", None) if doc else None
    if not pages:
        return None
    try:
        index = max(0, min(int(STATE.get("page") or 0), len(pages) - 1))
        page = pages[index]
    except (TypeError, ValueError, IndexError):
        return None
    top, _ = _layout_metrics(ctx)
    for item in page:
        if item.get("type") != "image" or item.get("url") != url:
            continue
        return (int(item["x"]), int(top + item["y"]),
                int(item["width"]), int(item["height"]))
    return None


def _prefetch_images(ctx, document, pages_ahead=2):
    """只预取当前页及其后少量页的插图，交给 ImageCache 做去重与限流。"""
    pages = getattr(document, "pages", None) or []
    if not pages:
        return
    first = max(0, min(int(STATE.get("page") or 0), len(pages) - 1))
    window = pages[first:first + max(0, int(pages_ahead)) + 1]
    for distance, page in enumerate(window):
        priority = 0 if distance == 0 else (3 if distance == 1 else 6)
        for item in page:
            if item.get("type") != "image":
                continue
            url = item.get("url")
            if not url:
                continue
            target = height_bucket(item.get("height") or 1024)
            if ctx.images.is_cached(url, target):
                continue
            ctx.images.prefetch(
                url, ctx.config.get("strict_tls"), height=target,
                priority=priority,
                callback=lambda ok, url=url: _image_ready(ctx, url, ok),
            )


def _prefetch_neighbors(ctx, book_id, sort_num, chapters):
    index = _chapter_index(chapters, sort_num)
    for target_index in (index - 1, index + 1):
        if target_index < 0 or target_index >= len(chapters):
            continue
        target = _chapter_sort(chapters, target_index)
        if _chapter_cache_path(book_id, target, ctx.config.get("convert")).exists():
            continue

        def task(target=target):
            prefetch_chapter(ctx, book_id, target)

        ctx.run_async("reader", task, lambda _result: None, lambda _exc: None)


def _chapter_sort(chapters, index):
    """章节在第 index 位时的服务端 SortNum。"""
    if 0 <= index < len(chapters):
        item = chapters[index]
        if isinstance(item, dict):
            try:
                return int(item.get("SortNum") or index + 1)
            except (TypeError, ValueError):
                return index + 1
    return index + 1


def _chapter_index(chapters, sort_num):
    """服务端 SortNum 对应的列表下标（兼容字符串章节列表）。"""
    try:
        wanted = int(sort_num)
    except (TypeError, ValueError):
        wanted = 1
    for index, item in enumerate(chapters):
        if isinstance(item, dict):
            try:
                if int(item.get("SortNum") or 0) == wanted:
                    return index
            except (TypeError, ValueError):
                continue
        elif index + 1 == wanted:
            return index
    return wanted - 1


STATE = {
    "book_id": 0,
    "sort_num": 0,
    "data": None,
    "doc": None,
    "page": 0,
    "rects": {},
    "loading": False,
    "last_saved": -1,
    "catalog_page": 0,
    "signature": None,
    "image_pending": set(),
    "image_rects": {},
    "fullscreen_image": None,
    "last_turn_at": 0.0,
    "fitted_cache": {},
    "chrome_visible": False,
    "layout_generation": 0,
}


_GUIDE_LINES = [
    "点击左侧：上一页",
    "点击右侧：下一页",
    "顶端下滑：呼出控件",
    "点击中间：回到阅读",
    "点击图片：全屏预览",
    "再次点击：退出预览",
]


def _maybe_show_guide(ctx):
    if ctx.config.get("reader_guide_dismissed"):
        return
    # 章间切换走 replace，previous_page 仍是 reader，不重复弹指引
    if getattr(ctx, "previous_page", None) == "reader":
        return
    ctx.confirm(
        _GUIDE_LINES,
        lambda: None,
        lambda: ctx.config.set("reader_guide_dismissed", True),
        yes="感觉会忘记",
        no="不再提示",
    )


def _configure_swipe(ctx, delta):
    if not ctx.config.get("page_turn_animation"):
        return
    output = ctx.screen.output
    if not getattr(output, "supports_swipe_animation", False):
        return
    output.set_swipe_direction(int(delta) > 0)
    output.set_swipe_animations(True)


def _fit_image(url, image, width, height):
    key = (url, int(width), int(height))
    cache = STATE["fitted_cache"]
    cached = cache.get(key)
    if cached is not None:
        return cached
    fitted = ImageOps.contain(image, (int(width), int(height)),
                              method=Image.Resampling.LANCZOS)
    if len(cache) >= 8:
        try:
            cache.pop(next(iter(cache)))
        except StopIteration:
            pass
    cache[key] = fitted
    return fitted


_CHROME_FOOTER = 92


def _layout_metrics(ctx):
    # 正文永远按 compact 几何分页：控件层是覆盖式浮层，显隐不触发重排
    top = max(40, int(ctx.height * 0.035))
    return top, max(160, int(ctx.height) - top)


def _chrome_header_height(ctx):
    # 与 Canvas.header 的高度公式保持一致，用于控件层的点击判定
    return max(72, int(ctx.height * 0.085))


def _signature(ctx, book_id, sort_num):
    top, _ = _layout_metrics(ctx)
    return (
        int(book_id), int(sort_num), int(ctx.config.get("font_size") or 48),
        float(ctx.config.get("line_spacing") or 1.42),
        int(ctx.config.get("reader_margin") or 34),
        int(ctx.width), int(ctx.height), top,
        str(ctx.config.get("font_path") or ""),
        str(ctx.config.get("convert") or ""),
        bool(ctx.config.get("first_line_indent")),
    )


def _prepare_document(ctx, chapter):
    image = Image.new("L", (8, 8), 255)
    draw = ImageDraw.Draw(image)
    document = ReaderDocument(chapter, ctx.api.server,
                              ctx.config.get("font_path"), ctx.config)
    _, content_height = _layout_metrics(ctx)
    document.prepare(draw, ctx.width, content_height)
    return document


def _set_chrome_visible(ctx, visible):
    visible = bool(visible)
    if bool(STATE.get("chrome_visible")) == visible:
        return
    STATE["chrome_visible"] = visible
    ctx.show()


def enter(ctx):
    STATE["fullscreen_image"] = None
    book_id = int(ctx.params.get("book_id") or 0)
    sort_num = int(ctx.params.get("sort_num") or 1)
    fresh = bool(ctx.params.get("fresh"))
    at_last = bool(ctx.params.get("at_last"))
    swipe_delta = int(ctx.params.get("swipe_delta") or 0)
    # 进入阅读器一律从 compact 视图开始，控件层需由顶端下滑唤出
    STATE["chrome_visible"] = False
    _maybe_show_guide(ctx)
    # 一次性意图，消费后移除，防止从目录/设置返回时重置页码
    ctx.params.pop("fresh", None)
    ctx.params.pop("at_last", None)
    ctx.params.pop("swipe_delta", None)
    STATE["layout_generation"] += 1
    generation = STATE["layout_generation"]
    signature = _signature(ctx, book_id, sort_num)
    if (STATE["data"] and STATE["book_id"] == book_id and
            STATE["sort_num"] == sort_num and STATE["signature"] == signature):
        if fresh:
            STATE["page"] = 0
        elif at_last:
            STATE["page"] = max(0, STATE["doc"].page_count - 1)
        ctx.show()
        return
    if (STATE["data"] and STATE["book_id"] == book_id and
            STATE["sort_num"] == sort_num and STATE["doc"]):
        current_path, current_offset = STATE["doc"].first_anchor_on_page(
            STATE["page"])
        STATE["loading"] = True
        ctx.show()

        def success(document):
            if (generation != STATE["layout_generation"]
                    or STATE["book_id"] != book_id
                    or STATE["sort_num"] != sort_num):
                return
            STATE["doc"] = document
            if at_last:
                STATE["page"] = max(0, document.page_count - 1)
            elif fresh:
                STATE["page"] = 0
            else:
                STATE["page"] = document.page_for_path(
                    current_path, current_offset)
            STATE["signature"] = signature
            STATE["last_saved"] = -1
            STATE["loading"] = False

        ctx.run_async("reader", lambda: _prepare_document(
            ctx, STATE["data"].get("Chapter") or {}), success,
            lambda exc: ctx.message(["重新排版失败", str(exc)]))
        return
    STATE.update({"book_id": book_id, "sort_num": sort_num, "data": None,
                  "doc": None, "page": 0, "loading": True, "last_saved": -1,
                  "signature": signature, "fitted_cache": {}})
    ctx.show()

    def operation():
        response = _load_chapter(ctx, book_id, sort_num)
        chapter = response.get("Chapter") or {}
        document = _prepare_document(ctx, chapter)
        return response, document

    def success(result):
        if (generation != STATE["layout_generation"]
                or STATE["book_id"] != book_id
                or STATE["sort_num"] != sort_num):
            return
        response, document = result
        chapter = response.get("Chapter") or {}
        STATE["data"] = response
        STATE["doc"] = document
        STATE["signature"] = signature
        STATE["loading"] = False
        if chapter.get("Font") and not document.font_resolver.custom_font_loaded:
            ctx.toast("章节字体加载失败，正文可能显示异常")
        position = response.get("ReadPosition") or {}
        local = _load_progress(book_id, sort_num)
        if at_last:
            STATE["page"] = max(0, document.page_count - 1)
        elif fresh:
            STATE["page"] = 0
        else:
            server_page = None
            if int(position.get("ChapterId") or 0) == int(chapter.get("Id") or 0):
                server_page = document.page_for_path(position.get("Position") or "")
            local_page = None
            if local:
                local_page = document.page_for_path(local["path"], local["offset"])
            # 多端进度取最远页，避免本机旧缓存覆盖其他设备的新进度
            candidates = [p for p in (server_page, local_page) if p is not None]
            STATE["page"] = max(candidates) if candidates else 0
        chapters = (response.get("Chapter") or {}).get("Chapters") or []
        if chapters:
            _save_progress(ctx)
            if ctx.config.get("prefetch_chapters"):
                _prefetch_neighbors(ctx, book_id, sort_num, chapters)
        _prefetch_images(ctx, document)
        if swipe_delta:
            _configure_swipe(ctx, swipe_delta)

    def error(exc):
        if (generation != STATE["layout_generation"]
                or STATE["book_id"] != book_id
                or STATE["sort_num"] != sort_num):
            return
        STATE["loading"] = False
        ctx.message(["章节加载失败", str(exc)])

    ctx.run_async("reader", operation, success, error)


def _save_progress(ctx):
    if not ctx.api.user or not STATE["doc"] or not STATE["data"]:
        return
    page = int(STATE["page"])
    if page == STATE["last_saved"]:
        return
    STATE["last_saved"] = page
    chapter = (STATE["data"].get("Chapter") or {})
    book_id = int(chapter.get("BookId") or STATE["book_id"])
    path, offset = STATE["doc"].first_anchor_on_page(page)
    try:
        atomic_write(
            _progress_path(book_id, STATE["sort_num"]),
            json.dumps(
                {"path": path, "offset": offset, "page": page},
                ensure_ascii=False,
            ),
        )
    except OSError:
        pass


def upload_progress(ctx):
    if not ctx.api.user or not STATE["doc"] or not STATE["data"]:
        return
    page = int(STATE["page"])
    chapter = (STATE["data"].get("Chapter") or {})
    book_id = int(chapter.get("BookId") or STATE["book_id"])
    chapter_id = int(chapter.get("Id") or 0)
    xpath = STATE["doc"].first_path_on_page(page)
    ctx.run_async("reader", lambda: ctx.api.save_read_position(book_id, chapter_id, xpath),
                  lambda _: None, lambda _: None)


def _turn(ctx, delta):
    doc = STATE["doc"]
    if not doc:
        return
    target = int(STATE["page"]) + int(delta)
    if 0 <= target < doc.page_count:
        _configure_swipe(ctx, delta)
        STATE["page"] = target
        STATE["last_turn_at"] = time.monotonic()
        _save_progress(ctx)
        _prefetch_images(ctx, doc)
        ctx.show()
        return
    _change_chapter(ctx, delta, at_last=(delta < 0))


def _change_chapter(ctx, delta, at_last=False):
    chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
    index = _chapter_index(chapters, STATE["sort_num"]) + int(delta)
    if index < 0 or index >= len(chapters):
        ctx.toast("已经是%s" % ("第一页" if delta < 0 else "最后一页"))
        return
    target = _chapter_sort(chapters, index)
    ctx.replace("reader", book_id=STATE["book_id"], sort_num=target,
                at_last=bool(at_last), swipe_delta=int(delta))
    STATE["last_turn_at"] = time.monotonic()


def header_blocked():
    return time.monotonic() - float(STATE.get("last_turn_at") or 0.0) < 0.35


def render(ctx, canvas):
    if STATE["fullscreen_image"]:
        url = STATE["fullscreen_image"]
        target = height_bucket(canvas.height)
        image = ctx.images.get(url, target)
        if image is None:
            ctx.images.prefetch(
                url, ctx.config.get("strict_tls"), height=target,
                priority=0,
                callback=lambda ok, url=url: _fullscreen_ready(ctx, url, ok),
            )
        if image is not None:
            fitted = _fit_image("fullscreen:" + url, image,
                                canvas.width, canvas.height)
            x = (canvas.width - fitted.width) // 2
            y = (canvas.height - fitted.height) // 2
            canvas.image.paste(fitted, (x, y))
        else:
            canvas.centered_text("图片加载中…", ctx.fonts["body"],
                                 canvas.width // 2, canvas.height // 2)
        return
    doc = STATE["doc"]
    title = (STATE["data"] or {}).get("Chapter", {}).get("Title") or "阅读"
    chrome_visible = bool(STATE.get("chrome_visible"))
    top, content_height = _layout_metrics(ctx)
    if not chrome_visible:
        page_label = "%s/%s" % (STATE["page"] + 1, doc.page_count) if doc else "1/1"
        top = canvas.compact_header(title, page_label)
    STATE["rects"] = {}
    if not doc:
        canvas.centered_text("正在下载并排版…" if STATE["loading"] else "暂无正文",
                             ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
        if chrome_visible:
            _render_chrome(ctx, canvas, title, None)
        return
    page = doc.pages[max(0, min(STATE["page"], len(doc.pages) - 1))]
    STATE["image_rects"] = {}
    for item in page:
        if item["type"] == "text":
            y = top + item["y"]
            if y + item.get("size", 0) > top + content_height:
                continue
            canvas.text_fallback(
                (item["x"], y), item["text"], item["font"],
                item.get("fallback_font"),
            )
        elif item["type"] == "image":
            url = item["url"]
            target = height_bucket(item.get("height") or 1024)
            image = ctx.images.get(url, target)
            if image is None:
                ctx.images.prefetch(
                    url, ctx.config.get("strict_tls"), height=target,
                    priority=0,
                    callback=lambda ok, url=url: _image_ready(ctx, url, ok),
                )
            if image is not None:
                fitted = _fit_image(url, image, item["width"], item["height"])
                x = item["x"] + (item["width"] - fitted.width) // 2
                image_y = top + item["y"]
                canvas.image.paste(fitted, (x, image_y))
                STATE["image_rects"][(item.get("path"), item.get("y"))] = (
                    x, image_y, fitted.width, fitted.height, item["url"])
            else:
                canvas.centered_text("[图片]", ctx.fonts["small"],
                                     canvas.width // 2, top + item["y"] + item["height"] // 2,
                                     fill=canvas.theme.muted)
    if chrome_visible:
        # 控件层画在正文之上：正文分页几何不随控件显隐变化
        _render_chrome(ctx, canvas, title, doc)


def _render_chrome(ctx, canvas, title, doc):
    # 控件层顶栏显示书名而非章节名，章节位置由底栏进度条表达
    book_name = (STATE["data"] or {}).get("Chapter", {}).get("BookName") or title
    canvas.header(book_name, left="返回", right="主页")
    footer_top = canvas.height - _CHROME_FOOTER
    canvas.draw.rectangle([0, footer_top, canvas.width, canvas.height],
                          fill=canvas.theme.background)
    bar_y = canvas.height - 76
    margin = int(canvas.width * 0.025)
    gap = 6
    button_width = (canvas.width - 2 * margin - 3 * gap) // 4
    buttons = [("prev", "上一章"), ("catalog", "目录"),
               ("settings", "设置"), ("next", "下一章")]
    for index, (action, label) in enumerate(buttons):
        rect = (margin + index * (button_width + gap), bar_y, button_width, 56)
        canvas.button(rect, label, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    if not doc:
        return
    progress = int(
        (STATE["page"] + 1) / max(1, doc.page_count)
        * (canvas.width - 2 * margin)
    )
    canvas.draw.rectangle(
        [margin, canvas.height - 10, margin + progress, canvas.height - 5],
        fill=canvas.theme.foreground,
    )


def handle(data, ctx):
    gesture = data.get("gesture")
    if gesture == "down":
        start = data.get("start") or {}
        if (not STATE.get("chrome_visible")
                and float(start.get("y-ratio") or 1.0) < 0.16):
            _set_chrome_visible(ctx, True)
        return
    if gesture != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if STATE["fullscreen_image"]:
        STATE["fullscreen_image"] = None
        ctx.show()
        return
    if STATE.get("chrome_visible"):
        # 控件层可见时只有顶栏（返回/主页由 PageContext 拦截）和底栏可操作，
        # 点中间正文区仅收起控件层，不翻页
        if y >= ctx.height - _CHROME_FOOTER:
            for key, rect in STATE["rects"].items():
                rx, ry, width, height = rect
                if not (rx <= x < rx + width and ry <= y < ry + height):
                    continue
                action = key[0]
                if action == "prev":
                    _change_chapter(ctx, -1, at_last=True)
                elif action == "next":
                    _change_chapter(ctx, 1)
                elif action == "catalog":
                    STATE["catalog_page"] = 0
                    ctx.navigate(
                        "catalog",
                        book_id=STATE["book_id"],
                        sort_num=STATE["sort_num"],
                    )
                elif action == "settings":
                    ctx.navigate("settings")
                return
            return
        _set_chrome_visible(ctx, False)
        return
    top, _ = _layout_metrics(ctx)
    if y < top:
        return
    # 仅命中插图实际渲染区域才进预览；控件层可见时上面已优先收起控件
    for rect in STATE["image_rects"].values():
        rx, ry, width, height, url = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            STATE["fullscreen_image"] = url
            ctx.show()
            return
    if x < int(ctx.width * 0.25):
        _turn(ctx, -1)
        return
    if x > int(ctx.width * 0.75):
        _turn(ctx, 1)


def _catalog_layout(ctx):
    row_height = max(62, int(ctx.height * 0.052))
    rows = max(1, (ctx.height - _chrome_header_height(ctx) - 100) // row_height)
    return row_height, rows


def render_catalog(ctx, canvas):
    top = canvas.header("章节目录", left="返回", right="主页")
    chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
    margin = int(canvas.width * 0.035)
    row_height, rows = _catalog_layout(ctx)
    pages = max(1, (len(chapters) + rows - 1) // rows)
    STATE["catalog_page"] = min(STATE["catalog_page"], pages - 1)
    start = STATE["catalog_page"] * rows
    STATE["rects"] = {}
    for row in range(rows):
        index = start + row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        if index >= len(chapters):
            continue
        STATE["rects"][("catalog", index)] = rect
        current = int(STATE["sort_num"]) == _chapter_sort(chapters, index)
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid,
                                      fill=canvas.theme.inverse_bg if current else canvas.theme.background,
                                      width=1)
        fill = canvas.theme.inverse_fg if current else canvas.theme.foreground
        canvas.text((rect[0] + 12, y + 10),
                    canvas.fit_text("%s. %s" % (index + 1, chapters[index]),
                                    ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"], fill=fill)
    nav_y = canvas.height - 72
    width = int(canvas.width * 0.25)
    for key, rect, label in [
        ("prev", (margin, nav_y, width, 52), "上页"),
        ("count", ((canvas.width - width) // 2, nav_y, width, 52),
         "%s/%s" % (STATE["catalog_page"] + 1, pages)),
        ("next", (canvas.width - margin - width, nav_y, width, 52), "下页"),
    ]:
        canvas.button(rect, label, font=ctx.fonts["tiny"])
        STATE["rects"][(key, 0)] = rect


def handle_catalog(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        if key[0] == "catalog":
            # 目录是 reader 压栈进来的；replace 前先丢掉栈里那个旧 reader，
            # 否则返回键会回到跳转前的章节而不是书籍详情
            if ctx.stack and ctx.stack[-1][0] == "reader":
                ctx.stack.pop()
            chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
            if not 0 <= key[1] < len(chapters):
                return
            ctx.replace("reader", book_id=STATE["book_id"],
                        sort_num=_chapter_sort(chapters, key[1]), fresh=True)
        elif key[0] == "prev" and STATE["catalog_page"] > 0:
            STATE["catalog_page"] -= 1
            ctx.show()
        elif key[0] == "next":
            chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
            _, rows = _catalog_layout(ctx)
            pages = max(1, (len(chapters) + rows - 1) // rows)
            if STATE["catalog_page"] < pages - 1:
                STATE["catalog_page"] += 1
                ctx.show()
        return
