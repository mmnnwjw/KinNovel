import time

from .. import VERSION
from ..config import CACHE_DIR
from ..utils import cache_size, clear_cache


STATE = {
    "rects": {}, "server": "",
    "cache_text": "", "cache_loaded_at": 0.0, "cache_loading": False,
}
_CACHE_TTL = 60.0


def _cache_text():
    now = time.monotonic()
    if (STATE["cache_text"]
            and now - float(STATE.get("cache_loaded_at") or 0.0) < _CACHE_TTL):
        return STATE["cache_text"]
    return STATE["cache_text"] or "统计中…"


def _refresh_cache_size(ctx, force=False):
    now = time.monotonic()
    if not force and STATE["cache_text"] and (
            now - float(STATE.get("cache_loaded_at") or 0.0) < _CACHE_TTL):
        return
    if STATE.get("cache_loading"):
        return
    STATE["cache_loading"] = True

    def operation():
        return cache_size(CACHE_DIR)

    def success(size):
        STATE["cache_text"] = "%.1f MB" % (float(size) / 1024.0 / 1024.0)
        STATE["cache_loaded_at"] = time.monotonic()
        STATE["cache_loading"] = False

    def error(_exc):
        STATE["cache_loading"] = False
        if not STATE["cache_text"]:
            STATE["cache_text"] = "统计失败"

    try:
        ctx.run_async("settings", operation, success, error, refresh=False, sticky=True)
    except TypeError:
        ctx.run_async("settings", operation, success, error)


def enter(ctx):
    _refresh_cache_size(ctx)


def render(ctx, canvas):
    top = canvas.header("设置", left="返回", right="主页")
    margin = int(canvas.width * 0.05)
    y = top + 18
    # 行高随屏幕高度自适应,低分辨率设备(如 600x800)不溢出
    row_count = 14
    available = canvas.height - top - 30
    gap = 14 if available >= 13 * 64 + 12 * 14 else 4
    height = max(40, min(64, (available - gap * (row_count - 1)) // row_count))

    def row(label, value=None, action=None, progress=None):
        nonlocal y
        rect = (margin, y, canvas.width - 2 * margin, height)
        STATE["rects"][(action or label, 0)] = rect
        canvas.draw.rounded_rectangle([rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
                                      radius=9, outline=canvas.theme.mid, width=1)
        canvas.text((rect[0] + 14, y + max(4, (height - ctx.fonts["small"].size) // 2)),
                    label, font=ctx.fonts["small"])
        if value is not None:
            text = canvas.fit_text(str(value), ctx.fonts["tiny"], rect[2] // 2 - 20)
            bbox = canvas.draw.textbbox((0, 0), text, font=ctx.fonts["tiny"])
            canvas.text((rect[0] + rect[2] - (bbox[2] - bbox[0]) - 14,
                         y + max(4, (height - ctx.fonts["tiny"].size) // 2)),
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
        canvas.text((rect[0] + 14, y + max(4, (height - ctx.fonts["small"].size) // 2)),
                    label, font=ctx.fonts["small"])
        button_size = min(50, height - 8)
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

    row("服务器", ctx.api.server, None)
    stepper_row("正文字号", str(ctx.config.get("font_size")), "font")
    stepper_row("行距", "%.2f" % float(ctx.config.get("line_spacing")), "spacing")
    row("夜间模式", "开" if ctx.config.get("night_mode") else "关", "night")
    row("首行缩进", "开" if ctx.config.get("first_line_indent") else "关", "indent")
    convert = ctx.config.get("convert")
    row("简繁转换", {"t2s": "繁转简", "s2t": "简转繁"}.get(convert, "关闭"), "convert")
    row("忽略日文", "开" if ctx.config.get("ignore_japanese") else "关", "ignore_japanese")
    row("忽略 AI", "开" if ctx.config.get("ignore_ai") else "关", "ignore_ai")
    row("预加载章节", "开" if ctx.config.get("prefetch_chapters") else "关", "prefetch")
    row("详情页预热", "开" if ctx.config.get("prefetch_reading_target") else "关",
        "prefetch_target")
    animation_supported = bool(
        getattr(ctx.screen.output, "supports_swipe_animation", False)
    )
    row(
        "翻页动画（仅较新型号支持）",
        ("开" if ctx.config.get("page_turn_animation") else "关")
        if animation_supported else "不支持",
        "animation" if animation_supported else None,
    )
    row("翻页闪屏", "开" if ctx.config.get("page_flash") else "关", "flash")
    row("缓存", _cache_text(), "clear_cache")
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
        elif action in ("night", "indent", "flash", "ignore_japanese",
                        "ignore_ai", "prefetch", "animation"):
            mapping = {"night": "night_mode", "indent": "first_line_indent",
                       "flash": "page_flash", "prefetch": "prefetch_chapters",
                       "animation": "page_turn_animation"}
            key = mapping.get(action, action)
            ctx.config.set(key, not bool(ctx.config.get(key)))
            ctx.show()
        elif action == "prefetch_target":
            if ctx.config.get("prefetch_reading_target"):
                ctx.config.set("prefetch_reading_target", False)
                ctx.show()
            else:
                ctx.confirm(
                    "开启后打开书籍详情会预取章节内容，可能产生额外阅读记录。确定开启？",
                    lambda: (ctx.config.set("prefetch_reading_target", True),
                             ctx.show()),
                )
        elif action == "clear_cache":
            ctx.confirm("确认清空封面、正文和字体的磁盘缓存？", lambda: _clear(ctx))
        elif action == "account":
            if ctx.api.user:
                ctx.confirm("确认退出登录？", lambda: _logout(ctx))
            else:
                ctx.navigate("account")
        return


def _logout(ctx):
    ctx.api.session.clear_credentials()
    try:
        ctx.api.hub.close()
    except Exception:
        pass
    ctx.toast("已退出登录")
    ctx.show()


def _clear(ctx):
    for name in ("covers", "fonts", "images", "content"):
        clear_cache(CACHE_DIR / name)
    ctx.images.clear_memory()
    STATE["cache_text"] = ""
    STATE["cache_loaded_at"] = 0.0
    _refresh_cache_size(ctx, force=True)
    ctx.toast("缓存已清空")


ABOUT_LINES = [
    "KinNovel " + VERSION,
    "运行于 Kindle 原生系统的轻书架客户端",
    "",
    "开发参考",
    "LightNovelShelf/Web",
    "接口、业务逻辑、阅读器行为、章节字体机制",
    "kComics",
    "framebuffer、EPDC、evdev、启动和恢复流程",
    "KOReader",
    "休眠与电源事件调度、内置 FreeType 与 WOFF2 字体支持",
    "FBInk",
    "各代 EPDC 波形、ioctl 结构体与刷新区域对齐规范",
    "",
    "主要依赖",
    "Python 3.14、Pillow、lxml、python-evdev",
    "",
    "本项目按 GPLv3 发布。",
    "LightNovelShelf 内容与接口归原站及其权利人所有。",
    "请遵守站点规则和内容版权。",
]


def render_about(ctx, canvas):
    top = canvas.header("关于", left="返回", right="主页")
    y = top + 30
    for line in ABOUT_LINES:
        canvas.text((int(canvas.width * 0.07), y), line,
                    font=ctx.fonts["small"], fill=canvas.theme.muted if not line else None)
        y += 48
