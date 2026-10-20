//! 锁屏密码 (6 位数字; 设置 → 内容 → 锁屏密码 / 修改锁屏密码)。设置 `lock_pin` 存加盐摘要
//! ([`hash_pin`]); 旧版存的是明文, 启动时换成摘要 ([`migrate`])。连续输错几次后要等一会儿才能再输。
//! - 启动: 代替首页 (`KinNovel::home_page`), 解锁后回首页 (书架 / 欢迎页)。
//! - 休眠唤醒: 盖在原来的页面上 (`KinNovel::take_transition`), 解锁后返回。
//! - 设置页: 开启 = 输两遍新密码; 关闭 / 修改 = 先输当前密码。
//! 输满 6 位自动验证, 不用按确认; 错了清空重输。解锁时左下角是「退出」(框架暂停着, 忘了密码也能回到系统),
//! 设置流程里是「取消」。
//!
//! 样子是居中的弹窗 (参照 KOReader 的 screenlockpin 插件): 启动 / 唤醒时背景就是屏幕上原来的画面
//! (启动画面 / 屏保, 从帧缓冲读回, `Backdrop::Screen`), 设置流程里是设置页 (`Backdrop::Below`); 只刷新弹窗。

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, Ink};
use kn_ui::{Backdrop, Cx, HitId, Page, PageId, RefreshHint, Transition};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::store::Config;
use crate::KinNovel;

pub const PIN_LEN: usize = 6;
const KEY_BASE: u32 = 10;
const HIT_DELETE: HitId = HitId(20);
const HIT_EXIT: HitId = HitId(21);

const TIMER_RETRY: u32 = 1;
/// 连续输错这么多次后要等一会儿才能再输
const FREE_TRIES: u32 = 5;
const FIRST_WAIT: Duration = Duration::from_secs(30);
/// 保存的格式: `sha256:<轮数>:<盐 hex>:<摘要 hex>`
const HASH_PREFIX: &str = "sha256";
const HASH_ROUNDS: u32 = 10_000;
const SALT_LEN: usize = 16;

/// 旧格式: 明文 6 位数字 (字符串, 或不以 0 开头的 6 位整数)。
fn plain_pin(config: &Config) -> Option<String> {
    let pin = match config.int("lock_pin", -1) {
        n @ 100_000..=999_999 => n.to_string(),
        _ => config.string("lock_pin", ""),
    };
    (pin.len() == PIN_LEN && pin.bytes().all(|b| b.is_ascii_digit())).then_some(pin)
}

/// 解析保存的摘要: (轮数, 盐, 摘要)。
fn parse_hash(text: &str) -> Option<(u32, Vec<u8>, [u8; 32])> {
    let mut parts = text.split(':');
    if parts.next()? != HASH_PREFIX {
        return None;
    }
    let rounds: u32 = parts.next()?.parse().ok().filter(|r| (1..=1_000_000).contains(r))?;
    let salt = hex_decode(parts.next()?).filter(|s| !s.is_empty())?;
    let digest: [u8; 32] = hex_decode(parts.next()?)?.try_into().ok()?;
    parts.next().is_none().then_some((rounds, salt, digest))
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn digest(rounds: u32, salt: &[u8], pin: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut d: [u8; 32] = Sha256::new().chain_update(salt).chain_update(pin.as_bytes()).finalize().into();
    for _ in 1..rounds {
        d = Sha256::new().chain_update(salt).chain_update(d).finalize().into();
    }
    d
}

/// 随机盐: Linux 上读 /dev/urandom, 读不到时用标准库的随机哈希种子 (取自系统随机数) 加时间。
fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    #[cfg(unix)]
    {
        use std::io::Read;
        if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut salt)).is_ok() {
            return salt;
        }
    }
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    for (i, chunk) in salt.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_usize(i);
        h.write_u32(std::process::id());
        chunk.copy_from_slice(&h.finish().to_le_bytes()[..chunk.len()]);
    }
    salt
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 要保存的形式 (加盐摘要, 不存明文)。
pub fn hash_pin(pin: &str) -> String {
    let salt = random_salt();
    format!("{HASH_PREFIX}:{HASH_ROUNDS}:{}:{}", hex(&salt), hex(&digest(HASH_ROUNDS, &salt, pin)))
}

/// 设了锁屏密码: 摘要, 或旧版的明文 6 位数字; 其余一律视为未设置。
pub fn pin_set(config: &Config) -> bool {
    plain_pin(config).is_some() || parse_hash(&config.string("lock_pin", "")).is_some()
}

/// 输入的密码与设置里的相符 (两种格式都认)。
pub fn pin_matches(config: &Config, entered: &str) -> bool {
    if let Some(pin) = plain_pin(config) {
        return pin == entered;
    }
    let Some((rounds, salt, want)) = parse_hash(&config.string("lock_pin", "")) else { return false };
    let got = digest(rounds, &salt, entered);
    got.iter().zip(want.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// 启动时把旧版的明文密码换成摘要 (原来的密码照样能用)。
pub fn migrate(config: &mut Config) {
    if let Some(pin) = plain_pin(config) {
        config.set("lock_pin", Value::from(hash_pin(&pin)));
    }
}

/// 连续输错的次数和要等到的时刻 (只在内存里, 解锁 / 验证成功后清零)
struct Throttle {
    fails: u32,
    until: Option<Instant>,
}

static THROTTLE: Mutex<Throttle> = Mutex::new(Throttle { fails: 0, until: None });

fn throttle() -> std::sync::MutexGuard<'static, Throttle> {
    THROTTLE.lock().unwrap_or_else(|e| e.into_inner())
}

/// 还要等多久才能再输。
fn wait_left() -> Option<Duration> {
    let until = throttle().until?;
    until.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())
}

/// 输错一次: 前几次不限制, 之后每次都要等, 等待时间翻倍 (最长约 32 分钟)。
fn record_failure() {
    let mut t = throttle();
    t.fails += 1;
    if t.fails >= FREE_TRIES {
        t.until = Some(Instant::now() + FIRST_WAIT * (1u32 << (t.fails - FREE_TRIES).min(6)));
    }
}

fn record_success() {
    *throttle() = Throttle { fails: 0, until: None };
}

fn wait_text(left: Duration) -> String {
    let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    if secs <= 90 {
        format!("错误太多, 请 {secs} 秒后再试")
    } else {
        format!("错误太多, 请 {} 分钟后再试", secs.div_ceil(60))
    }
}

/// 验证当前密码之后做什么
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum After {
    Disable,
    Change,
}

#[derive(Clone, PartialEq, Eq)]
enum Mode {
    /// 解锁; `resume` = 唤醒后盖在原页面上 (解锁 = 返回), 否则是启动时的首页 (解锁 = 回首页)
    Unlock { resume: bool },
    /// 设置页: 先输当前密码
    Verify(After),
    /// 设置页: 输新密码 / 再输一次
    New,
    Confirm(String),
}

pub struct LockPage {
    mode: Mode,
    digits: String,
    /// 提示行显示的错误 (密码错误 / 两次不一致)
    message: Option<&'static str>,
    /// 弹窗后面的画面; 运行时给过一次之后不再要
    backdrop: Option<Bitmap>,
    backdrop_set: bool,
}

impl LockPage {
    fn with(mode: Mode) -> Self {
        LockPage { mode, digits: String::new(), message: None, backdrop: None, backdrop_set: false }
    }

    pub fn startup() -> Self {
        Self::with(Mode::Unlock { resume: false })
    }

    pub fn resume() -> Self {
        Self::with(Mode::Unlock { resume: true })
    }

    /// 设置页: 开启 (设新密码)
    pub fn enable() -> Self {
        Self::with(Mode::New)
    }

    /// 设置页: 关闭 / 修改 (先验证当前密码)
    pub fn verify(after: After) -> Self {
        Self::with(Mode::Verify(after))
    }

    fn unlocking(&self) -> bool {
        matches!(self.mode, Mode::Unlock { .. })
    }

    /// 这一步要核对已设的密码 (受输错次数限制)
    fn checks_pin(&self) -> bool {
        matches!(self.mode, Mode::Unlock { .. } | Mode::Verify(_))
    }

    fn prompt(&self) -> String {
        if let Some(left) = wait_left().filter(|_| self.checks_pin()) {
            return wait_text(left);
        }
        if let Some(m) = self.message {
            return m.to_string();
        }
        match self.mode {
            Mode::Unlock { .. } => "输入密码",
            Mode::Verify(_) => "输入当前密码",
            Mode::New => "设置 6 位数字密码",
            Mode::Confirm(_) => "再输入一次",
        }
        .to_string()
    }

    fn press(&mut self, cx: &mut Cx<KinNovel>, digit: char) -> Transition<KinNovel> {
        if self.digits.len() >= PIN_LEN {
            return Transition::None;
        }
        if self.checks_pin() && wait_left().is_some() {
            // 等待中不收输入, 只刷新剩余时间
            cx.request_redraw(RefreshHint::Ui);
            return Transition::None;
        }
        self.digits.push(digit);
        self.message = None;
        cx.request_redraw(RefreshHint::Ui);
        if self.digits.len() < PIN_LEN {
            return Transition::None;
        }
        let entered = std::mem::take(&mut self.digits);
        let correct = !pin_set(&cx.app.config) || pin_matches(&cx.app.config, &entered);
        if self.checks_pin() {
            if correct {
                record_success();
            } else {
                record_failure();
            }
        }
        match self.mode.clone() {
            Mode::Unlock { resume } if correct => {
                cx.app.unlock();
                // 换成完整的页面 (背景是启动画面 / 屏保): 整屏闪刷
                cx.request_redraw(RefreshHint::Flash);
                return if resume { Transition::Back } else { Transition::Home };
            }
            Mode::Verify(After::Disable) if correct => {
                cx.app.config.set("lock_pin", Value::from(""));
                cx.request_redraw(RefreshHint::Clean);
                return Transition::Back;
            }
            Mode::Verify(After::Change) if correct => self.mode = Mode::New,
            Mode::Unlock { .. } | Mode::Verify(_) => self.message = Some("密码错误"),
            Mode::New => self.mode = Mode::Confirm(entered),
            Mode::Confirm(first) if first == entered => {
                cx.app.config.set("lock_pin", Value::from(hash_pin(&entered)));
                // 本次运行已经在用了, 不必马上再解锁
                cx.app.unlock();
                cx.request_redraw(RefreshHint::Clean);
                return Transition::Back;
            }
            Mode::Confirm(_) => {
                self.mode = Mode::New;
                self.message = Some("两次输入不一致, 重新设置");
            }
        }
        Transition::None
    }
}

impl Page<KinNovel> for LockPage {
    fn id(&self) -> PageId {
        PageId(195)
    }

    /// 弹窗只刷新自己那块, 进来时不闪
    fn flash_on_enter(&self) -> bool {
        false
    }

    fn wants_backdrop(&self) -> Backdrop {
        match self.mode {
            _ if self.backdrop_set => Backdrop::None,
            Mode::Unlock { .. } => Backdrop::Screen,
            _ => Backdrop::Below,
        }
    }

    fn set_backdrop(&mut self, backdrop: Option<Bitmap>) {
        self.backdrop = backdrop;
        self.backdrop_set = true;
    }

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let (w, h) = (cx.width, cx.height);
        match &self.backdrop {
            Some(bg) if bg.width() == w && bg.height() == h => frame.blit(bg, bg.bounds(), 0, 0),
            // 读不回屏幕时 (主机预览等): 启动用启动画面, 其余用纯底色
            _ if self.mode == (Mode::Unlock { resume: false }) => {
                let splash = crate::splash::render(w, h);
                frame.blit(&splash, splash.bounds(), 0, 0);
            }
            _ => frame.fill_rect(frame.bounds(), theme.background),
        }
        // 等待结束时刷新提示
        if let Some(left) = wait_left().filter(|_| self.checks_pin()) {
            cx.after(left, TIMER_RETRY);
        }
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };

        // 卡片: 提示 + 圆点, 下面 3x4 键格 (细线分隔); 框外点击吞掉
        // 尺寸照 screenlockpin 的 40% 缩放: 键格约占屏宽 62%, 键高约为键宽的 0.57 (KPW5: 255 x 145 px),
        // 按屏宽走, 大屏 (Scribe) 不会因为界面缩放的上限变扁; 小屏至少 1.2 个触控高度
        let key_w = (w as f32 * 0.62 / 3.0) as i32;
        let key_h = ((key_w as f32 * 0.57) as i32).max((m.touch as f32 * 1.2) as i32);
        let card_w = (key_w * 3).min(w as i32 - 2 * m.margin as i32);
        let key_w = card_w / 3;
        let head_h = (m.body * 1.4) as i32 + (m.body * 0.55) as i32 + m.margin as i32 * 2;
        let card_h = head_h + key_h * 4;
        let card = Rect::new((w as i32 - card_w) / 2, (h as i32 - card_h) / 2, card_w as u32, card_h as u32);
        cx.hits.add(HitId(u32::MAX), Rect::new(0, 0, w, h)).no_feedback();
        widgets::card(frame, &theme, &m, card);

        let body_h = (m.body * 1.4) as i32;
        let mut y = card.y + m.margin as i32 * 3 / 4;
        let prompt = ink.fit(&self.prompt(), m.body, (card.w as i32 - 2 * m.margin as i32) as f32);
        ink.text_centered(frame, Rect::new(card.x, y, card.w, body_h as u32), &prompt, m.body, theme.foreground);
        y += body_h + m.margin as i32 / 4;
        let dot = (m.body * 0.55) as i32;
        let gap = dot;
        let total = PIN_LEN as i32 * dot + (PIN_LEN as i32 - 1) * gap;
        let mut x = card.x + (card_w - total) / 2;
        for i in 0..PIN_LEN {
            let r = Rect::new(x, y, dot as u32, dot as u32);
            let filled = i < self.digits.len();
            frame.rounded_rect(r, (dot / 2) as u32, filled.then_some(theme.foreground), Some(theme.foreground), (dot / 10).max(2) as u32);
            x += dot + gap;
        }

        let top = card.y + head_h;
        let exit = if self.unlocking() { "退出" } else { "取消" };
        for (i, label) in ["1", "2", "3", "4", "5", "6", "7", "8", "9", exit, "0", "删除"].iter().enumerate() {
            let (col, row) = ((i % 3) as i32, (i / 3) as i32);
            // 最右一列补齐卡片宽度的余数; 最下一行收进圆角里
            let cw = if col == 2 { card_w - 2 * key_w } else { key_w };
            let r = Rect::new(card.x + col * key_w, top + row * key_h, cw as u32, key_h as u32);
            let id = match i {
                9 => HIT_EXIT,
                11 => HIT_DELETE,
                10 => HitId(KEY_BASE),
                n => HitId(KEY_BASE + n as u32 + 1),
            };
            let size = if label.len() == 1 { m.title } else { m.body };
            let color = if label.len() == 1 { theme.foreground } else { theme.muted };
            ink.text_centered(frame, r, label, size, color);
            cx.hits.add(id, r.inflate(-2));
        }
        // 键格细线: 横线 4 条 (含顶边), 竖线 2 条
        for row in 0..4 {
            frame.fill_rect(Rect::new(card.x + 2, top + row * key_h, card.w - 4, 1), theme.mid);
        }
        for col in 1..3 {
            frame.fill_rect(Rect::new(card.x + col * key_w, top, 1, (key_h * 4 - (m.radius as i32)) as u32), theme.mid);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<KinNovel>, token: u32) {
        if token == TIMER_RETRY {
            cx.request_redraw(RefreshHint::Ui);
        }
    }

    fn on_input(&mut self, cx: &mut Cx<KinNovel>, event: &InputEvent) -> Transition<KinNovel> {
        let InputEvent::Gesture(g) = event else { return Transition::None };
        if g.kind != GestureKind::Tap {
            return Transition::None;
        }
        match cx.hits.at(Point { x: g.start.0, y: g.start.1 }) {
            Some(HIT_EXIT) if self.unlocking() => Transition::Exit,
            Some(HIT_EXIT) => Transition::Back,
            Some(HIT_DELETE) => {
                if self.digits.pop().is_some() || self.message.is_some() {
                    self.message = None;
                    cx.request_redraw(RefreshHint::Ui);
                }
                Transition::None
            }
            Some(HitId(n)) if (KEY_BASE..KEY_BASE + 10).contains(&n) => {
                let digit = char::from(b'0' + (n - KEY_BASE) as u8);
                self.press(cx, digit)
            }
            _ => Transition::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(v: Value) -> Config {
        let mut c = Config::in_memory();
        c.set("lock_pin", v);
        c
    }

    #[test]
    fn only_six_digits_count() {
        let pin = |v| plain_pin(&config(v));
        assert_eq!(pin(json!("012345")), Some("012345".into()));
        assert_eq!(pin(json!(123456)), Some("123456".into()));
        for bad in [json!(""), json!("12345"), json!("1234567"), json!("12a456"), json!(" 12345"), json!("１２３４５６"), json!(12345), json!(null), json!(true)] {
            assert_eq!(pin(bad.clone()), None, "{bad}");
            assert!(!pin_set(&config(bad.clone())), "{bad}");
        }
        assert!(!pin_set(&Config::in_memory()));
    }

    #[test]
    fn plain_pins_become_salted_hashes_and_keep_working() {
        for (old, entered) in [(json!("012345"), "012345"), (json!(123456), "123456")] {
            let mut c = config(old);
            assert!(pin_set(&c) && pin_matches(&c, entered) && !pin_matches(&c, "000000"));
            migrate(&mut c);
            let stored = c.string("lock_pin", "");
            assert!(stored.starts_with("sha256:10000:") && !stored.contains(entered), "{stored}");
            assert!(pin_set(&c) && pin_matches(&c, entered) && !pin_matches(&c, "000000"));
            // 已经是摘要: 不再改
            migrate(&mut c);
            assert_eq!(c.string("lock_pin", ""), stored);
        }
        // 同一个密码每次的盐不同
        assert_ne!(hash_pin("111111"), hash_pin("111111"));
        // 固定的盐和轮数: 摘要格式稳定
        let fixed = format!("sha256:2:0011:{}", hex(&digest(2, &[0x00, 0x11], "654321")));
        assert!(pin_matches(&config(json!(fixed)), "654321"));
        for bad in ["sha256:0:00:00", "sha256:10:zz:00", "sha256:10:00:abcd", "md5:1:00:00", "sha256:1::", &format!("{fixed}:x")] {
            assert!(!pin_set(&config(json!(bad))), "{bad}");
            assert!(!pin_matches(&config(json!(bad)), "654321"), "{bad}");
        }
    }

    #[test]
    fn wrong_pins_make_the_user_wait_longer_each_time() {
        record_success();
        for _ in 0..FREE_TRIES - 1 {
            record_failure();
            assert!(wait_left().is_none());
        }
        record_failure();
        let first = wait_left().unwrap();
        assert!(first > Duration::from_secs(28) && first <= FIRST_WAIT, "{first:?}");
        record_failure();
        let second = wait_left().unwrap();
        assert!(second > Duration::from_secs(58) && second <= FIRST_WAIT * 2, "{second:?}");
        for _ in 0..20 {
            record_failure();
        }
        assert!(wait_left().unwrap() <= FIRST_WAIT * 64);
        record_success();
        assert!(wait_left().is_none());
        assert_eq!(wait_text(Duration::from_millis(29_200)), "错误太多, 请 30 秒后再试");
        assert_eq!(wait_text(Duration::from_secs(240)), "错误太多, 请 4 分钟后再试");
    }
}
