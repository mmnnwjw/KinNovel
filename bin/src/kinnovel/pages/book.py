from ..utils import format_time


STATE = {
    "book_id": 0,
    "data": None,
    "chapter_page": 0,
    "bound": None,
    "rects": {},
    "loading": False,
    "generation": 0,
}


def _load(ctx, force=False):
    if not ctx.api.user:
        ctx.toast("查看详情需要登录")
        ctx.navigate("account")
        return
    book_id = int(ctx.params.get("book_id") or 0)
    if book_id <= 0:
        ctx.message("书籍 ID 无效")
        return
    if not force and STATE["data"] and STATE["book_id"] == book_id:
        return
    STATE["book_id"] = book_id
    STATE["loading"] = True
    STATE["generation"] += 1
    generation = STATE["generation"]

    def operation():
        return ctx.api.get_book_info(book_id)

    def success(result):
        if generation != STATE["generation"] or book_id != STATE["book_id"]:
            return
        STATE["data"] = result
        STATE["loading"] = False
        STATE["chapter_page"] = 0
        cover = (result.get("Book") or {}).get("Cover")
        if cover:
            ctx.run_async("book", lambda: ctx.images.prefetch(cover, ctx.config.get("strict_tls")))
        if ctx.api.user:
            def shelf_result(shelf):
                ids = {item.get("id") for item in (shelf.get("data") or [])
                       if str(item.get("type")) != "FOLDER"}
                STATE["bound"] = book_id in ids
                ctx.show()
            ctx.run_async("book", ctx.api.get_book_shelf, shelf_result, lambda _: None)

    def error(exc):
        if generation != STATE["generation"] or book_id != STATE["book_id"]:
            return
        STATE["loading"] = False
        ctx.message(["加载书籍失败", str(exc)])

    ctx.run_async("book", operation, success, error)


def enter(ctx):
    _load(ctx)


def _chapter_rows(ctx, canvas, start_y):
    data = STATE["data"] or {}
    chapters = (data.get("Book") or {}).get("Chapters") or []
    per_page = 6
    total_pages = max(1, (len(chapters) + per_page - 1) // per_page)
    STATE["chapter_page"] = min(STATE["chapter_page"], total_pages - 1)
    start = STATE["chapter_page"] * per_page
    row_height = max(58, int(canvas.height * 0.047))
    available = canvas.height - start_y - 150
    row_height = min(row_height, max(46, available // per_page))
    for row in range(per_page):
        index = start + row
        y = start_y + row * row_height
        rect = (int(canvas.width * 0.035), y, int(canvas.width * 0.93), row_height - 5)
        STATE["rects"][("chapter", index)] = rect
        if index >= len(chapters):
            continue
        chapter = chapters[index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=9, outline=canvas.theme.mid, width=1)
        label = canvas.fit_text(chapter.get("Title") or ("第 %s 章" % (index + 1)),
                                ctx.fonts["small"], rect[2] - 24)
        canvas.text((rect[0] + 12, y + (row_height - 30) // 2), label,
                    font=ctx.fonts["small"])
    return total_pages


def _chapter_sort(chapters, index):
    try:
        value = int(chapters[index].get("SortNum"))
        if value > 0:
            return value
    except (IndexError, TypeError, ValueError):
        pass
    return index + 1


def render(ctx, canvas):
    top = canvas.header("书籍详情", left="返回", right="主页")
    if not STATE["data"]:
        canvas.centered_text("加载中…" if STATE["loading"] else "暂无数据",
                             ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
        return
    data = STATE["data"]
    book = data.get("Book") or {}
    classification = (book.get("Extra") or {}).get("classification") or {}
    margin = int(canvas.width * 0.035)
    cover_width = int(canvas.width * 0.23)
    cover_height = int(cover_width * 1.45)
    cover_y = top + 20
    cover = ctx.images.cover(book.get("Cover"), cover_width, cover_height,
                             strict_tls=ctx.config.get("strict_tls"))
    if cover is not None:
        canvas.image.paste(cover, (margin, cover_y))
    canvas.draw.rectangle([margin, cover_y, margin + cover_width, cover_y + cover_height],
                          outline=canvas.theme.mid, width=1)
    info_x = margin + cover_width + 24
    info_width = canvas.width - info_x - margin
    title_lines = canvas.wrap(book.get("Title") or "未知", ctx.fonts["title"], info_width)[:2]
    y = cover_y
    for line in title_lines:
        canvas.text((info_x, y), line, font=ctx.fonts["title"])
        y += ctx.fonts["title"].size + 8
    author = book.get("Author") or classification.get("author") or "未知"
    details = [
        "作者: " + str(author),
        "最后更新: " + str(book.get("LastUpdatedChapter") or "未知"),
        "更新时间: " + format_time(book.get("LastUpdatedAt")),
        "浏览: %s  收藏: %s" % (book.get("Views", 0), book.get("Favorite", 0)),
    ]
    for line in details:
        canvas.text((info_x, y), canvas.fit_text(line, ctx.fonts["small"], info_width),
                    font=ctx.fonts["small"], fill=canvas.theme.muted)
        y += ctx.fonts["small"].size + 8
    summary_y = cover_y + cover_height + 12
    tags = classification.get("tags") or []
    if tags:
        canvas.text((margin, summary_y), "标签: " + "、".join(tags[:6]),
                    font=ctx.fonts["small"], fill=canvas.theme.muted)
        summary_y += ctx.fonts["small"].size + 8
    intro = _clean_intro(book.get("Introduction") or "")
    canvas.text((margin, summary_y), "简介", font=ctx.fonts["body"])
    summary_y += ctx.fonts["body"].size + 4
    summary_lines = canvas.wrap(intro, ctx.fonts["small"], canvas.width - 2 * margin)[:3]
    for line in summary_lines:
        canvas.text((margin, summary_y), line, font=ctx.fonts["small"],
                    fill=canvas.theme.muted)
        summary_y += ctx.fonts["small"].size + 5
    chapter_y = max(summary_y + 12, int(canvas.height * 0.46))
    canvas.text((margin, chapter_y), "章节", font=ctx.fonts["body"])
    total_pages = _chapter_rows(ctx, canvas, chapter_y + ctx.fonts["body"].size + 8)
    bottom_y = canvas.height - 82
    buttons = [
        ("read", "继续阅读" if data.get("ReadPosition") else "开始阅读"),
        ("shelf", "移出书架" if STATE["bound"] else "加入书架"),
        ("comments", "评论"),
        ("prev", "上页"),
        ("next", "下页 (%s/%s)" % (STATE["chapter_page"] + 1, total_pages)),
    ]
    gap = 8
    width = (canvas.width - 2 * margin - gap * 4) // 5
    for index, (action, label) in enumerate(buttons):
        rect = (margin + index * (width + gap), bottom_y, width, 58)
        active = action not in ("prev", "next") or (
            action == "prev" and STATE["chapter_page"] > 0) or (
            action == "next" and STATE["chapter_page"] < total_pages - 1)
        canvas.button(rect, label, active=active, font=ctx.fonts["small"])
        STATE["rects"][(action, 0)] = rect


def _clean_intro(value):
    import re
    return re.sub(r"<[^>]+>", " ", str(value or "")).replace("&nbsp;", " ").strip()


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        action = key[0]
        if action == "chapter":
            chapters = (STATE["data"].get("Book") or {}).get("Chapters") or []
            if key[1] < len(chapters):
                ctx.navigate("reader", book_id=STATE["book_id"],
                             sort_num=_chapter_sort(chapters, key[1]),
                             fresh=True)
        elif action == "read":
            chapters = (STATE["data"].get("Book") or {}).get("Chapters") or []
            position = STATE["data"].get("ReadPosition") or {}
            target = None
            for chapter in chapters:
                if int(chapter.get("Id") or 0) == int(position.get("ChapterId") or 0):
                    target = chapter
                    break
            target = target or (chapters[0] if chapters else None)
            if target:
                target_index = chapters.index(target)
                ctx.navigate("reader", book_id=STATE["book_id"],
                             sort_num=_chapter_sort(chapters, target_index))
        elif action == "shelf":
            _toggle_shelf(ctx)
        elif action == "comments":
            ctx.navigate("comments", comment_type="Book", target_id=STATE["book_id"])
        elif action == "prev" and STATE["chapter_page"] > 0:
            STATE["chapter_page"] -= 1
            ctx.show()
        elif action == "next":
            chapters = (STATE["data"].get("Book") or {}).get("Chapters") or []
            pages = max(1, (len(chapters) + 5) // 6)
            if STATE["chapter_page"] < pages - 1:
                STATE["chapter_page"] += 1
                ctx.show()
        return


def _toggle_shelf(ctx):
    if not ctx.api.user:
        ctx.toast("请先登录")
        ctx.navigate("account")
        return
    book = STATE["data"].get("Book") or {}
    book_type = "COMIC" if book.get("Type") == "Comic" else "NOVEL"
    operation = lambda: _set_shelf(ctx, int(STATE["book_id"]), book_type,
                                   bound=bool(STATE["bound"]))
    def success(value):
        STATE["bound"] = value
        ctx.toast("已加入书架" if value else "已移出书架")
    ctx.run_async("book", operation, success, lambda exc: ctx.message(["操作失败", str(exc)]))


def _set_shelf(ctx, book_id, book_type, bound):
    result = ctx.api.get_book_shelf()
    items = list(result.get("data") or [])
    if bound:
        items = [item for item in items if int(item.get("id") or 0) != book_id]
    else:
        if not any(int(item.get("id") or 0) == book_id for item in items):
            items.insert(0, {
                "id": book_id,
                "type": book_type,
                "parents": [],
                "index": 0,
                "updateAt": __import__("datetime").datetime.utcnow().isoformat() + "Z",
            })
    ctx.api.save_book_shelf(items)
    return not bound
