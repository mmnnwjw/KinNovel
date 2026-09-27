#!/bin/sh
set -e

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
TARGET="/mnt/us/extensions/kinnovel"

mkdir -p "$TARGET"

# 升级时保留设备上已有的用户配置(账号、字体、排版设置都存在这个文件里)
CONFIG="$TARGET/bin/config.json"
BACKUP="/tmp/kinnovel-config.json"
rm -f "$BACKUP"
if [ -f "$CONFIG" ]; then
  cp "$CONFIG" "$BACKUP"
fi

cp -R "$SCRIPT_DIR/bin" "$TARGET/"

if [ -f "$BACKUP" ]; then
  cp "$BACKUP" "$CONFIG"
  rm -f "$BACKUP"
fi

cp "$SCRIPT_DIR/config.xml" "$TARGET/"
cp "$SCRIPT_DIR/menu.json" "$TARGET/"
cp "$SCRIPT_DIR/manifest.json" "$TARGET/"
cp "$SCRIPT_DIR/LICENSE" "$TARGET/"
cp "$SCRIPT_DIR/THIRD-PARTY-NOTICES.md" "$TARGET/"
chmod +x "$TARGET/bin/start.sh"

mkdir -p /mnt/us/documents/kinnovel
echo "KinNovel installed to $TARGET"
