#!/usr/bin/env python3
"""Build the KinNovel 1.0 release zip (Rust binary + KUAL extension files).

    python tools/package_release.py [--no-build] [output_dir]

1. Cross-compiles the device binary (`rust/build.sh kindle`) unless --no-build.
2. Collects the KUAL extension layout under `KinNovel/` inside the zip:
       KinNovel/bin/kinnovel        static armv7 binary (0755)
       KinNovel/bin/kinnovel.sh     launcher (0755)
       KinNovel/{config.xml,menu.json}  what KUAL reads
       KinNovel/{LICENSE,LICENSE-rime-pinyin-simp,LICENSE-NotoEmoji,LICENSE-LXGW,README.md,THIRD-PARTY-NOTICES.md}
   Installing = copying the folder's contents to extensions/kinnovel/ (no install scripts).
3. Refuses to package anything that looks like a credential.
"""

import re
import subprocess
import sys
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust" / "target" / "armv7-unknown-linux-musleabihf" / "release" / "kinnovel"

FILES = [
    ("bin/kinnovel", BINARY),
    ("bin/kinnovel.sh", ROOT / "bin" / "kinnovel.sh"),
    ("config.xml", ROOT / "config.xml"),
    ("menu.json", ROOT / "menu.json"),
    ("LICENSE", ROOT / "LICENSE"),
    ("README.md", ROOT / "README.md"),
    ("THIRD-PARTY-NOTICES.md", ROOT / "THIRD-PARTY-NOTICES.md"),
    # Apache-2.0 requires the license text next to the embedded pinyin dictionary
    ("LICENSE-rime-pinyin-simp", ROOT / "rust" / "crates" / "kn-ime" / "data" / "LICENSE-rime-pinyin-simp"),
    # OFL-1.1 requires the license with the embedded emoji font
    ("LICENSE-NotoEmoji", ROOT / "rust" / "crates" / "kn-app" / "data" / "LICENSE-NotoEmoji"),
    # IPA Font License 1.0: the agreement accompanies the embedded symbol font (a derived program)
    ("LICENSE-LXGW", ROOT / "rust" / "crates" / "kn-app" / "data" / "LICENSE-LXGW"),
]
EXECUTABLE = {"bin/kinnovel", "bin/kinnovel.sh"}

SECRET_CONTENT_PATTERNS = (
    re.compile(rb"ghp_[A-Za-z0-9]{20,}"),
    re.compile(rb"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"),
)


def version():
    text = (ROOT / "rust" / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        sys.exit("cannot find workspace version in rust/Cargo.toml")
    return match.group(1)


def check_versions(v):
    if "<version>%s</version>" % v not in (ROOT / "config.xml").read_text(encoding="utf-8"):
        sys.exit("config.xml version != %s" % v)


def build():
    print("Building device binary ...")
    subprocess.run(["sh", str(ROOT / "rust" / "build.sh"), "kindle"], check=True)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--no-build" not in sys.argv:
        build()
    v = version()
    check_versions(v)
    if not BINARY.is_file():
        sys.exit("missing %s (run without --no-build)" % BINARY)
    out_dir = Path(args[0]) if args else ROOT / "build"
    out_dir.mkdir(parents=True, exist_ok=True)
    zip_path = out_dir / ("KinNovel-v%s.zip" % v)
    now = time.localtime()[:6]
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zout:
        for arcname, source in FILES:
            data = source.read_bytes()
            for pattern in SECRET_CONTENT_PATTERNS:
                if pattern.search(data):
                    sys.exit("credential-like content in %s" % source)
            info = zipfile.ZipInfo("KinNovel/" + arcname, date_time=now)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (0o100755 if arcname in EXECUTABLE else 0o100644) << 16
            zout.writestr(info, data)
            print("  %-28s %9d bytes" % (arcname, len(data)))
    print("Created %s (%d bytes)" % (zip_path, zip_path.stat().st_size))


if __name__ == "__main__":
    main()
