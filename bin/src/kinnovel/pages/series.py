STATE = {
    "title": "",
    "series_name": "",
    "items": [],
    "current_id": 0,
    "page": 0,
    "total_pages": 1,
    "server_paged": False,
    "rects": {},
    "loading": False,
    "loaded": False,
    "error": "",
    "generation": 0,
}


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    row_height = max(76, int(ctx.height * 0.063))
    rows = max(1, (ctx.height - (top + 12) - 92) // row_height)
    return top, row_height, rows


def _page_items(ctx):
    items = STATE.get("items") or []
    if STATE.get("server_paged"):
        # 服务端已按页返回, 当前页条目即渲染列表。
        return list(items)
    rows = _layout(ctx)[2]
    page = max(0, int(STATE.get("page") or 0))
    return list(items[page * rows:(page + 1) * rows])


def _page_count(ctx):
    if STATE.get("server_paged"):
        return max(1, int(STATE.get("total_pages") or 1))
    rows = _layout(ctx)[2]
    items = STATE.get("items") or []
    return max(1, (len(items) + rows - 1) // rows)


def enter(ctx):
    STATE["title"] = str(ctx.params.get("title") or "系列")
    STATE["series_name"] = str(ctx.params.get("series_name") or "").strip()
    books = list(ctx.params.get("books") or [])
    STATE["items"] = books
    STATE["current_id"] = int(ctx.params.get("current_id") or 0)
    STATE["rects"] = {}
    STATE["loading"] = False
    STATE["error"] = ""
    has_other = any(
        int(item.get("Id") or 0) != STATE["current_id"]
        for item in books
    )
    if has_other:
        # 书籍详情已提供完整系列: 本地分页渲染, 不请求服务端。
        STATE["server_paged"] = False
        STATE["total_pages"] = 1
        if not ctx.returning:
            STATE["page"] = 0
        STATE["loaded"] = True
        return
    STATE["loaded"] = not STATE["series_name"]
    if STATE["series_name"]:
        # 走接口分页: 返回时保留当前页码, 只请求该页。
        _load(ctx, page=STATE["page"] if ctx.returning else 0)


def _load(ctx, page=None):
    STATE["generation"] += 1
    generation = STATE["generation"]
    STATE["loading"] = True
    STATE["error"] = ""
    if page is not None:
        STATE["page"] = max(0, int(page))
    rows = _layout(ctx)[2]

    def operation():
        return ctx.api.get_books_by_series(
            STATE["series_name"],
            page=STATE["page"] + 1,
            size=rows,
            ignore_japanese=bool(ctx.config.get("ignore_japanese")),
            ignore_ai=bool(ctx.config.get("ignore_ai")),
        )

    def success(result):
        if generation != STATE["generation"]:
            return
        if isinstance(result, dict):
            STATE["items"] = result.get("Data") or result.get("data") or []
            total = max(1, int(result.get("TotalPages") or 1))
            try:
                current = int(result.get("Page") or (STATE["page"] + 1))
            except (TypeError, ValueError):
                current = STATE["page"] + 1
            STATE["total_pages"] = total
            STATE["page"] = max(0, min(current - 1, total - 1))
        else:
            STATE["items"] = result or []
            STATE["total_pages"] = 1
            STATE["page"] = 0
        STATE["server_paged"] = True
        STATE["loading"] = False
        STATE["loaded"] = True

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        STATE["error"] = str(exc)
        ctx.message(["系列加载失败", str(exc)])

    ctx.run_async("series", operation, success, error)


def render(ctx, canvas):
    top = canvas.header(STATE["title"] or "系列", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    _top, row_height, rows = _layout(ctx)
    start_y = top + 12
    pages = _page_count(ctx)
    STATE["page"] = min(STATE["page"], pages - 1)
    items = _page_items(ctx)
    STATE["rects"] = {}
    for row in range(rows):
        y = start_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 7)
        STATE["rects"][("item", row)] = rect
        if row >= len(items):
            continue
        item = items[row]
        if not isinstance(item, dict):
            continue
        current = int(item.get("Id") or 0) == STATE["current_id"]
        canvas.draw.rounded_rectangle(
            [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
            radius=8,
            outline=canvas.theme.mid,
            fill=canvas.theme.inverse_bg if current else canvas.theme.background,
            width=1,
        )
        text_x = rect[0] + 14
        fill = canvas.theme.inverse_fg if current else canvas.theme.foreground
        canvas.text(
            (text_x, y + 9),
            canvas.fit_text(
                item.get("Title") or "未知",
                ctx.fonts["small"],
                rect[2] - 28,
            ),
            font=ctx.fonts["small"],
            fill=fill,
        )
        if current:
            canvas.text(
                (text_x, y + 42),
                "当前书籍",
                font=ctx.fonts["tiny"],
                fill=canvas.theme.muted,
            )
    nav_y = canvas.height - 68
    width = int(canvas.width * 0.25)
    for key, rect, label in (
        ("prev", (margin, nav_y, width, 52), "上一页"),
        ("count", ((canvas.width - width) // 2, nav_y, width, 52),
         "%s/%s" % (STATE["page"] + 1, pages)),
        ("next", (canvas.width - margin - width, nav_y, width, 52), "下一页"),
    ):
        active = key == "count" or (
            key == "prev" and STATE["page"] > 0
        ) or (
            key == "next" and STATE["page"] < pages - 1
        )
        canvas.button(rect, label, active=active, font=ctx.fonts["tiny"])
        STATE["rects"][(key, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text(
            "加载中…",
            ctx.fonts["body"],
            canvas.width // 2,
            canvas.height // 2,
        )
    elif STATE["loaded"] and not items:
        canvas.centered_text(
            "本系列暂无其他书籍",
            ctx.fonts["body"],
            canvas.width // 2,
            canvas.height // 2,
            fill=canvas.theme.muted,
        )


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x = int(data.get("x-pixel") or 0)
    y = int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        action = key[0]
        if action == "item":
            items = _page_items(ctx)
            if key[1] < len(items):
                item = items[key[1]]
                if int(item.get("Id") or 0) != STATE["current_id"]:
                    ctx.navigate("book", book_id=item.get("Id"))
        elif action == "prev" and STATE["page"] > 0:
            STATE["page"] -= 1
            if STATE.get("server_paged"):
                _load(ctx, page=STATE["page"])
            else:
                ctx.show()
        elif action == "next":
            pages = _page_count(ctx)
            if STATE["page"] < pages - 1:
                STATE["page"] += 1
                if STATE.get("server_paged"):
                    _load(ctx, page=STATE["page"])
                else:
                    ctx.show()
        return
