#!/usr/bin/env python3
"""Download and decode one remote image outside the main UI process."""

import os
import ssl
import sys
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
    if len(sys.argv) != 4:
        return 2
    url, output, strict_raw = sys.argv[1], sys.argv[2], sys.argv[3]
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
        image = Image.open(BytesIO(data))
        image.load()
        if image.mode != "L":
            image = image.convert("L")
        output_path.parent.mkdir(parents=True, exist_ok=True)
        temp = output_path.with_suffix(output_path.suffix + ".tmp")
        image.save(temp, "PNG")
        os.replace(temp, output_path)
        return 0
    except (OSError, ValueError, Image.DecompressionBombError):
        return 4


if __name__ == "__main__":
    sys.exit(main())
