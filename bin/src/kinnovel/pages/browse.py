from .. import widgets
from ..utils import format_time


STATE = {
    "items": [],
    "accepted": [],
    "next_server_page": 1,
    "server_total_pages": None,
    "page": 1,
    "total_pages": 1,
    "order": "latest",
    "categories": [],
    "category": 0,
    "loading": False,
    "loaded": False,
    "error": "",
    "rects": {},
    "generation": 0,
}

ORDER_LABELS = {
    "latest": "最近更新",
    "new": "上架时间",
    "view": "总点击",
}

_loader = widgets.ListLoader(STATE)


def _layout(ctx):
    # 注意: 这里的行高/翻页条尺寸特意保持原值不变(而不是用更大的
    # widgets 默认值), 因为服务端分页大小与 per_page 强耦合(见
    # test_browse_fills_sparse_filtered_pages 等), 改动行高会改变
    # per_page 从而改变服务端请求次数与断言, 所以单独保留。
    top = max(72, int(ctx.height * 0.085))
    filter_y = top + 16
    filter_height = 60
    list_y = filter_y + filter_height + 18
    row_height = max(64, int(ctx.height * 0.058))
    nav_y = ctx.height - 72
    per_page = max(1, (nav_y - list_y - 12) // row_height)
    return top, filter_y, filter_height, list_y, row_height, per_page, nav_y


def _category(ctx):
    labels = ["全部类型"]
    for item in STATE["categories"]:
        labels.append(item.get("Name") or "未命名")
    return labels


def _response_parts(result, requested_page):
    if isinstance(result, dict):
        data = result.get("Data") or result.get("data") or []
        try:
            page = int(result.get("Page") or requested_page)
        except (TypeError, ValueError):
            page = requested_page
        try:
            total = int(result.get("TotalPages") or page)
        except (TypeError, ValueError):
            total = page
        return list(data or []), max(1, page), max(1, total)
    return list(result or []), requested_page, requested_page


def _load(ctx, page=None, reset=False):
    owner = ctx.page_name
    if page is not None:
        STATE["page"] = max(1, int(page))
    if reset:
        STATE["accepted"] = []
        STATE["next_server_page"] = 1
        STATE["server_total_pages"] = None
    _top, _fy, _fh, _ly, _rh, per_page, _nav = _layout(ctx)
    target_page = max(1, int(STATE["page"]))

    def operation():
        category = None
        if STATE["categories"] and STATE["category"] > 0:
            category = STATE["categories"][STATE["category"] - 1].get("Id")
        accepted = list(STATE.get("accepted") or [])
        server_page = max(1, int(STATE.get("next_server_page") or 1))
        server_total = STATE.get("server_total_pages")
        requests = 0
        while len(accepted) < target_page * per_page and requests < 16:
            if server_total is not None and server_page > server_total:
                break
            ctx.app.log(
                "[browse] request server_page=%s size=%s order=%s category=%s"
                % (server_page, per_page, STATE["order"], category)
            )
            result = ctx.api.get_book_list(
                page=server_page, size=per_page, order=STATE["order"],
                category_id=category,
                ignore_japanese=ctx.config.get("ignore_japanese", False),
                ignore_ai=ctx.config.get("ignore_ai", False),
            )
            batch, response_page, server_total = _response_parts(result, server_page)
            accepted.extend(item for item in batch if isinstance(item, dict))
            server_page = max(server_page, response_page) + 1
            requests += 1
            if not batch and server_page > server_total:
                break
        return accepted, server_page, server_total

    def done(result):
        accepted, next_server_page, server_total = result
        STATE["accepted"] = accepted
        STATE["next_server_page"] = next_server_page
        STATE["server_total_pages"] = server_total
        loaded_pages = max(1, (len(accepted) + per_page - 1) // per_page)
        exhausted = server_total is not None and next_server_page > server_total
        total_pages = loaded_pages if exhausted else max(loaded_pages + 1, target_page)
        page = min(target_page, total_pages)
        start = (page - 1) * per_page
        STATE["items"] = accepted[start:start + per_page]
        STATE["page"] = page
        STATE["total_pages"] = total_pages
        ctx.app.log(
            "[browse] mapped server_page=%s page=%s items=%s accepted=%s total=%s"
            % (next_server_page, STATE["page"], len(STATE["items"]),
               len(accepted), STATE["total_pages"])
        )

    def failed(exc):
        # 清掉旧列表，避免新筛选标签继续搭配上一批数据
        STATE["items"] = []
        STATE["accepted"] = []
        STATE["next_server_page"] = 1
        STATE["server_total_pages"] = None
        STATE["total_pages"] = 1
        ctx.app.log("[browse] error page=%s: %s" % (STATE["page"], exc))

    ctx.show()
    _loader.start(ctx, owner, operation, done, failed)


def _load_categories(ctx):
    def operation():
        return ctx.api.get_book_categories("Novel")

    def success(result):
        STATE["categories"] = result or []

    ctx.run_async("browse", operation, success, lambda _: None)


def enter(ctx):
    # 从主页/其他页面新进入时回到第一页；从书籍详情返回时保留页码。
    _load(ctx, STATE["page"] if ctx.returning else 1, reset=not ctx.returning)
    if not STATE["categories"]:
        _load_categories(ctx)


def render(ctx, canvas):
    top, filter_y, filter_height, list_y, row_height, per_page, nav_y = _layout(ctx)
    margin = int(canvas.width * 0.035)
    canvas.header("最近/分类", left="返回", right="主页")
    gap = 10
    filters = [
        ORDER_LABELS.get(STATE["order"], STATE["order"]),
        _category(ctx)[STATE["category"]] if _category(ctx) else "全部类型",
        "刷新",
    ]
    button_width = (canvas.width - 2 * margin - gap * 2) // 3
    STATE["rects"] = {}
    for index, label in enumerate(filters):
        rect = (margin + index * (button_width + gap), filter_y, button_width, filter_height)
        canvas.button(rect, label, active=index != 2, font=ctx.fonts["small"])
        STATE["rects"][("filter", index)] = rect

    # 服务端已按页返回数据,行索引即当前页内索引
    for row in range(per_page):
        index = row
        y = list_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        if not isinstance(item, dict):
            continue
        title = item.get("Title") or "未知"
        author = item.get("UserName") or "未知作者"
        updated = format_time(item.get("LastUpdatedAt"))
        widgets.row_card(canvas, ctx, rect, title, "%s · %s" % (author, updated),
                         index_label=(STATE["page"] - 1) * per_page + index + 1)

    if STATE["error"]:
        rect = (margin, nav_y, canvas.width - 2 * margin, 54)
        widgets.error_state(canvas, ctx, STATE["error"], STATE["rects"], rect)
    else:
        pager_rect = (margin, nav_y, canvas.width - 2 * margin, 54)
        widgets.pager_bar(canvas, ctx, pager_rect, STATE["page"] > 1,
                          STATE["page"] < STATE["total_pages"],
                          "%s/%s" % (STATE["page"], STATE["total_pages"]),
                          STATE["rects"])
        if STATE["loading"]:
            widgets.loading_state(canvas, ctx)
        elif STATE["loaded"] and not STATE["items"]:
            widgets.empty_state(canvas, ctx, "暂无内容")


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    key = widgets.hit_test(STATE["rects"], x, y)
    if key is None:
        return
    if key == ("retry", 0):
        _load(ctx, STATE["page"])
        return
    if key[0] in ("prev", "next"):
        if key[0] == "prev" and STATE["page"] > 1:
            _load(ctx, STATE["page"] - 1)
        elif key[0] == "next" and STATE["page"] < STATE["total_pages"]:
            _load(ctx, STATE["page"] + 1)
        else:
            _load(ctx, STATE["page"])
        return
    kind = key[0]
    if kind == "filter":
        index = key[1]
        if index == 0:
            values = ["latest", "new", "view"]
            STATE["order"] = values[(values.index(STATE["order"]) + 1) % len(values)]
            _load(ctx, 1, reset=True)
        elif index == 1:
            STATE["category"] = (STATE["category"] + 1) % (len(STATE["categories"]) + 1)
            _load(ctx, 1, reset=True)
        else:
            _load(ctx, STATE["page"], reset=True)
    elif kind == "item" and key[1] < len(STATE["items"]):
        ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))


def refresh(ctx):
    _load(ctx, STATE["page"], reset=True)
