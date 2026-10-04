from ..api import dict_items


STATE = {
    "items": [],
    "visible": [],
    "books": {},
    "path": [],
    "page": 0,
    "rects": {},
    "loading": False,
    "loaded": False,
    "error": "",
    "generation": 0,
}


def is_folder(item):
    """文件夹 id 由 nanoid 生成, 不能进入 int()。"""
    if not isinstance(item, dict):
        return False
    return str(item.get("type") or "").strip().upper() == "FOLDER"


def is_comic_item(item):
    if not isinstance(item, dict):
        return False
    return str(item.get("type") or "").strip().upper() == "COMIC"


def shelf_book_id(item):
    """返回可比较的书籍整数 id; 文件夹、损坏条目和未知类型返回 None。"""
    if not isinstance(item, dict) or is_folder(item):
        return None
    try:
        return int(item.get("id"))
    except (TypeError, ValueError):
        return None


def _index_key(item):
    if not isinstance(item, dict):
        return 0
    try:
        return int(item.get("index") or 0)
    except (TypeError, ValueError):
        return 0


def _last_parent(item):
    if not isinstance(item, dict):
        return None
    parents = item.get("parents") or []
    return parents[-1] if parents else None


def _layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    row_height = max(74, int(ctx.height * 0.060))
    per_page = max(1, (ctx.height - top - 110) // row_height)
    return top, row_height, per_page


def enter(ctx):
    if not ctx.api.user:
        ctx.replace("account")
        return
    # 从主页/其他页面新进入时回到第一页；从书籍详情返回时保留页码。
    _load(ctx, reset_page=not ctx.returning)


def _load(ctx, reset_page=True):
    STATE["loading"] = True
    STATE["error"] = ""
    STATE["generation"] += 1
    generation = STATE["generation"]
    reset_page = bool(reset_page)

    def operation():
        shelf = ctx.api.get_book_shelf()
        if isinstance(shelf, dict):
            key = "data" if "data" in shelf else "Data"
            full_items = dict_items(shelf.get(key))
        else:
            full_items = dict_items(shelf)
        folder_ids = {item.get("id") for item in full_items if is_folder(item)}
        path = [value for value in STATE["path"] if value in folder_ids]
        parent = path[-1] if path else None
        # 渲染层才过滤漫画; 文件夹保持原样, 完整条目留给写回。
        visible = [
            item for item in full_items
            if not is_comic_item(item) and _last_parent(item) == parent
        ]
        visible.sort(key=_index_key)
        per_page = _layout(ctx)[2]
        pages = max(1, (len(visible) + per_page - 1) // per_page)
        requested = 0 if reset_page else int(STATE.get("page") or 0)
        page = max(0, min(requested, pages - 1))
        start = page * per_page
        ids = [
            book_id for book_id in (
                shelf_book_id(item) for item in visible[start:start + per_page]
            ) if book_id is not None and book_id not in STATE["books"]
        ]
        # 元数据按当前页取, 由 API 层按 24 条一块请求。
        books = ctx.api.get_book_list_by_ids_chunked(ids, "Novel") if ids else []
        return full_items, visible, path, page, books

    def success(result):
        if generation != STATE["generation"]:
            return
        full_items, visible, path, page, books = result
        STATE["items"] = full_items
        STATE["visible"] = visible
        STATE["path"] = path
        STATE["page"] = page
        for book in books or []:
            if not isinstance(book, dict):
                continue
            try:
                STATE["books"][int(book.get("Id"))] = book
            except (TypeError, ValueError):
                continue
        STATE["loading"] = False
        STATE["loaded"] = True
        STATE["error"] = ""

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        STATE["error"] = str(exc)
        ctx.message(["书架同步失败", str(exc)])

    ctx.run_async("shelf", operation, success, error)


def _load_page(ctx):
    """翻页时只补当前页缺失的书籍元数据, 已缓存的页面直接复用。"""
    _top, _row_height, per_page = _layout(ctx)
    items = STATE.get("visible") or []
    page = max(0, int(STATE.get("page") or 0))
    start = page * per_page
    ids = [
        book_id for book_id in (
            shelf_book_id(item) for item in items[start:start + per_page]
        ) if book_id is not None and book_id not in STATE["books"]
    ]
    if not ids:
        ctx.show()
        return
    STATE["loading"] = True
    STATE["generation"] += 1
    generation = STATE["generation"]

    def success(result):
        if generation != STATE["generation"]:
            return
        for book in result or []:
            if not isinstance(book, dict):
                continue
            try:
                STATE["books"][int(book.get("Id"))] = book
            except (TypeError, ValueError):
                continue
        STATE["loading"] = False
        ctx.show()

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["加载失败", str(exc)])

    ctx.run_async(
        "shelf",
        lambda: ctx.api.get_book_list_by_ids_chunked(ids, "Novel"),
        success, error, refresh=False,
    )


def render(ctx, canvas):
    folder_name = "根目录"
    if STATE["path"]:
        for item in STATE["items"]:
            if is_folder(item) and item.get("id") == STATE["path"][-1]:
                folder_name = item.get("title") or "文件夹"
                break
    top = canvas.header("书架 · " + folder_name, left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    _top, row_height, per_page = _layout(ctx)
    items = STATE.get("visible") or []
    pages = max(1, (len(items) + per_page - 1) // per_page)
    STATE["page"] = min(STATE["page"], pages - 1)
    start = STATE["page"] * per_page
    STATE["rects"] = {}
    for row in range(per_page):
        index = start + row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(items):
            continue
        item = items[index]
        if not isinstance(item, dict):
            continue
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        if is_folder(item):
            title = "文件夹  " + str(item.get("title") or "未命名")
            subtitle = "长按删除"
        else:
            book = STATE["books"].get(shelf_book_id(item)) or {}
            title = book.get("Title") or ("书籍 #%s" % item.get("id"))
            subtitle = book.get("UserName") or ""
        canvas.text((rect[0] + 14, y + 8),
                    canvas.fit_text(title, ctx.fonts["small"], rect[2] - 28),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 14, y + 40),
                    canvas.fit_text(subtitle, ctx.fonts["tiny"], rect[2] - 28),
                    font=ctx.fonts["tiny"], fill=canvas.theme.muted)
    bottom_y = canvas.height - 72
    gap = 8
    width = (canvas.width - 2 * margin - gap * 3) // 4
    buttons = [
        ("up", "上一页", STATE["page"] > 0),
        ("sync", "同步", True),
        ("prev_folder", "上一层", bool(STATE["path"])),
        ("down", "下一页 (%s/%s)" % (STATE["page"] + 1, pages), STATE["page"] < pages - 1),
    ]
    for index, (action, label, enabled) in enumerate(buttons):
        rect = (margin + index * (width + gap), bottom_y, width, 54)
        canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("同步中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
    elif STATE["loaded"] and not items:
        canvas.centered_text("书架为空", ctx.fonts["body"], canvas.width // 2,
                             canvas.height // 2, fill=canvas.theme.muted)


def handle(data, ctx):
    x = int(data.get("x-pixel") or 0)
    y = int(data.get("y-pixel") or 0)
    if data.get("gesture") == "long":
        for key, rect in STATE["rects"].items():
            if key[0] == "item":
                rx, ry, width, height = rect
                if rx <= x < rx + width and ry <= y < ry + height:
                    items = STATE.get("visible") or []
                    if key[1] < len(items):
                        _long_press(ctx, items[key[1]])
                    return
    if data.get("gesture") != "tap":
        return
    for key in (("up", 0), ("sync", 0), ("prev_folder", 0), ("down", 0)):
        rect = STATE["rects"].get(key)
        if not rect:
            continue
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        if key[0] == "sync":
            _load(ctx)
        elif key[0] == "prev_folder" and STATE["path"]:
            STATE["path"].pop()
            _load(ctx)
        elif key[0] == "up" and STATE["page"] > 0:
            STATE["page"] -= 1
            _load_page(ctx)
        elif key[0] == "down":
            items = STATE.get("visible") or []
            per_page = _layout(ctx)[2]
            pages = max(1, (len(items) + per_page - 1) // per_page)
            if STATE["page"] < pages - 1:
                STATE["page"] += 1
                _load_page(ctx)
        return
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        action = key[0]
        if action == "item":
            items = STATE.get("visible") or []
            if key[1] < len(items):
                item = items[key[1]]
                if is_folder(item):
                    STATE["path"].append(item.get("id"))
                    _load(ctx)
                else:
                    ctx.navigate("book", book_id=item.get("id"))
        return


def _long_press(ctx, item):
    if is_folder(item):
        ctx.confirm("删除文件夹？其中的书籍会移到上一层",
                    lambda: _delete_folder(ctx, item.get("id")))
        return
    book_id = shelf_book_id(item)
    if book_id is None:
        ctx.message(["无法识别该书籍", "条目已损坏"])
        return
    ctx.confirm("从书架移出书籍？", lambda: _remove_book(ctx, book_id))


def _delete_folder(ctx, folder_id):
    items = []
    for item in STATE["items"]:
        if not isinstance(item, dict):
            items.append(item)
            continue
        if item.get("id") == folder_id:
            continue
        parents = [value for value in (item.get("parents") or []) if value != folder_id]
        item = dict(item)
        item["parents"] = parents
        items.append(item)

    def success(_):
        STATE["path"] = [value for value in STATE["path"] if value != folder_id]
        STATE["items"] = items
        _load(ctx)

    ctx.run_async("shelf", lambda: ctx.api.save_book_shelf(items), success,
                  lambda exc: ctx.message(["删除失败", str(exc)]))


def _remove_book(ctx, book_id):
    # 文件夹与损坏条目的 shelf_book_id 为 None, 不会等于 book_id, 原样保留。
    items = [item for item in STATE["items"] if shelf_book_id(item) != book_id]

    def success(_):
        STATE["items"] = items
        _load(ctx)

    ctx.run_async("shelf", lambda: ctx.api.save_book_shelf(items), success,
                  lambda exc: ctx.message(["移出失败", str(exc)]))
