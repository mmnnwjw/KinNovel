import io
import threading
import time
import urllib.error
import urllib.request
from collections import OrderedDict
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps

from .config import CACHE_DIR, Config
from .utils import prune_cache, safe_filename


class Theme:
    def __init__(self, night=False):
        self.night = bool(night)
        self.background = 0 if self.night else 255
        self.foreground = 255 if self.night else 0
        self.muted = 165 if self.night else 105
        self.light = 40 if self.night else 225
        self.mid = 85 if self.night else 170
        self.inverse_fg = self.background
        self.inverse_bg = self.foreground


class Canvas:
    def __init__(self, image, fonts, theme):
        self.image = image
        self.draw = ImageDraw.Draw(image)
        self.fonts = fonts
        self.theme = theme
        self.width, self.height = image.size

    def centered(self, text, font, cx, cy):
        bbox = self.draw.textbbox((0, 0), str(text), font=font)
        return (cx - (bbox[2] - bbox[0]) // 2 - bbox[0],
                cy - (bbox[3] - bbox[1]) // 2 - bbox[1])

    def fit_text(self, text, font, max_width):
        text = str(text or "")
        if self.draw.textlength(text, font=font) <= max_width:
            return text
        suffix = "…"
        while text and self.draw.textlength(text + suffix, font=font) > max_width:
            text = text[:-1]
        return text + suffix

    def wrap(self, text, font, max_width):
        text = str(text or "")
        output = []
        for paragraph in text.splitlines() or [""]:
            current = ""
            for char in paragraph:
                candidate = current + char
                if current and self.draw.textlength(candidate, font=font) > max_width:
                    output.append(current)
                    current = char
                else:
                    current = candidate
            output.append(current)
        return output

    def text(self, xy, value, font=None, fill=None, anchor=None):
        self.draw.text(xy, str(value), font=font or self.fonts["body"],
                       fill=self.theme.foreground if fill is None else fill, anchor=anchor)

    def centered_text(self, value, font, cx, cy, fill=None):
        self.draw.text(self.centered(value, font, cx, cy), str(value),
                       font=font, fill=self.theme.foreground if fill is None else fill)

    def button(self, rect, label, active=True, font=None):
        x, y, width, height = rect
        fill = self.theme.inverse_bg if active else self.theme.background
        text_fill = self.theme.inverse_fg if active else self.theme.muted
        self.draw.rounded_rectangle([x, y, x + width, y + height],
                                    radius=10, outline=self.theme.foreground,
                                    fill=fill, width=2)
        self.centered_text(label, font or self.fonts["small"],
                           x + width // 2, y + height // 2, fill=text_fill)

    def header(self, title, left="返回", right="主页"):
        height = max(72, int(self.height * 0.085))
        self.draw.rectangle([0, 0, self.width, height], fill=self.theme.light)
        self.centered_text(self.fit_text(title, self.fonts["title"], self.width - 320),
                           self.fonts["title"], self.width // 2, height // 2)
        self._draw_back_icon(height)
        self._draw_home_icon(height)
        self.header_state = {"height": height, "left": left, "right": right}
        return height

    def _draw_back_icon(self, header_height):
        color = self.theme.foreground
        cx = max(34, header_height // 2)
        cy = header_height // 2
        size = max(14, min(24, header_height // 4))
        self.draw.line([cx + size, cy, cx - size, cy], fill=color, width=5)
        self.draw.line([cx + size, cy, cx, cy - size], fill=color, width=5)
        self.draw.line([cx + size, cy, cx, cy + size], fill=color, width=5)

    def _draw_home_icon(self, header_height):
        color = self.theme.foreground
        cx = self.width - max(34, header_height // 2)
        cy = header_height // 2
        size = max(13, min(22, header_height // 4))
        roof_y = cy - size
        wall_y = cy + size
        self.draw.line([cx - size, cy, cx, roof_y], fill=color, width=5)
        self.draw.line([cx, roof_y, cx + size, cy], fill=color, width=5)
        self.draw.line([cx - size + 3, cy, cx - size + 3, wall_y],
                       fill=color, width=4)
        self.draw.line([cx + size - 3, cy, cx + size - 3, wall_y],
                       fill=color, width=4)
        self.draw.line([cx - size + 3, wall_y, cx + size - 3, wall_y],
                       fill=color, width=4)

    def separator(self, y):
        self.draw.line([0, y, self.width, y], fill=self.theme.mid, width=1)

    def popup(self, lines, buttons=None):
        width = int(self.width * 0.78)
        line_height = max(self.fonts["small"].size + 10, 52)
        buttons = buttons or []
        height = max(180, 72 + line_height * max(1, len(lines)) + 70 * bool(buttons))
        x = (self.width - width) // 2
        y = (self.height - height) // 2
        self.draw.rounded_rectangle([x, y, x + width, y + height],
                                    radius=18, fill=self.theme.background,
                                    outline=self.theme.foreground, width=3)
        text_y = y + 46
        for line in lines:
            for wrapped in self.wrap(line, self.fonts["small"], width - 40):
                self.centered_text(wrapped, self.fonts["small"],
                                   self.width // 2, text_y)
                text_y += line_height
        return self._popup_buttons(x, y, width, height, buttons)

    def _popup_buttons(self, x, y, width, height, buttons):
        if not buttons:
            return []
        gap = 16
        button_width = int((width - 32 - gap * (len(buttons) - 1)) / len(buttons))
        button_height = 52
        bx = x + 16
        by = y + height - button_height - 16
        rects = []
        for label in buttons:
            rect = (bx, by, button_width, button_height)
            self.button(rect, label, active=True, font=self.fonts["body"])
            rects.append((label, rect))
            bx += button_width + gap
        return rects


class ImageCache:
    def __init__(self, maximum=24):
        self.maximum = maximum
        self._memory = OrderedDict()
        self._lock = threading.RLock()

    def _path(self, url):
        suffix = ".img"
        for extension in (".jpg", ".jpeg", ".png", ".webp", ".gif", ".ttf", ".otf"):
            if url.lower().split("?", 1)[0].endswith(extension):
                suffix = extension
                break
        from .utils import stable_cache_name
        return CACHE_DIR / "covers" / (stable_cache_name(url) + suffix)

    def get(self, url):
        if not url:
            return None
        with self._lock:
            cached = self._memory.get(url)
            if cached is not None:
                self._memory.move_to_end(url)
                return cached
        path = self._path(url)
        try:
            image = Image.open(path)
            image.load()
            image = image.convert("L")
        except (OSError, ValueError):
            return None
        with self._lock:
            self._memory[url] = image
            self._memory.move_to_end(url)
            while len(self._memory) > self.maximum:
                self._memory.popitem(last=False)
        return image

    def prefetch(self, url, strict_tls=False):
        if not url or self.get(url) is not None:
            return
        path = self._path(url)
        if path.exists() and path.stat().st_size:
            return
        context = None
        if url.startswith("https://"):
            import ssl
            context = ssl.create_default_context()
            if not strict_tls:
                context.check_hostname = False
                context.verify_mode = ssl.CERT_NONE
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "KinNovel/0.1"})
            with urllib.request.urlopen(request, timeout=12, context=context) as response:
                data = response.read(8 * 1024 * 1024)
            path.parent.mkdir(parents=True, exist_ok=True)
            temp = path.with_suffix(path.suffix + ".tmp")
            with temp.open("wb") as handle:
                handle.write(data)
            temp.replace(path)
        except (OSError, urllib.error.URLError, ValueError):
            return

    def cover(self, url, width, height, strict_tls=False, fetch=False):
        image = self.get(url)
        if image is None and fetch:
            self.prefetch(url, strict_tls=strict_tls)
            image = self.get(url)
        if image is None:
            return None
        return ImageOps.fit(image, (max(1, int(width)), max(1, int(height))),
                            method=Image.Resampling.LANCZOS)


class PageContext:
    def __init__(self, app):
        self.app = app
        self.screen = app.screen
        self.fonts = app.fonts
        self.config = app.config
        self.api = app.api
        self.images = app.images
        self.pages = {}
        self.stack = []
        self.page_name = "home"
        self.params = {}
        self.modal = None
        self.status = ""
        self._header_state = None
        self._show_lock = threading.RLock()
        self._closed = False

    @property
    def width(self):
        return self.screen.output.resolution[0]

    @property
    def height(self):
        return self.screen.output.resolution[1]

    def register(self, name, module):
        self.pages[name] = module

    def navigate(self, name, push=True, **params):
        if name not in self.pages:
            raise KeyError("unknown page: " + name)
        if push and name != self.page_name:
            self.stack.append((self.page_name, self.params))
            if len(self.stack) > 20:
                self.stack.pop(0)
        self.page_name = name
        self.params = dict(params)
        self.modal = None
        _call_enter(self.pages[name], self)
        self.show()

    def replace(self, name, **params):
        self.page_name = name
        self.params = dict(params)
        self.modal = None
        _call_enter(self.pages[name], self)
        self.show()

    def back(self):
        if self.modal:
            self.modal = None
            self.show()
            return
        if self.stack:
            self.page_name, self.params = self.stack.pop()
        else:
            self.page_name, self.params = "home", {}
        _call_enter(self.pages[self.page_name], self)
        self.show()

    def home(self):
        self.stack = []
        self.page_name = "home"
        self.params = {}
        self.modal = None
        _call_enter(self.pages["home"], self)
        self.show()

    def render(self):
        theme = Theme(self.config.get("night_mode"))
        image = Image.new("L", (self.width, self.height), theme.background)
        canvas = Canvas(image, self.fonts, theme)
        page = self.pages[self.page_name]
        page.render(self, canvas)
        self._header_state = getattr(canvas, "header_state", None)
        if self.status:
            width = min(self.width - 80, max(300, len(self.status) * 24))
            x = (self.width - width) // 2
            y = self.height - 90
            canvas.draw.rounded_rectangle([x, y, x + width, y + 64],
                                          radius=14, fill=theme.background,
                                          outline=theme.foreground, width=2)
            canvas.centered_text(self.status, self.fonts["small"],
                                 self.width // 2, y + 32)
        if self.modal:
            self.modal["rects"] = canvas.popup(
                self.modal.get("lines") or [],
                self.modal.get("buttons") or [],
            )
        return image

    def show(self):
        if self._closed:
            return
        with self._show_lock:
            image = self.render()
            try:
                self.screen.output.show(image, is_flashing=bool(self.config.get("page_flash")))
            except OSError:
                pass

    def handle(self, data):
        gesture = data.get("gesture")
        if gesture not in ("tap", "long"):
            return None
        if self.modal:
            if gesture != "tap":
                return None
            self._handle_modal(data)
            return None
        if gesture == "tap" and self._header_state:
            x = int(data.get("x-pixel") or 0)
            y = int(data.get("y-pixel") or 0)
            header_height = int(self._header_state.get("height") or 0)
            if y < header_height:
                if x < int(self.width * 0.22):
                    self.back()
                    return None
                if x > int(self.width * 0.78) and self._header_state.get("right") == "主页":
                    self.home()
                    return None
        return self.pages[self.page_name].handle(data, self)

    def _handle_modal(self, data):
        x, y = int(data.get("x-pixel") or 0), int(data.get("y-pixel") or 0)
        buttons = self.modal.get("rects") or []
        for label, (bx, by, width, height) in buttons:
            if bx <= x < bx + width and by <= y < by + height:
                action = self.modal.get("actions", {}).get(label)
                self.modal = None
                if callable(action):
                    action()
                self.show()
                return

    def toast(self, message, seconds=2.0):
        self.status = str(message)
        self.show()

        def clear():
            time.sleep(seconds)
            self.status = ""
            self.show()
        threading.Thread(target=clear, daemon=True).start()

    def confirm(self, title, on_yes, on_no=None, yes="确定", no="取消"):
        # The popup rects are computed during render, so store the desired actions.
        self.modal = {"lines": [title], "buttons": [yes, no],
                      "actions": {yes: on_yes, no: on_no}}
        self.show()

    def message(self, lines, close_label="确定", on_close=None):
        if isinstance(lines, str):
            lines = [lines]
        self.modal = {"lines": lines, "buttons": [close_label],
                      "actions": {close_label: on_close}}
        self.show()

    def run_async(self, owner_page, operation, on_success=None, on_error=None):
        def worker():
            try:
                result = operation()
            except Exception as exc:
                if self.page_name == owner_page:
                    try:
                        if callable(on_error):
                            on_error(exc)
                        else:
                            self.message("操作失败", on_close=None)
                        self.show()
                    except Exception as callback_error:
                        self.message(["操作失败", str(callback_error)])
                return
            if self.page_name == owner_page:
                try:
                    if callable(on_success):
                        on_success(result)
                    self.show()
                except Exception as callback_error:
                    self.message(["界面更新失败", str(callback_error)])
        threading.Thread(target=worker, daemon=True).start()

    def prune_cache(self):
        limit = int(self.config.get("cache_limit_mb") or 192) * 1024 * 1024
        per_directory = max(1, limit // 4)
        removed = 0
        for name in ("covers", "fonts", "images", "content"):
            removed += prune_cache(CACHE_DIR / name, per_directory)
        return removed


def _call_enter(module, context):
    enter = getattr(module, "enter", None)
    if callable(enter):
        enter(context)
