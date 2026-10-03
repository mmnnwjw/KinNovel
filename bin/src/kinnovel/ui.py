import heapq
import os
import queue
import re
import ssl
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import OrderedDict
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps

from .config import CACHE_DIR, Config
from .reader import _INVISIBLE_RE, split_font_runs, text_width
from .utils import battery_level, prune_cache, touch


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
        text = _INVISIBLE_RE.sub("", str(text or ""))
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

    def text_fallback(self, xy, value, font, fallback_font, fill=None):
        x, y = xy
        color = self.theme.foreground if fill is None else fill
        for run, selected_font in split_font_runs(value, font, fallback_font):
            self.draw.text((x, y), run, font=selected_font, fill=color)
            x += text_width(self.draw, run, selected_font)

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
        status = time.strftime("%H:%M")
        level = battery_level()
        if level is not None:
            status += " · %d%%" % level
        status_bbox = self.draw.textbbox((0, 0), status, font=self.fonts["tiny"])
        status_width = status_bbox[2] - status_bbox[0]
        home_cx = self.width - max(34, height // 2)
        status_x = max(self.width // 2 + 60, home_cx - 36 - status_width)
        self.draw.text(
            (status_x, height // 2 - (status_bbox[3] - status_bbox[1]) // 2 - status_bbox[1]),
            status, font=self.fonts["tiny"], fill=self.theme.foreground)
        title_width = max(120, self.width - 360 - status_width)
        self.centered_text(self.fit_text(title, self.fonts["title"], title_width),
                           self.fonts["title"], self.width // 2, height // 2)
        if left:
            self._draw_back_icon(height)
        if right:
            self._draw_home_icon(height)
        self.header_state = {"height": height, "left": left, "right": right}
        return height

    def compact_header(self, title, progress=""):
        height = max(40, int(self.height * 0.035))
        self.draw.rectangle(
            [0, 0, self.width, height], fill=self.theme.background
        )
        status = time.strftime("%H:%M")
        level = battery_level()
        if level is not None:
            status += " · %d%%" % level
        font = self.fonts["tiny"]
        margin = max(10, int(self.width * 0.012))
        status_bbox = self.draw.textbbox((0, 0), status, font=font)
        status_width = status_bbox[2] - status_bbox[0]
        status_x = self.width - margin - status_width
        self.draw.text(
            (status_x, (height - font.size) // 2),
            status,
            font=font,
            fill=self.theme.foreground,
        )
        label = str(title or "阅读")
        if progress:
            label += "  " + str(progress)
        max_width = max(
            80, status_x - margin * 3
        )
        self.draw.text(
            (margin, (height - font.size) // 2),
            self.fit_text(label, font, max_width),
            font=font,
            fill=self.theme.foreground,
        )
        self.draw.line(
            [0, height - 1, self.width, height - 1],
            fill=self.theme.mid,
            width=1,
        )
        self.header_state = {"height": height, "left": "", "right": ""}
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


_IMAGE_HEIGHTS = (256, 384, 512, 768, 1024, 1536, 2048)
_SYSTEM_SIZE_RE = re.compile(r"^([1-9]\d*)x([1-9]\d*)$")
_IMAGE_MEMORY_BYTES = 48 * 1024 * 1024
_FITTED_MEMORY_BYTES = 8 * 1024 * 1024
_IMAGE_DOWNLOAD_LIMIT = 16 * 1024 * 1024


def height_bucket(height, fallback=1024):
    """Snap a requested pixel height to a stable cache/CDN bucket."""
    try:
        value = int(height)
    except (TypeError, ValueError):
        value = int(fallback or 1024)
    for bucket in _IMAGE_HEIGHTS:
        if value <= bucket:
            return bucket
    return _IMAGE_HEIGHTS[-1]


def system_image_size(url):
    """Return (width, height) for a LightNovelShelf ``size=WxH`` image URL."""
    if not url:
        return None
    try:
        pairs = urllib.parse.parse_qsl(
            urllib.parse.urlsplit(url).query, keep_blank_values=True)
    except ValueError:
        return None
    for key, value in pairs:
        if key == "size":
            match = _SYSTEM_SIZE_RE.match(value or "")
            if match:
                return int(match.group(1)), int(match.group(2))
    return None


def with_image_height(url, height):
    """Set the CDN ``height`` parameter, matching the Web client contract."""
    if not url:
        return url
    parts = urllib.parse.urlsplit(url)
    query = urllib.parse.parse_qsl(parts.query, keep_blank_values=True)
    wanted = str(int(height))
    replaced = False
    output = []
    for key, value in query:
        if key == "height":
            output.append((key, wanted))
            replaced = True
        else:
            output.append((key, value))
    if not replaced:
        output.append(("height", wanted))
    return urllib.parse.urlunsplit(
        (parts.scheme, parts.netloc, parts.path,
         urllib.parse.urlencode(output), parts.fragment))


def scaled_image_url(url, height=None):
    """Server-side resize request; only ``size=WxH`` system images qualify."""
    if not url or system_image_size(url) is None:
        return url
    return with_image_height(url, height_bucket(height) if height else 1024)


def _scaled_url(url):
    # 兼容旧调用；默认取 1024 高度
    return scaled_image_url(url, 1024)


class ImageCache:
    """Bounded disk + memory image cache.

    取代旧实现的“每张图一条 daemon 线程 + 一个 Python 子进程”模型：
    下载/解码统一走有界线程池，按 URL+尺寸变体去重，失败进入退避，
    内存按字节预算淘汰，避免整章插图预取把 Kindle 拖垮。
    """

    def __init__(self, maximum=24, max_bytes=_IMAGE_MEMORY_BYTES,
                 fitted_max_bytes=_FITTED_MEMORY_BYTES, workers=2):
        self.maximum = int(maximum)
        self.max_bytes = int(max_bytes)
        self.fitted_max_bytes = int(fitted_max_bytes)
        self._memory = OrderedDict()
        self._memory_bytes = 0
        self._fitted = OrderedDict()
        self._fitted_bytes = 0
        self._lock = threading.RLock()
        self._callbacks = {}
        self._queued = {}
        self._running = set()
        self._failures = {}
        self._sequence = 0
        self._jobs = queue.PriorityQueue()
        self._retry_heap = []
        self._retry_cv = threading.Condition()
        self._closed = False
        self._worker_count = max(1, min(4, int(workers or 1)))
        self._workers = []
        for index in range(self._worker_count):
            worker = threading.Thread(
                target=self._worker_loop, daemon=True,
                name="kinnovel-img-%d" % index)
            worker.start()
            self._workers.append(worker)
        self._scheduler = threading.Thread(
            target=self._retry_loop, daemon=True, name="kinnovel-img-retry")
        self._scheduler.start()

    def _worker_loop(self):
        while True:
            job = self._jobs.get()
            if job is None:
                return
            _priority, _seq, key, url, height, strict_tls, priority = job
            with self._lock:
                if self._queued.get(key) != (_priority, _seq):
                    # 已有更高优先级的请求取代了这条任务
                    continue
                self._queued.pop(key, None)
                self._running.add(key)
            try:
                self._download(url, key, height, strict_tls, priority)
            except Exception:
                pass
            finally:
                requeue = False
                with self._lock:
                    self._running.discard(key)
                    if self._callbacks.get(key) and not self.is_cached(url, height):
                        requeue = True
                if requeue:
                    with self._lock:
                        self._enqueue_locked(
                            url, key, height, strict_tls, priority)

    def close(self):
        with self._retry_cv:
            self._closed = True
            self._retry_cv.notify_all()
        for _ in self._workers:
            self._jobs.put(None)
        self._workers = []

    def _enqueue_locked(self, url, key, height, strict_tls, priority):
        """把一个 key 放进优先队列; 更高优先级会取代已在排队的旧任务。"""
        priority = int(priority)
        current = self._queued.get(key)
        if current is not None and current[0] <= priority:
            return False
        self._sequence += 1
        self._queued[key] = (priority, self._sequence)
        self._jobs.put((priority, self._sequence, key, url, height,
                        bool(strict_tls), priority))
        return True

    def _retry_loop(self):
        while True:
            with self._retry_cv:
                while not self._closed and not self._retry_heap:
                    self._retry_cv.wait(1.0)
                if self._closed:
                    return
                due = self._retry_heap[0][0]
                now = time.monotonic()
                if due > now:
                    self._retry_cv.wait(min(due - now, 5.0))
                    continue
                (_due, _seq, url, _key, height, strict_tls,
                 priority, callbacks) = heapq.heappop(self._retry_heap)
            for callback in callbacks:
                self.prefetch(url, strict_tls, height=height,
                              callback=callback, priority=priority, retry=True)

    # ---- keys / paths -------------------------------------------------
    @staticmethod
    def _cache_key(url, height=None):
        if height is None:
            return str(url)
        return "%s#h=%d" % (url, height_bucket(height))

    def _path(self, url, height=None):
        from .utils import stable_cache_name
        return CACHE_DIR / "covers" / (
            stable_cache_name(self._cache_key(url, height)) + ".jpg")

    # ---- memory accounting -------------------------------------------
    @staticmethod
    def _image_bytes(image):
        try:
            return max(1, int(image.width) * int(image.height)
                       * max(1, len(image.getbands())))
        except Exception:
            return 1

    def _remember(self, key, image):
        size = self._image_bytes(image)
        with self._lock:
            old = self._memory.pop(key, None)
            if old is not None:
                self._memory_bytes -= self._image_bytes(old)
            self._memory[key] = image
            self._memory_bytes += size
            while self._memory and (
                    len(self._memory) > self.maximum
                    or self._memory_bytes > self.max_bytes):
                _, evicted = self._memory.popitem(last=False)
                self._memory_bytes -= self._image_bytes(evicted)
            self._memory_bytes = max(0, self._memory_bytes)

    def clear_memory(self):
        with self._lock:
            self._memory.clear()
            self._memory_bytes = 0
            self._fitted.clear()
            self._fitted_bytes = 0

    def _lookup(self, key, url):
        with self._lock:
            image = self._memory.get(key)
            if image is not None:
                self._memory.move_to_end(key)
                return image
            if key != url:
                image = self._memory.get(url)
                if image is not None:
                    self._memory.move_to_end(url)
                    return image
        return None

    # ---- public API ---------------------------------------------------
    def get(self, url, height=None):
        if not url:
            return None
        key = self._cache_key(url, height)
        image = self._lookup(key, url)
        if image is not None:
            return image
        path = self._path(url, height)
        if not path.exists():
            legacy = self._path(url)
            if legacy.exists():
                path = legacy
            else:
                return None
        try:
            with Image.open(path) as handle:
                handle.load()
                image = handle.convert("L")
        except (OSError, ValueError):
            # 损坏的缓存文件必须删除，否则会永远“粘住”不再重下
            try:
                path.unlink()
            except OSError:
                pass
            return None
        touch(path)
        self._remember(key, image)
        return image

    def is_cached(self, url, height=None):
        if not url:
            return False
        key = self._cache_key(url, height)
        with self._lock:
            if key in self._memory or url in self._memory:
                return True
        return self._path(url, height).exists() or self._path(url).exists()

    def prefetch(self, url, strict_tls=False, height=None, callback=None,
                 priority=0, retry=False):
        """Schedule one download; never blocks, never raises.

        ``priority`` 越小越紧急：0 = 当前可见，3 = 邻近页预取，6 = 后台。
        返回 True 表示已缓存/已排队，False 表示该 URL 正处于失败退避期。
        """
        if not url:
            return False
        key = self._cache_key(url, height)
        if self.is_cached(url, height):
            if callable(callback):
                callback(True)
            return True
        now = time.monotonic()
        with self._lock:
            failure = self._failures.get(key)
            if failure is not None and now < failure[1] and not retry:
                return False
            if callable(callback):
                self._callbacks.setdefault(key, []).append(callback)
            if key in self._running:
                return True
            self._enqueue_locked(url, key, height, strict_tls, priority)
        return True

    # ---- worker -------------------------------------------------------
    def _download(self, url, key, height, strict_tls, priority=0):
        ok = False
        image = None
        try:
            image = self._download_image(
                scaled_image_url(url, height), height, strict_tls)
            if image is not None:
                self._save(self._path(url, height), image)
                ok = True
        except Exception:
            ok = False
        with self._lock:
            if ok and image is not None:
                self._remember(key, image)
                self._failures.pop(key, None)
            else:
                count, _ = self._failures.get(key, (0, 0.0))
                count += 1
                self._failures[key] = (
                    count,
                    time.monotonic() + min(300.0, float(2 ** min(count, 8))),
                )
                if len(self._failures) > 512:
                    self._failures.clear()
            callbacks = self._callbacks.pop(key, [])
            if not ok and callbacks and count <= 6:
                # 失败不要立刻回调整屏重绘(会再次请求),而是按退避定时重试
                with self._retry_cv:
                    self._sequence += 1
                    heapq.heappush(
                        self._retry_heap,
                        (self._failures[key][1], self._sequence, url, key,
                         height, bool(strict_tls),
                         min(int(priority), 3), callbacks))
                    self._retry_cv.notify()
                callbacks = []
        for callback in callbacks:
            try:
                callback(ok)
            except Exception:
                pass

    def _download_image(self, url, height, strict_tls):
        request = urllib.request.Request(
            url, headers={"User-Agent": "KinNovel/0.7"})
        context = None
        if url.startswith("https://"):
            context = ssl.create_default_context()
            if not strict_tls:
                context.check_hostname = False
                context.verify_mode = ssl.CERT_NONE
        with urllib.request.urlopen(
                request, timeout=20, context=context) as response:
            data = response.read(_IMAGE_DOWNLOAD_LIMIT + 1)
        if not data or len(data) > _IMAGE_DOWNLOAD_LIMIT:
            return None
        from io import BytesIO
        target = height_bucket(height) if height else 0
        with Image.open(BytesIO(data)) as handle:
            if target and (handle.format or "").upper() == "JPEG":
                try:
                    handle.draft("L", (target, target))
                except Exception:
                    pass
            handle.load()
            image = handle.convert("L")
        if target:
            longest = max(image.width, image.height)
            if longest > target:
                ratio = target / float(longest)
                image = image.resize(
                    (max(1, int(image.width * ratio)),
                     max(1, int(image.height * ratio))),
                    Image.Resampling.LANCZOS)
        return image

    @staticmethod
    def _save(path, image):
        path.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temp_name = tempfile.mkstemp(
            dir=str(path.parent), prefix=path.name, suffix=".tmp")
        temp = Path(temp_name)
        try:
            os.close(descriptor)
            image.save(temp, "JPEG", quality=85)
            os.replace(temp, path)
        except BaseException:
            try:
                temp.unlink()
            except OSError:
                pass
            raise

    def _fitted_image(self, key, image, width, height):
        with self._lock:
            fitted = self._fitted.get(key)
            if fitted is not None:
                self._fitted.move_to_end(key)
                return fitted
        fitted = ImageOps.fit(
            image, (max(1, int(width)), max(1, int(height))),
            method=Image.Resampling.LANCZOS)
        size = self._image_bytes(fitted)
        with self._lock:
            previous = self._fitted.pop(key, None)
            if previous is not None:
                self._fitted_bytes -= self._image_bytes(previous)
            self._fitted[key] = fitted
            self._fitted_bytes += size
            while self._fitted and self._fitted_bytes > self.fitted_max_bytes:
                _, evicted = self._fitted.popitem(last=False)
                self._fitted_bytes -= self._image_bytes(evicted)
            self._fitted_bytes = max(0, self._fitted_bytes)
        return fitted

    def cover(self, url, width, height, strict_tls=False, fetch=False,
              request_height=None):
        target = request_height or height_bucket(height)
        image = self.get(url, target)
        if image is None and fetch:
            self.prefetch(url, strict_tls=strict_tls, height=target)
            image = self.get(url, target)
        if image is None:
            image = self.get(url)
        if image is None:
            return None
        return self._fitted_image(
            (url, target, int(width), int(height)), image, width, height)


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
        self.previous_page = None
        self.params = {}
        self.modal = None
        self.status = ""
        self._header_state = None
        self._last_back_at = 0.0
        self._show_lock = threading.RLock()
        self._closed = False
        self._ui_queue = queue.Queue()
        self._ui_loop_running = False
        self._refresh_requested = False

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
        self.previous_page = self.page_name
        self.page_name = name
        self.params = dict(params)
        self.modal = None
        _call_enter(self.pages[name], self)
        self.show()

    def replace(self, name, **params):
        self.previous_page = self.page_name
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
        now = time.monotonic()
        if now - self._last_back_at < 0.35:
            return
        self._last_back_at = now
        if self.stack:
            self.previous_page = self.page_name
            self.page_name, self.params = self.stack.pop()
        else:
            self.previous_page = self.page_name
            self.page_name, self.params = "home", {}
        _call_enter(self.pages[self.page_name], self)
        self.show()

    def home(self):
        self.stack = []
        self.previous_page = self.page_name
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

    def show(self, is_flashing=None, force=False, region=None,
             waveform=None, dither=False):
        # 页面自己渲染过一次后，就不再重复消费合并刷新标记
        self._refresh_requested = False
        if not force:
            power = getattr(self.app, "power", None)
            if power is not None and getattr(power, "is_sleeping", False):
                return
        if self._closed:
            return
        with self._show_lock:
            try:
                image = self.render()
            except Exception:
                logger = getattr(self.app, "log", None)
                if callable(logger):
                    import traceback
                    logger(traceback.format_exc())
                image = self._fallback_image()
            flashing = bool(self.config.get("page_flash")) if is_flashing is None else bool(is_flashing)
            extra = {}
            if region:
                extra["region"] = region
            if waveform is not None:
                extra["waveform_mode"] = waveform
            if dither:
                extra["dither"] = True
            try:
                self.screen.output.show(image, is_flashing=flashing, **extra)
            except OSError:
                pass

    def _fallback_image(self):
        """渲染失败时给出可恢复的错误页,而不是让异常冒泡终止应用。"""
        image = Image.new("L", (self.width, self.height), 255)
        draw = ImageDraw.Draw(image)
        for offset, text in enumerate(("页面渲染失败", "请查看 logs/kinnovel.log")):
            try:
                font = self.fonts.get("body")
                bbox = draw.textbbox((0, 0), text, font=font)
                draw.text(((self.width - (bbox[2] - bbox[0])) // 2,
                           self.height // 2 + offset * 60 - bbox[1]),
                          text, font=font, fill=0)
            except Exception:
                draw.text((20, self.height // 2 + offset * 20), text, fill=0)
        return image

    def post(self, callback):
        """Run ``callback`` on the input thread, or inline when no loop runs."""
        if self._ui_loop_running:
            self._ui_queue.put(callback)
        else:
            callback()

    def request_show(self):
        """Coalesce redraw requests so a burst of image arrivals shows once."""
        if self._ui_loop_running:
            self._refresh_requested = True
        else:
            self.show()

    def drain_ui_queue(self, limit=64):
        for _ in range(limit):
            try:
                callback = self._ui_queue.get_nowait()
            except queue.Empty:
                break
            try:
                callback()
            except Exception:
                logger = getattr(self.app, "log", None)
                if callable(logger):
                    import traceback
                    logger(traceback.format_exc())
        if self._refresh_requested:
            self._refresh_requested = False
            self.show()

    def handle(self, data):
        gesture = data.get("gesture")
        if gesture not in ("tap", "long", "down"):
            return None
        if gesture in ("tap", "long"):
            x = int(data.get("x-pixel") or 0)
            y = int(data.get("y-pixel") or 0)
            if x == 0 and y == 0:
                return None
            if x < 0 or y < 0 or x >= self.width or y >= self.height:
                return None
        if self.modal:
            if gesture != "tap":
                return None
            self._handle_modal(data)
            return None
        if gesture == "tap" and self._header_state:
            header_height = int(self._header_state.get("height") or 0)
            if y < header_height:
                blocker = getattr(self.pages[self.page_name], "header_blocked", None)
                if callable(blocker) and blocker():
                    return None
                if (x < int(self.width * 0.16)
                        and self._header_state.get("left")):
                    callback = getattr(self.pages[self.page_name], "upload_progress", None)
                    if callable(callback):
                        callback(self)
                    self.back()
                    return None
                if (x > int(self.width * 0.84)
                        and self._header_state.get("right") == "主页"):
                    callback = getattr(self.pages[self.page_name], "upload_progress", None)
                    if callable(callback):
                        callback(self)
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
        self._toast_generation = getattr(self, "_toast_generation", 0) + 1
        generation = self._toast_generation
        self.show()

        def clear():
            time.sleep(seconds)
            if generation == getattr(self, "_toast_generation", 0):
                self.status = ""
                self.show()
        threading.Thread(target=clear, daemon=True).start()

    def confirm(self, lines, on_yes, on_no=None, yes="确定", no="取消"):
        # The popup rects are computed during render, so store the desired actions.
        if isinstance(lines, str):
            lines = [lines]
        self.modal = {"lines": list(lines), "buttons": [yes, no],
                      "actions": {yes: on_yes, no: on_no}}
        self.show()

    def message(self, lines, close_label="确定", on_close=None):
        if isinstance(lines, str):
            lines = [lines]
        self.modal = {"lines": lines, "buttons": [close_label],
                      "actions": {close_label: on_close}}
        self.show()

    def run_async(self, owner_page, operation, on_success=None,
                  on_error=None, refresh=True, sticky=False):
        def worker():
            try:
                result = operation()
            except Exception as exc:
                if (sticky or self.page_name == owner_page) and not self._closed:
                    def failed(error=exc):
                        try:
                            if callable(on_error):
                                on_error(error)
                            else:
                                self.message("操作失败", on_close=None)
                        except Exception as callback_error:
                            self.message(["操作失败", str(callback_error)])
                    self.post(failed)
                return
            if (sticky or self.page_name == owner_page) and not self._closed:
                def finished(value=result):
                    try:
                        if callable(on_success):
                            on_success(value)
                    except Exception as callback_error:
                        self.message(["界面更新失败", str(callback_error)])
                        return
                    if refresh:
                        self.request_show()
                self.post(finished)
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
