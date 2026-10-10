//! 后台任务: 固定大小线程池, 结果 (任意 Send 消息 `T`) 回投 UI 线程。
//!
//! `Tasks::spawn(work)`: work 在工作线程执行并返回 `T`; UI 线程用 `drain()` 取回。
//! 投递后通过 `Waker` 写唤醒 fd 打断 poll。运行时用 `T = Delivery` 把结果路由到发起任务的页面实例。

use std::sync::{mpsc, Arc, Mutex};
use std::thread;

type Job<T> = Box<dyn FnOnce() -> T + Send + 'static>;

/// 跨线程唤醒主循环 (Linux: eventfd; 其它平台: 无操作, 主循环靠超时轮询)。
#[derive(Clone)]
pub struct Waker {
    #[cfg(target_os = "linux")]
    fd: Arc<OwnedEventFd>,
}

#[cfg(target_os = "linux")]
struct OwnedEventFd(libc::c_int);

#[cfg(target_os = "linux")]
impl Drop for OwnedEventFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl Waker {
    pub fn new() -> std::io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Waker { fd: Arc::new(OwnedEventFd(fd)) })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Waker {})
        }
    }

    pub fn wake(&self) {
        #[cfg(target_os = "linux")]
        unsafe {
            let one: u64 = 1;
            // 计数溢出/EAGAIN 说明已有未处理的唤醒, 忽略即可
            libc::write(self.fd.0, &one as *const u64 as *const libc::c_void, 8);
        }
    }

    /// 主循环 poll 的读端。
    #[cfg(unix)]
    pub fn fd(&self) -> std::os::fd::RawFd {
        #[cfg(target_os = "linux")]
        {
            self.fd.0
        }
        #[cfg(not(target_os = "linux"))]
        {
            -1
        }
    }

    /// 清空唤醒计数。
    pub fn drain(&self) {
        #[cfg(target_os = "linux")]
        unsafe {
            let mut value: u64 = 0;
            libc::read(self.fd.0, &mut value as *mut u64 as *mut libc::c_void, 8);
        }
    }
}

pub struct Tasks<T> {
    jobs: Option<mpsc::Sender<Job<T>>>,
    results_tx: mpsc::Sender<T>,
    results_rx: mpsc::Receiver<T>,
    waker: Waker,
    workers: Vec<thread::JoinHandle<()>>,
    /// 已提交但结果尚未送达的任务数
    pending: Arc<std::sync::atomic::AtomicUsize>,
}

impl<T: Send + 'static> Tasks<T> {
    /// `workers` 个工作线程 (设备上用 2)。
    pub fn new(workers: usize, waker: Waker) -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job<T>>();
        let (results_tx, results_rx) = mpsc::channel::<T>();
        let jobs_rx = Arc::new(Mutex::new(jobs_rx));
        let pending = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut handles = Vec::new();
        for index in 0..workers.max(1) {
            let jobs_rx = Arc::clone(&jobs_rx);
            let results_tx = results_tx.clone();
            let waker = waker.clone();
            let pending = Arc::clone(&pending);
            let handle = thread::Builder::new()
                .name(format!("kn-worker-{index}"))
                // 设备内存有限, 排版/解码不需要默认 2 MB 以上的栈
                .stack_size(512 * 1024)
                .spawn(move || loop {
                    let job = {
                        let guard = match jobs_rx.lock() {
                            Ok(guard) => guard,
                            Err(_) => return,
                        };
                        match guard.recv() {
                            Ok(job) => job,
                            Err(_) => return,
                        }
                    };
                    // 单个任务 panic 不应拖垮线程池 (release 为 panic=abort, 这里主要保护测试/调试构建)
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                    let sent = match result {
                        Ok(result) => results_tx.send(result).is_ok(),
                        Err(_) => true,
                    };
                    pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    if !sent {
                        return;
                    }
                    waker.wake();
                })
                .expect("spawn worker");
            handles.push(handle);
        }
        Tasks { jobs: Some(jobs_tx), results_tx, results_rx, waker, workers: handles, pending }
    }

    /// 提交后台工作; 返回值会经 `drain()` 交回 UI 线程。
    pub fn spawn<F>(&self, work: F)
    where
        F: FnOnce() -> T + Send + 'static,
    {
        if let Some(jobs) = &self.jobs {
            self.pending.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if jobs.send(Box::new(work)).is_err() {
                self.pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// 没有在途任务 (测试/预览用)。
    pub fn idle(&self) -> bool {
        self.pending.load(std::sync::atomic::Ordering::SeqCst) == 0
    }

    /// 可从任意线程直接投递消息 (例如网络线程)。
    pub fn poster(&self) -> Poster<T> {
        Poster { tx: self.results_tx.clone(), waker: self.waker.clone() }
    }

    /// 取出所有已完成的结果 (UI 线程调用)。
    pub fn drain(&self) -> Vec<T> {
        self.results_rx.try_iter().collect()
    }
}

impl<T> Drop for Tasks<T> {
    fn drop(&mut self) {
        // 关闭任务通道, 工作线程在 recv 失败后退出
        self.jobs.take();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

/// 可克隆、可跨线程的消息投递器。
pub struct Poster<T> {
    tx: mpsc::Sender<T>,
    waker: Waker,
}

impl<T> Clone for Poster<T> {
    fn clone(&self) -> Self {
        Poster { tx: self.tx.clone(), waker: self.waker.clone() }
    }
}

impl<T: Send + 'static> Poster<T> {
    pub fn post(&self, msg: T) {
        let _ = self.tx.send(msg);
        self.waker.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    type Cb<S> = Box<dyn FnOnce(&mut S) + Send>;

    fn wait_for<T: Send + 'static>(tasks: &Tasks<T>, n: usize) -> Vec<T> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while got.len() < n && Instant::now() < deadline {
            got.extend(tasks.drain());
            thread::sleep(Duration::from_millis(5));
        }
        got
    }

    #[test]
    fn results_run_on_caller_with_state() {
        let tasks: Tasks<Cb<Vec<u32>>> = Tasks::new(2, Waker::new().unwrap());
        for i in 0..10u32 {
            tasks.spawn(move || Box::new(move |state: &mut Vec<u32>| state.push(i * 2)) as Cb<Vec<u32>>);
        }
        let mut state = Vec::new();
        for cb in wait_for(&tasks, 10) {
            cb(&mut state);
        }
        state.sort();
        assert_eq!(state, (0..10).map(|i| i * 2).collect::<Vec<_>>());
    }

    #[test]
    fn poster_delivers_from_other_thread() {
        let tasks: Tasks<Cb<u32>> = Tasks::new(1, Waker::new().unwrap());
        let poster = tasks.poster();
        thread::spawn(move || poster.post(Box::new(|v: &mut u32| *v += 5))).join().unwrap();
        let mut v = 1;
        for cb in wait_for(&tasks, 1) {
            cb(&mut v);
        }
        assert_eq!(v, 6);
    }

    #[test]
    fn panicking_job_does_not_kill_pool() {
        let tasks: Tasks<Cb<u32>> = Tasks::new(1, Waker::new().unwrap());
        tasks.spawn(|| panic!("boom"));
        tasks.spawn(|| Box::new(|v: &mut u32| *v = 7) as Cb<u32>);
        let mut v = 0;
        for cb in wait_for(&tasks, 1) {
            cb(&mut v);
        }
        assert_eq!(v, 7);
    }
}
