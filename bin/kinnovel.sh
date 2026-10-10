#!/bin/sh
# KinNovel 1.0 启动脚本 (KUAL 通过 /bin/sh 调用; /mnt/us 是 vfat, 没有执行位)。
# 职责: 单实例锁 → 暂停占用 /dev/fb0 的框架进程 → 运行原生二进制 → 无论如何退出都恢复框架。
# 暂停的 PID 写入 /tmp/kinnovel_paused_pids, 休眠/唤醒时由程序自己 SIGCONT/SIGSTOP (kn-platform::power)。

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
APP_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
LOG_DIR="$APP_DIR/logs"
LOG_FILE="$LOG_DIR/kinnovel.log"
PAUSE_LIST="/tmp/kinnovel_paused_pids"
LOCK_DIR="/tmp/kinnovel.lock"
FB_DEV="/dev/fb0"
# vfat 上不能执行, 复制到 tmpfs 运行
BIN_SRC="$SCRIPT_DIR/kinnovel"
BIN_RUN="/tmp/kinnovel-bin"

mkdir -p "$LOG_DIR" 2>/dev/null || true
log() { echo "[launcher] $*" >> "$LOG_FILE"; }

# 日志超过 1 MB 时只保留后 512 KB
if [ -f "$LOG_FILE" ] && [ "$(wc -c < "$LOG_FILE")" -gt 1048576 ]; then
  tail -c 524288 "$LOG_FILE" > "$LOG_FILE.tmp" && mv "$LOG_FILE.tmp" "$LOG_FILE"
fi

lock_owner_alive() {
  [ -n "$1" ] || return 1
  kill -0 "$1" 2>/dev/null || return 1
  grep -q -E 'kinnovel' "/proc/$1/cmdline" 2>/dev/null
}

if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  OLD_PID=$(cat "$LOCK_DIR/pid" 2>/dev/null || true)
  if lock_owner_alive "$OLD_PID"; then
    log "already running as pid $OLD_PID"
    exit 1
  fi
  rm -rf "$LOCK_DIR"
  mkdir "$LOCK_DIR" || exit 1
fi
echo $$ > "$LOCK_DIR/pid"

find_fb_users() {
  for pid in $(ls /proc 2>/dev/null | grep -E '^[0-9]+$'); do
    for fd in /proc/$pid/fd/*; do
      [ "$(readlink "$fd" 2>/dev/null)" = "$FB_DEV" ] && { echo "$pid"; break; }
    done
  done | sort -u
}

resume_fb_users() {
  [ -f "$PAUSE_LIST" ] || return
  while read -r pid; do
    [ -n "$pid" ] && kill -CONT "$pid" 2>/dev/null
  done < "$PAUSE_LIST"
  rm -f "$PAUSE_LIST"
}

pause_fb_users() {
  : > "$PAUSE_LIST"
  for pid in $(find_fb_users); do
    [ "$pid" -ne $$ ] || continue
    if kill -STOP "$pid" 2>/dev/null; then
      echo "$pid" >> "$PAUSE_LIST"
      log "paused pid=$pid"
    fi
  done
}

on_exit() {
  resume_fb_users
  # 让系统主页重新绘制屏幕
  lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home >/dev/null 2>&1 || true
  rm -f "$BIN_RUN"
  rm -rf "$LOCK_DIR"
}
trap on_exit INT TERM EXIT

# 上次异常退出可能留下被暂停的进程
resume_fb_users
log "starting KinNovel (native)"
cp "$BIN_SRC" "$BIN_RUN" && chmod +x "$BIN_RUN" || { log "cannot stage binary"; exit 1; }
# 重力感应机型 (Oasis / Scribe) 的当前朝向 U/D/L/R (同 KOReader); winmgr 属于框架, 必须在暂停前读
[ -n "$KN_ORIENTATION" ] || KN_ORIENTATION=$(lipc-get-prop com.lab126.winmgr accelerometer 2>/dev/null)
export KN_ORIENTATION
pause_fb_users
usleep 200000 2>/dev/null || sleep 1
KN_APP_DIR="${KN_APP_DIR:-$APP_DIR}" "$BIN_RUN" >> "$LOG_FILE" 2>&1
RET=$?
log "exited with $RET"
exit $RET
