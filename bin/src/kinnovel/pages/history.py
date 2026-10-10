from .. import widgets

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

_loader = widgets.ListLoader(STATE)


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    row_height = widgets.list_row_height(ctx)
    start_y = top + 12
    bottom = widgets.pager_height(ctx) + 24
    rows = max(1, (ctx.height - start_y - bottom) // row_height)
    return top, row_height, rows


def enter(ctx):
    # 从主页/其他页面新进入时回到第一页；从书籍详情返回时保留页码。
    _load(ctx, reset_page=not ctx.returning)


def _load(ctx, reset_page=True):
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

    def done(result):
        ids, books, page = result
        STATE["history_ids"] = ids
        STATE["items"] = books
        STATE["page"] = page

    _loader.start(ctx, "history", operation, done)


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
        widgets.row_card(canvas, ctx, rect, item.get("Title") or "未知",
                         item.get("UserName") or "", wrap_title=True)
    bottom_height = widgets.pager_height(ctx)
    bottom_y = canvas.height - bottom_height - 16
    gap = 8
    buttons = [
        ("prev", "上一页", STATE["page"] > 0),
        ("next", "下一页", STATE["page"] < pages - 1),
        ("retry", "重试", bool(STATE["error"])),
        ("clear", "清空", bool(items) or bool(ids)),
    ]
    width = (canvas.width - 2 * margin - gap * len(buttons)) // (len(buttons) + 1)
    count_rect = (margin, bottom_y, width, bottom_height)
    canvas.centered_text("%s/%s" % (STATE["page"] + 1, pages), ctx.fonts["tiny"],
                         count_rect[0] + width // 2, bottom_y + bottom_height // 2,
                         fill=canvas.theme.muted)
    for index, (action, label, enabled) in enumerate(buttons):
        x = margin + (index + 1) * (width + gap)
        rect = (x, bottom_y, width, bottom_height)
        canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    # 这里不注册页码区域为可点击 rect, 保持和其它列表页一致:
    # 它只是文字指示器, 不是按钮。
    if STATE["loading"]:
        widgets.loading_state(canvas, ctx)
    elif STATE["error"]:
        canvas.centered_text("加载失败", ctx.fonts["body"], canvas.width // 2,
                             canvas.height // 2 - 30)
        canvas.centered_text(str(STATE["error"])[:40], ctx.fonts["tiny"],
                             canvas.width // 2, canvas.height // 2 + 30,
                             fill=canvas.theme.muted)
    elif STATE["loaded"] and not items:
        widgets.empty_state(canvas, ctx, "暂无阅读历史")


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    key = widgets.hit_test(STATE["rects"], x, y)
    if key is None:
        return
    if key[0] == "retry":
        _load(ctx)
        return
    if key[0] == "clear":
        if STATE.get("items") or STATE.get("history_ids"):
            ctx.confirm("确认清空阅读历史？", lambda: _clear(ctx))
        return
    if key[0] in ("prev", "next"):
        rows = _layout(ctx)[2]
        pages = max(1, (len(STATE.get("history_ids") or []) + rows - 1) // rows)
        if key[0] == "prev" and STATE["page"] > 0:
            STATE["page"] -= 1
            _load(ctx, reset_page=False)
        elif key[0] == "next" and STATE["page"] < pages - 1:
            STATE["page"] += 1
            _load(ctx, reset_page=False)
        return
    if key[0] == "item":
        items = STATE.get("items") or []
        if key[1] < len(items):
            ctx.navigate("book", book_id=items[key[1]].get("Id"))


def _clear(ctx):
    def success(_):
        STATE["items"] = []
        STATE["history_ids"] = []
        STATE["page"] = 0
        ctx.toast("已清空")
    ctx.run_async("history", ctx.api.clear_read_history, success,
                  lambda exc: ctx.message(["清空失败", str(exc)]))
