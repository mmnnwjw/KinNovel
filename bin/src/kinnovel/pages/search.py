from page import keyboard


STATE = {
    "query": "",
    "mode": "fuzzy",
    "items": [],
    "page": 1,
    "total_pages": 1,
    "rects": {},
    "loading": False,
    "generation": 0,
}

MODES = [
    ("fuzzy", "模糊"),
    ("exact", "精确"),
    ("title", "书名"),
    ("author", "作者"),
    ("name", "作品"),
    ("tags", "标签"),
]


def render(ctx, canvas):
    top = canvas.header("搜索", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    mode_y = top + 12
    mode_height = 52
    gap = 7
    width = (canvas.width - 2 * margin - gap * (len(MODES) - 1)) // len(MODES)
    STATE["rects"] = {}
    for index, (value, label) in enumerate(MODES):
        rect = (margin + index * (width + gap), mode_y, width, mode_height)
        canvas.button(rect, label, active=STATE["mode"] == value, font=ctx.fonts["tiny"])
        STATE["rects"][("mode", value)] = rect
    query_y = mode_y + mode_height + 14
    query_rect = (margin, query_y, canvas.width - 2 * margin, 66)
    canvas.draw.rounded_rectangle([query_rect[0], query_rect[1],
                                   query_rect[0] + query_rect[2],
                                   query_rect[1] + query_rect[3]],
                                  radius=10, outline=canvas.theme.foreground, width=2)
    canvas.text((query_rect[0] + 16, query_y + 16),
                STATE["query"] or "点击输入关键词", font=ctx.fonts["small"],
                fill=canvas.theme.foreground if STATE["query"] else canvas.theme.muted)
    STATE["rects"][("query", 0)] = query_rect
    list_y = query_y + 82
    row_height = max(64, int(canvas.height * 0.052))
    max_rows = max(1, (canvas.height - list_y - 110) // row_height)
    start = (STATE["page"] - 1) * max_rows
    for row in range(max_rows):
        index = start + row
        y = list_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", row)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        title = item.get("Title") or item.get("Name") or "未知"
        subtitle = item.get("UserName") or item.get("Author") or ""
        canvas.text((rect[0] + 12, y + 8),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
        if subtitle:
            canvas.text((rect[0] + 12, y + 38),
                        canvas.fit_text(subtitle, ctx.fonts["tiny"], rect[2] - 24),
                        font=ctx.fonts["tiny"], fill=canvas.theme.muted)
    nav_y = canvas.height - 76
    nav_width = int(canvas.width * 0.24)
    nav = [
        ("prev", (margin, nav_y, nav_width, 54), "上一页"),
        ("count", ((canvas.width - nav_width) // 2, nav_y, nav_width, 54),
         "%s/%s" % (STATE["page"], STATE["total_pages"])),
        ("next", (canvas.width - margin - nav_width, nav_y, nav_width, 54), "下一页"),
    ]
    for key, rect, label in nav:
        canvas.button(rect, label, font=ctx.fonts["tiny"])
        STATE["rects"][(key, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("搜索中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def _do_search(ctx, page=1):
    if not ctx.api.user:
        ctx.message(["搜索需要登录", "正在打开登录页…"])
        ctx.navigate("account")
        return
    query = STATE["query"].strip()
    if not query:
        ctx.toast("请输入搜索内容")
        return
    STATE["page"] = max(1, int(page))
    STATE["loading"] = True
    STATE["generation"] += 1
    generation = STATE["generation"]

    def operation():
        return ctx.api.search_books(
            STATE["mode"], query, page=STATE["page"], size=16,
            ignore_japanese=ctx.config.get("ignore_japanese", False),
            ignore_ai=ctx.config.get("ignore_ai", False),
        )

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result.get("Data") or []
        STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))
        STATE["loading"] = False

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["搜索失败", str(exc)])

    ctx.run_async("search", operation, success, error)


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        kind = key[0]
        if kind == "mode":
            STATE["mode"] = key[1]
            ctx.show()
        elif kind == "query":
            if not ctx.api.user:
                ctx.toast("搜索需要登录")
                ctx.navigate("account")
                return
            def submitted(value):
                STATE["query"] = value
                _do_search(ctx, 1)
                return None
            keyboard.start(ctx.screen, ctx.fonts, capabilities=("cn", "en", "numsym"),
                           hint="请输入书名、作者或标签", enter_label="搜索",
                           owner="search", on_submit=submitted)
            ctx.navigate("keyboard")
        elif kind == "item":
            row = key[1]
            per_page = max(1, len([k for k in STATE["rects"] if k[0] == "item"]))
            index = (STATE["page"] - 1) * per_page + row
            if index < len(STATE["items"]):
                item = STATE["items"][index]
                book_id = item.get("Id")
                if book_id:
                    ctx.navigate("book", book_id=book_id)
                elif item.get("Name"):
                    ctx.navigate("series", name=item.get("Name"))
        elif kind == "prev" and STATE["page"] > 1:
            _do_search(ctx, STATE["page"] - 1)
        elif kind == "next" and STATE["page"] < STATE["total_pages"]:
            _do_search(ctx, STATE["page"] + 1)
        elif kind == "count":
            _do_search(ctx, STATE["page"])
        return
