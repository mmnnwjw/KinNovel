from .. import widgets

STATE = {
    "kind": "daily",
    "items": [],
    "page": 1,
    "total_pages": 1,
    "rects": {},
    "loading": False,
    "loaded": False,
    "error": "",
    "generation": 0,
}

LETTERS = [("daily", "日榜"), ("weekly", "周榜"), ("monthly", "月榜")]

_loader = widgets.ListLoader(STATE)


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    list_y = top + 84
    row_height = widgets.list_row_height(ctx)
    pager = widgets.pager_height(ctx)
    nav_y = ctx.height - pager - 18
    per_page = max(1, (nav_y - list_y - 12) // row_height)
    return top, list_y, row_height, per_page, nav_y


def enter(ctx):
    # 从主页/其他页面新进入时回到第一页；从书籍详情返回时保留页码。
    _load(ctx, reset_page=not ctx.returning)


def _load(ctx, reset_page=True):
    if reset_page:
        STATE["page"] = 1
    days = {"daily": 1, "weekly": 7, "monthly": 31}[STATE["kind"]]

    def done(result):
        STATE["items"] = result or []
        _total_pages(ctx)

    def failed(_exc):
        # 清掉旧榜单，避免新标签继续搭配上一批数据
        STATE["items"] = []
        STATE["total_pages"] = 1

    _loader.start(ctx, "rank", lambda: ctx.api.get_rank(days), done, failed)


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
        if not isinstance(item, dict):
            continue
        widgets.row_card(canvas, ctx, rect, item.get("Title") or "未知",
                         item.get("UserName") or "", wrap_title=True,
                         index_label=index + 1)

    if STATE["error"]:
        rect = (margin, nav_y, canvas.width - 2 * margin, widgets.pager_height(ctx))
        widgets.error_state(canvas, ctx, STATE["error"], STATE["rects"], rect)
    else:
        pager_rect = (margin, nav_y, canvas.width - 2 * margin, widgets.pager_height(ctx))
        widgets.pager_bar(canvas, ctx, pager_rect, STATE["page"] > 1,
                          STATE["page"] < pages, "%s/%s" % (STATE["page"], pages),
                          STATE["rects"])
        if STATE["loading"]:
            widgets.loading_state(canvas, ctx)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    key = widgets.hit_test(STATE["rects"], x, y)
    if key is None:
        return
    if key == ("retry", 0):
        _load(ctx, reset_page=False)
        return
    if key in (("prev", 0), ("next", 0)):
        pages = _total_pages(ctx)
        if key[0] == "prev" and STATE["page"] > 1:
            STATE["page"] -= 1
        elif key[0] == "next" and STATE["page"] < pages:
            STATE["page"] += 1
        ctx.show()
        return
    if key[0] == "kind":
        STATE["kind"] = key[1]
        _load(ctx)
        return
    if key[0] == "item" and key[1] < len(STATE["items"]):
        ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))
