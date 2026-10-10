from .. import widgets

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

_loader = widgets.ListLoader(STATE)


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    row_height = widgets.list_row_height(ctx)
    bottom = widgets.pager_height(ctx) + 24
    rows = max(1, (ctx.height - (top + 12) - bottom) // row_height)
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

    def done(result):
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

    def failed(_exc):
        STATE["items"] = []
        STATE["total_pages"] = 1
        STATE["rects"] = {}
        ctx.show()

    _loader.start(ctx, "series", operation, done, failed)


def render(ctx, canvas):
    top = canvas.header(STATE["title"] or "系列", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    _top, row_height, rows = _layout(ctx)
    start_y = top + 12
    STATE["rects"] = {}
    pager_rect = (margin, canvas.height - widgets.pager_height(ctx) - 16,
                 canvas.width - 2 * margin, widgets.pager_height(ctx))
    if STATE["error"]:
        widgets.error_state(canvas, ctx, STATE["error"], STATE["rects"], pager_rect)
        return
    pages = _page_count(ctx)
    STATE["page"] = min(STATE["page"], pages - 1)
    items = _page_items(ctx)
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
        subtitle = "当前书籍" if current else ""
        widgets.row_card(canvas, ctx, rect, item.get("Title") or "未知",
                         subtitle, highlight=current, wrap_title=True)
    widgets.pager_bar(canvas, ctx, pager_rect, STATE["page"] > 0,
                      STATE["page"] < pages - 1,
                      "%s/%s" % (STATE["page"] + 1, pages), STATE["rects"])
    if STATE["loading"]:
        widgets.loading_state(canvas, ctx)
    elif STATE["loaded"] and not items:
        widgets.empty_state(canvas, ctx, "本系列暂无其他书籍")


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x = int(data.get("x-pixel") or 0)
    y = int(data.get("y-pixel") or 0)
    key = widgets.hit_test(STATE["rects"], x, y)
    if key is None:
        return
    if key == ("retry", 0):
        _load(ctx, page=STATE["page"])
        return
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
