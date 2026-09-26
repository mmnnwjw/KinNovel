#!/bin/sh
set -e

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
TARGET="/mnt/us/extensions/kinnovel"

mkdir -p "$TARGET"
cp -R "$SCRIPT_DIR/bin" "$TARGET/"
cp "$SCRIPT_DIR/config.xml" "$TARGET/"
cp "$SCRIPT_DIR/menu.json" "$TARGET/"
cp "$SCRIPT_DIR/manifest.json" "$TARGET/"
cp "$SCRIPT_DIR/LICENSE" "$TARGET/"
cp "$SCRIPT_DIR/THIRD-PARTY-NOTICES.md" "$TARGET/"
chmod +x "$TARGET/bin/start.sh"

mkdir -p /mnt/us/documents/kinnovel
echo "KinNovel installed to $TARGET"
