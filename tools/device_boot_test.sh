#!/bin/sh
# 在 Kindle 上启动 KinNovel,等待若干秒后发 SIGTERM,验证能干净退出并恢复屏幕。
# 用法: device_boot_test.sh [wait_seconds]
set -e

APP=/mnt/us/extensions/KinNovel
WAIT=${1:-15}

rm -rf /tmp/kinnovel.lock
setsid /bin/sh "$APP/bin/start.sh" >/dev/null 2>&1 &
sleep "$WAIT"

PID=""
for p in /proc/[0-9]*; do
  e=$(readlink "$p/exe" 2>/dev/null || true)
  case "$e" in
    *python3.14*)
      c=$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null || true)
      case "$c" in
        *KinNovel/bin/app.py*) PID=${p#/proc/}; break ;;
      esac
      ;;
  esac
done
echo "app_pid=$PID"

if [ -z "$PID" ]; then
  echo "APP_NOT_RUNNING"
  tail -20 "$APP/logs/kinnovel.log"
  exit 1
fi

T0=$(date +%s)
kill -TERM "$PID"
for _ in $(seq 1 30); do
  [ -d "/proc/$PID" ] || break
  sleep 1
done
T1=$(date +%s)
echo "exit_elapsed=$((T1 - T0))s"

if [ -d "/proc/$PID" ]; then
  echo "STILL_RUNNING"
  exit 2
fi
echo "stopped"

[ -d /tmp/kinnovel.lock ] && echo "LOCK_LEFT" || echo "lock_removed"
tail -8 "$APP/logs/kinnovel.log"
