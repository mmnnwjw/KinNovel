#!/bin/sh

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
LOG_DIR="$SCRIPT_DIR/../logs"
LOG_FILE="$LOG_DIR/kinnovel.log"
PAUSE_LIST="/tmp/kinnovel_paused_pids"
FB_DEV="/dev/fb0"
FB_SNAPSHOT="/tmp/kinnovel_fb.bin"
PYTHON=${PYTHON:-/mnt/us/python3/bin/python3.14}
LOCK_DIR="/tmp/kinnovel.lock"

export LD_LIBRARY_PATH="$SCRIPT_DIR/lib:$LD_LIBRARY_PATH"
export SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

# The bundled FreeType matches the Kindle Pillow ABI but lacks WOFF2/Brotli.
# Prefer the WOFF2-capable FreeType bundled with the release. If it is not
# present, use KOReader's copy when available.
BUNDLED_FREETYPE="$SCRIPT_DIR/lib/freetype-woff2/libfreetype.so.6"
KOREADER_FREETYPE=/mnt/us/koreader/libs/libfreetype.so.6
if [ -f "$BUNDLED_FREETYPE" ]; then
  LD_LIBRARY_PATH="$SCRIPT_DIR/lib/freetype-woff2:$SCRIPT_DIR/lib:$LD_LIBRARY_PATH"
  export LD_LIBRARY_PATH
  LD_PRELOAD="$BUNDLED_FREETYPE${LD_PRELOAD:+ $LD_PRELOAD}"
  export LD_PRELOAD
elif [ -f "$KOREADER_FREETYPE" ]; then
  LD_PRELOAD="$KOREADER_FREETYPE${LD_PRELOAD:+ $LD_PRELOAD}"
  export LD_PRELOAD
fi

mkdir -p "$LOG_DIR" 2>/dev/null || true
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  OLD_PID=$(cat "$LOCK_DIR/pid" 2>/dev/null || true)
  if [ -n "$OLD_PID" ] && kill -0 "$OLD_PID" 2>/dev/null; then
    echo "KinNovel is already running as pid $OLD_PID." >> "$LOG_FILE"
    exit 1
  fi
  rm -rf "$LOCK_DIR"
  mkdir "$LOCK_DIR" 2>/dev/null || exit 1
fi
echo $$ > "$LOCK_DIR/pid"

log() {
  echo "[launcher] $*" >> "$LOG_FILE"
}

find_fb_users() {
  for pid in $(ls /proc 2>/dev/null | grep -E '^[0-9]+$'); do
    fd_dir="/proc/$pid/fd"
    [ -d "$fd_dir" ] || continue
    for fd in "$fd_dir"/*; do
      [ -L "$fd" ] || continue
      target=$(readlink "$fd" 2>/dev/null)
      if [ "$target" = "$FB_DEV" ]; then
        echo "$pid"
        break
      fi
    done
  done | sort -u
}

pause_fb_users() {
  rm -f "$PAUSE_LIST"
  touch "$PAUSE_LIST"
  for pid in $(find_fb_users); do
    if [ -n "$pid" ] && [ "$pid" -ne $$ ] && kill -0 "$pid" 2>/dev/null; then
      log "pausing pid=$pid"
      kill -STOP "$pid" 2>>"$LOG_FILE" && echo "$pid" >> "$PAUSE_LIST"
    fi
  done
}

resume_fb_users() {
  [ -f "$PAUSE_LIST" ] || return
  while read -r pid; do
    [ -z "$pid" ] && continue
    if kill -0 "$pid" 2>/dev/null; then
      kill -CONT "$pid" 2>>"$LOG_FILE" || true
    fi
  done < "$PAUSE_LIST"
  rm -f "$PAUSE_LIST"
}

save_snapshot() {
  "$PYTHON" "$SCRIPT_DIR/fb_snapshot.py" save "$FB_SNAPSHOT" >>"$LOG_FILE" 2>&1 || true
}

restore_snapshot() {
  if [ -f "$FB_SNAPSHOT" ]; then
    "$PYTHON" "$SCRIPT_DIR/fb_snapshot.py" restore "$FB_SNAPSHOT" >>"$LOG_FILE" 2>&1 || true
    rm -f "$FB_SNAPSHOT"
  fi
}

reload_modules() {
  lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home >/dev/null 2>&1 || true
}

on_exit() {
  restore_snapshot
  resume_fb_users
  reload_modules
  rm -rf "$LOCK_DIR"
}

trap 'on_exit' INT TERM EXIT

: > "$LOG_FILE"
log "starting KinNovel"
pause_fb_users
save_snapshot
usleep 300000 2>/dev/null || sleep 1
"$PYTHON" "$SCRIPT_DIR/app.py" >>"$LOG_FILE" 2>&1
RET=$?
trap - INT TERM EXIT
on_exit
exit $RET
