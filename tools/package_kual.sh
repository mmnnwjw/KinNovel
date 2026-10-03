#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)

BINARY_PATH="${1:-$REPO_ROOT/cpp/build/kinnovel}"
OUTPUT_DIR="${2:-$REPO_ROOT/build}"

if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: Binary not found at $BINARY_PATH" >&2
    echo "Please build kinnovel first: (cd cpp/build && make kinnovel)" >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

# Extract version from manifest.json
VERSION=$(grep -o '"version": \[[0-9, ]*\]' "$REPO_ROOT/manifest.json" | tr -d ' "version:[]' | tr ',' '.')
if [ -z "$VERSION" ]; then
    VERSION="0.6.0"
fi

echo "=========================================="
echo "Packaging KinNovel KUAL Extension (C++)"
echo "Version: $VERSION"
echo "Binary:  $BINARY_PATH"
echo "Output:  $OUTPUT_DIR"
echo "=========================================="

TMP_BASE="${TMPDIR:-/tmp}"
STAGE_DIR=$(mktemp -d "${TMP_BASE}/kinnovel_kual_pkg.XXXXXX")
trap 'rm -rf "$STAGE_DIR"' EXIT

PKG_DIR="$STAGE_DIR/KinNovel"
mkdir -p "$PKG_DIR/bin"

# Copy root configs & manifests
cp "$REPO_ROOT/config.xml" "$PKG_DIR/"
cp "$REPO_ROOT/menu.json" "$PKG_DIR/"
cp "$REPO_ROOT/manifest.json" "$PKG_DIR/"
cp "$REPO_ROOT/LICENSE" "$PKG_DIR/"
cp "$REPO_ROOT/THIRD-PARTY-NOTICES.md" "$PKG_DIR/"
cp "$REPO_ROOT/install.sh" "$PKG_DIR/"
cp "$REPO_ROOT/uninstall.sh" "$PKG_DIR/"
cp "$REPO_ROOT/launch.sh" "$PKG_DIR/"

# Copy bin items
cp "$REPO_ROOT/bin/start.sh" "$PKG_DIR/bin/"
cp "$REPO_ROOT/bin/config.json" "$PKG_DIR/bin/"
cp "$BINARY_PATH" "$PKG_DIR/bin/kinnovel"

# Set executable permissions
chmod +x "$PKG_DIR/bin/kinnovel"
chmod +x "$PKG_DIR/bin/start.sh"
chmod +x "$PKG_DIR/install.sh"
chmod +x "$PKG_DIR/uninstall.sh"
chmod +x "$PKG_DIR/launch.sh"

ZIP_NAME="KinNovel-v${VERSION}-kual.zip"
TARGET_ZIP="$OUTPUT_DIR/$ZIP_NAME"
rm -f "$TARGET_ZIP"

if command -v zip >/dev/null 2>&1; then
    (
        cd "$STAGE_DIR"
        zip -r -9 "$TARGET_ZIP" KinNovel
    )
else
    python3 -c "
import os, sys, zipfile
stage = sys.argv[1]
target = sys.argv[2]
with zipfile.ZipFile(target, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
    for root, dirs, files in os.walk(os.path.join(stage, 'KinNovel')):
        for file in files:
            full = os.path.join(root, file)
            rel = os.path.relpath(full, stage)
            st = os.stat(full)
            zi = zipfile.ZipInfo(rel)
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = (st.st_mode & 0o777) << 16
            zf.writestr(zi, open(full, 'rb').read())
" "$STAGE_DIR" "$TARGET_ZIP"
fi

echo "Created package: $TARGET_ZIP ($(du -h "$TARGET_ZIP" | cut -f1))"
echo "Verifying archive structure..."

if command -v unzip >/dev/null 2>&1; then
    unzip -l "$TARGET_ZIP"
else
    python3 -c "
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], 'r') as zf:
    print('  Length      Mode  Name')
    print('--------  --------  ----')
    for info in zf.infolist():
        mode = oct(info.external_attr >> 16)
        print(f'{info.file_size:8d}  {mode:8s}  {info.filename}')
" "$TARGET_ZIP"
fi

echo "=========================================="
echo "Packaging complete!"
echo "=========================================="
