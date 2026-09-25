from ..utils import format_time


STATE = {
    "items": [],
    "page": 1,
    "total_pages": 1,
    "order": "latest",
    "categories": [],
    "category": 0,
    "loading": False,
    "loaded": False,
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


def _load(ctx, page=None):
    if STATE["loading"]:
        return
    STATE["loading"] = True
    STATE["generation"] += 1
    generation = STATE["generation"]
    owner = ctx.page_name
    if page is not None:
        STATE["page"] = max(1, int(page))
    _top, _fy, _fh, _ly, _rh, per_page, _nav = _layout(ctx)

    def operation():
        category = None
        if STATE["categories"] and STATE["category"] > 0:
            category = STATE["categories"][STATE["category"] - 1].get("Id")
        return ctx.api.get_book_list(
            page=STATE["page"], size=per_page, order=STATE["order"],
            category_id=category,
            ignore_japanese=ctx.config.get("ignore_japanese", False),
            ignore_ai=ctx.config.get("ignore_ai", False),
        )

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result.get("Data") or []
        STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))
        STATE["loaded"] = True
        STATE["loading"] = False

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["加载失败", str(exc)])

    ctx.run_async(owner, operation, success, error)


def _load_categories(ctx):
    def operation():
        return ctx.api.get_book_categories("Novel")

    def success(result):
        STATE["categories"] = result or []

    ctx.run_async("browse", operation, success, lambda _: None)


def enter(ctx):
    if not STATE["loaded"]:
        _load(ctx, 1)
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

    start = (STATE["page"] - 1) * per_page
    for row in range(per_page):
        index = start + row
        y = list_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.centered_text(str(index + 1), ctx.fonts["body"],
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


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
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
                _load(ctx, 1)
            elif index == 1:
                STATE["category"] = (STATE["category"] + 1) % (len(STATE["categories"]) + 1)
                _load(ctx, 1)
            else:
                _load(ctx, STATE["page"])
        elif kind == "item" and key[1] < len(STATE["items"]):
            ctx.navigate("book", book_id=STATE["items"][key[1]].get("Id"))
        return


def refresh(ctx):
    _load(ctx, STATE["page"])
