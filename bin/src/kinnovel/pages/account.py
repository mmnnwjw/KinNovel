STATE = {
    "mode": "login",
    "rects": {},
    "loading": False,
    "attempted": False,
    "error": "",
}


def enter(ctx):
    if ctx.api.user:
        STATE["mode"] = "profile"
        STATE["error"] = ""
        ctx.show()
        return
    STATE["mode"] = "login"
    ctx.show()
    if not STATE["loading"] and not STATE["attempted"]:
        _auto_login(ctx)


def render(ctx, canvas):
    if STATE["mode"] == "profile" and ctx.api.user:
        return _render_profile(ctx, canvas)
    top = canvas.header("账号", left="返回", right="主页")
    margin = int(canvas.width * 0.10)
    width = canvas.width - 2 * margin
    y = top + 60
    canvas.centered_text("自动登录", ctx.fonts["body"], canvas.width // 2, y)
    y += 80
    email = str(ctx.config.get("account_email") or "")
    password = str(ctx.config.get("account_password") or "")
    configured = bool(email and password)
    lines = [
        "账号来源: bin/config.json",
        "账号状态: 已配置" if configured else "账号状态: 未配置",
        "",
        "无需输入账号密码。",
        "启动后会使用配置文件凭据自动登录。",
    ]
    if not configured:
        lines.extend([
            "",
            "请在设备上编辑:",
            "/mnt/us/extensions/kinnovel/bin/config.json",
            "设置 account_email 和 account_password。",
        ])
    if STATE["loading"]:
        lines.extend(["", "正在登录…"])
    if STATE["error"]:
        lines.extend(["", "登录失败:", STATE["error"][:80]])
    for line in lines:
        canvas.text((margin, y), line, font=ctx.fonts["small"],
                    fill=canvas.theme.muted if line else canvas.theme.foreground)
        y += ctx.fonts["small"].size + 9
    STATE["rects"] = {}
    if configured and not STATE["loading"]:
        rect = (margin, y + 24, width, 62)
        canvas.button(rect, "重试自动登录", font=ctx.fonts["body"])
        STATE["rects"][("retry", 0)] = rect


def _auto_login(ctx, force=False):
    if STATE["loading"]:
        return
    if STATE["attempted"] and not force:
        return
    email = str(ctx.config.get("account_email") or "").strip()
    password = str(ctx.config.get("account_password") or "")
    STATE["attempted"] = True
    STATE["error"] = ""
    if not email or not password:
        STATE["error"] = "配置文件中未设置账号或密码"
        ctx.show()
        return
    STATE["loading"] = True
    ctx.show()

    def success(_user):
        STATE["loading"] = False
        STATE["mode"] = "profile"
        STATE["error"] = ""

    def error(exc):
        STATE["loading"] = False
        STATE["error"] = str(exc)

    ctx.run_async("account", lambda: ctx.api.login(email, password), success, error)


def _render_profile(ctx, canvas):
    top = canvas.header("我的账号", left="返回", right="主页")
    user = ctx.api.user or {}
    growth = user.get("Growth") or {}
    margin = int(canvas.width * 0.07)
    width = canvas.width - 2 * margin
    y = top + 24
    fields = [
        ("用户名", user.get("UserName") or "未知"),
        ("邮箱", user.get("Email") or ""),
        ("等级", str(user.get("Level") or 0)),
        ("经验", str(growth.get("Exp") or 0)),
        ("金币", str(growth.get("Coin") or 0)),
        ("漫画额度", "%s 永久 / %s 今日" % (
            growth.get("ComicQuota", 0), growth.get("ComicQuotaToday", 0))),
        ("连续签到", "%s 天" % growth.get("SignStreak", 0)),
    ]
    STATE["rects"] = {}
    for label, value in fields:
        canvas.text((margin, y), label, font=ctx.fonts["small"], fill=canvas.theme.muted)
        canvas.text((margin + 180, y), str(value), font=ctx.fonts["small"])
        y += 52
    buttons = [
        ("sign", "每日签到"),
        ("notifications", "通知"),
        ("shop", "商城"),
    ]
    gap = 12
    button_width = (width - gap) // 2
    for index, (action, label) in enumerate(buttons):
        row, column = divmod(index, 2)
        rect = (margin + column * (button_width + gap), y + row * 76, button_width, 62)
        canvas.button(rect, label, font=ctx.fonts["small"])
        STATE["rects"][(action, 0)] = rect


def handle(data, ctx):
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        kind = key[0]
        if kind == "retry":
            _auto_login(ctx, force=True)
        elif kind == "sign":
            _sign_in(ctx)
        elif kind in ("notifications", "shop"):
            ctx.navigate(kind)
        return


def _sign_in(ctx):
    def success(result):
        ctx.api.refresh_user()
        ctx.toast("签到成功 +%s 经验" % result.get("Reward", 0))

    ctx.run_async("account", ctx.api.sign_in, success,
                  lambda exc: ctx.message(["签到失败", str(exc)]))


NOTIFICATION_STATE = {
    "items": [],
    "page": 1,
    "total_pages": 1,
    "rects": {},
    "loading": True,
    "generation": 0,
}


def enter_notifications(ctx):
    _load_notifications(ctx)


def _load_notifications(ctx, page=None):
    if page is not None:
        NOTIFICATION_STATE["page"] = max(1, int(page))
    NOTIFICATION_STATE["generation"] += 1
    generation = NOTIFICATION_STATE["generation"]

    def success(result):
        if generation != NOTIFICATION_STATE["generation"]:
            return
        NOTIFICATION_STATE["items"] = result.get("Data") or []
        NOTIFICATION_STATE["page"] = int(result.get("Page") or 1)
        NOTIFICATION_STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))
        NOTIFICATION_STATE["loading"] = False

    ctx.run_async("notifications", lambda: ctx.api.get_notifications(
        NOTIFICATION_STATE["page"], 16), success,
        lambda exc: _notification_error(generation, exc, ctx))


def _notification_error(generation, exc, ctx):
    if generation == NOTIFICATION_STATE["generation"]:
        NOTIFICATION_STATE["loading"] = False
        ctx.message(["通知加载失败", str(exc)])


def render_notifications(ctx, canvas):
    top = canvas.header("通知", left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    row_height = max(76, int(canvas.height * 0.063))
    per_page = max(1, (canvas.height - top - 90) // row_height)
    items = NOTIFICATION_STATE["items"]
    pages = NOTIFICATION_STATE["total_pages"]
    NOTIFICATION_STATE["rects"] = {}
    for row in range(per_page):
        index = row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        NOTIFICATION_STATE["rects"][("item", index)] = rect
        if index >= len(items):
            continue
        item = items[index]
        canvas.draw.rounded_rectangle(
            [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
            radius=8, outline=canvas.theme.mid,
            fill=canvas.theme.light if not item.get("IsRead") else canvas.theme.background,
            width=1)
        canvas.text((rect[0] + 12, y + 8),
                    canvas.fit_text(item.get("Title") or "通知",
                                    ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 12, y + 40),
                    canvas.fit_text(item.get("Body") or "",
                                    ctx.fonts["tiny"], rect[2] - 24),
                    font=ctx.fonts["tiny"], fill=canvas.theme.muted)
    if pages > 1:
        bottom = canvas.height - 68
        width = int(canvas.width * 0.24)
        for action, x, label in (
            ("prev", margin, "上一页"),
            ("count", (canvas.width - width) // 2,
             "%s/%s" % (NOTIFICATION_STATE["page"], pages)),
            ("next", canvas.width - margin - width, "下一页"),
        ):
            rect = (x, bottom, width, 50)
            canvas.button(rect, label, font=ctx.fonts["tiny"])
            NOTIFICATION_STATE["rects"][(action, 0)] = rect
    if NOTIFICATION_STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"],
                             canvas.width // 2, canvas.height // 2)


def handle_notifications(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.68):
        ids = [int(item.get("Id")) for item in NOTIFICATION_STATE["items"]
               if not item.get("IsRead")]
        if ids:
            ctx.run_async("notifications",
                          lambda: ctx.api.mark_notifications(ids),
                          lambda _: (_mark_notifications_local(), ctx.show()),
                          lambda _: None)
        return
    for action in ("prev", "next", "count"):
        rect = NOTIFICATION_STATE["rects"].get((action, 0))
        if rect and rect[0] <= x < rect[0] + rect[2] and rect[1] <= y < rect[1] + rect[3]:
            if action == "prev" and NOTIFICATION_STATE["page"] > 1:
                _load_notifications(ctx, NOTIFICATION_STATE["page"] - 1)
            elif action == "next" and NOTIFICATION_STATE["page"] < NOTIFICATION_STATE["total_pages"]:
                _load_notifications(ctx, NOTIFICATION_STATE["page"] + 1)
            elif action == "count":
                _load_notifications(ctx, NOTIFICATION_STATE["page"])
            return
    for key, rect in NOTIFICATION_STATE["rects"].items():
        if key[0] != "item":
            continue
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            if key[1] < len(NOTIFICATION_STATE["items"]):
                item = NOTIFICATION_STATE["items"][key[1]]
                if not item.get("IsRead"):
                    ctx.run_async("notifications",
                                  lambda: ctx.api.mark_notifications([int(item.get("Id"))]),
                                  lambda _: ctx.show(), lambda _: None)
                    item["IsRead"] = True
            return


def _mark_notifications_local():
    for item in NOTIFICATION_STATE["items"]:
        item["IsRead"] = True


SHOP_STATE = {"shop": {}, "items": [], "owned": [], "rects": {}, "loading": True}


def enter_shop(ctx):
    def operation():
        return ctx.api.get_shop(), ctx.api.get_my_items()

    def success(result):
        SHOP_STATE["shop"], mine = result
        SHOP_STATE["items"] = SHOP_STATE["shop"].get("Items") or []
        SHOP_STATE["owned"] = mine.get("Items") or []
        SHOP_STATE["loading"] = False

    ctx.run_async("shop", operation, success,
                  lambda exc: (SHOP_STATE.update({"loading": False}),
                               ctx.message(["商城加载失败", str(exc)])))


def render_shop(ctx, canvas):
    top = canvas.header("商城 · 金币 %s" % SHOP_STATE["shop"].get("Coin", 0),
                        left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    row_height = max(84, int(canvas.height * 0.070))
    items = SHOP_STATE["items"]
    SHOP_STATE["rects"] = {}
    for index, item in enumerate(items[:8]):
        y = top + 12 + index * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 8)
        SHOP_STATE["rects"][("item", index)] = rect
        canvas.draw.rounded_rectangle(
            [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
            radius=8, outline=canvas.theme.mid, width=1)
        canvas.text((rect[0] + 12, y + 10), item.get("Name") or item.get("Key"),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 12, y + 44),
                    "%s 金币 · 持有 %s" % (item.get("Price", 0), item.get("Owned", 0)),
                    font=ctx.fonts["tiny"], fill=canvas.theme.muted)
    if SHOP_STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"],
                             canvas.width // 2, canvas.height // 2)


def handle_shop(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in SHOP_STATE["rects"].items():
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            item = SHOP_STATE["items"][key[1]]
            ctx.confirm("购买 %s？" % item.get("Name"),
                        lambda item=item: _buy(ctx, item))
            return


def _buy(ctx, item):
    def success(_):
        ctx.toast("购买成功")
        enter_shop(ctx)

    ctx.run_async("shop", lambda: ctx.api.buy_shop_item(item.get("Key"), 1), success,
                  lambda exc: ctx.message(["购买失败", str(exc)]))
