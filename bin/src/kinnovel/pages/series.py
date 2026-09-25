STATE = {
    "name": "",
    "items": [],
    "page": 1,
    "total_pages": 1,
    "rects": {},
    "loading": False,
    "generation": 0,
}


def enter(ctx):
    name = str(ctx.params.get("name") or "")
    if name != STATE["name"]:
        STATE.update({"name": name, "page": 1, "items": [], "loading": True})
    _load(ctx, STATE["page"])


def _load(ctx, page=1):
    STATE["page"] = max(1, int(page))
    STATE["loading"] = True
    STATE["generation"] += 1
    generation = STATE["generation"]

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result.get("Data") or []
        STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))
        STATE["loading"] = False

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["系列加载失败", str(exc)])

    ctx.run_async("series", lambda: ctx.api.get_books_by_series(
        STATE["name"], STATE["page"], 16), success, error)


def render(ctx, canvas):
    top = canvas.header("系列 · " + STATE["name"], left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    row_height = max(72, int(canvas.height * 0.060))
    rows = max(1, (canvas.height - top - 90) // row_height)
    start = (STATE["page"] - 1) * rows
    STATE["rects"] = {}
    for row in range(rows):
        index = start + row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.text((rect[0] + 12, y + 10),
                    canvas.fit_text(item.get("Title") or "未知", ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
    if STATE["total_pages"] > 1:
        canvas.centered_text("%s/%s" % (STATE["page"], STATE["total_pages"]),
                             ctx.fonts["tiny"], canvas.width // 2, canvas.height - 36)


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[1] < len(STATE["items"]):
                ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))
            return
    if y > ctx.height - 70:
        if x < ctx.width / 3 and STATE["page"] > 1:
            _load(ctx, STATE["page"] - 1)
        elif x > ctx.width * 2 / 3 and STATE["page"] < STATE["total_pages"]:
            _load(ctx, STATE["page"] + 1)
