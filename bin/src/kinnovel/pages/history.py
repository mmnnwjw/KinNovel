STATE = {
    "items": [],
    "page": 0,
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
        STATE["page"] = 0
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
    rows = max(1, (canvas.height - start_y - 84) // row_height)
    items = STATE["items"]
    pages = max(1, (len(items) + rows - 1) // rows)
    STATE["page"] = min(STATE["page"], pages - 1)
    start = STATE["page"] * rows
    STATE["rects"] = {}
    for row in range(rows):
        index = start + row
        y = start_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(items):
            continue
        item = items[index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        title = item.get("Title") or "未知"
        canvas.text((rect[0] + 12, y + 10),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 12, y + 42),
                    item.get("UserName") or "", font=ctx.fonts["tiny"],
                    fill=canvas.theme.muted)
    bottom_y = canvas.height - 68
    gap = 8
    width = (canvas.width - 2 * margin - gap * 3) // 4
    buttons = [
        ("prev", "上一页", STATE["page"] > 0),
        ("count", "%s/%s" % (STATE["page"] + 1, pages), True),
        ("next", "下一页", STATE["page"] < pages - 1),
        ("clear", "清空", bool(items)),
    ]
    for index, (action, label, enabled) in enumerate(buttons):
        rect = (margin + index * (width + gap), bottom_y, width, 52)
        canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
    elif STATE["loaded"] and not items:
        canvas.centered_text("暂无阅读历史", ctx.fonts["body"], canvas.width // 2,
                             canvas.height // 2, fill=canvas.theme.muted)


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key in (("prev", 0), ("next", 0), ("clear", 0)):
        rect = STATE["rects"].get(key)
        if not rect:
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[0] == "clear":
                if STATE["items"]:
                    ctx.confirm("确认清空阅读历史？", lambda: _clear(ctx))
                return
            row_height = max(76, int(ctx.height * 0.063))
            top = max(72, int(ctx.height * 0.085))
            rows = max(1, (ctx.height - top - 12 - 84) // row_height)
            pages = max(1, (len(STATE["items"]) + rows - 1) // rows)
            if key[0] == "prev" and STATE["page"] > 0:
                STATE["page"] -= 1
                ctx.show()
            elif key[0] == "next" and STATE["page"] < pages - 1:
                STATE["page"] += 1
                ctx.show()
            return
    for key, rect in STATE["rects"].items():
        if key[0] != "item":
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[1] < len(STATE["items"]):
                ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))
            return


def _clear(ctx):
    def success(_):
        STATE["items"] = []
        STATE["page"] = 0
        ctx.toast("已清空")
    ctx.run_async("history", ctx.api.clear_read_history, success,
                  lambda exc: ctx.message(["清空失败", str(exc)]))
