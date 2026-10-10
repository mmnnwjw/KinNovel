import time

from .. import widgets

STATE = {
    "mode": "login",
    "rects": {},
    "loading": False,
    "attempted": False,
    "error": "",
    "today_signed": False,
}


def _is_today_signed(user):
    growth = (user or {}).get("Growth") or {}
    if growth.get("TodaySigned"):
        return True
    value = growth.get("LastSignAt") or growth.get("LastSignTime")
    text = str(value or "").strip()
    if not text:
        return False
    try:
        parsed = time.strptime(text[:10], "%Y-%m-%d")
    except (TypeError, ValueError):
        return False
    return tuple(parsed[:3]) == tuple(time.localtime()[:3])


def enter(ctx):
    if ctx.api.user:
        STATE["mode"] = "profile"
        STATE["error"] = ""
        STATE["today_signed"] = _is_today_signed(ctx.api.user)
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
        if ctx.page_name == "account":
            ctx.show()

    def error(exc):
        STATE["loading"] = False
        STATE["error"] = str(exc)
        if ctx.page_name == "account":
            ctx.show()

    # sticky: 离开账号页也要清掉 loading，否则回来会永久卡在“正在登录…”
    ctx.run_async("account", lambda: ctx.api.login(email, password),
                  success, error, refresh=False, sticky=True)


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
    signed = bool(STATE.get("today_signed")) or _is_today_signed(user)
    buttons = [
        ("sign", "今日已签到" if signed else "每日签到"),
        ("notifications", "通知"),
        ("shop", "商城"),
    ]
    gap = 12
    button_width = (width - gap) // 2
    for index, (action, label) in enumerate(buttons):
        row, column = divmod(index, 2)
        rect = (margin + column * (button_width + gap), y + row * 76, button_width, 62)
        canvas.button(rect, label, active=(action != "sign" or not signed),
                      font=ctx.fonts["small"])
        STATE["rects"][(action, 0)] = rect


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        kind = key[0]
        if kind == "retry":
            _auto_login(ctx, force=True)
        elif kind == "sign":
            if STATE.get("today_signed"):
                return
            _sign_in(ctx)
        elif kind in ("notifications", "shop"):
            ctx.navigate(kind)
        return


def _sign_in(ctx):
    def operation():
        result = ctx.api.sign_in()
        try:
            ctx.api.refresh_user()
        except Exception:
            pass
        return result

    def success(result):
        STATE["today_signed"] = True
        ctx.toast("签到成功 +%s 经验" % (result or {}).get("Reward", 0))

    ctx.run_async("account", operation, success,
                  lambda exc: ctx.message(["签到失败", str(exc)]))


NOTIFICATION_STATE = {
    "items": [],
    "page": 1,
    "total_pages": 1,
    "rects": {},
    "loading": True,
    "error": "",
    "generation": 0,
}

_notification_loader = widgets.ListLoader(NOTIFICATION_STATE)


def enter_notifications(ctx):
    # 从主页新进入时回到第一页；从通知详情(标记已读等)返回时保留页码。
    _load_notifications(ctx, NOTIFICATION_STATE["page"] if ctx.returning else 1)


def _load_notifications(ctx, page=None):
    if page is not None:
        NOTIFICATION_STATE["page"] = max(1, int(page))

    def done(result):
        NOTIFICATION_STATE["items"] = result.get("Data") or []
        NOTIFICATION_STATE["page"] = int(result.get("Page") or 1)
        NOTIFICATION_STATE["total_pages"] = max(1, int(result.get("TotalPages") or 1))

    def failed(_exc):
        # 清掉旧通知，避免错误态还显示上一页数据
        NOTIFICATION_STATE["items"] = []
        NOTIFICATION_STATE["total_pages"] = 1

    _top, _row_height, per_page = _notification_layout(ctx)
    _notification_loader.start(ctx, "notifications", lambda: ctx.api.get_notifications(
        NOTIFICATION_STATE["page"], per_page), done, failed)


def _notification_layout(ctx):
    # 同公告页：拉取条数必须等于可渲染行数，否则每页尾部通知永远翻不到
    top = max(72, int(ctx.height * 0.085))
    action_height = 66
    list_top = top + action_height
    row_height = widgets.list_row_height(ctx)
    bottom = widgets.pager_height(ctx) + 24
    per_page = max(1, (ctx.height - list_top - bottom) // row_height)
    return top, row_height, per_page


def render_notifications(ctx, canvas):
    canvas.header("通知", left="返回", right="主页")
    top, row_height, per_page = _notification_layout(ctx)
    margin = int(canvas.width * 0.035)
    items = NOTIFICATION_STATE["items"]
    pages = NOTIFICATION_STATE["total_pages"]
    NOTIFICATION_STATE["rects"] = {}
    if NOTIFICATION_STATE["error"]:
        rect = (margin, canvas.height - widgets.pager_height(ctx) - 16,
               canvas.width - 2 * margin, widgets.pager_height(ctx))
        widgets.error_state(canvas, ctx, NOTIFICATION_STATE["error"],
                            NOTIFICATION_STATE["rects"], rect)
        return
    # “全部已读”必须是可见按钮，不能复用状态栏的隐藏热区
    action_rect = (canvas.width - margin - 240, top + 6, 240, 54)
    has_unread = any(not item.get("IsRead") for item in items)
    canvas.button(action_rect, "全部已读", active=has_unread, font=ctx.fonts["small"])
    NOTIFICATION_STATE["rects"][("readall", 0)] = action_rect
    list_y = top + 66
    for row in range(per_page):
        index = row
        y = list_y + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 6)
        NOTIFICATION_STATE["rects"][("item", index)] = rect
        if index >= len(items):
            continue
        item = items[index]
        if not isinstance(item, dict):
            continue
        widgets.row_card(canvas, ctx, rect, item.get("Title") or "通知",
                         item.get("Body") or "", highlight=not item.get("IsRead"),
                         wrap_title=True)
    if pages > 1:
        bottom_y = canvas.height - widgets.pager_height(ctx) - 16
        pager_rect = (margin, bottom_y, canvas.width - 2 * margin, widgets.pager_height(ctx))
        widgets.pager_bar(canvas, ctx, pager_rect, NOTIFICATION_STATE["page"] > 1,
                          NOTIFICATION_STATE["page"] < pages,
                          "%s/%s" % (NOTIFICATION_STATE["page"], pages),
                          NOTIFICATION_STATE["rects"])
    if NOTIFICATION_STATE["loading"]:
        widgets.loading_state(canvas, ctx)


def handle_notifications(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    key = widgets.hit_test(NOTIFICATION_STATE["rects"], x, y)
    if key is None:
        return
    if key == ("retry", 0):
        _load_notifications(ctx, NOTIFICATION_STATE["page"])
        return
    if key == ("readall", 0):
        ids = [int(item.get("Id")) for item in NOTIFICATION_STATE["items"]
               if not item.get("IsRead")]
        if ids:
            ctx.run_async("notifications",
                          lambda: ctx.api.mark_notifications(ids),
                          lambda _: (_mark_notifications_local(), ctx.show()),
                          lambda _: None)
        return
    if key[0] in ("prev", "next"):
        if key[0] == "prev" and NOTIFICATION_STATE["page"] > 1:
            _load_notifications(ctx, NOTIFICATION_STATE["page"] - 1)
        elif key[0] == "next" and NOTIFICATION_STATE["page"] < NOTIFICATION_STATE["total_pages"]:
            _load_notifications(ctx, NOTIFICATION_STATE["page"] + 1)
        return
    if key[0] == "item" and key[1] < len(NOTIFICATION_STATE["items"]):
        item = NOTIFICATION_STATE["items"][key[1]]
        if not item.get("IsRead"):
            ctx.run_async("notifications",
                          lambda: ctx.api.mark_notifications([int(item.get("Id"))]),
                          lambda _: ctx.show(), lambda _: None)
            item["IsRead"] = True


def _mark_notifications_local():
    for item in NOTIFICATION_STATE["items"]:
        item["IsRead"] = True


SHOP_STATE = {"shop": {}, "items": [], "rects": {}, "loading": True,
              "error": "", "page": 0}


def enter_shop(ctx):
    SHOP_STATE["loading"] = True
    SHOP_STATE["error"] = ""

    def operation():
        return ctx.api.get_shop()

    def success(shop):
        SHOP_STATE["shop"] = shop or {}
        SHOP_STATE["items"] = SHOP_STATE["shop"].get("Items") or []
        SHOP_STATE["page"] = 0
        SHOP_STATE["loading"] = False
        SHOP_STATE["error"] = ""

    def error(exc):
        SHOP_STATE["loading"] = False
        SHOP_STATE["error"] = str(exc)
        SHOP_STATE["items"] = []

    ctx.run_async("shop", operation, success, error)


def _shop_layout(ctx):
    top = max(72, int(ctx.height * 0.085))
    row_height = max(84, int(ctx.height * 0.070))
    bottom = widgets.pager_height(ctx) + 24
    per_page = max(1, (ctx.height - top - bottom) // row_height)
    return top, row_height, per_page


def render_shop(ctx, canvas):
    top, row_height, per_page = _shop_layout(ctx)
    canvas.header("商城 · 金币 %s" % SHOP_STATE["shop"].get("Coin", 0),
                 left="返回", right="主页")
    margin = int(canvas.width * 0.035)
    items = SHOP_STATE["items"]
    pages = max(1, (len(items) + per_page - 1) // per_page)
    SHOP_STATE["page"] = min(SHOP_STATE["page"], pages - 1)
    start = SHOP_STATE["page"] * per_page
    SHOP_STATE["rects"] = {}
    nav_y = canvas.height - widgets.pager_height(ctx) - 16
    pager_rect = (margin, nav_y, canvas.width - 2 * margin, widgets.pager_height(ctx))
    if SHOP_STATE["error"]:
        widgets.error_state(canvas, ctx, SHOP_STATE["error"], SHOP_STATE["rects"], pager_rect)
        return
    if SHOP_STATE["loading"]:
        widgets.loading_state(canvas, ctx)
        return
    for row in range(per_page):
        index = start + row
        if index >= len(items):
            break
        item = items[index]
        if not isinstance(item, dict):
            continue
        y = top + 12 + row * row_height
        rect = (margin, y, canvas.width - 2 * margin, row_height - 8)
        SHOP_STATE["rects"][("item", index)] = rect
        widgets.row_card(canvas, ctx, rect, item.get("Name") or item.get("Key") or "",
                         "%s 金币 · 持有 %s" % (item.get("Price", 0), item.get("Owned", 0)))
    widgets.pager_bar(canvas, ctx, pager_rect, SHOP_STATE["page"] > 0,
                      SHOP_STATE["page"] < pages - 1,
                      "%s/%s" % (SHOP_STATE["page"] + 1, pages), SHOP_STATE["rects"])
    if not items:
        widgets.empty_state(canvas, ctx, "暂无商品")


def handle_shop(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    key = widgets.hit_test(SHOP_STATE["rects"], x, y)
    if key is None:
        return
    if key == ("retry", 0):
        enter_shop(ctx)
        return
    if key[0] == "item":
        item = SHOP_STATE["items"][key[1]]
        ctx.confirm("购买 %s？" % item.get("Name"),
                    lambda item=item: _buy(ctx, item))
    elif key[0] == "prev" and SHOP_STATE["page"] > 0:
        SHOP_STATE["page"] -= 1
        ctx.show()
    elif key[0] == "next":
        _top, _row_height, per_page = _shop_layout(ctx)
        pages = max(1, (len(SHOP_STATE["items"]) + per_page - 1) // per_page)
        if SHOP_STATE["page"] < pages - 1:
            SHOP_STATE["page"] += 1
            ctx.show()


def _buy(ctx, item):
    def success(_):
        ctx.toast("购买成功")
        enter_shop(ctx)

    ctx.run_async("shop", lambda: ctx.api.buy_shop_item(item.get("Key"), 1), success,
                  lambda exc: ctx.message(["购买失败", str(exc)]))
