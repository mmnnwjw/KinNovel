import re

from .. import widgets


STATE = {
    "items": [],
    "page": 1,
    "total_pages": 1,
    "data": None,
    "rects": {},
    "loading": False,
    "loaded": False,
    "error": "",
    "detail_error": "",
    "detail_rects": {},
    "generation": 0,
}

_loader = widgets.ListLoader(STATE)


def enter(ctx):
    # 从主页新进入时回到第一页；从公告详情返回时保留页码(与其它列表页一致)。
    _load(ctx, STATE["page"] if ctx.returning else 1)


def _load(ctx, page=1):
    STATE["page"] = int(page)

    def done(result):
        STATE["items"] = result.get("Data") or []
        STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))

    def failed(_exc):
        STATE["items"] = []
        STATE["total_pages"] = 1

    _top, _row_height, per_page = _layout(ctx)
    _loader.start(ctx, "announcements", lambda: ctx.api.get_announcement_list(
        STATE["page"], per_page), done, failed)


def _layout(ctx):
    # 服务器分页大小必须等于实际可渲染行数，否则每页尾部条目永远翻不到
    top = max(72, int(ctx.height * 0.085))
    row_height = widgets.list_row_height(ctx)
    bottom = widgets.pager_height(ctx) + 24
    per_page = max(1, (ctx.height - top - bottom) // row_height)
    return top, row_height, per_page


def render(ctx, canvas):
    canvas.header("公告", left="返回", right="主页")
    top, row_height, per_page = _layout(ctx)
    margin = int(canvas.width * 0.035)
    STATE["rects"] = {}
    for row in range(per_page):
        index = row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        if not isinstance(item, dict):
            continue
        created = str(item.get("CreatedAt") or "")[:10]
        widgets.row_card(canvas, ctx, rect,
                         "[%s] %s" % (created, item.get("Title") or ""),
                         wrap_title=True)
    nav_y = canvas.height - widgets.pager_height(ctx) - 16
    pager_rect = (margin, nav_y, canvas.width - 2 * margin, widgets.pager_height(ctx))
    if STATE["error"]:
        widgets.error_state(canvas, ctx, STATE["error"], STATE["rects"], pager_rect)
    else:
        widgets.pager_bar(canvas, ctx, pager_rect, STATE["page"] > 1,
                          STATE["page"] < STATE["total_pages"],
                          "%s/%s" % (STATE["page"], STATE["total_pages"]),
                          STATE["rects"])
        if STATE["loading"]:
            widgets.loading_state(canvas, ctx)
        elif STATE["loaded"] and not STATE["items"]:
            widgets.empty_state(canvas, ctx, "暂无公告")


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
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
        return
    if key[0] == "item" and key[1] < len(STATE["items"]):
        ctx.navigate("announcement", announcement_id=STATE["items"][key[1]].get("Id"))


def enter_detail(ctx):
    announcement_id = int(ctx.params.get("announcement_id") or 0)
    STATE["data"] = None
    STATE["detail_error"] = ""
    STATE["detail_rects"] = {}

    def success(result):
        STATE["data"] = result
        STATE["detail_error"] = ""

    def error(exc):
        STATE["data"] = None
        STATE["detail_error"] = str(exc)

    ctx.run_async("announcement", lambda: ctx.api.get_announcement_detail(announcement_id),
                  success, error)


def render_detail(ctx, canvas):
    top = canvas.header("公告详情", left="返回", right="主页")
    margin = int(canvas.width * 0.05)
    width = canvas.width - 2 * margin
    STATE["detail_rects"] = {}
    if STATE["detail_error"]:
        canvas.centered_text("加载失败", ctx.fonts["body"],
                             canvas.width // 2, canvas.height // 2 - 30)
        canvas.centered_text(str(STATE["detail_error"])[:40], ctx.fonts["tiny"],
                             canvas.width // 2, canvas.height // 2 + 30,
                             fill=canvas.theme.muted)
        rect = (margin, canvas.height - 72, width, 54)
        canvas.button(rect, "重试", font=ctx.fonts["small"])
        STATE["detail_rects"][("retry", 0)] = rect
        return
    data = STATE.get("data")
    if not data:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
        return
    # 评论入口必须是可见按钮，不能依赖状态栏区域的隐藏热区
    comment_rect = (canvas.width - margin - 240, top + 12, 240, 54)
    canvas.button(comment_rect, "查看评论", font=ctx.fonts["small"])
    STATE["detail_rects"][("comments", 0)] = comment_rect
    y = top + 82
    for line in canvas.wrap(data.get("Title") or "", ctx.fonts["body"], width):
        canvas.text((margin, y), line, font=ctx.fonts["body"])
        y += ctx.fonts["body"].size + 8
    y += 10
    content = re.sub(r"<br\s*/?>", "\n", str(data.get("Content") or ""), flags=re.I)
    content = re.sub(r"</p\s*>", "\n\n", content, flags=re.I)
    content = re.sub(r"<[^>]+>", "", content)
    content = content.replace("&nbsp;", " ").replace("&amp;", "&")
    line_height = max(ctx.fonts["small"].size + 8, 42)
    for paragraph in content.splitlines():
        for line in canvas.wrap(paragraph, ctx.fonts["small"], width):
            if y + line_height > canvas.height - 20:
                canvas.text((margin, canvas.height - 28), "…", font=ctx.fonts["small"])
                return
            canvas.text((margin, y), line, font=ctx.fonts["small"])
            y += line_height


def handle_detail(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    retry = STATE["detail_rects"].get(("retry", 0))
    if retry and retry[0] <= x < retry[0] + retry[2] and retry[1] <= y < retry[1] + retry[3]:
        enter_detail(ctx)
        return
    comment_rect = STATE["detail_rects"].get(("comments", 0))
    if (comment_rect
            and comment_rect[0] <= x < comment_rect[0] + comment_rect[2]
            and comment_rect[1] <= y < comment_rect[1] + comment_rect[3]):
        if not ctx.api.user:
            ctx.toast("请先登录")
            return
        ctx.navigate("comments", comment_type="Announcement",
                     target_id=ctx.params.get("announcement_id"))


COMMENT_STATE = {"data": None, "rects": {}, "loading": True, "error": ""}


def enter_comments(ctx):
    comment_type = ctx.params.get("comment_type") or "Book"
    target_id = int(ctx.params.get("target_id") or 0)
    COMMENT_STATE["loading"] = True
    COMMENT_STATE["error"] = ""

    def success(result):
        COMMENT_STATE["data"] = result
        COMMENT_STATE["loading"] = False
        COMMENT_STATE["error"] = ""

    def error(exc):
        COMMENT_STATE["loading"] = False
        COMMENT_STATE["error"] = str(exc)

    ctx.run_async("comments", lambda: ctx.api.get_comments(comment_type, target_id, 1),
                  success, error)


def render_comments(ctx, canvas):
    top = canvas.header("评论", left="返回", right="主页")
    data = COMMENT_STATE.get("data") or {}
    users = data.get("Users") or {}
    commentaries = data.get("Commentaries") or {}
    rows = data.get("Data") or []
    y = top + 12
    line_height = ctx.fonts["small"].size + 8
    COMMENT_STATE["rects"] = {}
    for index, row in enumerate(rows[:10]):
        if y + 96 > canvas.height - 20:
            break
        comment_id = str(row.get("Id"))
        comment = commentaries.get(comment_id) or commentaries.get(row.get("Id")) or {}
        user_id = str(comment.get("UserId") or "")
        user = users.get(user_id) or {}
        canvas.draw.rounded_rectangle([20, y, canvas.width - 20, y + 92],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.text((32, y + 8), user.get("UserName") or "用户",
                    font=ctx.fonts["small"])
        content = str(comment.get("Content") or "")
        content_y = y + 40
        for line in canvas.wrap(content, ctx.fonts["tiny"], canvas.width - 64)[:2]:
            canvas.text((32, content_y), line, font=ctx.fonts["tiny"],
                        fill=canvas.theme.muted)
            content_y += ctx.fonts["tiny"].size + 6
        y += 100
    if COMMENT_STATE["error"]:
        canvas.centered_text("加载失败", ctx.fonts["body"],
                             canvas.width // 2, canvas.height // 2 - 30)
        canvas.centered_text(str(COMMENT_STATE["error"])[:40], ctx.fonts["tiny"],
                             canvas.width // 2, canvas.height // 2 + 30,
                             fill=canvas.theme.muted)
        rect = (20, canvas.height - 72, canvas.width - 40, 54)
        canvas.button(rect, "重试", font=ctx.fonts["small"])
        COMMENT_STATE["rects"][("retry", 0)] = rect
    elif COMMENT_STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle_comments(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    retry = COMMENT_STATE["rects"].get(("retry", 0))
    if retry and retry[0] <= x < retry[0] + retry[2] and retry[1] <= y < retry[1] + retry[3]:
        enter_comments(ctx)
