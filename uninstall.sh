#!/bin/sh

TARGET="/mnt/us/extensions/kinnovel"

if [ -d "$TARGET" ]; then
  rm -rf "$TARGET"
fi

rm -rf /mnt/us/documents/kinnovel
echo "KinNovel removed. User cache under the extension directory was removed with it."
