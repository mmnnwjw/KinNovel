#!/usr/bin/env python3
"""Dump a running 8bpp framebuffer to PNG for remote visual verification.

用法: fb_grab.py <out.png> [width height line_length] [framebuffer]
"""

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bin" / "vendor"))

from PIL import Image  # noqa: E402


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/fb.png"
    width, height, stride = 1236, 1648, 1248
    device = "/dev/fb0"
    if len(sys.argv) >= 5:
        width, height, stride = (int(sys.argv[2]), int(sys.argv[3]),
                                 int(sys.argv[4]))
    if len(sys.argv) >= 6:
        device = sys.argv[5]
    needed = stride * height
    with open(device, "rb") as handle:
        data = handle.read(needed)
    if len(data) < needed:
        print("framebuffer 数据不足: %d < %d" % (len(data), needed))
        return 1
    image = Image.frombytes("L", (stride, height), data).crop(
        (0, 0, width, height))
    image.save(out)
    print("%s %dx%d stride=%d" % (out, width, height, stride))
    return 0


if __name__ == "__main__":
    sys.exit(main())
