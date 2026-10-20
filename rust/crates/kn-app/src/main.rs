//! KinNovel 入口。
//!
//! - 设备上: `kinnovel` (由 KUAL 启动脚本拉起, 脚本负责暂停/恢复框架进程, 并设置 `KN_APP_DIR`)。
//! - 主机上: `kinnovel --splash out.pgm [WxH]` 输出启动画面;
//!   `kinnovel --preview out.pgm [--read BOOK SORT] [--comic BOOK CHAPTER] [--login] [--sleep] [--tap X,Y]... [--long X,Y]... [--swipe left|right|up|down]...`
//!   用真实页面逻辑 (无显示/输入设备) 渲染最后一帧; `KN_APP_DIR` 指向一份设备缓存的拷贝。

mod account;
mod api;
mod comic;
mod crash;
mod covers;
mod net;
mod pages;
mod sleep;
mod splash;
mod stats;
mod store;
mod tz;
mod update;
mod userdata;

use std::path::{Path, PathBuf};
use std::time::Duration;

use kn_platform::{Gesture, GestureKind, InputEvent};
use kn_render::{FontId, FontStore};
use kn_ui::widgets::Metrics;
use kn_ui::{App, Page, Theme};

use store::{Config, Paths};

const SYSTEM_FONTS: &[&str] = &[
    "/usr/java/lib/fonts/STHeitiMedium.ttf",
    // 主机预览用
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/msyh.ttc",
];

/// 应用全局状态 (只在 UI 线程访问)。页面自己的状态放在各页面结构体里。
/// `KN_DEBUG=1`: 额外的耗时日志 (与 kn-ui 每帧日志同一开关)。
pub fn debug() -> bool {
    static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("KN_DEBUG").is_some())
}

pub struct KinNovel {
    pub paths: Paths,
    pub config: Config,
    /// 界面字体链 (系统字体)
    pub ui_fonts: Vec<FontId>,
    pub metrics: Metrics,
    pub night: bool,
    /// 当前加载的章节字体 (文件, FontId)。只保留一个: 换成别的字体时卸载旧的;
    /// 相邻章节常共用同一字体, 换章时可直接复用。
    pub chapter_font: Option<(PathBuf, FontId)>,
    /// 前光亮度/色温 (阅读菜单的快捷面板)
    pub light: kn_platform::Frontlight,
    /// 屏幕 ppi; 正文字号、页边距的默认值与范围按 300 ppi 设计, 按它换算 (KOReader 的字号同样按 dpi 换算)
    pub dpi: u32,
    /// 网络客户端; 离线模式 (主机预览, KN_OFFLINE=1) 下不使用
    net_client: kn_net::Client,
    online: bool,
    /// 从阅读页打开的目录/书签列表选了位置: 返回阅读页后由它就地跳转 (不再叠一层阅读页)
    pub reader_jump: Option<pages::reader::ReaderJump>,
    /// 启动时的一次性提示 (上次异常退出), 书架页显示
    pub startup_notice: Option<String>,
    /// 正在弹出的操作提示 (`pages::tips`)
    pub tip_open: Option<&'static str>,
    /// 本次运行里点了「感觉会忘记」的提示: 下次启动再提示
    pub tips_later: std::collections::HashSet<&'static str>,
    /// 本次运行已经显示过欢迎页 (之后回首页直接到书架)
    welcome_shown: bool,
    /// 启动画面已经画在屏幕上 (设备上); 见 `startup_hold`
    pub splash_shown: bool,
    /// 锁屏 (`pages::lock`): 本次已解锁 / 锁屏页正在显示 / 唤醒后要盖一层锁屏
    unlocked: bool,
    lock_showing: bool,
    lock_pending: bool,
    /// KUAL 菜单「KinNovel 工具箱」里的入口 (`--update` / `--uninstall`): 首页换成对应页面, 用一次
    pub start: StartMode,
    /// 自动检查更新: None = 关闭, Some(true) = 本次启动要在后台检查 (距上次超过一天), Some(false) = 不用查
    update_check: Option<bool>,
    /// 发现了还没提示过的新版本: 下次栈顶是首页类页面时弹提示
    update_prompt: Option<String>,
    /// 上次取到的账号信息: 再进「我的」时先显示它, 后台刷新
    pub my_info: Option<api::MyInfo>,
}

/// 程序以哪个页面开始 (启动脚本把 KUAL 菜单项的参数传进来)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartMode {
    Normal,
    Update,
    Uninstall,
}

impl StartMode {
    fn from_args(args: &[String]) -> Self {
        match args.get(1).map(String::as_str) {
            Some("--update") => StartMode::Update,
            Some("--uninstall") => StartMode::Uninstall,
            _ => StartMode::Normal,
        }
    }
}

impl App for KinNovel {
    fn home_page(&mut self) -> Box<dyn Page<Self>> {
        if self.locked() {
            self.lock_showing = true;
            return Box::new(pages::lock::LockPage::startup());
        }
        match std::mem::replace(&mut self.start, StartMode::Normal) {
            StartMode::Update => return Box::new(pages::update::UpdatePage::new(pages::update::Origin::Kual)),
            StartMode::Uninstall => return Box::new(pages::update::UninstallPage::default()),
            StartMode::Normal => {}
        }
        // 第一次启动 (或还没登录): 欢迎页; 登录或「先逛逛」后进书架
        if self.wants_welcome() && !self.welcome_shown {
            self.welcome_shown = true;
            return Box::new(pages::login::LoginPage::welcome());
        }
        pages::tab_page(pages::Tab::Shelf)
    }

    fn theme(&self) -> Theme {
        Theme::new(self.night)
    }

    /// 有令牌就不动; 没有令牌但保存了账号时, 后台登录。走任务池: 账号被拒时结果送回 UI 线程,
    /// 随后的 `take_transition` 弹出登录页。
    fn on_start(cx: &mut kn_ui::Cx<Self>) {
        Self::start_update_check(cx);
        let Some(net) = cx.app.net() else { return };
        if net.user().is_some() || !net.has_account() {
            return;
        }
        cx.spawn_app(move || {
            match net::ensure_login(&net) {
                Ok(()) => eprintln!("[net] 已登录"),
                Err(e) => eprintln!("[net] {e}"),
            }
            Box::new(|_: &mut KinNovel| {})
        });
    }

    fn startup_hold(&self) -> Duration {
        // 设备上画了启动画面: 首页数据到了再换掉它, 最多 2.5 s (之后照常显示「加载中」; KPW5 实测有会话时 0.8–2.2 s)
        if self.splash_shown { Duration::from_millis(2500) } else { Duration::ZERO }
    }

    fn on_resume(&mut self) {
        // 休眠唤醒: 设了锁屏密码就重新上锁 (锁屏页已经在显示时不再叠一层)
        if pages::lock::configured_pin(&self.config).is_some() && !self.lock_showing {
            self.unlocked = false;
            self.lock_pending = true;
        }
    }

    fn take_transition(&mut self, top: kn_ui::PageId) -> kn_ui::Transition<Self> {
        // 新版本提示只盖在书架/历史/发现/我的上, 不打断阅读、锁屏、登录等
        if self.update_prompt.is_some() && pages::is_tab_page(top) && !self.lock_pending {
            let version = self.update_prompt.take().unwrap_or_default();
            update::mark_prompted(&self.paths, &version);
            return kn_ui::Transition::Push(Box::new(pages::update::UpdatePrompt::new(version)));
        }
        if self.lock_pending {
            self.lock_pending = false;
            self.lock_showing = true;
            return kn_ui::Transition::Push(Box::new(pages::lock::LockPage::resume()));
        }
        if !self.net_client.take_account_rejected() {
            return kn_ui::Transition::None;
        }
        // 保存的账号登录被拒 (密码改了等): 删掉它, 带着邮箱回登录页
        let email = account::load(&self.paths).map(|a| a.email).unwrap_or_default();
        account::clear(&self.paths);
        eprintln!("[account] 保存的账号登录被拒, 已删除");
        kn_ui::Transition::Push(Box::new(pages::login::LoginPage::rejected(&email)))
    }

    fn sleep_screen(&mut self) -> Option<Box<dyn Page<Self>>> {
        sleep::enabled().then(|| Box::new(sleep::SleepPage) as Box<dyn Page<Self>>)
    }
}

impl KinNovel {
    /// 启动时的自动检查更新: 需要时后台查 GitHub, 查完 (或不用查但之前查到过) 有没提示过的新版本就排队弹提示。
    /// 在单独的线程里做 (网络最长要等 30 秒, 不占页面的任务池), 并且至少等到启动几秒后才交回,
    /// 免得书架还没加载完, 弹窗的背景就冻结在空书架上。
    fn start_update_check(cx: &mut kn_ui::Cx<Self>) {
        const PROMPT_NOT_BEFORE: Duration = Duration::from_secs(5);
        let Some(due) = cx.app.update_check else { return };
        let (paths, net, poster) = (cx.app.paths.clone(), cx.app.net(), cx.app_poster());
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            if let (true, Some(net)) = (due, net) {
                update::background_check(&paths, &net);
            }
            let Some(prompt) = update::unprompted(&paths) else { return };
            std::thread::sleep(PROMPT_NOT_BEFORE.saturating_sub(started.elapsed()));
            poster.post(kn_ui::Delivery::App(Box::new(move |app: &mut KinNovel| app.update_prompt = Some(prompt))));
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn new(paths: Paths, config: Config, ui_fonts: Vec<FontId>, width: u32, height: u32, dpi: u32, light: kn_platform::Frontlight, online: bool) -> Self {
        let night = config.bool("night_mode", false);
        let net_client = net::client(&paths, &config);
        let account = account::load(&paths).map(|a| (a.email, a.password_hash));
        // 会话失效 (刷新令牌也被拒) 时由客户端用这个账号自动重新登录
        net_client.set_account(account);
        let mut config = config;
        // 锁屏密码默认留空 (设置 → 内容里开关 / 修改)
        if !config.contains("lock_pin") {
            config.set("lock_pin", serde_json::Value::from(""));
        }
        KinNovel {
            paths,
            config,
            ui_fonts,
            metrics: Metrics::for_screen(width, height),
            night,
            chapter_font: None,
            light,
            dpi,
            net_client,
            online,
            reader_jump: None,
            startup_notice: None,
            tip_open: None,
            tips_later: Default::default(),
            welcome_shown: false,
            splash_shown: false,
            unlocked: false,
            lock_showing: false,
            lock_pending: false,
            update_check: None,
            update_prompt: None,
            my_info: None,
            start: StartMode::Normal,
        }
    }

    /// 设了锁屏密码且本次还没解锁。
    fn locked(&self) -> bool {
        !self.unlocked && pages::lock::configured_pin(&self.config).is_some()
    }

    /// 锁屏页输对了密码。
    pub fn unlock(&mut self) {
        self.unlocked = true;
        self.lock_showing = false;
    }

    /// 按 300 ppi 设计的像素值换算到本机。
    pub fn dpi_px(&self, v: f64) -> i64 {
        (v * self.dpi as f64 / 300.0).round() as i64
    }

    /// 在线时返回网络客户端 (廉价克隆, 可带进后台任务)。
    pub fn net(&self) -> Option<kn_net::Client> {
        self.online.then(|| self.net_client.clone())
    }


    /// 登录成功: 保存账号 (密码只存哈希), 之后会话失效时用它自动重新登录。
    pub fn remember_account(&mut self, account: account::Account) {
        if let Err(e) = account::save(&self.paths, &account) {
            eprintln!("[account] 保存失败: {e}");
        }
        self.net_client.set_account(Some((account.email, account.password_hash)));
    }

    /// 退出登录: 删掉保存的账号和会话令牌。
    pub fn forget_account(&mut self) {
        self.my_info = None;
        account::clear(&self.paths);
        if let Err(e) = self.net_client.logout() {
            eprintln!("[account] 清除会话失败: {e}");
        }
    }

    /// 需要显示欢迎/登录页: 联网、没有登录、也没在欢迎页点过「先逛逛」。
    pub fn wants_welcome(&self) -> bool {
        !api::fake_mode() && self.online && !self.signed_in() && !self.config.bool("welcome_skipped", false)
    }

    /// 已登录, 或保存了账号 (需要时自动登录); fixture 预览模式也算。
    pub fn signed_in(&self) -> bool {
        api::fake_mode() || self.net().is_some_and(|n| n.user().is_some() || n.has_refresh_token() || n.has_account())
    }
}

/// 补充符号字体 (霞鹜新晰黑屏幕版的子集, IPA 字体许可, 由 tools/gen_symbol_font.py 生成): 只含 Kindle 系统黑体
/// 缺的符号/标点/拉丁扩展 (♡ ♢ ♤ ♧ ✿ ✦ 〜 …), 编进程序, 接在界面字体后面。
static SYMBOL_FONT: &[u8] = include_bytes!("../data/KinNovelSymbols.ttf");
/// 黑白 emoji 字体 (Noto Emoji, OFL-1.1, 由 tools/gen_emoji_font.py 精简), 编进程序, 放在界面字体链最后:
/// 界面字体没有的 emoji 用它画; 都没有的字符画成空白 (它的 .notdef 是空的), 不再是方框。
static EMOJI_FONT: &[u8] = include_bytes!("../data/NotoEmoji-subset.ttf");

/// 界面字体: KN_FONT 环境变量 > 配置 font_path (与 Python 版同键) > 系统黑体; 后接补充符号字体和 emoji 字体。
/// 阅读页的字体链 = 章节字体 + 这条链。
fn load_ui_fonts(fonts: &mut FontStore, config: &Config) -> Vec<FontId> {
    let mut chain = vec![load_ui_font(fonts, config)];
    for (name, bytes) in [("symbols", SYMBOL_FONT), ("emoji", EMOJI_FONT)] {
        match fonts.load_bytes(bytes.to_vec()) {
            Ok(id) => chain.push(id),
            Err(e) => eprintln!("[font] {name}: {e:?}"),
        }
    }
    chain
}

fn load_ui_font(fonts: &mut FontStore, config: &Config) -> FontId {
    let mut candidates: Vec<PathBuf> = std::env::var("KN_FONT").ok().map(PathBuf::from).into_iter().collect();
    let configured = config.string("font_path", "");
    if !configured.trim().is_empty() {
        candidates.push(PathBuf::from(configured.trim()));
    }
    candidates.extend(SYSTEM_FONTS.iter().map(PathBuf::from));
    for path in candidates {
        if Path::new(&path).exists() {
            match fonts.load_file(&path) {
                Ok(id) => return id,
                Err(e) => eprintln!("[font] {}: {:?}", path.display(), e),
            }
        }
    }
    eprintln!("[font] 找不到系统字体, 设置 KN_FONT 指向一个 TTF");
    std::process::exit(2);
}

fn gesture(kind: GestureKind, start: (i32, i32), end: (i32, i32)) -> InputEvent {
    InputEvent::Gesture(Gesture { kind, start, end, duration_ms: 80 })
}

/// 主机预览: 驱动真实页面, 按参数依次执行操作, 输出最后一帧。
fn preview(args: &[String]) {
    // KN_PREVIEW_SIZE=600x800 之类可模拟其它机型的分辨率
    let (w, h) = std::env::var("KN_PREVIEW_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
        .unwrap_or((1236, 1648));
    let out = &args[0];
    let paths = Paths::from_env();
    account::migrate_legacy_config(&paths);
    let config = Config::load(paths.settings_file());
    let mut fonts = FontStore::new();
    let ui_fonts = load_ui_fonts(&mut fonts, &config);
    // 预览默认离线 (不访问服务器); KN_ONLINE=1 时联网
    let online = std::env::var_os("KN_ONLINE").is_some();
    // 按分辨率对应到 Kindle 的 ppi: 600x800 = 167, 758x1024 = 212, 其余 300
    let dpi = match w {
        0..=600 => 167,
        601..=758 => 212,
        _ => 300,
    };
    let mut app = KinNovel::new(paths, config, ui_fonts, w, h, dpi, kn_platform::Frontlight::detect(None), online);
    // KN_START=update / uninstall: 预览 KUAL 工具箱的页面
    app.start = StartMode::from_args(&["".into(), format!("--{}", std::env::var("KN_START").unwrap_or_default())]);
    // 只读出上次的检查结果 (不联网)
    update::start(&app.paths, false);
    // KN_TEST_UPDATE_PROMPT=1.7.0: 预览新版本提示弹窗
    app.update_prompt = std::env::var("KN_TEST_UPDATE_PROMPT").ok();
    let mut ui = kn_ui::Headless::new(app, fonts, w, h).expect("headless");
    let settle = |ui: &mut kn_ui::Headless<KinNovel>| ui.settle(Duration::from_secs(20));
    settle(&mut ui);
    let mut i = 1;
    while i < args.len() {
        let started = std::time::Instant::now();
        match args[i].as_str() {
            "--read" => {
                let book: i64 = args[i + 1].parse().expect("book id");
                let sort: i64 = args[i + 2].parse().expect("sort num");
                ui.push(Box::new(pages::reader::ReaderPage::new(book, sort, pages::reader::Entry::Resume)));
                i += 3;
            }
            "--comic" => {
                let book: i64 = args[i + 1].parse().expect("book id");
                let chapter: i64 = args[i + 2].parse().expect("chapter id");
                ui.push(Box::new(pages::comic::ComicReaderPage::new(book, chapter, pages::reader::Entry::Resume)));
                i += 3;
            }
            // 欢迎/登录页
            "--login" => {
                ui.push(Box::new(pages::login::LoginPage::welcome()));
                i += 1;
            }
            // 屏保页 (封面屏保的样子)
            "--sleep" => {
                ui.push(Box::new(sleep::SleepPage));
                i += 1;
            }
            "--tap" | "--long" => {
                let (x, y) = args[i + 1].split_once(',').expect("x,y");
                let p = (x.parse().unwrap(), y.parse().unwrap());
                let kind = if args[i] == "--long" { GestureKind::Long } else { GestureKind::Tap };
                ui.input(&gesture(kind, p, p));
                i += 2;
            }
            "--swipe" => {
                let (kind, s, e) = match args[i + 1].as_str() {
                    "left" => (GestureKind::SwipeLeft, (900, 800), (300, 800)),
                    "right" => (GestureKind::SwipeRight, (300, 800), (900, 800)),
                    "up" => (GestureKind::SwipeUp, (600, 1200), (600, 400)),
                    _ => (GestureKind::SwipeDown, (600, 400), (600, 1200)),
                };
                ui.input(&gesture(kind, s, e));
                i += 2;
            }
            other => panic!("unknown preview arg {other}"),
        }
        settle(&mut ui);
        eprintln!("[preview] {} -> {} ms", args[i - 1], started.elapsed().as_millis());
    }
    // 页面在 render 里才请求封面/图片: 先渲染一次, 等这些后台任务完成后再出最终帧
    ui.frame();
    settle(&mut ui);
    let started = std::time::Instant::now();
    let (frame, _) = ui.frame();
    eprintln!("[preview] final render {} ms", started.elapsed().as_millis());
    std::fs::write(out, frame.to_pgm()).expect("write preview");
    eprintln!("preview written to {out}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    eprintln!("kinnovel {}", env!("CARGO_PKG_VERSION"));
    if args.len() >= 3 && args[1] == "--splash" {
        let (w, h) = args.get(3).and_then(|s| s.split_once('x')).and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))).unwrap_or((1236, 1648));
        std::fs::write(&args[2], splash::render(w, h).to_pgm()).expect("write splash");
        return;
    }
    if args.len() >= 2 && args[1] == "--restore-display" {
        // 启动脚本在程序异常退出后调用: 恢复硬件反色等驱动全局状态
        kn_platform::display::restore_after_crash();
        return;
    }
    if args.len() >= 3 && args[1] == "--preview" {
        preview(&args[2..]);
        return;
    }
    run_device(StartMode::from_args(&args));
}

#[cfg(target_os = "linux")]
fn run_device(start: StartMode) {
    use kn_platform::{Display, FbinkDisplay, GestureConfig, InputReader, PowerMonitor};
    use kn_ui::{RefreshPolicy, RefreshScheduler};

    let paths = Paths::from_env();
    crash::install_panic_hook(&paths);
    let crash_notice = crash::take_crash_notice(&paths);
    // 上次应用内更新中途断电: 回到旧版本的文件
    update::recover_install(&update::program_dir(&paths));
    let mut display = FbinkDisplay::open().expect("FBInk 初始化失败");
    let info = display.info().clone();
    eprintln!(
        "[device] {} ({}, id {:#x}) {}x{} {} ppi, {} bpp, stride={} mtk={} reagl={} touch swap={} mirror={}/{}",
        info.device_name,
        info.device_codename,
        info.device_id,
        info.width,
        info.height,
        info.dpi,
        info.bytes_per_pixel * 8,
        info.stride,
        info.is_mtk,
        info.supports_reagl,
        info.touch_swap_axes,
        info.touch_mirror_x,
        info.touch_mirror_y
    );
    // 测试钩子 KN_TEST_GSENSOR: 在没有重力感应的机型上也走这条路径 (配合 KN_ORIENTATION=D)。
    // 值为 "flip" 时假装框架没转过屏幕, 于是画面软件旋转 180° —— 在 KPW5 上倒着拿就能端到端验证。
    let test_gsensor = std::env::var("KN_TEST_GSENSOR").ok();
    let gsensor = info.has_gsensor() || test_gsensor.is_some();
    // KOReader (Oasis / Scribe init): 启动时读 `com.lab126.winmgr accelerometer` (U/D/L/R) 定朝向。
    // winmgr 属于框架, 暂停后读不了, 由启动脚本在暂停前读好传进来。横屏 (L/R) 只在系统阅读器里出现, 按正向处理。
    let upside_down = gsensor && std::env::var("KN_ORIENTATION").as_deref() == Ok("D");
    if gsensor {
        let fb_upside_down = upside_down && test_gsensor.as_deref() != Some("flip");
        eprintln!("[device] gsensor: upside_down={upside_down} fb_upside_down={fb_upside_down} (fb rota {})", info.fb_rota);
        display.assume_fb_upside_down(fb_upside_down);
        display.set_rotation(kn_platform::Rotation::new(if upside_down { 2 } else { 0 }));
    }
    // 启动画面: 先于字体/配置加载, 首页第一帧整屏闪刷时换掉
    let started = std::time::Instant::now();
    let full = kn_render::Rect::new(0, 0, info.width, info.height);
    if let Err(e) = display.present(&splash::render(info.width, info.height), &kn_platform::RefreshRequest { rect: full, waveform: kn_platform::Waveform::Gc16, flash: true, swipe: None }) {
        eprintln!("[splash] {e}");
    }
    account::migrate_legacy_config(&paths);
    let config = Config::load(paths.settings_file());
    let mut fonts = FontStore::new();
    let ui_fonts = load_ui_fonts(&mut fonts, &config);
    let policy = RefreshPolicy {
        supports_reagl: info.supports_reagl,
        full_refresh_every: config.float("full_refresh_every", 6.0) as f32,
        flash_every_turn: config.bool("page_flash", false),
    };
    let online = std::env::var_os("KN_OFFLINE").is_none();
    let prune_paths = paths.clone();
    let limit_mb = config.int("cache_limit_mb", 192);
    let comic_mb = config.int("comic_cache_mb", 256);
    std::thread::spawn(move || {
        let removed = store::prune_cache(&prune_paths, limit_mb, comic_mb);
        if removed > 0 {
            eprintln!("[cache] 清理 {} KB", removed / 1024);
        }
    });
    let light = kn_platform::Frontlight::detect(info.caps);
    let mut app = KinNovel::new(paths, config, ui_fonts, info.width, info.height, info.dpi, light, online);
    app.startup_notice = crash_notice;
    app.splash_shown = true;
    app.start = start;
    let auto_check = app.config.bool("check_update", true);
    let due = update::start(&app.paths, auto_check);
    app.update_check = auto_check.then_some(due);
    // 测试钩子: 几秒后在后台线程 panic, 验证崩溃记录与框架恢复
    if let Ok(secs) = std::env::var("KN_TEST_PANIC").map(|v| v.parse::<u64>().unwrap_or(5)) {
        std::thread::Builder::new()
            .name("test-panic".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_secs(secs));
                panic!("KN_TEST_PANIC 测试崩溃");
            })
            .ok();
    }
    let mut input = InputReader::open(&info, GestureConfig::default()).expect("输入设备初始化失败");
    if gsensor {
        input.set_rotation(kn_platform::Rotation::new(if upside_down { 2 } else { 0 }));
    }
    let power = PowerMonitor::start();
    let scheduler = RefreshScheduler::new(info.width, info.height, policy);
    eprintln!("[splash] 启动画面之后的初始化 {} ms", started.elapsed().as_millis());
    if let Err(e) = kn_ui::run(app, Box::new(display), fonts, input, power, scheduler) {
        eprintln!("[main] {}", e);
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn run_device(_start: StartMode) {
    eprintln!("只能在 Kindle 上运行; 主机请用 --preview out.pgm");
}
