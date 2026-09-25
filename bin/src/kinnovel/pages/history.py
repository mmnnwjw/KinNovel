STATE = {
    "items": [],
    "rects": {},
    "loading": False,
    "loaded": False,
}


def enter(ctx):
    _load(ctx)


def _load(ctx):
    STATE["loading"] = True

    def operation():
        history = ctx.api.get_read_history() or {}
        ids = (history.get("Novel") or [])[:24]
        if not ids:
            return []
        return ctx.api.get_book_list_by_ids(ids, "Novel") or []

    def success(result):
        STATE["items"] = result
        STATE["loading"] = False
        STATE["loaded"] = True

    def error(exc):
        STATE["loading"] = False
        ctx.message(["历史加载失败", str(exc)])

    ctx.run_async("history", operation, success, error)


def render(ctx, canvas):
    top = canvas.header("阅读历史", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    row_height = max(76, int(canvas.height * 0.063))
    start_y = top + 12
    rows = max(1, (canvas.height - start_y - 30) // row_height)
    STATE["rects"] = {}
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
        title = item.get("Title") or "未知"
        canvas.text((rect[0] + 12, y + 10),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 12, y + 42),
                    item.get("UserName") or "", font=ctx.fonts["tiny"],
                    fill=canvas.theme.muted)
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.72):
        ctx.confirm("确认清空阅读历史？", lambda: _clear(ctx))
        return
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[1] < len(STATE["items"]):
                ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))
            return


def _clear(ctx):
    def success(_):
        STATE["items"] = []
        ctx.toast("已清空")
    ctx.run_async("history", ctx.api.clear_read_history, success,
                  lambda exc: ctx.message(["清空失败", str(exc)]))
