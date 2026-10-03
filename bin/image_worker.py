#!/usr/bin/env python3
"""Download and decode one remote image outside the main UI process.

用法: image_worker.py <url> <output> <strict_tls 0|1> [target_height]
输出扩展名为 .jpg/.jpeg 时按 JPEG 保存，否则保持 PNG(兼容旧调用)。
"""

import os
import ssl
import sys
import tempfile
import urllib.request
from pathlib import Path


try:
    from PIL import Image
except ImportError:
    BIN_DIR = Path(__file__).resolve().parent
    sys.path.insert(0, str(BIN_DIR / "vendor"))
    from PIL import Image


Image.MAX_IMAGE_PIXELS = 20_000_000


def main():
    if len(sys.argv) not in (4, 5):
        return 2
    url, output, strict_raw = sys.argv[1], sys.argv[2], sys.argv[3]
    try:
        target = max(0, int(sys.argv[4])) if len(sys.argv) == 5 else 0
    except (TypeError, ValueError):
        target = 0
    output_path = Path(output)
    context = None
    if url.startswith("https://"):
        context = ssl.create_default_context()
        if strict_raw != "1":
            context.check_hostname = False
            context.verify_mode = ssl.CERT_NONE
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "KinNovel/0.2"})
        with urllib.request.urlopen(request, timeout=12, context=context) as response:
            data = response.read(8 * 1024 * 1024)
        if not data:
            return 3
        from io import BytesIO
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
        output_path.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temp_name = tempfile.mkstemp(
            dir=str(output_path.parent), prefix=output_path.name, suffix=".tmp")
        temp = Path(temp_name)
        os.close(descriptor)
        if output_path.suffix.lower() in (".jpg", ".jpeg"):
            image.save(temp, "JPEG", quality=85)
        else:
            image.save(temp, "PNG")
        os.replace(temp, output_path)
        return 0
    except (OSError, ValueError, Image.DecompressionBombError):
        try:
            if "temp" in locals():
                Path(temp).unlink()
        except OSError:
            pass
        return 4


if __name__ == "__main__":
    sys.exit(main())
