"""Kindle power and sleep management.

Monitors physical power button presses and Kindle LIPC powerd events
(goingToScreenSaver / outOfScreenSaver) to gracefully release the framebuffer,
allow the system to display the screensaver and sleep, and seamlessly restore
the reading interface upon wakeup without ghosting or loss of state.
"""

import glob
import os
import shutil
import signal
import subprocess
import threading
import time
from pathlib import Path

try:
    from evdev import InputDevice, ecodes
except ImportError:
    InputDevice = None
    ecodes = None

DEFAULT_PAUSE_FILE = Path("/tmp/kinnovel_paused_pids")
FB_DEV = "/dev/fb0"


def find_fb_users(fb_dev=FB_DEV):
    """Scan /proc for processes holding fb_dev open."""
    my_pid = os.getpid()
    users = set()
    try:
        for name in os.listdir("/proc"):
            if not name.isdigit():
                continue
            pid = int(name)
            if pid in (1, my_pid):
                continue
            fd_dir = f"/proc/{pid}/fd"
            try:
                for fd in os.listdir(fd_dir):
                    link = f"{fd_dir}/{fd}"
                    try:
                        if os.path.islink(link) and os.readlink(link) == fb_dev:
                            users.add(pid)
                            break
                    except OSError:
                        continue
            except OSError:
                continue
    except OSError:
        pass
    return sorted(users)


def read_paused_pids(file_path=DEFAULT_PAUSE_FILE):
    """Read list of paused PIDs from launcher file."""
    if not file_path.exists():
        return []
    try:
        pids = []
        for line in file_path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.isdigit():
                pids.append(int(line))
        return pids
    except OSError:
        return []


def write_paused_pids(pids, file_path=DEFAULT_PAUSE_FILE):
    """Save active paused PIDs to file for start.sh trap synchronization."""
    try:
        content = "\n".join(str(p) for p in sorted(set(pids))) + "\n"
        file_path.write_text(content, encoding="utf-8")
    except OSError:
        pass


def find_power_device_paths():
    """Find input device paths that report KEY_POWER capabilities."""
    if not InputDevice or not ecodes:
        return []
    paths = []
    for path in sorted(glob.glob("/dev/input/event*")):
        try:
            dev = InputDevice(path)
            caps = dev.capabilities()
            if ecodes.EV_KEY in caps:
                keys = set(caps[ecodes.EV_KEY])
                if ecodes.KEY_POWER in keys or getattr(ecodes, "KEY_POWER2", -1) in keys:
                    paths.append(path)
            dev.close()
        except Exception:
            pass
    return paths


class PowerManager:
    """Coordinates Kindle sleep/wake transitions with native system daemons."""

    def __init__(self, app, pause_file=DEFAULT_PAUSE_FILE):
        self.app = app
        self.pause_file = Path(pause_file)
        self.is_sleeping = False
        self._stopped = False
        self._lock = threading.Lock()
        self._tracked_pids = []
        self._threads = []
        self._lipc_proc = None
        self._power_devices = []
        self._last_power_press_at = 0.0

    def log(self, message):
        if hasattr(self.app, "log") and callable(self.app.log):
            self.app.log("[电源] " + str(message))
        else:
            print("[电源] " + str(message), flush=True)

    def start(self):
        """Start background monitoring threads."""
        self._stopped = False
        # 1. Start LIPC event listener if lipc-wait-event is available
        if shutil.which("lipc-wait-event"):
            t_lipc = threading.Thread(target=self._lipc_event_loop, daemon=True, name="lipc-power-listener")
            t_lipc.start()
            self._threads.append(t_lipc)
            self.log("已启动 LIPC 电源事件监听 (goingToScreenSaver / outOfScreenSaver)")
        else:
            self.log("lipc-wait-event 不可用，跳过 LIPC 监听")

        # 2. Start hardware power key listeners
        power_paths = find_power_device_paths()
        if power_paths:
            for p_path in power_paths:
                t_key = threading.Thread(target=self._power_key_loop, args=(p_path,),
                                         daemon=True, name=f"key-power-{os.path.basename(p_path)}")
                t_key.start()
                self._threads.append(t_key)
                self.log(f"已监听物理电源键输入: {p_path}")
        else:
            self.log("未发现单独的 KEY_POWER 输入设备")

    def stop(self):
        """Stop all monitoring threads and terminate child processes."""
        self._stopped = True
        if self._lipc_proc and self._lipc_proc.poll() is None:
            try:
                self._lipc_proc.terminate()
            except Exception:
                pass
        for dev in self._power_devices:
            try:
                dev.close()
            except Exception:
                pass
        self._power_devices.clear()
        if self.is_sleeping:
            pids = self._tracked_pids or read_paused_pids(self.pause_file)
            for pid in pids:
                try:
                    os.kill(pid, signal.SIGCONT)
                except OSError:
                    pass

    def _lipc_event_loop(self):
        """Listen to com.lab126.powerd state change events."""
        cmd = ["lipc-wait-event", "-m", "com.lab126.powerd", "goingToScreenSaver,outOfScreenSaver"]
        while not self._stopped:
            try:
                self._lipc_proc = subprocess.Popen(
                    cmd,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.DEVNULL,
                    text=True,
                    bufsize=1,
                )
                for line in self._lipc_proc.stdout:
                    if self._stopped:
                        break
                    event = line.strip()
                    if not event:
                        continue
                    if "goingToScreenSaver" in event:
                        self.log(f"捕获到休眠事件: {event}")
                        self.handle_suspend()
                    elif "outOfScreenSaver" in event:
                        self.log(f"捕获到唤醒事件: {event}")
                        self.handle_resume()
                if self._lipc_proc.poll() is not None and not self._stopped:
                    time.sleep(1.0)
            except Exception as exc:
                if self._stopped:
                    break
                self.log(f"lipc 进程异常: {exc}, 3秒后重启监听")
                time.sleep(3.0)

    def _power_key_loop(self, device_path):
        """Read evdev events from a power button device."""
        if not InputDevice or not ecodes:
            return
        try:
            dev = InputDevice(device_path)
            self._power_devices.append(dev)
            for event in dev.read_loop():
                if self._stopped:
                    break
                if event.type == ecodes.EV_KEY:
                    is_power = (event.code == ecodes.KEY_POWER or
                                event.code == getattr(ecodes, "KEY_POWER2", -1))
                    # value == 1 means key pressed down
                    if is_power and event.value == 1:
                        now = time.monotonic()
                        if now - self._last_power_press_at < 0.8:
                            continue
                        self._last_power_press_at = now
                        self.log(f"物理按键按下: code={event.code}")
                        self.handle_power_key()
        except Exception as exc:
            if not self._stopped:
                self.log(f"电源键设备 {device_path} 监听结束: {exc}")

    def handle_power_key(self):
        """Handle physical power key press."""
        with self._lock:
            if self.is_sleeping:
                # If LIPC is active, wait for native outOfScreenSaver event
                if not shutil.which("lipc-wait-event"):
                    self.log("休眠中收到电源键 (无 LIPC)，执行唤醒流程")
                    self._resume_locked()
                else:
                    self.log("休眠中收到电源键，等待系统 outOfScreenSaver 事件")
                return

            self.log("收到电源键，执行休眠前置流程")
            self._suspend_locked()

        # Let system know we are ready to sleep
        if shutil.which("lipc-set-prop"):
            try:
                subprocess.run(
                    ["lipc-set-prop", "com.lab126.powerd", "state", "screenSaver"],
                    timeout=2.0,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
            except Exception:
                pass

    def handle_suspend(self):
        """Handle system suspend (screen saver) trigger."""
        with self._lock:
            if self.is_sleeping:
                return
            self._suspend_locked()

    def _suspend_locked(self):
        """Perform suspend operations with lock held."""
        self.is_sleeping = True
        self.log("正在挂起应用，释放触摸屏并恢复系统 UI 进程...")

        # 1. Ungrab touchscreen device so native lockscreen can receive input
        try:
            if self.app.screen and self.app.screen.input and self.app.screen.input.device:
                self.app.screen.input.device.ungrab()
        except Exception as exc:
            self.log(f"释放触摸屏失败: {exc}")

        # 2. Resume paused system processes so system can render screensaver
        pids = read_paused_pids(self.pause_file)
        if not pids:
            pids = self._tracked_pids or find_fb_users()
        self._tracked_pids = list(pids)

        for pid in pids:
            try:
                os.kill(pid, signal.SIGCONT)
            except OSError:
                pass
        self.log(f"已向 {len(pids)} 个系统进程发送 SIGCONT")

    def handle_resume(self):
        """Handle system resume (out of screen saver) trigger."""
        with self._lock:
            if not self.is_sleeping:
                return
            self._resume_locked()

    def _resume_locked(self):
        """Perform resume operations with lock held."""
        self.log("正在恢复应用，准备重新接管屏幕...")

        # 1. Small delay to let Kindle kernel / powerd restore backlight & power
        time.sleep(0.35)

        # 2. Re-pause system processes to prevent framebuffer conflict
        pids = self._tracked_pids or read_paused_pids(self.pause_file) or find_fb_users()
        paused = []
        for pid in pids:
            try:
                os.kill(pid, signal.SIGSTOP)
                paused.append(pid)
            except OSError:
                pass
        write_paused_pids(paused, self.pause_file)
        self._tracked_pids = paused
        self.log(f"已向 {len(paused)} 个系统进程发送 SIGSTOP")

        # 3. Re-grab touchscreen device
        try:
            if self.app.screen and self.app.screen.input and self.app.screen.input.device:
                self.app.screen.input.device.grab()
                self.log("已重新独占触摸屏")
        except Exception as exc:
            self.log(f"重新独占触摸屏失败: {exc}")

        # 4. Refresh display with flashing mode to clear screensaver ghosting
        try:
            if self.app.context:
                self.log("正在以防残影模式重绘当前界面...")
                self.app.context.show(is_flashing=True)
        except Exception as exc:
            self.log(f"重绘当前界面失败: {exc}")

        self.is_sleeping = False
        self.log("屏幕与状态已完全恢复")
