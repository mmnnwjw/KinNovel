from ..config import CACHE_DIR
from ..utils import cache_size, clear_cache


STATE = {"rects": {}, "server": ""}


def render(ctx, canvas):
    top = canvas.header("设置", left="返回", right="主页")
    margin = int(canvas.width * 0.05)
    y = top + 18
    height = 64
    gap = 14

    def row(label, value=None, action=None, progress=None):
        nonlocal y
        rect = (margin, y, canvas.width - 2 * margin, height)
        STATE["rects"][(action or label, 0)] = rect
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=9, outline=canvas.theme.mid, width=1)
        canvas.text((rect[0] + 14, y + 16), label, font=ctx.fonts["small"])
        if value is not None:
            text = canvas.fit_text(str(value), ctx.fonts["tiny"], rect[2] // 2 - 20)
            bbox = canvas.draw.textbbox((0, 0), text, font=ctx.fonts["tiny"])
            canvas.text((rect[0] + rect[2] - (bbox[2] - bbox[0]) - 14, y + 22),
                        text, font=ctx.fonts["tiny"], fill=canvas.theme.muted)
        if progress is not None:
            bar_y = y + height - 8
            canvas.draw.rectangle([rect[0] + 8, bar_y,
                                   rect[0] + 8 + int((rect[2] - 16) * progress), bar_y + 3],
                                  fill=canvas.theme.foreground)
        y += height + gap

    def stepper_row(label, value, action):
        nonlocal y
        rect = (margin, y, canvas.width - 2 * margin, height)
        canvas.draw.rounded_rectangle(
            [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
            radius=9, outline=canvas.theme.mid, width=1)
        canvas.text((rect[0] + 14, y + 16), label, font=ctx.fonts["small"])
        button_size = 50
        plus_rect = (rect[0] + rect[2] - button_size - 8,
                     y + (height - button_size) // 2,
                     button_size, button_size)
        value_width = 78
        value_rect = (plus_rect[0] - value_width - 8,
                      y + (height - button_size) // 2,
                      value_width, button_size)
        minus_rect = (value_rect[0] - button_size - 8,
                      y + (height - button_size) // 2,
                      button_size, button_size)
        canvas.button(minus_rect, "-", font=ctx.fonts["body"])
        canvas.button(plus_rect, "+", font=ctx.fonts["body"])
        canvas.centered_text(str(value), ctx.fonts["small"],
                             value_rect[0] + value_rect[2] // 2,
                             value_rect[1] + value_rect[3] // 2)
        STATE["rects"][(action + "_down", 0)] = minus_rect
        STATE["rects"][(action + "_up", 0)] = plus_rect
        y += height + gap

    row("服务器（配置文件）", ctx.api.server, None)
    stepper_row("正文字号", str(ctx.config.get("font_size")), "font")
    stepper_row("行距", "%.2f" % float(ctx.config.get("line_spacing")), "spacing")
    row("夜间模式", "开" if ctx.config.get("night_mode") else "关", "night")
    row("首行缩进", "开" if ctx.config.get("first_line_indent") else "关", "indent")
    convert = ctx.config.get("convert")
    row("简繁转换", {"t2s": "繁转简", "s2t": "简转繁"}.get(convert, "关闭"), "convert")
    row("忽略日文", "开" if ctx.config.get("ignore_japanese") else "关", "ignore_japanese")
    row("忽略 AI", "开" if ctx.config.get("ignore_ai") else "关", "ignore_ai")
    row("翻页闪屏", "开" if ctx.config.get("page_flash") else "关", "flash")
    size_text = "%.1f MB" % (cache_size(CACHE_DIR) / 1024.0 / 1024.0)
    row("缓存", size_text, "clear_cache")
    row("退出登录" if ctx.api.user else "登录账号",
        (ctx.api.user or {}).get("UserName") or "未登录", "account")


def handle(data, ctx):
    if data.get("gesture") != "tap":
        return
    x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
    for key, rect in STATE["rects"].items():
        rx, ry, width, height = rect
        if not (rx <= x < rx + width and ry <= y < ry + height):
            continue
        action = key[0]
        if action in ("font_down", "font_up"):
            current = int(ctx.config.get("font_size") or 36)
            value = current + (-2 if action == "font_down" else 2)
            ctx.config.set("font_size", max(20, min(64, value)))
            ctx.show()
        elif action in ("spacing_down", "spacing_up"):
            current = float(ctx.config.get("line_spacing") or 1.42)
            value = current + (-0.05 if action == "spacing_down" else 0.05)
            ctx.config.set("line_spacing", round(max(1.0, min(2.0, value)), 2))
            ctx.show()
        elif action == "convert":
            values = [None, "t2s", "s2t"]
            current = ctx.config.get("convert")
            index = values.index(current) if current in values else 0
            ctx.config.set("convert", values[(index + 1) % len(values)])
            ctx.show()
        elif action in ("night", "indent", "flash", "ignore_japanese", "ignore_ai"):
            mapping = {"night": "night_mode", "indent": "first_line_indent", "flash": "page_flash"}
            key = mapping.get(action, action)
            ctx.config.set(key, not bool(ctx.config.get(key)))
            ctx.show()
        elif action == "clear_cache":
            ctx.confirm("确认清空封面、正文和字体的磁盘缓存？", lambda: _clear(ctx))
        elif action == "account":
            if ctx.api.user:
                ctx.navigate("account")
            else:
                ctx.navigate("account")
        return


def _clear(ctx):
    for name in ("covers", "fonts", "images", "content"):
        clear_cache(CACHE_DIR / name)
    ctx.images._memory.clear()
    ctx.toast("缓存已清空")


ABOUT_LINES = [
    "KinNovel 0.2.0",
    "LightNovelShelf 的 Kindle 客户端",
    "",
    "本项目使用 GPLv3 发布。",
    "复用 kComics 的 framebuffer、evdev 和 Pinyin 组件。",
    "",
    "LightNovelShelf 内容与接口归原站及其权利人所有。",
    "请遵守站点规则和内容版权。",
    "",
    "第三方组件: Pillow、lxml、evdev、KOReader。",
]


def render_about(ctx, canvas):
    top = canvas.header("关于", left="返回", right="主页")
    y = top + 30
    for line in ABOUT_LINES:
        canvas.text((int(canvas.width * 0.07), y), line,
                    font=ctx.fonts["small"], fill=canvas.theme.muted if not line else None)
        y += 48
