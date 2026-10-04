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


def _layout(ctx):
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
    STATE["loading"] = True
    STATE["error"] = ""
    STATE["generation"] += 1
    generation = STATE["generation"]
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

    def success(result):
        if generation != STATE["generation"]:
            return
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
        STATE["loaded"] = True
        STATE["loading"] = False
        STATE["error"] = ""
        ctx.app.log(
            "[browse] mapped server_page=%s page=%s items=%s accepted=%s total=%s"
            % (next_server_page, STATE["page"], len(STATE["items"]),
               len(accepted), STATE["total_pages"])
        )

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        STATE["loaded"] = True
        STATE["error"] = str(exc)
        # 清掉旧列表，避免新筛选标签继续搭配上一批数据
        STATE["items"] = []
        STATE["accepted"] = []
        STATE["next_server_page"] = 1
        STATE["server_total_pages"] = None
        STATE["total_pages"] = 1
        ctx.app.log("[browse] error page=%s: %s" % (STATE["page"], exc))

    ctx.show()
    ctx.run_async(owner, operation, success, error)


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
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.centered_text(str((STATE["page"] - 1) * per_page + index + 1),
                             ctx.fonts["body"],
                             rect[0] + 38, y + rect[3] // 2)
        title = item.get("Title") or "未知"
        author = item.get("UserName") or "未知作者"
        updated = format_time(item.get("LastUpdatedAt"))
        canvas.text((rect[0] + 78, y + 8),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 106),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 78, y + 40),
                    canvas.fit_text("%s · %s" % (author, updated),
                                    ctx.fonts["tiny"], rect[2] - 106),
                    font=ctx.fonts["tiny"], fill=canvas.theme.muted)

    if STATE["error"]:
        canvas.centered_text("加载失败", ctx.fonts["body"],
                             canvas.width // 2, canvas.height // 2 - 30)
        canvas.centered_text(str(STATE["error"])[:40], ctx.fonts["tiny"],
                             canvas.width // 2, canvas.height // 2 + 30,
                             fill=canvas.theme.muted)
        rect = (margin, nav_y, canvas.width - 2 * margin, 54)
        canvas.button(rect, "重试", font=ctx.fonts["small"])
        STATE["rects"][("retry", 0)] = rect
    else:
        nav_width = int(canvas.width * 0.25)
        for key, rect, label in (
            ("prev", (margin, nav_y, nav_width, 54), "上一页"),
            ("count", ((canvas.width - nav_width) // 2, nav_y, nav_width, 54),
             "%s/%s" % (STATE["page"], STATE["total_pages"])),
            ("next", (canvas.width - margin - nav_width, nav_y, nav_width, 54), "下一页"),
        ):
            enabled = key == "count" or (
                key == "prev" and STATE["page"] > 1) or (
                key == "next" and STATE["page"] < STATE["total_pages"])
            canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
            STATE["rects"][(key, 0)] = rect
        if STATE["loading"]:
            canvas.centered_text("加载中…", ctx.fonts["body"],
                                 canvas.width // 2, canvas.height // 2)
        elif STATE["loaded"] and not STATE["items"]:
            canvas.centered_text("暂无内容", ctx.fonts["body"],
                                 canvas.width // 2, canvas.height // 2,
                                 fill=canvas.theme.muted)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    retry = STATE["rects"].get(("retry", 0))
    if retry and retry[0] <= x < retry[0] + retry[2] and retry[1] <= y < retry[1] + retry[3]:
        _load(ctx, STATE["page"])
        return
    for key in (("prev", 0), ("count", 0), ("next", 0)):
        rect = STATE["rects"].get(key)
        if not rect:
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[0] == "prev" and STATE["page"] > 1:
                _load(ctx, STATE["page"] - 1)
            elif key[0] == "next" and STATE["page"] < STATE["total_pages"]:
                _load(ctx, STATE["page"] + 1)
            else:
                _load(ctx, STATE["page"])
            return
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
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
        return


def refresh(ctx):
    _load(ctx, STATE["page"], reset=True)
