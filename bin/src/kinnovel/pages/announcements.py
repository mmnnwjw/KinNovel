import re


STATE = {
    "items": [],
    "page": 1,
    "total_pages": 1,
    "data": None,
    "rects": {},
    "loading": False,
    "loaded": False,
    "generation": 0,
}


def enter(ctx):
    if not STATE["loaded"]:
        _load(ctx, 1)


def _load(ctx, page=1):
    STATE["generation"] += 1
    generation = STATE["generation"]
    STATE["page"] = int(page)
    STATE["loading"] = True

    def success(result):
        if generation != STATE["generation"]:
            return
        STATE["items"] = result.get("Data") or []
        STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))
        STATE["loading"] = False
        STATE["loaded"] = True

    def error(exc):
        if generation != STATE["generation"]:
            return
        STATE["loading"] = False
        ctx.message(["公告加载失败", str(exc)])

    ctx.run_async("announcements", lambda: ctx.api.get_announcement_list(
        STATE["page"], 16), success, error)


def render(ctx, canvas):
    top = canvas.header("公告", left="返回", right="刷新")
    margin = int(canvas.width * 0.035)
    row_height = max(74, int(canvas.height * 0.061))
    per_page = max(1, (canvas.height - top - 80) // row_height)
    start = (STATE["page"] - 1) * per_page
    STATE["rects"] = {}
    for row in range(per_page):
        index = start + row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        STATE["rects"][("item", index)] = rect
        if index >= len(STATE["items"]):
            continue
        item = STATE["items"][index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        created = str(item.get("CreatedAt") or "")[:10]
        canvas.text((rect[0] + 12, y + 8),
                    canvas.fit_text("[%s] %s" % (created, item.get("Title") or ""),
                                    ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
    if STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.72):
        _load(ctx, STATE["page"])
        return
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[1] < len(STATE["items"]):
                ctx.navigate("announcement", announcement_id=STATE["items"][key[1]].get("Id"))
            return


def enter_detail(ctx):
    announcement_id = int(ctx.params.get("announcement_id") or 0)
    STATE["data"] = None

    def success(result):
        STATE["data"] = result

    ctx.run_async("announcement", lambda: ctx.api.get_announcement_detail(announcement_id),
                  success, lambda exc: ctx.message(["公告加载失败", str(exc)]))


def render_detail(ctx, canvas):
    top = canvas.header("公告详情", left="返回", right="评论")
    data = STATE.get("data")
    if not data:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)
        return
    margin = int(canvas.width * 0.05)
    width = canvas.width - 2 * margin
    y = top + 14
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
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.72):
        if not ctx.api.user:
            ctx.toast("请先登录")
            return
        ctx.navigate("comments", comment_type="Announcement",
                     target_id=ctx.params.get("announcement_id"))


COMMENT_STATE = {"data": None, "rects": {}, "loading": True}


def enter_comments(ctx):
    comment_type = ctx.params.get("comment_type") or "Book"
    target_id = int(ctx.params.get("target_id") or 0)
    COMMENT_STATE["loading"] = True

    def success(result):
        COMMENT_STATE["data"] = result
        COMMENT_STATE["loading"] = False

    ctx.run_async("comments", lambda: ctx.api.get_comments(comment_type, target_id, 1),
                  success, lambda exc: (COMMENT_STATE.update({"loading": False}),
                                        ctx.message(["评论加载失败", str(exc)])))


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
        for line in canvas.wrap(content, ctx.fonts["tiny"], canvas.width - 64)[:2]:
            canvas.text((32, y + 40), line, font=ctx.fonts["tiny"],
                        fill=canvas.theme.muted)
            y += 0
        y += 100
    if COMMENT_STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle_comments(data, ctx):
    return None
