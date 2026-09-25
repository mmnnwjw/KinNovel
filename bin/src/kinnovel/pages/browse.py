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


def _load(ctx, page=None):
    if STATE["loading"]:
        return
    STATE["loading"] = True
    STATE["generation"] += 1
    generation = STATE["generation"]
    owner = ctx.page_name
    if page is not None:
        STATE["page"] = max(1, int(page))

    def operation():
        category = None
        if STATE["categories"] and STATE["category"] > 0:
            category = STATE["categories"][STATE["category"] - 1].get("Id")
        return ctx.api.get_book_list(
            page=STATE["page"], size=9, order=STATE["order"],
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
        covers = [item.get("Cover") for item in STATE["items"] if item.get("Cover")]
        ctx.run_async(owner, lambda: [ctx.images.prefetch(url, ctx.config.get("strict_tls")) for url in covers[:8]])

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
    width, height = canvas.width, canvas.height
    top = canvas.header("小说书库", left="返回", right="搜索")
    margin = int(width * 0.035)
    filter_y = top + 16
    filter_height = 60
    gap = 10
    filters = [
        ORDER_LABELS.get(STATE["order"], STATE["order"]),
        (STATE["categories"][STATE["category"] - 1].get("Name") if STATE["category"] else "全部类型"),
        "刷新",
    ]
    button_width = (width - 2 * margin - gap * 2) // 3
    STATE["rects"] = {}
    for index, label in enumerate(filters):
        rect = (margin + index * (button_width + gap), filter_y, button_width, filter_height)
        canvas.button(rect, label, active=index != 2, font=ctx.fonts["small"])
        STATE["rects"][("filter", index)] = rect

    grid_y = filter_y + filter_height + 18
    bottom_height = 96
    grid_height = height - grid_y - bottom_height
    columns, rows = 3, 3
    cell_gap_x, cell_gap_y = 14, 14
    cell_width = (width - 2 * margin - (columns - 1) * cell_gap_x) // columns
    cover_height = int(cell_width * 1.42)
    title_height = 58
    cell_height = cover_height + title_height
    for index in range(columns * rows):
        row, column = divmod(index, columns)
        x = margin + column * (cell_width + cell_gap_x)
        y = grid_y + row * (cell_height + cell_gap_y)
        STATE["rects"][("book", index)] = (x, y, cell_width, cell_height)
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        cover = ctx.images.cover(item.get("Cover"), cell_width - 4, cover_height - 4,
                                 strict_tls=ctx.config.get("strict_tls"))
        if cover is not None:
            canvas.image.paste(cover, (x + 2, y + 2))
        canvas.draw.rectangle([x, y, x + cell_width, y + cover_height],
                              outline=canvas.theme.mid, width=1)
        title = canvas.fit_text(item.get("Title") or "未知", ctx.fonts["small"],
                                cell_width - 6)
        canvas.centered_text(title, ctx.fonts["small"], x + cell_width // 2,
                             y + cover_height + title_height // 2)

    nav_y = height - 84
    nav_width = int(width * 0.26)
    nav_rects = [
        ("prev", (margin, nav_y, nav_width, 60), "上一页"),
        ("page", ((width - nav_width) // 2, nav_y, nav_width, 60),
         "%s/%s" % (STATE["page"], STATE["total_pages"])),
        ("next", (width - margin - nav_width, nav_y, nav_width, 60), "下一页"),
    ]
    for key, rect, label in nav_rects:
        canvas.button(rect, label, active=key not in ("prev", "next") or
                      (key == "prev" and STATE["page"] > 1) or
                      (key == "next" and STATE["page"] < STATE["total_pages"]),
                      font=ctx.fonts["small"])
        STATE["rects"][(key, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], width // 2, height // 2)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.72):
        ctx.navigate("search")
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
        elif kind == "book":
            if key[1] < len(STATE["items"]):
                item = STATE["items"][key[1]]
                ctx.navigate("book", book_id=item.get("Id"))
        elif kind == "prev" and STATE["page"] > 1:
            _load(ctx, STATE["page"] - 1)
        elif kind == "next" and STATE["page"] < STATE["total_pages"]:
            _load(ctx, STATE["page"] + 1)
        elif kind == "page":
            _load(ctx, STATE["page"])
        return


def refresh(ctx):
    _load(ctx, STATE["page"])
