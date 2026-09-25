STATE = {
    "kind": "daily",
    "items": [],
    "rects": {},
    "loading": False,
    "loaded": False,
    "generation": 0,
}

LETTERS = [("daily", "日榜"), ("weekly", "周榜"), ("monthly", "月榜")]


def enter(ctx):
    if not STATE["loaded"]:
        _load(ctx)


def _load(ctx):
    STATE["generation"] += 1
    generation = STATE["generation"]
    STATE["loading"] = True
    days = {"daily": 1, "weekly": 7, "monthly": 31}[STATE["kind"]]

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result or []
        STATE["loading"] = False
        STATE["loaded"] = True
        for item in STATE["items"][:6]:
            url = item.get("Cover")
            if url:
                ctx.run_async("rank", lambda url=url: ctx.images.prefetch(
                    url, ctx.config.get("strict_tls")))

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["排行加载失败", str(exc)])

    ctx.run_async("rank", lambda: ctx.api.get_rank(days), success, error)


def render(ctx, canvas):
    top = canvas.header("排行榜", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    gap = 10
    tab_width = (canvas.width - 2 * margin - 2 * gap) // 3
    STATE["rects"] = {}
    for index, (key, label) in enumerate(LETTERS):
        rect = (margin + index * (tab_width + gap), top + 12, tab_width, 58)
        canvas.button(rect, label, active=STATE["kind"] == key, font=ctx.fonts["small"])
        STATE["rects"][("kind", key)] = rect
    start_y = top + 84
    row_height = max(72, int(canvas.height * 0.061))
    rows = max(1, (canvas.height - start_y - 40) // row_height)
    for row in range(rows):
        index = row
        y = start_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", row)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.centered_text(str(index + 1), ctx.fonts["body"],
                             rect[0] + 34, y + rect[3] // 2)
        title = item.get("Title") or "未知"
        author = item.get("UserName") or ""
        canvas.text((rect[0] + 72, y + 10),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 100),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 72, y + 42),
                    canvas.fit_text(author, ctx.fonts["tiny"], rect[2] - 100),
                    font=ctx.fonts["tiny"], fill=canvas.theme.muted)
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
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
