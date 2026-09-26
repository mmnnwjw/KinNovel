#!/usr/bin/env python3
"""Package clean KinNovel release zip with proper executable permissions."""

import os
import re
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

EXCLUDE_NAMES = {
    ".git",
    ".gitignore",
    ".pytest_cache",
    "__pycache__",
    "build",
    "cache",
    "logs",
    "tests",
    "tools",
    "TESTACCOUNT.txt",
    "GithubToken",
    "audit_bin.txt",
}

EXCLUDE_EXTS = {
    ".pyc",
    ".pyo",
    ".pyd",
    ".log",
    ".DS_Store",
}


def get_version():
    init_py = ROOT / "bin" / "src" / "kinnovel" / "__init__.py"
    content = init_py.read_text(encoding="utf-8")
    m = re.search(r'VERSION\s*=\s*["\']([^"\']+)["\']', content)
    if m:
        return m.group(1)
    return "unknown"


def should_exclude(rel_path):
    parts = rel_path.parts
    if any(p in EXCLUDE_NAMES for p in parts):
        return True
    if rel_path.suffix in EXCLUDE_EXTS:
        return True
    return False


def build_release_zip(output_dir=None):
    version = get_version()
    if output_dir is None:
        output_dir = ROOT / "build"
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    zip_name = f"KinNovel-v{version}.zip"
    zip_path = output_dir / zip_name

    included_files = []
    # Collect files
    for base in ["bin", "config.xml", "manifest.json", "menu.json", "launch.sh", "install.sh", "uninstall.sh", "LICENSE", "THIRD-PARTY-NOTICES.md"]:
        p = ROOT / base
        if not p.exists():
            continue
        if p.is_file():
            rel = p.relative_to(ROOT)
            if not should_exclude(rel):
                included_files.append((p, rel))
        else:
            for item in p.rglob("*"):
                if item.is_file():
                    rel = item.relative_to(ROOT)
                    if not should_exclude(rel):
                        included_files.append((item, rel))

    print(f"Packaging {len(included_files)} files into {zip_path}...")
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zout:
        for full_path, rel_path in sorted(included_files, key=lambda x: str(x[1])):
            arcname = str(Path("KinNovel") / rel_path).replace("\\", "/")
            data = full_path.read_bytes()
            
            # Use file's last modified time
            st = full_path.stat()
            mtime = time_to_tuple(st.st_mtime)
            zinfo = zipfile.ZipInfo(arcname, date_time=mtime)
            zinfo.compress_type = zipfile.ZIP_DEFLATED
            
            # POSIX permission handling
            if arcname.endswith(".sh"):
                zinfo.external_attr = 0o100755 << 16
            else:
                zinfo.external_attr = 0o100644 << 16
            
            zout.writestr(zinfo, data)

    print(f"Created {zip_path} (size: {zip_path.stat().st_size:,} bytes)")
    return zip_path


def time_to_tuple(ts):
    import time
    t = time.localtime(ts)
    return (t.tm_year, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else None
    build_release_zip(out)
