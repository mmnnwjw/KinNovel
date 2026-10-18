#!/bin/sh
# KinNovel 启动脚本 (KUAL 通过 /bin/sh 调用; /mnt/us 是 vfat, 没有执行位)。
# 职责: 单实例锁 → 暂停占用 /dev/fb0 的框架进程 → 运行原生二进制 → 无论如何退出都恢复框架。
# 暂停的 PID 写入 /tmp/kinnovel_paused_pids, 休眠/唤醒时由程序自己 SIGCONT/SIGSTOP (kn-platform::power)。
# 参数原样传给程序: KUAL 菜单「KinNovel 工具箱」用 --update (检查更新) / --uninstall (卸载)。

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

# 旧版本留下的文件 (安装就是复制文件夹, 没有安装脚本来清理): 0.8.x 的 Python 运行时 (~40 MB),
# 1.5 及以前随包的 KPM 描述与安装脚本
rm -rf "$SCRIPT_DIR/src" "$SCRIPT_DIR/lib" "$SCRIPT_DIR/vendor" 2>/dev/null
rm -f "$SCRIPT_DIR/app.py" "$SCRIPT_DIR/start.sh" "$SCRIPT_DIR/image_worker.py" "$SCRIPT_DIR/fb_snapshot.py"   "$APP_DIR/manifest.json" "$APP_DIR/launch.sh" "$APP_DIR/install.sh" "$APP_DIR/uninstall.sh" 2>/dev/null

# 一次 ls 列出所有进程的 fd 链接 (KPW5 上 0.15 s; 以前每个 fd 调一次 readlink 要 ~7 s)
find_fb_users() {
  ls -l /proc/[0-9]*/fd/ 2>/dev/null | awk -v dev="$FB_DEV" '
    /^\/proc\/[0-9]+\/fd/ { split($0, p, "/"); pid = p[3]; next }
    $NF == dev { print pid }' | sort -u
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

SLEEP_COVER=0

on_exit() {
  resume_fb_users
  [ "$SLEEP_COVER" = 1 ] && lipc-set-prop com.lab126.blanket load screensaver >/dev/null 2>&1
  # 让系统主页重新绘制屏幕
  lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home >/dev/null 2>&1 || true
  rm -f "$BIN_RUN"
  rm -rf "$LOCK_DIR"
}
trap on_exit INT TERM EXIT

# 上次异常退出可能留下被暂停的进程
resume_fb_users
# 休眠显示封面 (设置 sleep_cover, 同 KOReader): 暂停框架前卸载系统屏保模块, 退出时加载回来。
# blanket 也会被暂停, 所以必须在 pause_fb_users 之前 / resume_fb_users 之后调用。有广告的机型 (ad_screensaver) 不动。
# 设置在 data/settings.json; 1.4 及以前是 bin/config.json (第一次启动新版时程序才迁移, 所以两个都看)
SETTINGS_DIR="${KN_APP_DIR:-$APP_DIR}"
if grep -q '"sleep_cover": *true' "$SETTINGS_DIR/data/settings.json" "$SETTINGS_DIR/bin/config.json" 2>/dev/null; then
  case "$(lipc-get-prop com.lab126.blanket load 2>/dev/null)" in
    *ad_screensaver*) log "special offers device, keeping the system screensaver" ;;
    *screensaver*) lipc-set-prop com.lab126.blanket unload screensaver 2>/dev/null && SLEEP_COVER=1 ;;
  esac
fi
export KN_SLEEP_COVER=$SLEEP_COVER
# 更新安装到这个目录 (数据目录 KN_APP_DIR 测试时可以在别处)
export KN_PROGRAM_DIR="$APP_DIR"
RESTART_MARKER=/tmp/kinnovel_restart
UNINSTALL_MARKER=/tmp/kinnovel_uninstall
rm -f "$RESTART_MARKER" "$UNINSTALL_MARKER"
log "starting KinNovel (native)"
cp "$BIN_SRC" "$BIN_RUN" && chmod +x "$BIN_RUN" || { log "cannot stage binary"; exit 1; }
# 重力感应机型 (Oasis / Scribe) 的当前朝向 U/D/L/R (同 KOReader); winmgr 属于框架, 必须在暂停前读
[ -n "$KN_ORIENTATION" ] || KN_ORIENTATION=$(lipc-get-prop com.lab126.winmgr accelerometer 2>/dev/null)
export KN_ORIENTATION
pause_fb_users
usleep 200000 2>/dev/null || sleep 1
KN_APP_DIR="${KN_APP_DIR:-$APP_DIR}" "$BIN_RUN" "$@" >> "$LOG_FILE" 2>&1
RET=$?
log "exited with $RET"
# 程序改过显示驱动的全局状态 (夜间模式的硬件反色) 却没能正常退出时留下这个文件: 恢复, 否则系统界面会一直反色
[ -f /tmp/kinnovel_grayscale ] && "$BIN_RUN" --restore-display >> "$LOG_FILE" 2>&1
# 异常退出: panic 时程序自己写了 last_crash.txt (含消息); 被信号杀死 (段错误等) 时在这里补一个,
# 下次启动书架页会提示。SIGTERM/SIGINT (正常的 kill / 关机) 不算。
CRASH_MARKER="$LOG_DIR/last_crash.txt"
case "$RET" in
  0|130|143) ;;
  *)
    if [ ! -f "$CRASH_MARKER" ]; then
      if [ "$RET" -gt 128 ]; then
        echo "程序被信号 $((RET - 128)) 终止" > "$CRASH_MARKER"
      else
        echo "退出码 $RET" > "$CRASH_MARKER"
      fi
    fi
    ;;
esac
# 更新页「立即重启」: 恢复框架、释放锁, 再用 (刚换上的) 新脚本从头启动。exec 不触发 EXIT trap, 所以手动收尾
if [ "$RET" = 0 ] && [ -f "$RESTART_MARKER" ]; then
  rm -f "$RESTART_MARKER"
  log "restarting after update"
  trap - INT TERM EXIT
  resume_fb_users
  [ "$SLEEP_COVER" = 1 ] && lipc-set-prop com.lab126.blanket load screensaver >/dev/null 2>&1
  rm -f "$BIN_RUN"
  rm -rf "$LOCK_DIR"
  exec /bin/sh "$SCRIPT_DIR/kinnovel.sh"
fi
# 卸载: 先照常恢复系统界面, 再删除整个扩展目录 (程序确认过; 只删带 KinNovel 的 config.xml 的目录)
if [ "$RET" = 0 ] && [ -f "$UNINSTALL_MARKER" ]; then
  rm -f "$UNINSTALL_MARKER"
  trap - INT TERM EXIT
  on_exit
  case "$APP_DIR" in
    /|/mnt/us|/mnt/us/extensions) exit 1 ;;
  esac
  if grep -q '<id>kinnovel</id>' "$APP_DIR/config.xml" 2>/dev/null; then
    cd /
    exec rm -rf "$APP_DIR"
  fi
fi
exit $RET
