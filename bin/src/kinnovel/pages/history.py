STATE = {
    "items": [],
    "history_ids": [],
    "page": 0,
    "rects": {},
    "loading": False,
    "loaded": False,
    "error": "",
    "generation": 0,
}


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    row_height = max(76, int(ctx.height * 0.063))
    start_y = top + 12
    rows = max(1, (ctx.height - start_y - 84) // row_height)
    return top, row_height, rows


def enter(ctx):
    # 从主页/其他页面新进入时回到第一页；从书籍详情返回时保留页码。
    _load(ctx, reset_page=not ctx.returning)


def _load(ctx, reset_page=True):
    STATE["loading"] = True
    STATE["error"] = ""
    STATE["generation"] += 1
    generation = STATE["generation"]
    reset_page = bool(reset_page)
    rows = _layout(ctx)[2]
    target_page = 0 if reset_page else max(0, int(STATE.get("page") or 0))

    def operation():
        history = ctx.api.get_read_history() or {}
        ids = list(history.get("Novel") or []) if isinstance(history, dict) else []
        if not ids:
            return ids, [], 0
        pages = max(1, (len(ids) + rows - 1) // rows)
        page = max(0, min(target_page, pages - 1))
        start = page * rows
        # 完整历史 ID 不截断; 元数据只取当前页, 由 API 层按 24 条一块请求。
        books = ctx.api.get_book_list_by_ids_chunked(ids[start:start + rows], "Novel")
        return ids, (books or []), page

    def success(result):
        if generation != STATE["generation"]:
            return
        ids, books, page = result
        STATE["history_ids"] = ids
        STATE["items"] = books
        STATE["page"] = page
        STATE["loading"] = False
        STATE["loaded"] = True
        STATE["error"] = ""

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        STATE["error"] = str(exc)

    ctx.run_async("history", operation, success, error)


def render(ctx, canvas):
    top = canvas.header("阅读历史", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    _top, row_height, rows = _layout(ctx)
    start_y = top + 12
    ids = STATE.get("history_ids") or []
    items = STATE.get("items") or []
    pages = max(1, (len(ids) + rows - 1) // rows)
    STATE["page"] = min(STATE["page"], pages - 1)
    STATE["rects"] = {}
    for row in range(rows):
        y = start_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", row)] = rect
        if row >= len(items):
            continue
        item = items[row]
        if not isinstance(item, dict):
            continue
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
    buttons = [
        ("prev", "上一页", STATE["page"] > 0),
        ("count", "%s/%s" % (STATE["page"] + 1, pages), True),
        ("next", "下一页", STATE["page"] < pages - 1),
        ("retry", "重试", bool(STATE["error"])),
        ("clear", "清空", bool(items) or bool(ids)),
    ]
    width = (canvas.width - 2 * margin - gap * (len(buttons) - 1)) // len(buttons)
    for index, (action, label, enabled) in enumerate(buttons):
        rect = (margin + index * (width + gap), bottom_y, width, 52)
        canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
    elif STATE["error"]:
        canvas.centered_text("加载失败", ctx.fonts["body"], canvas.width // 2,
                             canvas.height // 2 - 30)
        canvas.centered_text(str(STATE["error"])[:40], ctx.fonts["tiny"],
                             canvas.width // 2, canvas.height // 2 + 30,
                             fill=canvas.theme.muted)
    elif STATE["loaded"] and not items:
        canvas.centered_text("暂无阅读历史", ctx.fonts["body"], canvas.width // 2,
                             canvas.height // 2, fill=canvas.theme.muted)


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key in (("prev", 0), ("count", 0), ("next", 0), ("retry", 0), ("clear", 0)):
        rect = STATE["rects"].get(key)
        if not rect:
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[0] == "retry":
                _load(ctx)
                return
            if key[0] == "clear":
                if STATE.get("items") or STATE.get("history_ids"):
                    ctx.confirm("确认清空阅读历史？", lambda: _clear(ctx))
                return
            if key[0] == "count":
                return
            rows = _layout(ctx)[2]
            pages = max(1, (len(STATE.get("history_ids") or []) + rows - 1) // rows)
            if key[0] == "prev" and STATE["page"] > 0:
                STATE["page"] -= 1
                _load(ctx, reset_page=False)
            elif key[0] == "next" and STATE["page"] < pages - 1:
                STATE["page"] += 1
                _load(ctx, reset_page=False)
            return
    for key, rect in STATE["rects"].items():
        if key[0] != "item":
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            items = STATE.get("items") or []
            if key[1] < len(items):
                ctx.navigate("book", book_id=items[key[1]].get("Id"))
            return


def _clear(ctx):
    def success(_):
        STATE["items"] = []
        STATE["history_ids"] = []
        STATE["page"] = 0
        ctx.toast("已清空")
    ctx.run_async("history", ctx.api.clear_read_history, success,
                  lambda exc: ctx.message(["清空失败", str(exc)]))
