//! KinNovel 1.0 入口。
//!
//! - 设备上: `kinnovel` (由 KUAL 启动脚本拉起, 脚本负责暂停/恢复框架进程, 并设置 `KN_APP_DIR`)。
//! - 主机上: `kinnovel --preview out.pgm [--read BOOK SORT] [--tap X,Y]... [--swipe left|right|up|down]...`
//!   用真实页面逻辑 (无显示/输入设备) 渲染最后一帧; `KN_APP_DIR` 指向一份设备缓存的拷贝。

mod api;
mod comic;
mod covers;
mod net;
mod pages;
mod store;
mod tz;

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
}

impl App for KinNovel {
    fn home_page(&mut self) -> Box<dyn Page<Self>> {
        pages::tab_page(pages::Tab::Shelf)
    }

    fn theme(&self) -> Theme {
        Theme::new(self.night)
    }
}

impl KinNovel {
    #[allow(clippy::too_many_arguments)]
    fn new(paths: Paths, config: Config, ui_fonts: Vec<FontId>, width: u32, height: u32, dpi: u32, light: kn_platform::Frontlight, online: bool) -> Self {
        let night = config.bool("night_mode", false);
        let net_client = net::client(&paths, &config);
        let email = config.string("account_email", "").trim().to_string();
        let password = config.string("account_password", "");
        net::set_account((!email.is_empty() && !password.is_empty()).then_some((email, password)));
        KinNovel { paths, config, ui_fonts, metrics: Metrics::for_screen(width, height), night, chapter_font: None, light, dpi, net_client, online }
    }

    /// 按 300 ppi 设计的像素值换算到本机。
    pub fn dpi_px(&self, v: f64) -> i64 {
        (v * self.dpi as f64 / 300.0).round() as i64
    }

    /// 在线时返回网络客户端 (廉价克隆, 可带进后台任务)。
    pub fn net(&self) -> Option<kn_net::Client> {
        self.online.then(|| self.net_client.clone())
    }

    /// 有令牌就不动; 没有令牌但配置里有账号密码时, 后台登录 (与 Python 版账号页的做法一致)。
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn login_in_background(&self) {
        let Some(net) = self.net() else { return };
        if net.user().is_some() || !net::has_account() {
            return;
        }
        std::thread::spawn(move || match net::ensure_login(&net) {
            Ok(()) => eprintln!("[net] 已登录"),
            Err(e) => eprintln!("[net] {e}"),
        });
    }

    /// 已登录, 或配置了账号 (需要时自动登录); fixture 预览模式也算。
    pub fn signed_in(&self) -> bool {
        api::fake_mode() || self.net().is_some_and(|n| n.user().is_some() || n.has_refresh_token() || net::has_account())
    }
}

/// 界面字体: KN_FONT 环境变量 > 配置 font_path (与 Python 版同键) > 系统黑体。
fn load_ui_fonts(fonts: &mut FontStore, config: &Config) -> Vec<FontId> {
    let mut candidates: Vec<PathBuf> = std::env::var("KN_FONT").ok().map(PathBuf::from).into_iter().collect();
    let configured = config.string("font_path", "");
    if !configured.trim().is_empty() {
        candidates.push(PathBuf::from(configured.trim()));
    }
    candidates.extend(SYSTEM_FONTS.iter().map(PathBuf::from));
    for path in candidates {
        if Path::new(&path).exists() {
            match fonts.load_file(&path) {
                Ok(id) => return vec![id],
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
    let config = Config::load(paths.config_file());
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
    let app = KinNovel::new(paths, config, ui_fonts, w, h, dpi, kn_platform::Frontlight::detect(None), online);
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
            "--tap" => {
                let (x, y) = args[i + 1].split_once(',').expect("x,y");
                let p = (x.parse().unwrap(), y.parse().unwrap());
                ui.input(&gesture(GestureKind::Tap, p, p));
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
    if args.len() >= 3 && args[1] == "--preview" {
        preview(&args[2..]);
        return;
    }
    run_device();
}

#[cfg(target_os = "linux")]
fn run_device() {
    use kn_platform::{Display, FbinkDisplay, GestureConfig, InputReader, PowerMonitor};
    use kn_ui::{RefreshPolicy, RefreshScheduler};

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
    let paths = Paths::from_env();
    let config = Config::load(paths.config_file());
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
    let app = KinNovel::new(paths, config, ui_fonts, info.width, info.height, info.dpi, light, online);
    app.login_in_background();
    let mut input = InputReader::open(&info, GestureConfig::default()).expect("输入设备初始化失败");
    // KN_TEST_GSENSOR=1: 在没有重力感应的机型上也走这条路径 (配合 KN_ORIENTATION=D 实机测试倒拿)
    if info.has_gsensor() || std::env::var_os("KN_TEST_GSENSOR").is_some() {
        // KOReader (Oasis / Scribe init): 启动时读 `com.lab126.winmgr accelerometer` (U/D/L/R) 定朝向。
        // winmgr 属于框架, 暂停后读不了, 由启动脚本在暂停前读好传进来; 读不到就跟帧缓冲的旋转走。
        let upside_down = match std::env::var("KN_ORIENTATION").as_deref() {
            Ok("D") => true,
            Ok("U") => false,
            _ => info.fb_rota == 2,
        };
        eprintln!("[device] gsensor: upside_down={upside_down} (fb rota {})", info.fb_rota);
        display.set_upside_down(upside_down);
        input.set_upside_down(upside_down);
    }
    let power = PowerMonitor::start();
    let scheduler = RefreshScheduler::new(info.width, info.height, policy);
    if let Err(e) = kn_ui::run(app, Box::new(display), fonts, input, power, scheduler) {
        eprintln!("[main] {}", e);
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn run_device() {
    eprintln!("只能在 Kindle 上运行; 主机请用 --preview out.pgm");
}
