//! 电源事件与系统框架协作。
//!
//! 参考 Python `bin/src/kinnovel/power.py` 与 rust/research-kindle-devices.md:
//! - 启动 `lipc-wait-event -m com.lab126.powerd goingToScreenSaver,outOfScreenSaver,readyToSuspend`
//!   子进程, 逐行解析 stdout; 其 stdout fd 交给主循环 poll。子进程意外退出时自动重启 (带退避)。
//! - 看门狗: 进入休眠后若 30 s 内没有 outOfScreenSaver, 以 15 s 间隔调用 `lipc-get-prop com.lab126.powerd state`
//!   自检, 发现已经是 active 则合成 `OutOfScreenSaver` (防止事件丢失卡在休眠态)。
//! - 框架进程 (占用 /dev/fb0 的 awesome/cvm 等) 由启动脚本 SIGSTOP; 休眠时需要 SIGCONT 让系统锁屏显示,
//!   唤醒后再 SIGSTOP。PID 列表由启动脚本写入 `/tmp/kinnovel_paused_pids` (与 Python 版相同)。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerEvent {
    GoingToScreenSaver,
    OutOfScreenSaver,
    ReadyToSuspend,
}

#[cfg(unix)]
mod unix_impl {
    use super::PowerEvent;
    use std::os::fd::{AsRawFd, RawFd};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const PAUSE_FILE: &str = "/tmp/kinnovel_paused_pids";

    const WATCHDOG_FAST_INTERVAL_MS: u64 = 2_000;
    const WATCHDOG_FAST_WINDOW_MS: u64 = 30_000;
    const WATCHDOG_SLOW_INTERVAL_MS: u64 = 15_000;

    const RESTART_BACKOFF_BASE_MS: u64 = 1_000;
    const RESTART_BACKOFF_MAX_MS: u64 = 30_000;

    fn read_paused_pids(path: &Path) -> Vec<i32> {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        content
            .lines()
            .filter_map(|l| l.trim().parse::<i32>().ok())
            .collect()
    }

    fn set_nonblocking(fd: RawFd) {
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
    }

    fn spawn_lipc_wait_event() -> Option<Child> {
        Command::new("lipc-wait-event")
            .args([
                "-m",
                "com.lab126.powerd",
                "goingToScreenSaver,outOfScreenSaver,readyToSuspend",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    }

    fn query_powerd_state() -> Option<String> {
        let out = Command::new("lipc-get-prop")
            .args(["com.lab126.powerd", "state"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn is_sleep_state(state: &str) -> bool {
        let s = state.to_lowercase();
        s.contains("screensaver") || s.contains("suspend")
    }

    pub struct PowerMonitor {
        pause_file: PathBuf,
        child: Option<Child>,
        child_fd: Option<RawFd>,
        buf: Vec<u8>,
        is_sleeping: bool,
        tracked_pids: Vec<i32>,
        // Restart backoff state.
        backoff_ms: u64,
        next_restart_at: Option<Instant>,
        // Watchdog state.
        sleep_started_at: Option<Instant>,
        next_watchdog_at: Option<Instant>,
        consecutive_active: u32,
        lipc_available: bool,
    }

    impl PowerMonitor {
        pub fn start() -> Self {
            let lipc_available = which("lipc-wait-event") && which("lipc-get-prop");
            let mut mon = PowerMonitor {
                pause_file: PathBuf::from(PAUSE_FILE),
                child: None,
                child_fd: None,
                buf: Vec::new(),
                is_sleeping: false,
                tracked_pids: Vec::new(),
                backoff_ms: RESTART_BACKOFF_BASE_MS,
                next_restart_at: None,
                sleep_started_at: None,
                next_watchdog_at: None,
                consecutive_active: 0,
                lipc_available,
            };
            if lipc_available {
                mon.try_spawn();
            }
            mon
        }

        fn try_spawn(&mut self) {
            if let Some(mut child) = spawn_lipc_wait_event() {
                if let Some(stdout) = child.stdout.take() {
                    let fd = stdout.as_raw_fd();
                    set_nonblocking(fd);
                    // Leak the ChildStdout's ownership into our own fd bookkeeping;
                    // we read directly via the raw fd and close by dropping `child`.
                    std::mem::forget(stdout);
                    self.child_fd = Some(fd);
                }
                self.child = Some(child);
                self.backoff_ms = RESTART_BACKOFF_BASE_MS;
                self.next_restart_at = None;
            } else {
                self.schedule_restart();
            }
        }

        fn schedule_restart(&mut self) {
            self.next_restart_at = Some(Instant::now() + Duration::from_millis(self.backoff_ms));
            self.backoff_ms = (self.backoff_ms * 2).min(RESTART_BACKOFF_MAX_MS);
        }

        pub fn fd(&self) -> Option<RawFd> {
            self.child_fd
        }

        fn handle_line(&mut self, line: &str, out: &mut Vec<PowerEvent>) {
            if line.contains("goingToScreenSaver") {
                self.is_sleeping = true;
                self.sleep_started_at = Some(Instant::now());
                self.consecutive_active = 0;
                out.push(PowerEvent::GoingToScreenSaver);
            } else if line.contains("outOfScreenSaver") {
                self.is_sleeping = false;
                self.sleep_started_at = None;
                self.next_watchdog_at = None;
                out.push(PowerEvent::OutOfScreenSaver);
            } else if line.contains("readyToSuspend") {
                out.push(PowerEvent::ReadyToSuspend);
            }
        }

        fn drain_child_output(&mut self, out: &mut Vec<PowerEvent>) {
            let Some(fd) = self.child_fd else { return };
            let mut tmp = [0u8; 512];
            loop {
                let n = unsafe { libc::read(fd, tmp.as_mut_ptr() as *mut _, tmp.len()) };
                if n > 0 {
                    self.buf.extend_from_slice(&tmp[..n as usize]);
                    continue;
                }
                if n == 0 {
                    // EOF: child's stdout closed, process likely exiting.
                    break;
                }
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EAGAIN) {
                    break;
                }
                break;
            }
            while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)]).into_owned();
                let line = line.trim().to_string();
                if !line.is_empty() {
                    self.handle_line(&line, out);
                }
            }
        }

        fn check_child_alive(&mut self) {
            let exited = match &mut self.child {
                Some(child) => matches!(child.try_wait(), Ok(Some(_))),
                None => false,
            };
            if exited {
                self.child = None;
                self.close_child_fd();
                self.schedule_restart();
            }
        }

        /// stdout 管道在 try_spawn 中脱离了 ChildStdout 的所有权, 必须手动关闭, 否则每次重启泄漏一个 fd。
        fn close_child_fd(&mut self) {
            if let Some(fd) = self.child_fd.take() {
                unsafe {
                    libc::close(fd);
                }
            }
        }

        fn maybe_restart(&mut self) {
            if self.child.is_some() || !self.lipc_available {
                return;
            }
            if let Some(at) = self.next_restart_at {
                if Instant::now() >= at {
                    self.try_spawn();
                }
            } else {
                self.try_spawn();
            }
        }

        fn watchdog_interval(&self) -> Duration {
            let elapsed = self
                .sleep_started_at
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            if elapsed < WATCHDOG_FAST_WINDOW_MS {
                Duration::from_millis(WATCHDOG_FAST_INTERVAL_MS)
            } else {
                Duration::from_millis(WATCHDOG_SLOW_INTERVAL_MS)
            }
        }

        fn run_watchdog(&mut self, out: &mut Vec<PowerEvent>) {
            if !self.is_sleeping || !self.lipc_available {
                self.next_watchdog_at = None;
                return;
            }
            let now = Instant::now();
            if let Some(next_at) = self.next_watchdog_at {
                if now < next_at {
                    return;
                }
            }
            self.next_watchdog_at = Some(now + self.watchdog_interval());

            let Some(state) = query_powerd_state() else {
                return;
            };
            if is_sleep_state(&state) {
                self.consecutive_active = 0;
                return;
            }
            self.consecutive_active += 1;
            if self.consecutive_active >= 2 {
                // powerd reports "active" twice in a row while we still think
                // we're sleeping: we must have missed outOfScreenSaver. Self-heal.
                self.is_sleeping = false;
                self.sleep_started_at = None;
                self.next_watchdog_at = None;
                self.consecutive_active = 0;
                out.push(PowerEvent::OutOfScreenSaver);
            }
        }

        pub fn poll_events(&mut self, out: &mut Vec<PowerEvent>) {
            self.check_child_alive();
            self.maybe_restart();
            self.drain_child_output(out);
            self.run_watchdog(out);
        }

        pub fn next_deadline_ms(&self) -> Option<u64> {
            let mut deadlines: Vec<u64> = Vec::new();
            if let Some(at) = self.next_restart_at {
                let now = Instant::now();
                deadlines.push(if at > now {
                    (at - now).as_millis() as u64
                } else {
                    0
                });
            }
            if self.is_sleeping {
                let now = Instant::now();
                let at = self
                    .next_watchdog_at
                    .unwrap_or(now + self.watchdog_interval());
                deadlines.push(if at > now {
                    (at - now).as_millis() as u64
                } else {
                    0
                });
            }
            deadlines.into_iter().min()
        }

        pub fn release_framework(&self) {
            let pids = if self.tracked_pids.is_empty() {
                read_paused_pids(&self.pause_file)
            } else {
                self.tracked_pids.clone()
            };
            for pid in pids {
                unsafe {
                    libc::kill(pid, libc::SIGCONT);
                }
            }
        }

        pub fn reclaim_framework(&self) {
            let pids = read_paused_pids(&self.pause_file);
            for &pid in &pids {
                unsafe {
                    libc::kill(pid, libc::SIGSTOP);
                }
            }
        }
    }

    impl Drop for PowerMonitor {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.close_child_fd();
        }
    }

    fn which(bin: &str) -> bool {
        let Ok(path_var) = std::env::var("PATH") else {
            return false;
        };
        let sep = if cfg!(windows) { ';' } else { ':' };
        path_var.split(sep).any(|dir| {
            let p = Path::new(dir).join(bin);
            p.is_file()
        })
    }
}

#[cfg(unix)]
pub use unix_impl::PowerMonitor;

#[cfg(not(unix))]
pub struct PowerMonitor;

#[cfg(not(unix))]
impl PowerMonitor {
    /// 启动 lipc 监听; 设备上没有 lipc (主机) 时返回一个永远不产生事件的监视器。
    pub fn start() -> Self {
        PowerMonitor
    }

    pub fn poll_events(&mut self, _out: &mut Vec<PowerEvent>) {}

    pub fn next_deadline_ms(&self) -> Option<u64> {
        None
    }

    /// 休眠: 继续 (SIGCONT) 被暂停的框架进程, 让系统锁屏接管屏幕。
    pub fn release_framework(&self) {}

    /// 唤醒: 重新暂停 (SIGSTOP) 框架进程。
    pub fn reclaim_framework(&self) {}
}
