STATE = {
    "title": "",
    "series_name": "",
    "items": [],
    "current_id": 0,
    "page": 0,
    "total_pages": 1,
    "rects": {},
    "loading": False,
    "loaded": False,
    "generation": 0,
}


def enter(ctx):
    STATE["title"] = str(ctx.params.get("title") or "系列")
    STATE["series_name"] = str(ctx.params.get("series_name") or "").strip()
    STATE["items"] = list(ctx.params.get("books") or [])
    STATE["current_id"] = int(ctx.params.get("current_id") or 0)
    STATE["page"] = 0
    STATE["total_pages"] = 1
    STATE["rects"] = {}
    STATE["loading"] = False
    has_other = any(
        int(item.get("Id") or 0) != STATE["current_id"]
        for item in STATE["items"]
    )
    STATE["loaded"] = has_other or not STATE["series_name"]
    if not has_other and STATE["series_name"]:
        _load(ctx)


def _load(ctx):
    STATE["generation"] += 1
    generation = STATE["generation"]
    STATE["loading"] = True

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result.get("Data") or []
        STATE["page"] = 0
        STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))
        STATE["loading"] = False
        STATE["loaded"] = True

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["系列加载失败", str(exc)])

    ctx.run_async(
        "series",
        lambda: ctx.api.get_books_by_series(
            STATE["series_name"],
            page=1,
            size=24,
            ignore_japanese=bool(ctx.config.get("ignore_japanese")),
            ignore_ai=bool(ctx.config.get("ignore_ai")),
        ),
        success,
        error,
    )


def render(ctx, canvas):
    top = canvas.header(STATE["title"] or "系列", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    row_height = max(76, int(canvas.height * 0.063))
    start_y = top + 12
    rows = max(1, (canvas.height - start_y - 92) // row_height)
    items = STATE["items"]
    pages = max(1, (len(items) + rows - 1) // rows)
    STATE["page"] = min(STATE["page"], pages - 1)
    start = STATE["page"] * rows
    STATE["rects"] = {}
    for row in range(rows):
        index = start + row
        y = start_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 7)
        STATE["rects"][("item", index)] = rect
        if index >= len(items):
            continue
        item = items[index]
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
            items = STATE["items"]
            if key[1] < len(items):
                item = items[key[1]]
                if int(item.get("Id") or 0) != STATE["current_id"]:
                    ctx.navigate("book", book_id=item.get("Id"))
        elif action == "prev" and STATE["page"] > 0:
            STATE["page"] -= 1
            ctx.show()
        elif action == "next":
            row_height = max(76, int(ctx.height * 0.063))
            top = max(72, int(ctx.height * 0.085))
            rows = max(1, (ctx.height - top - 12 - 92) // row_height)
            pages = max(1, (len(STATE["items"]) + rows - 1) // rows)
            if STATE["page"] < pages - 1:
                STATE["page"] += 1
                ctx.show()
        return
