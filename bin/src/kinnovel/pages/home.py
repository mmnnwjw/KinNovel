from ..utils import wrap_text


STATE = {
    "rects": {},
    "online": None,
}


def _items(ctx):
    account = "我的账号" if ctx.api.user else "登录"
    return [
        ("browse", "最近/分类"),
        ("rank", "排行榜"),
        ("search", "搜索"),
        ("shelf", "书架"),
        ("history", "阅读历史"),
        ("announcements", "公告"),
        ("notifications", "通知"),
        ("account", account),
        ("shop", "商城"),
        ("settings", "设置"),
        ("about", "关于"),
        ("exit", "退出"),
    ]


def render(ctx, canvas):
    width, height = canvas.width, canvas.height
    title_font = ctx.fonts["hero"]
    canvas.centered_text("KinNovel", title_font, width // 2, int(height * 0.10))
    subtitle = "Kindle 轻书架"
    canvas.centered_text(subtitle, ctx.fonts["body"], width // 2, int(height * 0.16),
                         fill=canvas.theme.muted)
    user = ctx.api.user or {}
    status = user.get("UserName") if user else "未登录"
    if STATE["online"] is not None:
        status += "  ·  在线 %s" % STATE["online"]
    canvas.centered_text(status, ctx.fonts["small"], width // 2, int(height * 0.20),
                         fill=canvas.theme.muted)

    margin = int(width * 0.055)
    gap = max(12, int(width * 0.025))
    columns = 2
    button_width = (width - 2 * margin - gap * (columns - 1)) // columns
    button_height = max(64, int(height * 0.074))
    start_y = int(height * 0.245)
    row_gap = max(14, int(height * 0.018))
    STATE["rects"] = {}
    for index, (target, label) in enumerate(_items(ctx)):
        row, column = divmod(index, columns)
        x = margin + column * (button_width + gap)
        y = start_y + row * (button_height + row_gap)
        rect = (x, y, button_width, button_height)
        canvas.button(rect, label, active=(target == "shelf") or bool(user) or target not in ("account",))
        STATE["rects"][target] = rect


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for target, (rx, ry, width, height) in STATE["rects"].items():
        if rx <= x < rx + width and ry <= y < ry + height:
            if target == "exit":
                ctx.app.stop()
            elif target in ("shelf", "history", "notifications", "shop") and not ctx.api.user:
                ctx.toast("请先登录")
                ctx.navigate("account")
            else:
                ctx.navigate(target)
            return
