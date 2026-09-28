"""帧缓冲快照工具:保存/恢复 /dev/fb0

用法:
    fb_snapshot.py save <file>     # 把当前屏幕内容保存到文件
    fb_snapshot.py restore <file>  # 写回文件内容并全屏刷新
"""
import os
import sys

_BIN = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(_BIN, "vendor"))
sys.path.insert(0, os.path.join(_BIN, "src"))

from screen.output.framebuffer import EInkDisplay  # noqa: E402

FB_PATH = "/dev/fb0"
PROTOCOLS = ("mtk", "rex", "zelda", "mxcfb")


def _configured_protocols():
    """按 config.json 的 screen_protocol 决定探测顺序,默认与 app.py 一致"""
    configured = "auto"
    try:
        from kinnovel.config import Config
        configured = Config().get("screen_protocol") or "auto"
    except Exception:
        pass
    if configured in PROTOCOLS:
        return (configured,) + tuple(p for p in PROTOCOLS if p != configured)
    return PROTOCOLS


def _open_display(probe):
    """打开 framebuffer;probe=True 时逐个协议试刷新,避免在非 MTK 机型发错 ioctl"""
    last_error = None
    for protocol in _configured_protocols():
        try:
            display = EInkDisplay(FB_PATH, protocol=protocol)
        except OSError as exc:
            last_error = exc
            continue
        if not probe:
            return display
        try:
            if display.probe():
                return display
        except OSError as exc:
            last_error = exc
        display.close()
    raise RuntimeError("framebuffer 不可用: %s" % last_error)


# 保存
def save(path):
    display = _open_display(probe=False)
    try:
        size = display.smem_len
        data = display.mem[:size]
    finally:
        display.close()
    with open(path, "wb") as f:
        f.write(data)
    print(f"[快照] 已保存 {size} 字节 -> {path}")


# 恢复
def restore(path):
    with open(path, "rb") as f:
        data = f.read()
    display = _open_display(probe=True)
    try:
        if len(data) != display.smem_len:
            print(f"[快照] 文件大小 {len(data)} 与 smem_len {display.smem_len} 不符, 按较小长度写回")
        size = min(len(data), display.smem_len)
        display.mem[:size] = data[:size]
        print("[快照] 已写回帧缓冲,全屏刷新...")
        display.mxc_update(0, 0, display.width, display.height,
                           is_flashing=True, waveform_mode=display.W.GC16)
    finally:
        display.close()
    print("[快照] 恢复完成")


# 主函数
def main():
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(2)
    cmd, path = sys.argv[1], sys.argv[2]
    if cmd == "save":
        save(path)
    elif cmd == "restore":
        restore(path)
    else:
        print(f"未知命令: {cmd}")
        sys.exit(2)


if __name__ == "__main__":
    main()
