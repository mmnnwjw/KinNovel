#!/usr/bin/env python3
import os
import signal
import sys
import traceback


BIN_DIR = os.path.dirname(os.path.abspath(__file__))
for path in (os.path.join(BIN_DIR, "vendor"), os.path.join(BIN_DIR, "src")):
    if path not in sys.path:
        sys.path.insert(0, path)

from PIL import ImageFont

from kinnovel import VERSION
from kinnovel.api import ApiClient
from kinnovel.config import Config, LOG_DIR, ensure_directories
from kinnovel.pages import account, announcements, book, browse, history, home, rank, reader, series, settings, shelf
from kinnovel.ui import ImageCache, PageContext

from screen import Screen


class PageAdapter:
    def __init__(self, render, handle, enter=None):
        self._render = render
        self._handle = handle
        self._enter = enter

    def render(self, context, canvas):
        return self._render(context, canvas)

    def handle(self, data, context):
        return self._handle(data, context)

    def enter(self, context):
        if callable(self._enter):
            return self._enter(context)
        return None


class KinNovelApp:
    def __init__(self):
        ensure_directories()
        self.config = Config()
        self.api = ApiClient(self.config)
        self.images = ImageCache()
        self.screen = None
        self.fonts = {}
        self.context = None
        self.running = True
        self._log_file = None

    def log(self, message):
        try:
            print(message)
            if self._log_file:
                self._log_file.write(message + "\n")
                self._log_file.flush()
        except OSError:
            pass

    def load_fonts(self):
        path = self.config.get("font_path")
        sizes = {
            "hero": 82, "title": 50, "body": 38, "small": 31, "tiny": 25,
            96: 96, 48: 48, 36: 36, 28: 28,
        }
        fonts = {}
        for key, size in sizes.items():
            try:
                fonts[key] = ImageFont.truetype(path, size)
            except OSError:
                fonts[key] = ImageFont.load_default()
        self.fonts = fonts

    def initialize_screen(self):
        self.screen = Screen()
        fb_path = self.config.get("framebuffer") or "/dev/fb0"
        protocols = [self.config.get("screen_protocol") or "auto"]
        if protocols[0] in ("auto", "", None):
            protocols = ["mtk", "mxcfb"]
        initialized = False
        for protocol in protocols:
            try:
                kwargs = {"protocol": protocol} if protocol else {}
                if self.screen.output.initialization(fb_path, **kwargs):
                    initialized = True
                    self.log("[屏幕] 输出协议: " + protocol)
                    break
            except Exception as exc:
                self.log("[屏幕] %s 初始化异常: %s" % (protocol, exc))
        if not initialized:
            raise RuntimeError("framebuffer 初始化失败")
        width, height = self.screen.output.resolution
        if not self.screen.input.initialization(render_w=width, render_h=height):
            raise RuntimeError("触摸屏初始化失败")
        try:
            self.screen.input.device.grab()
        except Exception as exc:
            self.log("[输入] 独占触摸设备失败: %s" % exc)

    def build_pages(self):
        context = self.context
        context.register("home", home)
        context.register("browse", browse)
        context.register("rank", rank)
        context.register("book", book)
        context.register("series", series)
        context.register("history", history)
        context.register("reader", reader)
        context.register("catalog", PageAdapter(reader.render_catalog, reader.handle_catalog, None))
        context.register("shelf", shelf)
        context.register("settings", settings)
        context.register("about", PageAdapter(settings.render_about,
                                               lambda data, ctx: None, None))
        context.register("account", PageAdapter(account.render, account.handle, account.enter))
        context.register("notifications", PageAdapter(account.render_notifications,
                                                       account.handle_notifications,
                                                       account.enter_notifications))
        context.register("shop", PageAdapter(account.render_shop, account.handle_shop,
                                             account.enter_shop))
        context.register("announcements", announcements)
        context.register("announcement", PageAdapter(announcements.render_detail,
                                                       announcements.handle_detail,
                                                       announcements.enter_detail))
        context.register("comments", PageAdapter(announcements.render_comments,
                                                  announcements.handle_comments,
                                                  announcements.enter_comments))

    def stop(self, *_):
        self.running = False
        raise KeyboardInterrupt

    def run(self):
        LOG_DIR.mkdir(parents=True, exist_ok=True)
        try:
            self._log_file = open(os.path.join(LOG_DIR, "kinnovel.log"), "a", encoding="utf-8")
        except OSError:
            self._log_file = None
        self.log("[启动] KinNovel %s" % VERSION)
        self.load_fonts()
        self.initialize_screen()
        self.context = PageContext(self)
        self.build_pages()
        self.context.home()
        self.context.run_async("home", self.context.prune_cache)

        def on_gesture(data):
            try:
                result = self.context.handle(data)
                if isinstance(result, str):
                    self.context.replace(result)
            except KeyboardInterrupt:
                raise
            except Exception:
                self.log(traceback.format_exc())
                self.context.message(["运行时错误", "请查看 logs/kinnovel.log"])

        try:
            self.screen.input.listen(on_gesture=on_gesture)
        except KeyboardInterrupt:
            pass
        finally:
            self.shutdown()

    def shutdown(self):
        self.log("[退出] 正在关闭")
        try:
            if self.context:
                self.context._closed = True
            self.api.hub.close()
        except Exception:
            pass
        try:
            if self.screen and self.screen.input.device:
                self.screen.input.device.ungrab()
                self.screen.input.device.close()
        except Exception:
            pass
        if self._log_file:
            try:
                self._log_file.close()
            except OSError:
                pass


def main():
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            try:
                stream.reconfigure(line_buffering=True)
            except (OSError, ValueError):
                pass
    app = KinNovelApp()

    def stop_handler(*_):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, stop_handler)
    signal.signal(signal.SIGINT, stop_handler)
    app.run()


if __name__ == "__main__":
    main()
