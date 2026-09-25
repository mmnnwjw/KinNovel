from page import keyboard


STATE = {
    "mode": "login",
    "email": "",
    "password": "",
    "username": "",
    "code": "",
    "invite": "",
    "rects": {},
    "loading": False,
}


def enter(ctx):
    STATE["mode"] = "profile" if ctx.api.user else "login"
    ctx.show()


def _field_rects(ctx, canvas):
    top = canvas.header("账号", left="返回", right="主页")
    margin = int(canvas.width * 0.12)
    width = canvas.width - 2 * margin
    y = top + 30
    height = 66
    gap = 18
    fields = _fields()
    rects = {}
    for key, label in fields:
        rects[("field", key)] = (margin, y, width, height)
        y += height + gap
    rects[("submit", 0)] = (margin, y + 10, width, 64)
    y += 92
    rects[("switch", 0)] = (margin, y, width, 58)
    rects[("code", 0)] = (margin, y + 72, width, 58)
    return rects


def _fields():
    if STATE["mode"] == "register":
        return [
            ("username", "用户名"),
            ("email", "邮箱"),
            ("password", "密码"),
            ("invite", "邀请码（可空）"),
            ("code", "邮箱验证码"),
        ]
    if STATE["mode"] == "reset":
        return [
            ("email", "邮箱"),
            ("password", "新密码"),
            ("code", "邮箱验证码"),
        ]
    return [("email", "邮箱"), ("password", "密码")]


def render(ctx, canvas):
    if STATE["mode"] == "profile":
        return _render_profile(ctx, canvas)
    top = canvas.header("登录 / 注册", left="返回", right="主页")
    margin = int(canvas.width * 0.12)
    width = canvas.width - 2 * margin
    title = {"login": "登录轻书架", "register": "注册账号", "reset": "重置密码"}[STATE["mode"]]
    canvas.centered_text(title, ctx.fonts["body"], canvas.width // 2, top + 42)
    rects = _field_rects(ctx, canvas)
    STATE["rects"] = rects
    for key, label in _fields():
        rect = rects[("field", key)]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=9, outline=canvas.theme.foreground, width=2)
        value = STATE.get(key) or ""
        if key == "password" and value:
            value = "•" * len(value)
        canvas.text((rect[0] + 14, rect[1] + 17),
                    canvas.fit_text(value or label, ctx.fonts["small"], rect[2] - 28),
                    font=ctx.fonts["small"],
                    fill=canvas.theme.foreground if value else canvas.theme.muted)
    submit_label = {"login": "登录", "register": "注册", "reset": "重置密码"}[STATE["mode"]]
    canvas.button(rects[("submit", 0)], submit_label, font=ctx.fonts["body"])
    switch = {
        "login": ("还没有账号？去注册", "register"),
        "register": ("已有账号？去登录", "login"),
        "reset": ("返回登录", "login"),
    }[STATE["mode"]]
    canvas.button(rects[("switch", 0)], switch[0], active=False, font=ctx.fonts["small"])
    if STATE["mode"] in ("register", "reset"):
        canvas.button(rects[("code", 0)], "发送邮箱验证码", font=ctx.fonts["small"])
    if STATE["loading"]:
        canvas.centered_text("处理中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


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
        ("漫画额度", "%s 永久 / %s 今日" % (growth.get("ComicQuota", 0), growth.get("ComicQuotaToday", 0))),
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
        ("logout", "退出登录"),
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
        if STATE["mode"] == "profile":
            if kind == "sign":
                _sign_in(ctx)
            elif kind == "logout":
                ctx.api.logout()
                STATE["mode"] = "login"
                ctx.toast("已退出")
            elif kind in ("notifications", "shop"):
                ctx.navigate(kind)
        elif kind == "field":
            _edit_field(ctx, key[1])
        elif kind == "submit":
            _submit(ctx)
        elif kind == "switch":
            mapping = {"login": "register", "register": "login", "reset": "login"}
            STATE["mode"] = mapping[STATE["mode"]]
            ctx.show()
        elif kind == "code":
            _send_code(ctx)
        return


def _edit_field(ctx, field):
    caps = ("en", "numsym") if field != "password" else ("en", "numsym")
    hint = {
        "email": "请输入邮箱",
        "password": "请输入密码",
        "username": "请输入用户名",
        "code": "请输入邮箱验证码",
        "invite": "请输入邀请码",
    }.get(field, "请输入")

    def submitted(value):
        STATE[field] = value
        return None
    keyboard.start(ctx.screen, ctx.fonts, capabilities=caps, hint=hint,
                   enter_label="完成", owner=ctx.page_name, on_submit=submitted)
    ctx.navigate("keyboard")


def _submit(ctx):
    STATE["loading"] = True

    def operation():
        if STATE["mode"] == "login":
            return ctx.api.login(STATE["email"].strip(), STATE["password"])
        if STATE["mode"] == "register":
            return ctx.api.register(
                STATE["username"].strip(), STATE["email"].strip(),
                STATE["password"], STATE["code"].strip(), STATE["invite"].strip())
        return ctx.api.reset_password(
            STATE["email"].strip(), STATE["password"], STATE["code"].strip())

    def success(_):
        STATE["loading"] = False
        if STATE["mode"] == "reset":
            STATE["mode"] = "login"
            ctx.toast("密码已重置")
        else:
            STATE["mode"] = "profile"
            ctx.toast("登录成功")

    def error(exc):
        STATE["loading"] = False
        ctx.message(["操作失败", str(exc)])

    ctx.run_async(ctx.page_name, operation, success, error)


def _send_code(ctx):
    if not STATE["email"].strip():
        ctx.toast("请先填写邮箱")
        return
    operation = (ctx.api.send_register_email if STATE["mode"] == "register"
                 else ctx.api.send_reset_email)
    ctx.run_async(ctx.page_name, lambda: operation(STATE["email"].strip()),
                  lambda _: ctx.toast("验证码已发送"),
                  lambda exc: ctx.message(["发送失败", str(exc)]))


def _sign_in(ctx):
    def success(result):
        ctx.api.refresh_user()
        ctx.toast("签到成功 +%s 经验" % result.get("Reward", 0))
    ctx.run_async("account", ctx.api.sign_in, success,
                  lambda exc: ctx.message(["签到失败", str(exc)]))


NOTIFICATION_STATE = {"items": [], "page": 1, "total_pages": 1, "rects": {},
                      "loading": True, "generation": 0}


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
    top = canvas.header("通知", left="返回", right="全部已读")
    margin = int(canvas.width * 0.035)
    row_height = max(76, int(canvas.height * 0.063))
    per_page = max(1, (canvas.height - top - 90) // row_height)
    items = NOTIFICATION_STATE["items"]
    pages = NOTIFICATION_STATE["total_pages"]
    start = 0
    NOTIFICATION_STATE["rects"] = {}
    for row in range(per_page):
        index = start + row
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        NOTIFICATION_STATE["rects"][("item", index)] = rect
        if index >= len(items):
            continue
        item = items[index]
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid,
                                      fill=canvas.theme.light if not item.get("IsRead") else canvas.theme.background,
                                      width=1)
        canvas.text((rect[0] + 12, y + 8),
                    canvas.fit_text(item.get("Title") or "通知", ctx.fonts["small"], rect[2] - 24),
                    font=ctx.fonts["small"])
        canvas.text((rect[0] + 12, y + 40),
                    canvas.fit_text(item.get("Body") or "", ctx.fonts["tiny"], rect[2] - 24),
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
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def handle_notifications(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    if y < int(ctx.height * 0.09) and x > int(ctx.width * 0.68):
        ids = [int(item.get("Id")) for item in NOTIFICATION_STATE["items"] if not item.get("IsRead")]
        if ids:
            ctx.run_async("notifications", lambda: ctx.api.mark_notifications(ids),
                          lambda _: (_mark_local(), ctx.show()), lambda _: None)
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


def _mark_local():
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
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=8, outline=canvas.theme.mid, width=1)
        canvas.text((rect[0] + 12, y + 10), item.get("Name") or item.get("Key"),
                    font=ctx.fonts["small"])
        detail = "%s 金币 · 持有 %s" % (item.get("Price", 0), item.get("Owned", 0))
        canvas.text((rect[0] + 12, y + 44), detail, font=ctx.fonts["tiny"],
                    fill=canvas.theme.muted)
    if SHOP_STATE["loading"]:
        canvas.centered_text("加载中…", ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


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
