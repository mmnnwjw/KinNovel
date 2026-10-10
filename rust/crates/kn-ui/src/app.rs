//! 页面栈与主循环。
//!
//! 页面 (`Page`) 只关心: 进入/离开、渲染、处理输入、接收自己发起的后台任务结果、空闲工作。
//! 导航通过返回 `Transition` 表达, 由运行时统一维护页面栈 (最多 20 层, 与 Python 版一致)。
//!
//! 后台任务结果按"页面实例"路由: 每个入栈的页面分配唯一实例号, `cx.spawn` 记住发起者,
//! 结果只投递给仍在栈中的那个实例 (`Page::on_message`); 页面已关闭则丢弃 ——
//! 页面状态因此可以完全放在页面自身, 不需要 Python 版那样的模块级全局 STATE。

use std::any::Any;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kn_platform::{Display, GestureKind, InputEvent, InputReader, KeyCode, PowerEvent, PowerMonitor, SwipeDir};

/// 按下电源键后等待系统进入屏保的时限; 超时说明这次按键没有引起休眠, 重新接管屏幕。
const POWER_KEY_GRACE: Duration = Duration::from_secs(15);
use kn_render::{Bitmap, FontStore, GlyphCache, Point};

use crate::hits::Hits;
use crate::refresh::{RefreshHint, RefreshScheduler};
use crate::tasks::{Poster, Tasks, Waker};
use crate::theme::Theme;

/// 页面标识 (应用层定义的枚举序号)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageId(pub u16);

/// 页面处理事件后的导航意图。
pub enum Transition<A> {
    None,
    /// 压栈进入新页面
    Push(Box<dyn Page<A>>),
    /// 替换当前页面 (章节切换)
    Replace(Box<dyn Page<A>>),
    /// 返回上一页 (栈空则回首页)
    Back,
    /// 清栈回首页
    Home,
    /// 清栈并以该页面为根 (标签页切换)
    Root(Box<dyn Page<A>>),
    /// 退出应用
    Exit,
}

/// 应用状态需要实现的最小接口。
pub trait App: Sized + 'static {
    fn home_page(&mut self) -> Box<dyn Page<Self>>;
    fn theme(&self) -> Theme;
    /// 每分钟时钟走动时调用 (顶栏时间/电量), 返回是否需要重绘。
    fn on_minute(&mut self) -> bool {
        true
    }
    /// 休眠前 (落盘阅读进度等)。
    fn on_suspend(&mut self) {}
    fn on_resume(&mut self) {}
}

/// 后台结果的投递目标。
pub enum Delivery<A> {
    /// 交给应用全局状态
    App(Box<dyn FnOnce(&mut A) + Send>),
    /// 交给某个页面实例
    Page { instance: u64, msg: Box<dyn Any + Send> },
}

/// 渲染/事件处理时可用的上下文。
pub struct Cx<'a, A: App> {
    pub app: &'a mut A,
    pub fonts: &'a mut FontStore,
    pub glyphs: &'a mut GlyphCache,
    pub hits: &'a mut Hits,
    pub theme: Theme,
    pub width: u32,
    pub height: u32,
    instance: u64,
    tasks: &'a Tasks<Delivery<A>>,
    redraw: &'a mut Option<RefreshHint>,
    swipe: &'a mut Option<SwipeDir>,
    timers: &'a mut Timers,
}

/// 定时器: (到期时刻, 页面实例, 页面自定义 token)。数量很少 (提示条、轮询), 用 Vec 即可。
#[derive(Default)]
pub(crate) struct Timers {
    items: Vec<(std::time::Instant, u64, u32)>,
}

impl Timers {
    fn add(&mut self, at: std::time::Instant, instance: u64, token: u32) {
        // 同一实例的同一 token 只保留最新的一个 (例如提示条重复出现时重新计时)
        self.items.retain(|(_, i, t)| !(*i == instance && *t == token));
        self.items.push((at, instance, token));
    }

    /// 距最近一个到期还有多少毫秒。
    fn next_ms(&self) -> Option<u64> {
        let now = std::time::Instant::now();
        self.items.iter().map(|(at, _, _)| at.saturating_duration_since(now).as_millis() as u64).min()
    }

    fn take_due(&mut self) -> Vec<(u64, u32)> {
        let now = std::time::Instant::now();
        let mut due = Vec::new();
        self.items.retain(|(at, i, t)| {
            if *at <= now {
                due.push((*i, *t));
                false
            } else {
                true
            }
        });
        due
    }
}

impl<'a, A: App> Cx<'a, A> {
    /// 请求重绘; 多次请求合并, 取最强的 hint。
    pub fn request_redraw(&mut self, hint: RefreshHint) {
        *self.redraw = Some(match *self.redraw {
            Some(old) if old.strength() >= hint.strength() => old,
            _ => hint,
        });
    }

    /// 阅读翻页: 等同 `request_redraw(Turn)`, 并请求 MTK 原生翻页动画 (方向 = 内容移动方向;
    /// 设备不支持时显示层忽略, 本次要闪刷时动画让位)。
    pub fn request_turn(&mut self, swipe: Option<SwipeDir>) {
        self.request_redraw(RefreshHint::Turn);
        *self.swipe = swipe;
    }

    /// 翻页且整屏闪刷 (漫画: 大面积灰阶, 每页清残影), 同样带 MTK 翻页动画 —— FBInk 的动画标志与
    /// 闪刷互不排斥 (UPDATE_MODE_FULL + 动画)。小说阅读页不用它: 残影预算触发的闪刷仍不带动画。
    pub fn request_flash_turn(&mut self, swipe: Option<SwipeDir>) {
        self.request_redraw(RefreshHint::Flash);
        *self.swipe = swipe;
    }

    /// `delay` 之后调用当前页面实例的 `on_timer(token)` (页面已关闭则丢弃)。同一 token 重设会覆盖旧的。
    pub fn after(&mut self, delay: Duration, token: u32) {
        self.timers.add(std::time::Instant::now() + delay, self.instance, token);
    }

    /// 后台执行 `work`, 其返回值作为消息交给当前页面实例的 `on_message`。
    pub fn spawn<F, M>(&self, work: F)
    where
        F: FnOnce() -> M + Send + 'static,
        M: Any + Send + 'static,
    {
        let instance = self.instance;
        self.tasks.spawn(move || Delivery::Page { instance, msg: Box::new(work()) });
    }

    /// 后台执行, 结果回调作用于应用全局状态。
    pub fn spawn_app<F>(&self, work: F)
    where
        F: FnOnce() -> Box<dyn FnOnce(&mut A) + Send> + Send + 'static,
    {
        self.tasks.spawn(move || Delivery::App(work()));
    }

    /// 给其它线程 (如网络线程) 用的投递器, 消息发往当前页面实例。
    pub fn page_poster(&self) -> PagePoster<A> {
        PagePoster { instance: self.instance, poster: self.tasks.poster() }
    }

    /// 投递到应用全局状态的投递器。
    pub fn app_poster(&self) -> Poster<Delivery<A>> {
        self.tasks.poster()
    }
}

/// 绑定到某个页面实例的跨线程投递器。
pub struct PagePoster<A> {
    instance: u64,
    poster: Poster<Delivery<A>>,
}

impl<A> Clone for PagePoster<A> {
    fn clone(&self) -> Self {
        PagePoster { instance: self.instance, poster: self.poster.clone() }
    }
}

impl<A: 'static> PagePoster<A> {
    pub fn post<M: Any + Send + 'static>(&self, msg: M) {
        self.poster.post(Delivery::Page { instance: self.instance, msg: Box::new(msg) });
    }
}

pub trait Page<A: App> {
    /// 用于调试与"同类页面"判断。
    fn id(&self) -> PageId;

    /// 进入页面 (首次进入 returning = false; 从上层返回 returning = true)。
    fn enter(&mut self, _cx: &mut Cx<A>, _returning: bool) {}

    /// 离开页面 (被压栈或弹出前)。
    fn leave(&mut self, _cx: &mut Cx<A>) {}

    /// 整帧渲染到 `frame` (已用主题背景色清空) 并登记命中区域 (`cx.hits` 已清空)。
    fn render(&mut self, cx: &mut Cx<A>, frame: &mut Bitmap);

    /// 处理手势/按键。`Down` 手势也会送达 (多数页面忽略即可)。
    fn on_input(&mut self, cx: &mut Cx<A>, event: &InputEvent) -> Transition<A>;

    /// 接收本页面实例发起的后台任务结果。
    fn on_message(&mut self, _cx: &mut Cx<A>, _msg: Box<dyn Any + Send>) {}

    /// `cx.after` 设的定时器到期。
    fn on_timer(&mut self, _cx: &mut Cx<A>, _token: u32) {}

    /// 即将休眠 (落盘阅读进度等)。在 `App::on_suspend` 之前调用。
    fn on_suspend(&mut self, _cx: &mut Cx<A>) {}

    /// 页面自己画满整帧 (例如阅读页直接拷贝缓存好的正文位图): 运行时跳过渲染前的背景清空 (整屏 ~4 ms)。
    fn opaque(&self) -> bool {
        false
    }

    /// 进入/返回本页面时运行时是否整屏闪刷 (默认是)。内容异步加载的页面 (阅读页) 返回 false,
    /// 改为内容就绪时自己请求 `RefreshHint::Flash`, 免得闪刷落在 "加载中" 画面上、正文反而留下残影。
    fn flash_on_enter(&self) -> bool {
        true
    }

    /// 空闲时调用 (无输入、无手指按下); 返回 true 表示还有空闲工作要做。
    /// 每次调用应在 ~50 ms 内返回 (例如只预渲染一页)。
    fn on_idle(&mut self, _cx: &mut Cx<A>) -> bool {
        false
    }
}

const MAX_STACK: usize = 20;

struct Entry<A> {
    instance: u64,
    page: Box<dyn Page<A>>,
}

/// 运行时状态 (除页面栈外的部分), 拆开是为了能同时借用页面与上下文资源。
struct Runtime<A: App> {
    app: A,
    fonts: FontStore,
    glyphs: GlyphCache,
    hits: Hits,
    tasks: Tasks<Delivery<A>>,
    redraw: Option<RefreshHint>,
    swipe: Option<SwipeDir>,
    width: u32,
    height: u32,
    next_instance: u64,
    timers: Timers,
}

impl<A: App> Runtime<A> {
    fn cx(&mut self, instance: u64) -> Cx<'_, A> {
        let theme = self.app.theme();
        Cx {
            app: &mut self.app,
            fonts: &mut self.fonts,
            glyphs: &mut self.glyphs,
            hits: &mut self.hits,
            theme,
            width: self.width,
            height: self.height,
            instance,
            tasks: &self.tasks,
            redraw: &mut self.redraw,
            swipe: &mut self.swipe,
            timers: &mut self.timers,
        }
    }

    fn new_entry(&mut self, page: Box<dyn Page<A>>) -> Entry<A> {
        self.next_instance += 1;
        Entry { instance: self.next_instance, page }
    }
}

/// 页面栈操作 (纯逻辑, 便于测试)。
struct Stack<A> {
    entries: Vec<Entry<A>>,
}

impl<A: App> Stack<A> {
    fn top(&mut self) -> &mut Entry<A> {
        self.entries.last_mut().expect("page stack never empty")
    }

    fn enter_top(&mut self, rt: &mut Runtime<A>, returning: bool) {
        let top = self.top();
        let instance = top.instance;
        top.page.enter(&mut rt.cx(instance), returning);
        rt.redraw_ui();
    }

    fn leave_top(&mut self, rt: &mut Runtime<A>) {
        let top = self.top();
        let instance = top.instance;
        top.page.leave(&mut rt.cx(instance));
    }

    /// 执行导航; 返回 false 表示应退出。
    fn apply(&mut self, rt: &mut Runtime<A>, transition: Transition<A>) -> bool {
        let nav_flash = matches!(transition, Transition::Push(_) | Transition::Replace(_) | Transition::Back | Transition::Home);
        match transition {
            Transition::None => {}
            Transition::Push(page) => {
                self.leave_top(rt);
                let entry = rt.new_entry(page);
                self.entries.push(entry);
                if self.entries.len() > MAX_STACK {
                    // 最底层永远是首页, 丢弃次底层
                    self.entries.remove(1);
                }
                self.enter_top(rt, false);
            }
            Transition::Replace(page) => {
                self.leave_top(rt);
                self.entries.pop();
                let entry = rt.new_entry(page);
                self.entries.push(entry);
                self.enter_top(rt, false);
            }
            Transition::Back => {
                self.leave_top(rt);
                self.entries.pop();
                if self.entries.is_empty() {
                    let home = rt.app.home_page();
                    let entry = rt.new_entry(home);
                    self.entries.push(entry);
                    self.enter_top(rt, false);
                } else {
                    self.enter_top(rt, true);
                }
            }
            Transition::Home => {
                self.leave_top(rt);
                self.entries.clear();
                let home = rt.app.home_page();
                let entry = rt.new_entry(home);
                self.entries.push(entry);
                self.enter_top(rt, false);
            }
            Transition::Root(page) => {
                self.leave_top(rt);
                self.entries.clear();
                let entry = rt.new_entry(page);
                self.entries.push(entry);
                self.enter_top(rt, false);
            }
            Transition::Exit => {
                self.leave_top(rt);
                return false;
            }
        }
        if nav_flash && self.top().page.flash_on_enter() {
            // 进入/离开二级页面: 整屏闪刷, 清掉上一页的残影 (含被点那一行的按下反相)。
            // 切换标签 (Root) 画面结构相同, 走普通刷新 + 残影预算。
            rt.redraw = Some(RefreshHint::Flash);
        }
        true
    }

    fn fire_timers(&mut self, rt: &mut Runtime<A>) -> bool {
        let due = rt.timers.take_due();
        let any = !due.is_empty();
        for (instance, token) in due {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.instance == instance) {
                entry.page.on_timer(&mut rt.cx(instance), token);
            }
        }
        any
    }

    fn deliver(&mut self, rt: &mut Runtime<A>, delivery: Delivery<A>) {
        match delivery {
            Delivery::App(callback) => callback(&mut rt.app),
            Delivery::Page { instance, msg } => {
                if let Some(entry) = self.entries.iter_mut().find(|e| e.instance == instance) {
                    entry.page.on_message(&mut rt.cx(instance), msg);
                }
            }
        }
    }
}

impl<A: App> Runtime<A> {
    fn redraw_ui(&mut self) {
        if self.redraw.map_or(true, |h| h.strength() < RefreshHint::Ui.strength()) {
            self.redraw = Some(RefreshHint::Ui);
        }
    }
}

/// 只渲染一帧 (不进入主循环), 用于主机预览与快照测试。`frame` 会先用主题背景清空。
pub fn render_once<A: App>(
    app: &mut A,
    fonts: &mut FontStore,
    glyphs: &mut GlyphCache,
    hits: &mut Hits,
    page: &mut dyn Page<A>,
    frame: &mut Bitmap,
) {
    let tasks: Tasks<Delivery<A>> = Tasks::new(1, Waker::new().expect("waker"));
    let mut redraw = None;
    let mut swipe = None;
    let mut timers = Timers::default();
    let theme = app.theme();
    frame.fill_rect(frame.bounds(), theme.background);
    hits.clear();
    let (width, height) = (frame.width(), frame.height());
    let mut cx = Cx { app, fonts, glyphs, hits, theme, width, height, instance: 0, tasks: &tasks, redraw: &mut redraw, swipe: &mut swipe, timers: &mut timers };
    page.render(&mut cx, frame);
}

static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn install_signal_handlers() {
    #[cfg(unix)]
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

fn minute_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() / 60).unwrap_or(0)
}

fn ms_to_next_minute() -> u64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let into = (now.as_millis() % 60_000) as u64;
    60_000 - into + 50
}

/// 等待事件; 返回 (输入就绪, 电源就绪, 唤醒就绪)。主机上 (非 unix) 简单睡眠。
fn wait(input: &InputReader, power: &PowerMonitor, waker: &Waker, timeout_ms: u64) -> (bool, bool, bool) {
    #[cfg(unix)]
    {
        let mut fds: Vec<libc::pollfd> = Vec::new();
        for fd in input.fds() {
            fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        }
        let input_count = fds.len();
        let power_fd = power.fd();
        if let Some(fd) = power_fd {
            fds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
        }
        let waker_fd = waker.fd();
        let waker_index = fds.len();
        if waker_fd >= 0 {
            fds.push(libc::pollfd { fd: waker_fd, events: libc::POLLIN, revents: 0 });
        }
        let timeout = timeout_ms.min(i32::MAX as u64) as libc::c_int;
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if rc <= 0 {
            // 超时或 EINTR (信号)
            return (false, false, false);
        }
        let input_ready = fds[..input_count].iter().any(|p| p.revents != 0);
        let power_ready = power_fd.is_some() && fds[input_count].revents != 0;
        let waker_ready = waker_fd >= 0 && fds[waker_index].revents != 0;
        (input_ready, power_ready, waker_ready)
    }
    #[cfg(not(unix))]
    {
        let _ = (input, power, waker);
        std::thread::sleep(Duration::from_millis(timeout_ms.min(20)));
        (true, true, true)
    }
}

/// 主机上驱动页面的最小运行时 (预览与测试用): 处理导航、投递后台结果、跑空闲工作,
/// 每次 `frame()` 渲染当前页。不涉及显示与输入设备。
pub struct Headless<A: App> {
    rt: Runtime<A>,
    stack: Stack<A>,
    waker: Waker,
}

impl<A: App> Headless<A> {
    pub fn new(app: A, fonts: FontStore, width: u32, height: u32) -> std::io::Result<Self> {
        let waker = Waker::new()?;
        let mut rt = Runtime {
            app,
            fonts,
            glyphs: GlyphCache::new(4 * 1024 * 1024),
            hits: Hits::default(),
            tasks: Tasks::new(2, waker.clone()),
            redraw: None,
            swipe: None,
            width,
            height,
            next_instance: 0,
            timers: Timers::default(),
        };
        let home = rt.app.home_page();
        let entry = rt.new_entry(home);
        let mut stack = Stack { entries: vec![entry] };
        stack.enter_top(&mut rt, false);
        Ok(Headless { rt, stack, waker })
    }

    /// 导航到新页面 (等同当前页返回 Transition::Push)。
    pub fn push(&mut self, page: Box<dyn Page<A>>) {
        self.stack.apply(&mut self.rt, Transition::Push(page));
    }

    /// 把输入事件交给当前页; 返回 false 表示应用请求退出。
    /// 先渲染一次当前页 (与设备上一致: 用户点的是屏幕上最新的画面, 命中区域随渲染登记)。
    pub fn input(&mut self, event: &InputEvent) -> bool {
        let _ = self.frame();
        let top = self.stack.top();
        let instance = top.instance;
        let transition = top.page.on_input(&mut self.rt.cx(instance), event);
        self.stack.apply(&mut self.rt, transition)
    }

    /// 等待后台任务并处理结果、空闲工作, 直到没有任何待办或超时。
    pub fn settle(&mut self, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut busy = self.stack.fire_timers(&mut self.rt);
            for delivery in self.rt.tasks.drain() {
                self.stack.deliver(&mut self.rt, delivery);
                busy = true;
            }
            let top = self.stack.top();
            let instance = top.instance;
            if top.page.on_idle(&mut self.rt.cx(instance)) {
                busy = true;
            }
            if std::time::Instant::now() >= deadline || (!busy && self.rt.tasks.idle()) {
                break;
            }
            if !busy {
                std::thread::sleep(Duration::from_millis(5));
                self.waker.drain();
            }
        }
    }

    /// 渲染当前页到新帧, 返回 (帧, 待处理的重绘提示)。
    pub fn frame(&mut self) -> (Bitmap, Option<RefreshHint>) {
        let mut frame = Bitmap::new(self.rt.width, self.rt.height, self.rt.app.theme().background);
        self.rt.hits.clear();
        let top = self.stack.top();
        let instance = top.instance;
        top.page.render(&mut self.rt.cx(instance), &mut frame);
        (frame, self.rt.redraw.take())
    }

    pub fn app(&mut self) -> &mut A {
        &mut self.rt.app
    }

    pub fn hits(&self) -> &Hits {
        &self.rt.hits
    }
}

/// 运行主循环直到 `Transition::Exit` 或收到 SIGTERM/SIGINT。
///
/// 循环每轮:
/// 1. 计算超时 (见 lib.rs 文档), `poll` 输入/电源/唤醒 fd。
/// 2. 电源事件: GoingToScreenSaver → app.on_suspend、释放输入 grab、`power.release_framework()`;
///    OutOfScreenSaver → `power.reclaim_framework()`、grab、display.reinit、scheduler.invalidate、Flash 重绘、app.on_resume。
///    休眠期间忽略输入、不刷新屏幕。
/// 3. 输入事件: Down → 按下反馈 (反相 + A2, 只在 feedback 区域); 其它 → 当前页 on_input → 执行 Transition。
/// 4. 投递后台结果。
/// 5. 分钟变化 → app.on_minute。
/// 6. 若有重绘请求: 页面 render 到后备缓冲, scheduler.plan → display.present → scheduler.commit。
/// 7. 无事件且无手指按下时, 让当前页面做一次空闲工作。
pub fn run<A: App>(
    app: A,
    mut display: Box<dyn Display>,
    fonts: FontStore,
    mut input: InputReader,
    mut power: PowerMonitor,
    mut scheduler: RefreshScheduler,
) -> std::io::Result<()> {
    install_signal_handlers();
    let info = display.info().clone();
    let (width, height) = (info.width, info.height);
    let waker = Waker::new()?;
    let mut rt = Runtime {
        app,
        fonts,
        glyphs: GlyphCache::new(4 * 1024 * 1024),
        hits: Hits::default(),
        tasks: Tasks::new(2, waker.clone()),
        redraw: None,
        swipe: None,
        width,
        height,
        next_instance: 0,
        timers: Timers::default(),
    };
    let home = rt.app.home_page();
    let entry = rt.new_entry(home);
    let mut stack = Stack { entries: vec![entry] };
    stack.enter_top(&mut rt, false);

    let mut frame = Bitmap::new(width, height, rt.app.theme().background);
    let mut events: Vec<InputEvent> = Vec::new();
    let mut power_events: Vec<PowerEvent> = Vec::new();
    let mut sleeping = false;
    let mut last_minute = minute_now();
    // 按下反馈是否已经画在屏幕上 (需要随后的正常重绘把它还原)
    let mut feedback_shown = false;
    // 上一帧的配色: 日间/夜间切换后几乎每个像素都反转, 局部刷新残影很重, 要闪刷一次
    let mut last_theme = rt.app.theme();
    // 电源键按下时刻: 框架进程 (awesome 等) 被我们 SIGSTOP 时, powerd 进入屏保要向 winmgr 查询,
    // 查询会一直挂到超时, 设备根本睡不下去 (KPW5 FW 5.17 实测)。所以一看到电源键按下就先 SIGCONT 框架,
    // 让 powerd 的流程走完; 若 POWER_KEY_GRACE 内没进入屏保, 再暂停框架并重画。
    let mut power_key_at: Option<std::time::Instant> = None;
    let mut wants_idle = true;
    // KN_DEBUG=1: 每帧打印渲染/差分/提交耗时 (设备上测翻页延迟)
    let debug = std::env::var_os("KN_DEBUG").is_some();
    let _ = input.grab();

    loop {
        if STOP.load(std::sync::atomic::Ordering::SeqCst) {
            stack.leave_top(&mut rt);
            break;
        }

        // 1. 等待
        let mut timeout = ms_to_next_minute();
        if let Some(ms) = rt.timers.next_ms() {
            timeout = timeout.min(ms);
        }
        if let Some(deadline) = power.next_deadline_ms() {
            timeout = timeout.min(deadline);
        }
        if let Some(at) = power_key_at {
            timeout = timeout.min(POWER_KEY_GRACE.saturating_sub(at.elapsed()).as_millis() as u64);
        }
        if rt.redraw.is_some() || (wants_idle && !sleeping && !input.touch_active()) {
            timeout = 0;
        }
        let (input_ready, _power_ready, waker_ready) = wait(&input, &power, &waker, timeout);
        if waker_ready {
            waker.drain();
        }

        // 2. 电源
        power_events.clear();
        power.poll_events(&mut power_events);
        for event in power_events.drain(..) {
            match event {
                PowerEvent::GoingToScreenSaver if !sleeping => {
                    sleeping = true;
                    let top = stack.top();
                    let instance = top.instance;
                    top.page.on_suspend(&mut rt.cx(instance));
                    rt.app.on_suspend();
                    input.reset();
                    let _ = input.ungrab();
                    power.release_framework();
                }
                PowerEvent::OutOfScreenSaver if sleeping => {
                    sleeping = false;
                    power_key_at = None;
                    power.reclaim_framework();
                    // 给系统一点时间恢复背光/电源, 与 Python 版一致
                    std::thread::sleep(Duration::from_millis(350));
                    input.reset();
                    let _ = input.grab();
                    let _ = display.reinit();
                    scheduler.invalidate();
                    rt.app.on_resume();
                    rt.redraw = Some(RefreshHint::Flash);
                }
                _ => {}
            }
        }

        if let Some(at) = power_key_at {
            if sleeping {
                // 已进入屏保, 后续由 OutOfScreenSaver 接管
                power_key_at = None;
            } else if at.elapsed() >= POWER_KEY_GRACE {
                // 这次按键没有引起休眠: 重新暂停框架, 框架运行期间可能改写过屏幕, 整屏重画
                power_key_at = None;
                power.reclaim_framework();
                let _ = display.reinit();
                scheduler.invalidate();
                rt.redraw = Some(RefreshHint::Flash);
            }
        }

        // 3. 输入
        if input_ready {
            events.clear();
            let _ = input.read(&mut events);
            if sleeping {
                events.clear();
            }
            for event in events.drain(..) {
                if debug {
                    eprintln!("[input] {:?}", event);
                }
                if let InputEvent::Key { code: KeyCode::Power, pressed: true } = event {
                    if !sleeping && power_key_at.is_none() {
                        power.release_framework();
                        power_key_at = Some(std::time::Instant::now());
                    }
                    continue;
                }
                if let InputEvent::Gesture(g) = &event {
                    if g.kind == GestureKind::Down {
                        let p = Point { x: g.start.0, y: g.start.1 };
                        if let Some(rect) = rt.hits.feedback_rect(p) {
                            let mut pressed = scheduler.screen().clone();
                            pressed.invert_rect(rect);
                            if let Some(req) = scheduler.plan(&pressed, RefreshHint::Feedback) {
                                if display.present(&pressed, &req).is_ok() {
                                    scheduler.commit(&pressed, &req, RefreshHint::Feedback);
                                    feedback_shown = true;
                                }
                            }
                        }
                    }
                }
                let top = stack.top();
                let instance = top.instance;
                let transition = top.page.on_input(&mut rt.cx(instance), &event);
                if !stack.apply(&mut rt, transition) {
                    return Ok(());
                }
                if let InputEvent::Gesture(g) = &event {
                    if g.kind != GestureKind::Down && feedback_shown {
                        // 抬起后无论页面是否变化都要重绘一次, 把反相还原
                        rt.redraw_ui();
                        feedback_shown = false;
                    }
                }
                wants_idle = true;
            }
        }

        // 4. 定时器与后台结果
        if !sleeping && stack.fire_timers(&mut rt) {
            wants_idle = true;
        }
        for delivery in rt.tasks.drain() {
            stack.deliver(&mut rt, delivery);
            wants_idle = true;
        }

        // 5. 时钟
        let minute = minute_now();
        if minute != last_minute {
            last_minute = minute;
            if !sleeping && rt.app.on_minute() {
                rt.redraw_ui();
            }
        }

        // 6. 重绘
        if let Some(mut hint) = rt.redraw.take() {
            if !sleeping {
                let theme = rt.app.theme();
                if theme != last_theme {
                    last_theme = theme;
                    hint = RefreshHint::Flash;
                }
                let started = std::time::Instant::now();
                let top = stack.top();
                if !top.page.opaque() {
                    frame.fill_rect(frame.bounds(), rt.app.theme().background);
                }
                rt.hits.clear();
                let instance = top.instance;
                top.page.render(&mut rt.cx(instance), &mut frame);
                let rendered = started.elapsed();
                let swipe = rt.swipe.take();
                let mut plan = scheduler.plan(&frame, hint);
                if let Some(req) = plan.as_mut() {
                    // 翻页刷新 (不闪) 或 request_flash_turn 的整屏闪刷才带动画; swipe 只由这两者设置
                    if (hint == RefreshHint::Turn && !req.flash) || (hint == RefreshHint::Flash && req.flash) {
                        req.swipe = swipe;
                    }
                }
                let planned = started.elapsed();
                if let Some(req) = &plan {
                    match display.present(&frame, req) {
                        Ok(_) => scheduler.commit(&frame, req, hint),
                        Err(_) => scheduler.invalidate(),
                    }
                }
                if debug {
                    eprintln!(
                        "[frame] {:?} render {:.1} ms, plan {:.1} ms, present {:.1} ms, {:?}",
                        hint,
                        rendered.as_secs_f32() * 1e3,
                        (planned - rendered).as_secs_f32() * 1e3,
                        (started.elapsed() - planned).as_secs_f32() * 1e3,
                        plan.map(|r| (r.rect, r.waveform, r.flash, r.swipe))
                    );
                }
                wants_idle = true;
            }
            continue;
        }

        // 7. 空闲工作
        if wants_idle && !sleeping && !input.touch_active() {
            let top = stack.top();
            let instance = top.instance;
            wants_idle = top.page.on_idle(&mut rt.cx(instance));
        }
    }
    Ok(())
}
