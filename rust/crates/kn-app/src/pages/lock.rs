//! 锁屏密码 (设置 `lock_pin`: 6 位数字; 设置 → 内容 → 锁屏密码 / 修改锁屏密码)。
//! - 启动: 代替首页 (`KinNovel::home_page`), 解锁后回首页 (书架 / 欢迎页)。
//! - 休眠唤醒: 盖在原来的页面上 (`KinNovel::take_transition`), 解锁后返回。
//! - 设置页: 开启 = 输两遍新密码; 关闭 / 修改 = 先输当前密码。
//! 输满 6 位自动验证, 不用按确认; 错了清空重输。解锁时左下角是「退出」(框架暂停着, 忘了密码也能回到系统),
//! 设置流程里是「取消」。

use kn_platform::{GestureKind, InputEvent};
use kn_render::{Bitmap, Point, Rect};
use kn_ui::widgets::{self, ButtonStyle, Ink};
use kn_ui::{Cx, HitId, Page, PageId, RefreshHint, Transition};
use serde_json::Value;

use crate::KinNovel;

pub const PIN_LEN: usize = 6;
const KEY_BASE: u32 = 10;
const HIT_DELETE: HitId = HitId(20);
const HIT_EXIT: HitId = HitId(21);

/// 设置里的锁屏密码: 恰好 6 位数字才算 (字符串, 或不以 0 开头的 6 位整数); 其余一律视为未设置。
pub fn configured_pin(config: &crate::store::Config) -> Option<String> {
    let pin = match config.int("lock_pin", -1) {
        n @ 100_000..=999_999 => n.to_string(),
        _ => config.string("lock_pin", ""),
    };
    (pin.len() == PIN_LEN && pin.bytes().all(|b| b.is_ascii_digit())).then_some(pin)
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
}

impl LockPage {
    fn with(mode: Mode) -> Self {
        LockPage { mode, digits: String::new(), message: None }
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

    fn prompt(&self) -> &'static str {
        if let Some(m) = self.message {
            return m;
        }
        match self.mode {
            Mode::Unlock { .. } => "输入密码",
            Mode::Verify(_) => "输入当前密码",
            Mode::New => "设置 6 位数字密码",
            Mode::Confirm(_) => "再输入一次",
        }
    }

    fn press(&mut self, cx: &mut Cx<KinNovel>, digit: char) -> Transition<KinNovel> {
        if self.digits.len() >= PIN_LEN {
            return Transition::None;
        }
        self.digits.push(digit);
        self.message = None;
        cx.request_redraw(RefreshHint::Ui);
        if self.digits.len() < PIN_LEN {
            return Transition::None;
        }
        let entered = std::mem::take(&mut self.digits);
        let correct = configured_pin(&cx.app.config).is_none_or(|pin| pin == entered);
        match self.mode.clone() {
            Mode::Unlock { resume } if correct => {
                cx.app.unlock();
                return if resume { Transition::Back } else { Transition::Home };
            }
            Mode::Verify(After::Disable) if correct => {
                cx.app.config.set("lock_pin", Value::from(""));
                return Transition::Back;
            }
            Mode::Verify(After::Change) if correct => self.mode = Mode::New,
            Mode::Unlock { .. } | Mode::Verify(_) => self.message = Some("密码错误"),
            Mode::New => self.mode = Mode::Confirm(entered),
            Mode::Confirm(first) if first == entered => {
                cx.app.config.set("lock_pin", Value::from(entered));
                // 本次运行已经在用了, 不必马上再解锁
                cx.app.unlock();
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

    fn render(&mut self, cx: &mut Cx<KinNovel>, frame: &mut Bitmap) {
        let theme = cx.theme;
        let m = cx.app.metrics;
        let chain = cx.app.ui_fonts.clone();
        let mut ink = Ink { fonts: &*cx.fonts, glyphs: &mut *cx.glyphs, chain: &chain };
        let (w, h) = (cx.width as i32, cx.height as i32);
        let side = m.margin as i32;

        // 时间与日期
        let utc = crate::store::unix_now();
        let local = utc + crate::tz::local_utc_offset_secs(utc);
        let (days, minutes) = (local.div_euclid(86_400), local.rem_euclid(86_400) / 60);
        let (_, month, day) = crate::stats::civil_from_days(days);
        // 1970-01-01 是星期四
        let weekday = ["四", "五", "六", "日", "一", "二", "三"][days.rem_euclid(7) as usize];
        let mut y = h / 9;
        let hero_h = (m.hero * 1.3) as i32;
        ink.text_centered(frame, Rect::new(0, y, w as u32, hero_h as u32), &format!("{:02}:{:02}", minutes / 60, minutes % 60), m.hero, theme.foreground);
        y += hero_h;
        let small_h = (m.small * 1.4) as i32;
        let date = format!("{month} 月 {day} 日 · 星期{weekday}");
        ink.text_centered(frame, Rect::new(0, y, w as u32, small_h as u32), &date, m.small, theme.muted);
        y += small_h + m.margin as i32 * 2;

        // 提示 + 圆点
        let body_h = (m.body * 1.4) as i32;
        ink.text_centered(frame, Rect::new(0, y, w as u32, body_h as u32), self.prompt(), m.body, theme.foreground);
        y += body_h + m.margin as i32 / 2;
        let dot = (m.body * 0.55) as i32;
        let gap = dot;
        let total = PIN_LEN as i32 * dot + (PIN_LEN as i32 - 1) * gap;
        let mut x = (w - total) / 2;
        for i in 0..PIN_LEN {
            let r = Rect::new(x, y, dot as u32, dot as u32);
            let filled = i < self.digits.len();
            frame.rounded_rect(r, (dot / 2) as u32, filled.then_some(theme.foreground), Some(theme.foreground), (dot / 10).max(2) as u32);
            x += dot + gap;
        }

        // 数字键盘: 1-9, 退出 (设置流程里是取消) 0 删除
        let key_h = (m.touch as f32 * 1.1) as i32;
        let key_gap = m.margin as i32 / 2;
        let pad_w = (w - 2 * side).min(m.touch as i32 * 6);
        let key_w = (pad_w - 2 * key_gap) / 3;
        let pad_h = 4 * key_h + 3 * key_gap;
        let top = h - side * 2 - pad_h;
        let left = (w - pad_w) / 2;
        let exit = if self.unlocking() { "退出" } else { "取消" };
        for (i, label) in ["1", "2", "3", "4", "5", "6", "7", "8", "9", exit, "0", "删除"].iter().enumerate() {
            let (col, row) = ((i % 3) as i32, (i / 3) as i32);
            let r = Rect::new(left + col * (key_w + key_gap), top + row * (key_h + key_gap), key_w as u32, key_h as u32);
            let id = match i {
                9 => HIT_EXIT,
                11 => HIT_DELETE,
                10 => HitId(KEY_BASE),
                n => HitId(KEY_BASE + n as u32 + 1),
            };
            widgets::button(&mut ink, frame, &theme, &m, r, label, ButtonStyle::Secondary);
            cx.hits.add(id, r).rounded(m.radius);
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
    use super::configured_pin;
    use crate::store::Config;
    use serde_json::json;

    #[test]
    fn only_six_digits_count() {
        let pin = |v| {
            let mut c = Config::in_memory();
            c.set("lock_pin", v);
            configured_pin(&c)
        };
        assert_eq!(pin(json!("012345")), Some("012345".into()));
        assert_eq!(pin(json!(123456)), Some("123456".into()));
        for bad in [json!(""), json!("12345"), json!("1234567"), json!("12a456"), json!(" 12345"), json!("１２３４５６"), json!(12345), json!(null), json!(true)] {
            assert_eq!(pin(bad.clone()), None, "{bad}");
        }
        assert_eq!(configured_pin(&Config::in_memory()), None);
    }
}
