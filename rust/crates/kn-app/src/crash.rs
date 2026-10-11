//! 崩溃记录。
//!
//! release 用 `panic = "abort"`: 任何线程 panic 都会终止整个进程, 启动脚本随后恢复框架进程并回到系统主页。
//! 这里的钩子在终止前把线程名、位置、消息和调用栈 (函数名; 发布版保留符号表、去掉调试信息) 写进日志
//! (stderr 由启动脚本重定向到 `logs/kinnovel.log`), 并留下 `logs/last_crash.txt`。
//! 被信号杀死 (例如 SIGSEGV) 时没有 Rust 钩子可跑, 由启动脚本按退出码写同一个文件。
//! 下次启动时读出并删除它, 书架页提示一次。

use std::path::PathBuf;

use crate::store::Paths;

impl Paths {
    pub fn crash_marker(&self) -> PathBuf {
        self.app_dir.join("logs").join("last_crash.txt")
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn install_panic_hook(paths: &Paths) {
    let marker = paths.crash_marker();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(非字符串 panic)".into());
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("未命名").to_string();
        let backtrace = std::backtrace::Backtrace::force_capture();
        eprintln!(
            "[panic] kinnovel {} 线程 '{thread}' 在 {location} 崩溃: {msg}\n{backtrace}",
            env!("CARGO_PKG_VERSION")
        );
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&marker, format!("{msg} ({location})"));
    }));
}

/// 上次异常退出的说明 (读出后删除标记文件)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn take_crash_notice(paths: &Paths) -> Option<String> {
    let marker = paths.crash_marker();
    let text = std::fs::read_to_string(&marker).ok()?;
    let _ = std::fs::remove_file(&marker);
    eprintln!("[crash] 上次异常退出: {}", text.trim());
    // 提示条放不下完整消息, 详情 (消息、位置、调用栈) 在日志里
    Some("上次异常退出, 详情见 logs/kinnovel.log".to_string())
}
