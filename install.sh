#!/bin/sh
set -e

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
TARGET="/mnt/us/extensions/kinnovel"

mkdir -p "$TARGET"

# 设置和账号在 data/ (1.5 起), 安装包里没有, 覆盖安装不会动它们。
# 1.4 及以前的 bin/config.json 也不在安装包里, 会原样留下, 第一次启动时由程序迁移到 data/。

# 1.0 起是单个原生程序; 清理 0.8.x Python 版遗留的运行时 (约 40 MB)
rm -rf "$TARGET/bin/src" "$TARGET/bin/lib" "$TARGET/bin/vendor"
rm -f "$TARGET/bin/app.py" "$TARGET/bin/start.sh" "$TARGET/bin/image_worker.py" "$TARGET/bin/fb_snapshot.py"

cp -R "$SCRIPT_DIR/bin" "$TARGET/"

cp "$SCRIPT_DIR/config.xml" "$TARGET/"
cp "$SCRIPT_DIR/menu.json" "$TARGET/"
cp "$SCRIPT_DIR/manifest.json" "$TARGET/"
cp "$SCRIPT_DIR/LICENSE" "$TARGET/"
cp "$SCRIPT_DIR/THIRD-PARTY-NOTICES.md" "$SCRIPT_DIR/LICENSE-rime-pinyin-simp" "$SCRIPT_DIR/LICENSE-NotoEmoji" "$TARGET/"
cp "$SCRIPT_DIR/launch.sh" "$SCRIPT_DIR/uninstall.sh" "$TARGET/"
chmod +x "$TARGET/bin/kinnovel.sh" "$TARGET/bin/kinnovel" 2>/dev/null || true

mkdir -p /mnt/us/documents/kinnovel
echo "KinNovel installed to $TARGET"
