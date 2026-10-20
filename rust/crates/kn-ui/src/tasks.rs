//! 后台任务: 固定大小线程池, 结果 (任意 Send 消息 `T`) 回投 UI 线程。
//!
//! `Tasks::spawn(work)`: work 在工作线程执行并返回 `T`; UI 线程用 `drain()` 取回。
//! 两条队列 (`Lane`): 网络 (默认, 几个线程) 与本地 (解析/解码/排版, 单独的线程), 卡住的网络任务挡不住本地任务。
//! 投递后通过 `Waker` 写唤醒 fd 打断 poll。运行时用 `T = Delivery` 把结果路由到发起任务的页面实例。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

type Job<T> = Box<dyn FnOnce() -> Option<T> + Send + 'static>;

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

/// 任务走哪条队列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// 可能卡在网络上的任务 (默认): 一次 hub 调用在网络不好时能占住线程好几分钟
    Net,
    /// 纯本地的解析 / 解码 / 排版 / 读写缓存: 单独的线程, 不被卡住的网络任务挡住
    Local,
}

/// 工作线程栈大小。章节 HTML 嵌套很深时解析 / 分块是递归的, 512 KB 曾被撑爆 (P0-1)。
/// Linux 线程栈按需映射, 实际占用 (RSS) 只随用到的深度增长; 32 位进程里 4 条线程共保留 16 MB 虚拟地址, 可以接受。
const WORKER_STACK: usize = 4 * 1024 * 1024;

/// 可克隆、可跨线程的任务提交端 (任务里也能接着提交下一步, 例如读缓存未命中再下载)。
pub struct Submitter<T> {
    net: mpsc::Sender<Job<T>>,
    local: mpsc::Sender<Job<T>>,
    pending: Arc<AtomicUsize>,
}

impl<T> Clone for Submitter<T> {
    fn clone(&self) -> Self {
        Submitter { net: self.net.clone(), local: self.local.clone(), pending: Arc::clone(&self.pending) }
    }
}

impl<T: Send + 'static> Submitter<T> {
    /// 提交到 `lane`; `work` 返回 None 表示跳过 (没有结果要送回)。
    pub fn submit<F>(&self, lane: Lane, work: F)
    where
        F: FnOnce() -> Option<T> + Send + 'static,
    {
        let jobs = match lane {
            Lane::Net => &self.net,
            Lane::Local => &self.local,
        };
        self.pending.fetch_add(1, Ordering::SeqCst);
        if jobs.send(Box::new(work)).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

pub struct Tasks<T> {
    submitter: Option<Submitter<T>>,
    results_tx: mpsc::Sender<T>,
    results_rx: mpsc::Receiver<T>,
    waker: Waker,
    workers: Vec<thread::JoinHandle<()>>,
    /// 已提交但结果尚未送达 (或已跳过) 的任务数
    pending: Arc<AtomicUsize>,
}

impl<T: Send + 'static> Tasks<T> {
    /// `workers` 个网络线程 + 1 个本地线程。
    pub fn new(workers: usize, waker: Waker) -> Self {
        Self::with_lanes(workers, 1, waker)
    }

    /// 网络队列 `net` 个线程, 本地队列 `local` 个线程 (各至少 1 个)。
    pub fn with_lanes(net: usize, local: usize, waker: Waker) -> Self {
        let (results_tx, results_rx) = mpsc::channel::<T>();
        let pending = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        let mut lane = |name: &str, count: usize| {
            let (jobs_tx, jobs_rx) = mpsc::channel::<Job<T>>();
            let jobs_rx = Arc::new(Mutex::new(jobs_rx));
            for index in 0..count.max(1) {
                let jobs_rx = Arc::clone(&jobs_rx);
                let results_tx = results_tx.clone();
                let waker = waker.clone();
                let pending = Arc::clone(&pending);
                let handle = thread::Builder::new()
                    .name(format!("kn-{name}-{index}"))
                    .stack_size(WORKER_STACK)
                    .spawn(move || worker_loop(&jobs_rx, &results_tx, &waker, &pending))
                    .expect("spawn worker");
                workers.push(handle);
            }
            jobs_tx
        };
        let net = lane("net", net);
        let local = lane("local", local);
        let submitter = Submitter { net, local, pending: Arc::clone(&pending) };
        Tasks { submitter: Some(submitter), results_tx, results_rx, waker, workers, pending }
    }

    /// 提交后台工作 (网络队列); 返回值会经 `drain()` 交回 UI 线程。
    pub fn spawn<F>(&self, work: F)
    where
        F: FnOnce() -> T + Send + 'static,
    {
        self.submit(Lane::Net, move || Some(work()));
    }

    /// 提交到指定队列; `work` 返回 None 表示跳过。
    pub fn submit<F>(&self, lane: Lane, work: F)
    where
        F: FnOnce() -> Option<T> + Send + 'static,
    {
        if let Some(s) = &self.submitter {
            s.submit(lane, work);
        }
    }

    /// 提交端 (给任务里继续提交用)。线程池已关闭时为 None。
    pub fn submitter(&self) -> Option<Submitter<T>> {
        self.submitter.clone()
    }

    /// 没有在途任务 (测试/预览用)。
    pub fn idle(&self) -> bool {
        self.pending.load(Ordering::SeqCst) == 0
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

fn worker_loop<T>(jobs_rx: &Mutex<mpsc::Receiver<Job<T>>>, results_tx: &mpsc::Sender<T>, waker: &Waker, pending: &AtomicUsize) {
    loop {
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
        // 跳过的任务 (None) 与 panic 都没有结果要送
        let sent = match result {
            Ok(Some(result)) => Some(results_tx.send(result).is_ok()),
            Ok(None) | Err(_) => None,
        };
        pending.fetch_sub(1, Ordering::SeqCst);
        match sent {
            Some(false) => return,
            Some(true) => waker.wake(),
            None => {}
        }
    }
}

impl<T> Drop for Tasks<T> {
    fn drop(&mut self) {
        // 关闭任务通道, 空闲的工作线程在 recv 失败后退出 (任务里留着的 Submitter 会让通道多活一会儿, 不要紧)。
        // 不 join: 正在跑的任务可能卡在网络读
        // (每次读最多 30 s, 下载封面时可能连着好几次), 退出时等它会让程序迟迟不退,
        // 而启动脚本要等程序退出才恢复框架 —— 屏幕就一直停在 KinNovel 上。进程退出时线程随之结束。
        self.submitter.take();
        self.workers.clear();
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
    fn dropping_the_pool_does_not_wait_for_a_running_job() {
        let tasks: Tasks<u32> = Tasks::new(2, Waker::new().unwrap());
        // 模拟卡在网络读上的任务
        tasks.spawn(|| {
            thread::sleep(Duration::from_secs(5));
            1
        });
        thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        drop(tasks);
        assert!(started.elapsed() < Duration::from_secs(1), "drop waited {:?}", started.elapsed());
    }

    #[test]
    fn stuck_net_jobs_do_not_block_local_jobs() {
        let tasks: Tasks<u32> = Tasks::with_lanes(2, 1, Waker::new().unwrap());
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Arc::new(Mutex::new(release_rx));
        // 两个网络线程都卡住, 再排一个网络任务
        for i in 0..3u32 {
            let rx = Arc::clone(&release_rx);
            tasks.spawn(move || {
                let _ = rx.lock().unwrap().recv_timeout(Duration::from_secs(10));
                i
            });
        }
        tasks.submit(Lane::Local, || Some(99));
        assert_eq!(wait_for(&tasks, 1), vec![99]);
        assert!(!tasks.idle());
        for _ in 0..3 {
            release_tx.send(()).unwrap();
        }
        let mut rest = wait_for(&tasks, 3);
        rest.sort();
        assert_eq!(rest, vec![0, 1, 2]);
    }

    #[test]
    fn skipped_job_delivers_nothing_and_counts_as_done() {
        let tasks: Tasks<u32> = Tasks::new(1, Waker::new().unwrap());
        tasks.submit(Lane::Net, || None);
        tasks.submit(Lane::Local, || None);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tasks.idle() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(tasks.idle());
        assert!(tasks.drain().is_empty());
    }

    #[test]
    fn submitter_can_chain_from_a_job() {
        let tasks: Tasks<u32> = Tasks::new(1, Waker::new().unwrap());
        let next = tasks.submitter().unwrap();
        // 本地任务 "缓存未命中" → 接着交给网络队列
        tasks.submit(Lane::Local, move || {
            next.submit(Lane::Net, || Some(2));
            Some(1)
        });
        let mut got = wait_for(&tasks, 2);
        got.sort();
        assert_eq!(got, vec![1, 2]);
    }

    #[test]
    fn worker_stack_fits_deep_recursion() {
        // 约 1.5 MB 的递归: 512 KB 的栈会溢出 (P0-1)
        fn deep(n: u32) -> u32 {
            let pad = [n as u8; 1024];
            if n == 0 {
                pad[0] as u32
            } else {
                deep(n - 1) + std::hint::black_box(pad)[1] as u32
            }
        }
        let tasks: Tasks<u32> = Tasks::new(1, Waker::new().unwrap());
        tasks.submit(Lane::Local, || Some(deep(1536)));
        assert_eq!(wait_for(&tasks, 1).len(), 1);
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
