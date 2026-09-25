from page import keyboard

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

    row("服务器", ctx.api.server, "server")
    row("正文字号", str(ctx.config.get("font_size")), "font_size")
    row("行距", "%.2f" % float(ctx.config.get("line_spacing")), "line_spacing")
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
        if action == "server":
            keyboard.start(ctx.screen, ctx.fonts, capabilities=("en", "numsym"),
                           hint="请输入 API 服务器 URL", enter_label="保存",
                           owner="settings", on_submit=lambda text: _set_server(ctx, text))
            ctx.navigate("keyboard")
        elif action == "font_size":
            values = [26, 30, 34, 38, 42, 46, 50]
            current = int(ctx.config.get("font_size") or 34)
            next_value = min(values, key=lambda value: (abs(value - current), value > current))
            if next_value == current:
                next_value = values[(values.index(current) + 1) % len(values)] if current in values else 34
            ctx.config.set("font_size", next_value)
            ctx.show()
        elif action == "line_spacing":
            values = [1.20, 1.35, 1.42, 1.55, 1.70]
            current = float(ctx.config.get("line_spacing") or 1.42)
            next_value = values[(min(range(len(values)), key=lambda i: abs(values[i] - current)) + 1) % len(values)]
            ctx.config.set("line_spacing", next_value)
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


def _set_server(ctx, value):
    value = str(value or "").strip().rstrip("/")
    if not value.startswith(("http://", "https://")):
        ctx.message("服务器地址必须以 http:// 或 https:// 开头")
        return None
    ctx.api.set_server(value)
    ctx.toast("服务器已更新")
    return None


def _clear(ctx):
    for name in ("covers", "fonts", "images", "content"):
        clear_cache(CACHE_DIR / name)
    ctx.images._memory.clear()
    ctx.toast("缓存已清空")


ABOUT_LINES = [
    "KinNovel 0.1.0",
    "LightNovelShelf 的 Kindle 客户端",
    "",
    "本项目使用 GPLv3 发布。",
    "复用 kComics 的 framebuffer、evdev 和 Pinyin 组件。",
    "",
    "LightNovelShelf 内容与接口归原站及其权利人所有。",
    "请遵守站点规则和内容版权。",
    "",
    "第三方组件: Pillow、lxml、evdev、pinyin-data、KOReader。",
]


def render_about(ctx, canvas):
    top = canvas.header("关于", left="返回", right="主页")
    y = top + 30
    for line in ABOUT_LINES:
        canvas.text((int(canvas.width * 0.07), y), line,
                    font=ctx.fonts["small"], fill=canvas.theme.muted if not line else None)
        y += 48
