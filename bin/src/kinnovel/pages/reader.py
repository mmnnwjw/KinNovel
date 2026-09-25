import threading

from PIL import Image, ImageDraw, ImageOps

from ..reader import ReaderDocument


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
}


def _signature(ctx, book_id, sort_num):
    top = max(72, int(ctx.height * 0.085))
    return (
        int(book_id), int(sort_num), int(ctx.config.get("font_size") or 36),
        float(ctx.config.get("line_spacing") or 1.42),
        int(ctx.config.get("reader_margin") or 34),
        int(ctx.width), int(ctx.height), top,
        str(ctx.config.get("font_path") or ""),
    )


def _prepare_document(ctx, chapter):
    image = Image.new("L", (8, 8), 255)
    draw = ImageDraw.Draw(image)
    document = ReaderDocument(chapter, ctx.api.server,
                              ctx.config.get("font_path"), ctx.config)
    top = max(72, int(ctx.height * 0.085))
    content_height = max(240, int(ctx.height) - top - 92)
    document.prepare(draw, ctx.width, content_height)
    return document


def enter(ctx):
    book_id = int(ctx.params.get("book_id") or 0)
    sort_num = int(ctx.params.get("sort_num") or 1)
    signature = _signature(ctx, book_id, sort_num)
    if (STATE["data"] and STATE["book_id"] == book_id and
            STATE["sort_num"] == sort_num and STATE["signature"] == signature):
        ctx.show()
        return
    if (STATE["data"] and STATE["book_id"] == book_id and
            STATE["sort_num"] == sort_num and STATE["doc"]):
        current_path = STATE["doc"].first_path_on_page(STATE["page"])
        STATE["loading"] = True
        ctx.show()

        def success(document):
            STATE["doc"] = document
            STATE["page"] = document.page_for_path(current_path)
            STATE["signature"] = signature
            STATE["last_saved"] = -1
            STATE["loading"] = False

        ctx.run_async("reader", lambda: _prepare_document(
            ctx, STATE["data"].get("Chapter") or {}), success,
            lambda exc: ctx.message(["重新排版失败", str(exc)]))
        return
    STATE.update({"book_id": book_id, "sort_num": sort_num, "data": None,
                  "doc": None, "page": 0, "loading": True, "last_saved": -1,
                  "signature": signature})
    ctx.show()

    def operation():
        response = ctx.api.get_novel_content(
            book_id, sort_num, convert=ctx.config.get("convert"))
        chapter = response.get("Chapter") or {}
        document = _prepare_document(ctx, chapter)
        return response, document

    def success(result):
        response, document = result
        chapter = response.get("Chapter") or {}
        STATE["data"] = response
        STATE["doc"] = document
        STATE["signature"] = signature
        STATE["loading"] = False
        if chapter.get("Font") and not document.font_resolver.custom_font_loaded:
            ctx.toast("章节字体加载失败，正文可能显示异常")
        position = response.get("ReadPosition") or {}
        if int(position.get("ChapterId") or 0) == int((response.get("Chapter") or {}).get("Id") or 0):
            STATE["page"] = document.page_for_path(position.get("Position") or "")
        else:
            STATE["page"] = 0
        chapters = (response.get("Chapter") or {}).get("Chapters") or []
        if chapters:
            _save_progress(ctx)

    def error(exc):
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
        STATE["page"] = target
        _save_progress(ctx)
        ctx.show()
        return
    _change_chapter(ctx, delta)


def _change_chapter(ctx, delta):
    sort_num = int(STATE["sort_num"]) + int(delta)
    chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
    if sort_num < 1 or sort_num > len(chapters):
        ctx.toast("已经是%s" % ("第一页" if delta < 0 else "最后一页"))
        return
    ctx.replace("reader", book_id=STATE["book_id"], sort_num=sort_num)


def render(ctx, canvas):
    top = canvas.header((STATE["data"] or {}).get("Chapter", {}).get("Title") or "阅读",
                        left="返回", right="主页")
    doc = STATE["doc"]
    if not doc:
        canvas.centered_text("正在下载并排版…" if STATE["loading"] else "暂无正文",
                             ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
        return
    content_height = canvas.height - top - 92
    page = doc.pages[max(0, min(STATE["page"], len(doc.pages) - 1))]
    for item in page:
        if item["type"] == "text":
            y = top + item["y"]
            if y + item.get("size", 0) > top + content_height:
                continue
            canvas.text((item["x"], y), item["text"], font=item["font"])
        elif item["type"] == "image":
            image = ctx.images.get(item["url"])
            if image is None:
                url = item["url"]
                if url not in STATE["image_pending"]:
                    STATE["image_pending"].add(url)
                    ctx.run_async(
                        "reader",
                        lambda url=url: ctx.images.prefetch(url, ctx.config.get("strict_tls")),
                        lambda _result, url=url: STATE["image_pending"].discard(url),
                        lambda _exc, url=url: STATE["image_pending"].discard(url),
                    )
            if image is not None:
                fitted = ImageOps.contain(image, (item["width"], item["height"]),
                                          method=Image.Resampling.LANCZOS)
                x = item["x"] + (item["width"] - fitted.width) // 2
                canvas.image.paste(fitted, (x, top + item["y"]))
            else:
                canvas.centered_text("[图片]", ctx.fonts["small"],
                                     canvas.width // 2, top + item["y"] + item["height"] // 2,
                                     fill=canvas.theme.muted)
    bar_y = canvas.height - 76
    margin = int(canvas.width * 0.025)
    gap = 6
    button_width = (canvas.width - 2 * margin - 3 * gap) // 4
    page_label = "%s/%s" % (STATE["page"] + 1, doc.page_count)
    buttons = [("prev", "上一章"), ("catalog", page_label),
               ("settings", "设置"), ("next", "下一章")]
    STATE["rects"] = {}
    for index, (action, label) in enumerate(buttons):
        rect = (margin + index * (button_width + gap), bar_y, button_width, 56)
        canvas.button(rect, label, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    progress = int((STATE["page"] + 1) / max(1, doc.page_count) * (canvas.width - 2 * margin))
    canvas.draw.rectangle([margin, canvas.height - 10, margin + progress, canvas.height - 5],
                          fill=canvas.theme.foreground)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.72):
        STATE["catalog_page"] = 0
        ctx.navigate("catalog", book_id=STATE["book_id"], sort_num=STATE["sort_num"])
        return
    if x < int(ctx.width * 0.18) and y > int(ctx.height * 0.09) and y < ctx.height - 80:
        _turn(ctx, -1)
        return
    if x > int(ctx.width * 0.82) and y > int(ctx.height * 0.09) and y < ctx.height - 80:
        _turn(ctx, 1)
        return
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        action = key[0]
        if action == "prev":
            _change_chapter(ctx, -1)
        elif action == "next":
            _change_chapter(ctx, 1)
        elif action == "catalog":
            STATE["catalog_page"] = 0
            ctx.navigate("catalog", book_id=STATE["book_id"], sort_num=STATE["sort_num"])
        elif action == "settings":
            ctx.navigate("settings")
        return


def render_catalog(ctx, canvas):
    top = canvas.header("章节目录", left="返回", right="主页")
    chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
    margin = int(canvas.width * 0.035)
    row_height = max(62, int(canvas.height * 0.052))
    rows = max(1, (canvas.height - top - 100) // row_height)
    pages = max(1, (len(chapters) + rows - 1) // rows)
    STATE["catalog_page"] = min(STATE["catalog_page"], pages - 1)
    start = STATE["catalog_page"] * rows
    STATE["rects"] = {}
    for row in range(rows):
        index = start + row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("catalog", index)] = rect
        if index >= len(chapters):
            continue
        current = int(STATE["sort_num"]) == index + 1
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
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        if key[0] == "catalog":
            ctx.replace("reader", book_id=STATE["book_id"], sort_num=key[1] + 1)
        elif key[0] == "prev" and STATE["catalog_page"] > 0:
            STATE["catalog_page"] -= 1
            ctx.show()
        elif key[0] == "next":
            chapters = ((STATE["data"] or {}).get("Chapter") or {}).get("Chapters") or []
            row_height = max(62, int(ctx.height * 0.052))
            rows = max(1, (ctx.height - int(ctx.height * 0.085) - 100) // row_height)
            pages = max(1, (len(chapters) + rows - 1) // rows)
            if STATE["catalog_page"] < pages - 1:
                STATE["catalog_page"] += 1
                ctx.show()
        return
