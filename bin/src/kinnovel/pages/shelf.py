import uuid
from datetime import datetime, timezone

from page import keyboard


STATE = {
    "items": [],
    "books": {},
    "path": [],
    "page": 0,
    "rects": {},
    "loading": False,
    "loaded": False,
}


def enter(ctx):
    if not ctx.api.user:
        ctx.navigate("account")
        return
    _load(ctx)


def _load(ctx):
    STATE["loading"] = True

    def operation():
        shelf = ctx.api.get_book_shelf()
        items = list(shelf.get("data") or [])
        folder_ids = {item.get("id") for item in items if item.get("type") == "FOLDER"}
        STATE["path"] = [value for value in STATE["path"] if value in folder_ids]
        parent = STATE["path"][-1] if STATE["path"] else None
        visible = [
            item for item in items
            if _last_parent(item) == parent
        ]
        ids = [int(item.get("id")) for item in visible
               if str(item.get("type")) != "FOLDER"][:24]
        books = ctx.api.get_book_list_by_ids(ids, "Novel") if ids else []
        return items, visible, books

    def success(result):
        items, visible, books = result
        STATE["items"] = items
        STATE["books"] = {int(book.get("Id")): book for book in books}
        STATE["visible"] = sorted(visible, key=lambda item: int(item.get("index") or 0))
        STATE["page"] = 0
        STATE["loading"] = False
        STATE["loaded"] = True
        for book in books[:8]:
            url = book.get("Cover")
            if url:
                ctx.run_async("shelf", lambda url=url: ctx.images.prefetch(url, ctx.config.get("strict_tls")))

    def error(exc):
        STATE["loading"] = False
        ctx.message(["书架同步失败", str(exc)])

    ctx.run_async("shelf", operation, success, error)


def _last_parent(item):
    parents = item.get("parents") or []
    return parents[-1] if parents else None


def render(ctx, canvas):
    folder_name = "根目录"
    if STATE["path"]:
        for item in STATE["items"]:
            if item.get("id") == STATE["path"][-1]:
                folder_name = item.get("title") or "文件夹"
                break
    top = canvas.header("书架 · " + folder_name, left="返回", right="同步")
    margin = int(canvas.width * 0.035)
    row_height = max(74, int(canvas.height * 0.060))
    per_page = max(1, (canvas.height - top - 110) // row_height)
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
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        if item.get("type") == "FOLDER":
            title = "文件夹  " + str(item.get("title") or "未命名")
            subtitle = "长按删除"
        else:
            book = STATE["books"].get(int(item.get("id") or 0)) or {}
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
        ("folder", "新建文件夹", True),
        ("prev_folder", "上一层", bool(STATE["path"])),
        ("down", "下一页 (%s/%s)" % (STATE["page"] + 1, pages), STATE["page"] < pages - 1),
    ]
    for index, (action, label, enabled) in enumerate(buttons):
        rect = (margin + index * (width + gap), bottom_y, width, 54)
        canvas.button(rect, label, active=enabled, font=ctx.fonts["tiny"])
        STATE["rects"][(action, 0)] = rect
    if STATE["loading"]:
        canvas.centered_text("同步中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle(data, ctx):
    x = int(data.get("x-pixel") or 0)
    y = int(data.get("y-pixel") or 0)
    if data.get("gesture") == "tap" and y < int(ctx.height * 0.09) and x > int(ctx.width * 0.72):
        _load(ctx)
        return
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
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        action = key[0]
        if action == "item":
            items = STATE.get("visible") or []
            if key[1] < len(items):
                item = items[key[1]]
                if item.get("type") == "FOLDER":
                    STATE["path"].append(item.get("id"))
                    _load(ctx)
                else:
                    ctx.navigate("book", book_id=item.get("id"))
        elif action == "folder":
            keyboard.start(ctx.screen, ctx.fonts, capabilities=("cn", "en", "numsym"),
                           hint="请输入文件夹名称", enter_label="创建",
                           owner="shelf", on_submit=lambda text: _create_folder(ctx, text))
            ctx.navigate("keyboard")
        elif action == "prev_folder" and STATE["path"]:
            STATE["path"].pop()
            _load(ctx)
        elif action == "up" and STATE["page"] > 0:
            STATE["page"] -= 1
            ctx.show()
        elif action == "down":
            items = STATE.get("visible") or []
            row_height = max(74, int(ctx.height * 0.060))
            per_page = max(1, (ctx.height - int(ctx.height * 0.085) - 110) // row_height)
            pages = max(1, (len(items) + per_page - 1) // per_page)
            if STATE["page"] < pages - 1:
                STATE["page"] += 1
                ctx.show()
        return


def _create_folder(ctx, name):
    name = str(name or "").strip()
    if not name:
        return None
    folder_id = uuid.uuid4().hex
    item = {
        "type": "FOLDER",
        "id": folder_id,
        "index": 0,
        "parents": list(STATE["path"]),
        "title": name,
        "updateAt": datetime.now(timezone.utc).isoformat(),
    }
    items = list(STATE["items"])
    parent = STATE["path"][-1] if STATE["path"] else None
    for existing in items:
        if _last_parent(existing) == parent:
            existing["index"] = int(existing.get("index") or 0) + 1
    items.append(item)

    def success(_):
        STATE["items"] = items
        _load(ctx)

    ctx.run_async("shelf", lambda: ctx.api.save_book_shelf(items), success,
                  lambda exc: ctx.message(["创建失败", str(exc)]))
    return None


def _long_press(ctx, item):
    if item.get("type") == "FOLDER":
        ctx.confirm("删除文件夹及其下级？", lambda: _delete_folder(ctx, item.get("id")))
    else:
        ctx.confirm("从书架移出书籍？", lambda: _remove_book(ctx, int(item.get("id"))))


def _delete_folder(ctx, folder_id):
    items = []
    for item in STATE["items"]:
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
    items = [item for item in STATE["items"] if int(item.get("id") or 0) != book_id]

    def success(_):
        STATE["items"] = items
        _load(ctx)

    ctx.run_async("shelf", lambda: ctx.api.save_book_shelf(items), success,
                  lambda exc: ctx.message(["移出失败", str(exc)]))
