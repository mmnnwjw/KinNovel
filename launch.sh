#!/bin/sh

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
exec /bin/sh "$SCRIPT_DIR/bin/start.sh"
