STATE = {
    "kind": "daily",
    "items": [],
    "page": 1,
    "total_pages": 1,
    "rects": {},
    "loading": False,
    "loaded": False,
    "generation": 0,
}

LETTERS = [("daily", "日榜"), ("weekly", "周榜"), ("monthly", "月榜")]


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    list_y = top + 84
    row_height = max(64, int(ctx.height * 0.058))
    nav_y = ctx.height - 72
    per_page = max(1, (nav_y - list_y - 12) // row_height)
    return top, list_y, row_height, per_page, nav_y


def enter(ctx):
    if not STATE["loaded"]:
        _load(ctx)


def _load(ctx):
    STATE["generation"] += 1
    generation = STATE["generation"]
    STATE["loading"] = True
    STATE["page"] = 1
    days = {"daily": 1, "weekly": 7, "monthly": 31}[STATE["kind"]]

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result or []
        _total_pages(ctx)
        STATE["loading"] = False
        STATE["loaded"] = True

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["排行加载失败", str(exc)])

    ctx.run_async("rank", lambda: ctx.api.get_rank(days), success, error)


def _total_pages(ctx):
    _top, _list_y, _row_height, per_page, _nav_y = _layout(ctx)
    STATE["total_pages"] = max(1, (len(STATE["items"]) + per_page - 1) // per_page)
    STATE["page"] = max(1, min(int(STATE["page"]), STATE["total_pages"]))
    return STATE["total_pages"]


def render(ctx, canvas):
    top, list_y, row_height, per_page, nav_y = _layout(ctx)
    canvas.header("排行榜", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    gap = 10
    tab_width = (canvas.width - 2 * margin - 2 * gap) // 3
    STATE["rects"] = {}
    for index, (key, label) in enumerate(LETTERS):
        rect = (margin + index * (tab_width + gap), top + 12, tab_width, 58)
        canvas.button(rect, label, active=STATE["kind"] == key, font=ctx.fonts["small"])
        STATE["rects"][("kind", key)] = rect

    pages = _total_pages(ctx)
    start = (STATE["page"] - 1) * per_page
    for row in range(per_page):
        index = start + row
        y = list_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.centered_text(str(index + 1), ctx.fonts["body"],
                             rect[0] + 38, y + rect[3] // 2)
        title = item.get("Title") or "未知"
        author = item.get("UserName") or ""
        canvas.text((rect[0] + 78, y + 8),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 106),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 78, y + 40),
                    canvas.fit_text(author, ctx.fonts["tiny"], rect[2] - 106),
                    font=ctx.fonts["tiny"], fill=canvas.theme.muted)

    nav_width = int(canvas.width * 0.25)
    for key, rect, label in (
        ("prev", (margin, nav_y, nav_width, 54), "上一页"),
        ("count", ((canvas.width - nav_width) // 2, nav_y, nav_width, 54),
         "%s/%s" % (STATE["page"], pages)),
        ("next", (canvas.width - margin - nav_width, nav_y, nav_width, 54), "下一页"),
    ):
        enabled = key == "count" or (
            key == "prev" and STATE["page"] > 1) or (
            key == "next" and STATE["page"] < pages)
        canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
        STATE["rects"][(key, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"],
                             canvas.width // 2, canvas.height // 2)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key in (("prev", 0), ("next", 0), ("count", 0)):
        rect = STATE["rects"].get(key)
        if not rect:
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            pages = _total_pages(ctx)
            if key[0] == "prev" and STATE["page"] > 1:
                STATE["page"] -= 1
            elif key[0] == "next" and STATE["page"] < pages:
                STATE["page"] += 1
            ctx.show()
            return
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        if key[0] == "kind":
            STATE["kind"] = key[1]
            _load(ctx)
        elif key[0] == "item" and key[1] < len(STATE["items"]):
            ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))
        return
