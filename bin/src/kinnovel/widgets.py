"""列表页共用的小部件: 命中测试、异步加载状态机、翻页条、空/错误态、行卡片。

抽出各列表页(rank/browse/history/series/announcements/shelf/account 的
notifications/shop)里重复的 generation/loading/error/分页 样板代码,
纯函数/小状态类, 复用已有的 Canvas API, 不依赖具体页面模块。
"""


def hit_test(rects, x, y):
    """在 rects(dict: key -> (x, y, w, h))中返回命中 (x, y) 的 key, 否则 None。"""
    for key, rect in rects.items():
        rx, ry, width, height = rect
        if rx <= x < rx + width and ry <= y < ry + height:
            return key
    return None


class ListLoader:
    """替代各列表页重复的 generation/loading/error 状态机。

    ``state`` 是页面模块自己的 STATE 字典, 复用其 loading/loaded/error/
    generation 字段。``start`` 每次调用都会递增 generation, 异步结果里
    generation 不匹配(被更新的请求顶替)时直接丢弃, 避免翻页/切换筛选
    时旧请求覆盖新状态。
    """

    def __init__(self, state):
        self.state = state
        state.setdefault("generation", 0)
        state.setdefault("loading", False)
        state.setdefault("loaded", False)
        state.setdefault("error", "")

    def start(self, ctx, owner_page, fetch, on_done=None, on_error=None,
              sticky=False):
        state = self.state
        state["generation"] += 1
        generation = state["generation"]
        state["loading"] = True
        state["error"] = ""

        def success(result):
            if generation != state["generation"]:
                return
            state["loading"] = False
            state["loaded"] = True
            state["error"] = ""
            if callable(on_done):
                on_done(result)

        def failure(exc):
            if generation != state["generation"]:
                return
            state["loading"] = False
            state["loaded"] = True
            state["error"] = str(exc)
            if callable(on_error):
                on_error(exc)

        if sticky:
            ctx.run_async(owner_page, fetch, success, failure, sticky=True)
        else:
            ctx.run_async(owner_page, fetch, success, failure)
        return generation

    def is_current(self, generation):
        return generation == self.state["generation"]


def list_row_height(ctx, min_px=96, frac=0.066):
    """列表行高: e-ink 触控目标 ~9mm, 按屏幕高度等比放大, 下限 ``min_px``。"""
    return max(int(min_px), int(ctx.height * frac))


def pager_height(ctx, min_px=88, frac=0.06):
    """翻页条高度, 同样保持 ~9mm 触控下限。"""
    return max(int(min_px), int(ctx.height * frac))


def pager_bar(canvas, ctx, rect, has_prev, has_next, label, rects,
              key_prefix=""):
    """绘制 上一页 / 页码 / 下一页。

    页码只是居中文字, 不是按钮(之前看起来像按钮却点不动, 这里不再注册
    它的命中区域)。只有 prev/next 两个 key 会写进 ``rects``。

    ``rect`` 为 (x, y, width, height); ``key_prefix`` 用于同一页面内
    多组翻页条(目前未用到, 保留扩展)。
    """
    x, y, width, height = rect
    nav_width = int(width * 0.25)
    prev_key = (key_prefix + "prev", 0)
    next_key = (key_prefix + "next", 0)
    prev_rect = (x, y, nav_width, height)
    next_rect = (x + width - nav_width, y, nav_width, height)
    canvas.button(prev_rect, "上一页", active=bool(has_prev), font=ctx.fonts["tiny"])
    canvas.button(next_rect, "下一页", active=bool(has_next), font=ctx.fonts["tiny"])
    rects[prev_key] = prev_rect
    rects[next_key] = next_rect
    canvas.centered_text(label, ctx.fonts["tiny"], x + width // 2, y + height // 2,
                         fill=canvas.theme.muted)
    return prev_rect, next_rect


def error_state(canvas, ctx, message, rects, retry_rect, retry_key=("retry", 0)):
    """错误提示 + 重试按钮, 并把重试按钮登记进 ``rects``。"""
    canvas.centered_text("加载失败", ctx.fonts["body"],
                         canvas.width // 2, canvas.height // 2 - 30)
    canvas.centered_text(str(message)[:40], ctx.fonts["tiny"],
                         canvas.width // 2, canvas.height // 2 + 30,
                         fill=canvas.theme.muted)
    canvas.button(retry_rect, "重试", font=ctx.fonts["small"])
    rects[retry_key] = retry_rect


def loading_state(canvas, ctx, text="加载中…"):
    canvas.centered_text(text, ctx.fonts["body"], canvas.width // 2, canvas.height // 2)


def empty_state(canvas, ctx, text):
    canvas.centered_text(text, ctx.fonts["body"], canvas.width // 2,
                         canvas.height // 2, fill=canvas.theme.muted)


def row_card(canvas, ctx, rect, title, subtitle="", highlight=False, wrap_title=False,
            index_label=None):
    """标题 + 副标题的通用行卡片; ``wrap_title`` 且行高足够时标题可换行到两行。

    ``index_label`` 用于排行榜那种左侧数字序号(居中画在行左边一小块)。
    """
    x, y, width, height = rect
    fill = canvas.theme.inverse_bg if highlight else canvas.theme.background
    text_fill = canvas.theme.inverse_fg if highlight else canvas.theme.foreground
    muted_fill = text_fill if highlight else canvas.theme.muted
    canvas.draw.rounded_rectangle([x, y, x + width, y + height], radius=8,
                                  outline=canvas.theme.mid, fill=fill, width=1)
    text_x = x + 14
    if index_label is not None:
        canvas.centered_text(str(index_label), ctx.fonts["body"], x + 38, y + height // 2,
                             fill=text_fill)
        text_x = x + 78
    max_width = x + width - text_x - 14

    title_font = ctx.fonts["small"]
    sub_font = ctx.fonts["tiny"]
    title_step = title_font.size + 6
    sub_height = (sub_font.size + 6) if subtitle else 0
    # 标题能放下两行时才换行, 否则退回单行省略号。
    lines = [title]
    if wrap_title and height >= 2 * title_step + sub_height + 8:
        lines = canvas.wrap(title, title_font, max_width)[:2] or [""]
        if len(canvas.wrap(title, title_font, max_width)) > 2:
            lines[-1] = canvas.fit_text(lines[-1] + "…", title_font, max_width)
    lines = [canvas.fit_text(line, title_font, max_width) for line in lines]
    # 文字块在行内垂直居中
    block = title_step * len(lines) - 6 + sub_height
    line_y = y + max(4, (height - block) // 2)
    for line in lines:
        canvas.text((text_x, line_y), line, font=title_font, fill=text_fill)
        line_y += title_step
    if subtitle:
        canvas.text((text_x, line_y),
                    canvas.fit_text(subtitle, sub_font, max_width),
                    font=sub_font, fill=muted_fill)
