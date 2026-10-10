//! evdev 输入。
//!
//! - 设备发现: Linux 上用 FBInk `fbink_input_scan` (触摸屏 / 翻页键 / Home / 电源键),
//!   失败时回退为扫描 `/dev/input/event*` 的能力位 (EVIOCGBIT)。
//! - 读取: 非阻塞 `read()` 原始 `input_event`; 注意 armv7 上 `timeval` 是两个 32 位 long (16 字节/事件)。
//! - 坐标: 用 EVIOCGABS 取范围, 映射到屏幕像素后按 DeviceInfo 的 swap/mirror 变换, 再交给 GestureRecognizer。
//! - 独占: `grab()` / `ungrab()` (EVIOCGRAB), 休眠时释放给系统锁屏。
//! - 机型差异照搬 KOReader (`frontend/device/kindle/device.lua`, `device/input.lua`):
//!   翻页键方向 (Oasis 与 Voyage 相反)、倒拿时翻页键互换、Oasis/Scribe 重力感应事件、
//!   Voyage 压感键旁的触摸冷区、PW6/ColorSoft 的 "敲边框" (KEY_F7 = 下一页)。

// `DeviceInfo`/`GestureConfig` are used by the `open()` signature on every
// platform, but each cfg'd impl below (linux_impl vs. the non-linux stub)
// re-imports what it needs locally, so these top-level imports are only
// actually read on non-linux builds.
#[allow(unused_imports)]
use crate::display::DeviceInfo;
#[allow(unused_imports)]
use crate::gesture::{Gesture, GestureConfig, TouchSample};

/// 物理按键 (Voyage/Oasis 翻页键等), evdev code 见 research-kindle-devices.md。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    PageForward,
    PageBack,
    Home,
    Power,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    Gesture(Gesture),
    Key { code: KeyCode, pressed: bool },
    /// 重力感应: 设备转到竖屏正向 (false) / 倒置 (true)。横屏事件不上报 (界面只有竖屏)。
    Rotation { upside_down: bool },
}

// Standard Linux evdev constants (linux/input-event-codes.h), stable across
// kernel versions; hardcoded here to avoid depending on libc exposing them.
mod evcodes {
    pub const EV_SYN: u16 = 0x00;
    pub const EV_KEY: u16 = 0x01;
    pub const EV_ABS: u16 = 0x03;

    pub const ABS_X: u16 = 0x00;
    pub const ABS_Y: u16 = 0x01;
    pub const ABS_MT_SLOT: u16 = 0x2f;
    pub const ABS_MT_POSITION_X: u16 = 0x35;
    pub const ABS_MT_POSITION_Y: u16 = 0x36;
    pub const ABS_MT_TRACKING_ID: u16 = 0x39;

    pub const BTN_TOUCH: u16 = 0x14a;

    pub const ABS_PRESSURE: u16 = 0x18;

    pub const KEY_POWER: u16 = 116;
    /// Kindle Touch 的 Home 键 (KOReader event_map: 102 = "Home")
    pub const KEY_HOME: u16 = 102;
    /// PW6 / ColorSoft "敲两下边框" (KOReader: 65 → "RPgFwd")
    pub const KEY_FRAME_TAP: u16 = 65;
    // Pagination keys (Voyage/Oasis WhisperTouch), see research-kindle-devices.md.
    pub const KEY_PAGEUP: u16 = 104;
    pub const KEY_PAGEDOWN: u16 = 109;
}

/// 把 evdev key code 映射为 `KeyCode`。`reversed` 用于 Oasis 系列 (翻页键方向与
/// Voyage/非 Oasis 机型相反, 见 research-kindle-devices.md 触屏机型表)。
fn map_key_code(code: u16, reversed: bool) -> KeyCode {
    use evcodes::*;
    match code {
        KEY_POWER => KeyCode::Power,
        KEY_HOME => KeyCode::Home,
        KEY_FRAME_TAP => KeyCode::PageForward,
        KEY_PAGEUP => {
            if reversed {
                KeyCode::PageForward
            } else {
                KeyCode::PageBack
            }
        }
        KEY_PAGEDOWN => {
            if reversed {
                KeyCode::PageBack
            } else {
                KeyCode::PageForward
            }
        }
        other => KeyCode::Other(other),
    }
}

/// Oasis / Scribe 重力感应 (EV_ABS:ABS_PRESSURE 的自定义取值, KOReader `OasisGyroTranslation`):
/// 竖屏正向 → Some(false), 竖屏倒置 → Some(true), 横屏 / 其它 → None。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn orientation_from_gyro(value: i32) -> Option<bool> {
    match value {
        15 | 17 | 19 => Some(false),
        16 | 18 | 20 => Some(true),
        _ => None,
    }
}

/// Voyage 两侧 PagePress 压感键附近的触摸冷区 (KOReader `KindleVoyage.cold_spots`, 屏幕坐标, 与旋转无关)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn in_voyage_cold_spot(x: i32, y: i32) -> bool {
    const SPOTS: [(i32, i32, i32); 4] = [(1080 + 50, 485, 80), (1080 + 70, 910, 120), (-50, 485, 80), (-70, 910, 120)];
    SPOTS.iter().any(|&(sx, sy, r)| (sx - x) * (sx - x) + (sy - y) * (sy - y) < r * r)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn screen_scale(raw: i32, min: i32, max: i32, screen: u32) -> i32 {
    if max <= min {
        return raw;
    }
    let span = (max - min).max(1) as i64;
    let v = ((raw - min) as i64 * screen as i64) / span;
    v.clamp(0, screen as i64 - 1) as i32
}

/// 一个原始轴的值 → 屏幕坐标样本。`raw_x`: 来自触摸芯片的 X 轴 (ABS_X / ABS_MT_POSITION_X)。
/// 先按 swap 决定落到屏幕哪一轴 (并按那一轴的像素数缩放), 再按该轴 mirror (FBInk 语义: 先交换后镜像)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn axis_sample(info: &DeviceInfo, raw_x: bool, raw: i32, min: i32, max: i32) -> TouchSample {
    if raw_x != info.touch_swap_axes {
        let x = screen_scale(raw, min, max, info.width);
        TouchSample::X(if info.touch_mirror_x { info.width as i32 - 1 - x } else { x })
    } else {
        let y = screen_scale(raw, min, max, info.height);
        TouchSample::Y(if info.touch_mirror_y { info.height as i32 - 1 - y } else { y })
    }
}

/// 一对原始坐标 → 屏幕 (x, y)。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn to_screen(info: &DeviceInfo, raw: (i32, i32), xr: (i32, i32), yr: (i32, i32)) -> (i32, i32) {
    let (mut x, mut y) = (0, 0);
    for s in [axis_sample(info, true, raw.0, xr.0, xr.1), axis_sample(info, false, raw.1, yr.0, yr.1)] {
        match s {
            TouchSample::X(v) => x = v,
            TouchSample::Y(v) => y = v,
            _ => {}
        }
    }
    (x, y)
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::evcodes::*;
    use super::{axis_sample, in_voyage_cold_spot, map_key_code, orientation_from_gyro, to_screen, InputEvent, KeyCode};
    use crate::display::DeviceInfo;
    use crate::ffi;
    use crate::gesture::{GestureConfig, GestureRecognizer, TouchSample};
    use std::os::fd::RawFd;

    // NOTE (from rust/spike-fbink): on armv7 (32-bit) with this old kernel,
    // `struct timeval`'s fields are 32-bit `long`, not 64-bit -- using i64
    // here would silently desync `read()` framing against the kernel's
    // actual 16-byte `struct input_event`.
    #[repr(C)]
    #[derive(Debug, Default, Clone, Copy)]
    struct RawInputEvent {
        tv_sec: i32,
        tv_usec: i32,
        type_: u16,
        code: u16,
        value: i32,
    }

    #[repr(C)]
    #[derive(Debug, Default, Clone, Copy)]
    struct InputAbsInfo {
        value: i32,
        minimum: i32,
        maximum: i32,
        fuzz: i32,
        flat: i32,
        resolution: i32,
    }

    // `ioctl`'s request parameter type varies by libc (glibc: c_ulong, musl:
    // c_int) -- build the numeric value in u32 and cast at each call site via
    // `as _` so this compiles for both the musl (device) and glibc/gnu (host)
    // targets.
    fn ioc(dir: u64, ty: u64, nr: u64, size: u64) -> u32 {
        ((dir << 30) | (size << 16) | (ty << 8) | nr) as u32
    }

    const IOC_READ: u64 = 2;
    const IOC_WRITE: u64 = 1;
    const EV_IOC_TYPE: u64 = 0x45; // 'E'

    fn eviocgrab() -> u32 {
        ioc(IOC_WRITE, EV_IOC_TYPE, 0x90, 4)
    }
    /// 让该 fd 的事件时间戳使用 CLOCK_MONOTONIC (默认是会被 NTP/用户改时间影响的 REALTIME)。
    fn eviocsclockid() -> u32 {
        ioc(IOC_WRITE, EV_IOC_TYPE, 0xa0, 4)
    }
    fn eviocgabs(abs: u16) -> u32 {
        ioc(
            IOC_READ,
            EV_IOC_TYPE,
            0x40 + abs as u64,
            std::mem::size_of::<InputAbsInfo>() as u64,
        )
    }
    /// MT 触点数上限 (读 EVIOCGMTSLOTS 用; KPW5 是 10 个 slot)。
    const MT_SLOTS: usize = 16;
    fn eviocgmtslots() -> u32 {
        ioc(IOC_READ, EV_IOC_TYPE, 0x0a, (std::mem::size_of::<i32>() * (1 + MT_SLOTS)) as u64)
    }

    /// slot 0 上某个 MT 轴的当前值。MT 轴的 `EVIOCGABS` value 并不跟踪各 slot 的实际坐标
    /// (内核把 MT 值存在 input_mt 的 slot 里), 实测 KPW5 重启后读到 0, 只能用 EVIOCGMTSLOTS。
    fn mt_slot0_value(fd: RawFd, code: u16) -> Option<i32> {
        let mut buf = [0i32; 1 + MT_SLOTS];
        buf[0] = code as i32;
        let rc = unsafe { libc::ioctl(fd, eviocgmtslots() as _, buf.as_mut_ptr()) };
        (rc >= 0).then_some(buf[1])
    }

    fn eviocgbit(ev_type: u16, size: usize) -> u32 {
        ioc(IOC_READ, EV_IOC_TYPE, 0x20 + ev_type as u64, size as u64)
    }

    enum Kind {
        Touch {
            recognizer: GestureRecognizer,
            // Raw ABS ranges, used to scale to screen pixels.
            x_min: i32,
            x_max: i32,
            y_min: i32,
            y_max: i32,
            #[allow(dead_code)]
            single_axis: bool, // device only reports ABS_X/ABS_Y + BTN_TOUCH (reserved for future per-axis fallback refinement)
        },
        Key,
        /// 重力感应 (只读 ABS_PRESSURE)
        Rotation,
    }

    struct Device {
        fd: RawFd,
        kind: Kind,
        grabbed: bool,
    }

    pub struct InputReader {
        devices: Vec<Device>,
        /// 坐标变换用的设备信息 (倒拿时在 FBInk 的 mirror 上再叠加两轴镜像)
        info: DeviceInfo,
        base_info: DeviceInfo,
        key_reversed: bool,
        upside_down: bool,
        voyage: bool,
    }

    impl InputReader {
        pub fn open(info: &DeviceInfo, config: GestureConfig) -> std::io::Result<Self> {
            let match_mask = ffi::INPUT_TOUCHSCREEN
                | ffi::INPUT_SCALED_TABLET
                | ffi::INPUT_PAGINATION_BUTTONS
                | ffi::INPUT_HOME_BUTTON
                | ffi::INPUT_DPAD
                | ffi::INPUT_POWER_BUTTON
                | ffi::INPUT_KINDLE_FRAME_TAP;

            let mut raw = [ffi::ShimInputDevice::default(); 32];
            let n =
                unsafe { ffi::shim_input_scan(match_mask, 0, raw.as_mut_ptr(), raw.len(), 0) };

            let mut devices = Vec::new();
            let is_touch_type = ffi::INPUT_TOUCHSCREEN | ffi::INPUT_SCALED_TABLET;
            let is_key_type = ffi::INPUT_PAGINATION_BUTTONS
                | ffi::INPUT_HOME_BUTTON
                | ffi::INPUT_POWER_BUTTON
                | ffi::INPUT_KINDLE_FRAME_TAP;

            for d in raw.iter().take(n.min(raw.len())) {
                if d.matched == 0 || d.fd < 0 {
                    continue;
                }
                if d.type_ & is_touch_type != 0 {
                    devices.push(Self::build_touch_device(d.fd, info, config));
                } else if d.type_ & is_key_type != 0 {
                    devices.push(Device {
                        fd: d.fd,
                        kind: Kind::Key,
                        grabbed: false,
                    });
                } else {
                    unsafe {
                        libc::close(d.fd);
                    }
                }
            }

            if devices.is_empty() {
                Self::fallback_scan(info, config, &mut devices);
            }

            // 重力感应: 报 ABS_PRESSURE 但不是触摸屏/手写笔的设备 (与 KOReader 相同的扫描条件)
            if info.has_gsensor() {
                let mut rot = [ffi::ShimInputDevice::default(); 8];
                let n = unsafe {
                    ffi::shim_input_scan(ffi::INPUT_ROTATION_EVENT, ffi::INPUT_TABLET | ffi::INPUT_TOUCHSCREEN, rot.as_mut_ptr(), rot.len(), 0)
                };
                for d in rot.iter().take(n.min(rot.len())) {
                    if d.matched != 0 && d.fd >= 0 {
                        devices.push(Device { fd: d.fd, kind: Kind::Rotation, grabbed: false });
                    }
                }
            }

            // KOReader: Oasis 系列 104 = 下一页, 109 = 上一页; Voyage 及其它相反
            let key_reversed = info.device_codename.to_lowercase().contains("oasis")
                || info.device_name.to_lowercase().contains("oasis");

            Ok(InputReader {
                devices,
                info: info.clone(),
                base_info: info.clone(),
                key_reversed,
                upside_down: false,
                voyage: info.caps.is_some_and(|c| c.voyage),
            })
        }

        /// 用户倒拿 (竖屏 180°): 触摸两轴镜像, 翻页键互换 (KOReader `rotation_map` 的 UPSIDE_DOWN)。
        pub fn set_upside_down(&mut self, upside_down: bool) {
            self.upside_down = upside_down;
            self.info = self.base_info.clone();
            self.info.touch_mirror_x ^= upside_down;
            self.info.touch_mirror_y ^= upside_down;
            for dev in self.devices.iter_mut() {
                if let Kind::Touch { recognizer, .. } = &mut dev.kind {
                    recognizer.reset();
                }
            }
        }

        fn build_touch_device(fd: RawFd, info: &DeviceInfo, config: GestureConfig) -> Device {
            let clock: i32 = libc::CLOCK_MONOTONIC;
            unsafe {
                libc::ioctl(fd, eviocsclockid() as _, &clock as *const i32);
            }
            let mut abs = InputAbsInfo::default();
            let rc = unsafe { libc::ioctl(fd, eviocgabs(ABS_MT_POSITION_X) as _, &mut abs as *mut _) };
            let (mut x_min, mut x_max) = if rc == 0 && abs.maximum > abs.minimum {
                (abs.minimum, abs.maximum)
            } else {
                (0, 0)
            };
            let mut abs_y = InputAbsInfo::default();
            let rc_y =
                unsafe { libc::ioctl(fd, eviocgabs(ABS_MT_POSITION_Y) as _, &mut abs_y as *mut _) };
            let (mut y_min, mut y_max) = if rc_y == 0 && abs_y.maximum > abs_y.minimum {
                (abs_y.minimum, abs_y.maximum)
            } else {
                (0, 0)
            };

            // 内核当前的 slot 坐标: 下一次触摸若落在同一 X/Y, 该轴不会重发 (见 GestureRecognizer::last_pos)
            let mut seed = match (mt_slot0_value(fd, ABS_MT_POSITION_X), mt_slot0_value(fd, ABS_MT_POSITION_Y)) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => (rc == 0 && rc_y == 0).then_some((abs.value, abs_y.value)),
            };
            let mut single_axis = false;
            if x_max <= x_min || y_max <= y_min {
                seed = None;
                // Fall back to single-touch ABS_X/ABS_Y (no ABS_MT_*).
                let mut ax = InputAbsInfo::default();
                if unsafe { libc::ioctl(fd, eviocgabs(ABS_X) as _, &mut ax as *mut _) } == 0
                    && ax.maximum > ax.minimum
                {
                    x_min = ax.minimum;
                    x_max = ax.maximum;
                    single_axis = true;
                }
                let mut ay = InputAbsInfo::default();
                if unsafe { libc::ioctl(fd, eviocgabs(ABS_Y) as _, &mut ay as *mut _) } == 0
                    && ay.maximum > ay.minimum
                {
                    y_min = ay.minimum;
                    y_max = ay.maximum;
                    single_axis = true;
                }
                if single_axis {
                    seed = Some((ax.value, ay.value));
                }
            }
            // 读不到范围时按屏幕像素 (交换轴时原始 X 对应屏幕高度)
            let (raw_x_px, raw_y_px) = if info.touch_swap_axes { (info.height, info.width) } else { (info.width, info.height) };
            if x_max <= x_min {
                x_min = 0;
                x_max = raw_x_px as i32;
            }
            if y_max <= y_min {
                y_min = 0;
                y_max = raw_y_px as i32;
            }

            let mut recognizer = GestureRecognizer::new(config);
            if let Some(raw) = seed {
                let (sx, sy) = to_screen(info, raw, (x_min, x_max), (y_min, y_max));
                recognizer.seed_position(0, sx, sy);
            }

            Device {
                fd,
                kind: Kind::Touch {
                    recognizer,
                    x_min,
                    x_max,
                    y_min,
                    y_max,
                    single_axis,
                },
                grabbed: false,
            }
        }

        /// EVIOCGBIT fallback: manually scan `/dev/input/event*` when
        /// `fbink_input_scan` found nothing (e.g. MINIMAL build quirk or a
        /// non-Kindle Linux host).
        fn fallback_scan(info: &DeviceInfo, config: GestureConfig, devices: &mut Vec<Device>) {
            let entries = match std::fs::read_dir("/dev/input") {
                Ok(e) => e,
                Err(_) => return,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) if n.starts_with("event") => n.to_string(),
                    _ => continue,
                };
                let _ = name;
                let c_path = match std::ffi::CString::new(path.to_string_lossy().as_bytes()) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
                if fd < 0 {
                    continue;
                }

                let mut key_bits = [0u8; 96]; // covers KEY_* up to ~768 bits
                let has_key = unsafe {
                    libc::ioctl(
                        fd,
                        eviocgbit(EV_KEY, key_bits.len()) as _,
                        key_bits.as_mut_ptr(),
                    )
                } >= 0;
                let bit_set = |bits: &[u8], code: u16| -> bool {
                    let byte = code as usize / 8;
                    let bit = code as usize % 8;
                    byte < bits.len() && (bits[byte] >> bit) & 1 != 0
                };

                let mut abs_bits = [0u8; 16];
                let has_abs = unsafe {
                    libc::ioctl(
                        fd,
                        eviocgbit(EV_ABS, abs_bits.len()) as _,
                        abs_bits.as_mut_ptr(),
                    )
                } >= 0;

                if has_abs
                    && (bit_set(&abs_bits, ABS_MT_POSITION_X) || bit_set(&abs_bits, ABS_X))
                {
                    devices.push(Self::build_touch_device(fd, info, config));
                } else if has_key
                    && (bit_set(&key_bits, KEY_POWER)
                        || bit_set(&key_bits, KEY_PAGEUP)
                        || bit_set(&key_bits, KEY_PAGEDOWN)
                        || bit_set(&key_bits, KEY_HOME))
                {
                    devices.push(Device {
                        fd,
                        kind: Kind::Key,
                        grabbed: false,
                    });
                } else {
                    unsafe {
                        libc::close(fd);
                    }
                }
            }
        }

        pub fn fds(&self) -> Vec<RawFd> {
            self.devices.iter().map(|d| d.fd).collect()
        }

        pub fn read(&mut self, out: &mut Vec<InputEvent>) -> std::io::Result<()> {
            let start = out.len();
            for dev in self.devices.iter_mut() {
                loop {
                    let mut ev = RawInputEvent::default();
                    let n = unsafe {
                        libc::read(
                            dev.fd,
                            &mut ev as *mut _ as *mut libc::c_void,
                            std::mem::size_of::<RawInputEvent>(),
                        )
                    };
                    if n != std::mem::size_of::<RawInputEvent>() as isize {
                        if n < 0 {
                            let err = std::io::Error::last_os_error();
                            if err.raw_os_error() == Some(libc::EAGAIN) {
                                break;
                            }
                            // Device gone or other error: stop reading this fd for this pass.
                        }
                        break;
                    }

                    match &mut dev.kind {
                        Kind::Touch {
                            recognizer,
                            x_min,
                            x_max,
                            y_min,
                            y_max,
                            single_axis: _,
                        } => {
                            let mut gestures = Vec::new();
                            // Coordinate scaling + swap/mirror transform happens
                            // inside feed_touch_event using the device-wide
                            // DeviceInfo (self.info), kept as a free function so
                            // this match arm doesn't need to borrow `self` while
                            // `self.devices` is already borrowed mutably.
                            feed_touch_event(
                                &self.info,
                                recognizer,
                                *x_min,
                                *x_max,
                                *y_min,
                                *y_max,
                                &ev,
                                &mut gestures,
                            );
                            for g in gestures {
                                out.push(InputEvent::Gesture(g));
                            }
                        }
                        Kind::Key => {
                            if ev.type_ == EV_KEY {
                                let code = match map_key_code(ev.code, self.key_reversed) {
                                    KeyCode::PageForward if self.upside_down => KeyCode::PageBack,
                                    KeyCode::PageBack if self.upside_down => KeyCode::PageForward,
                                    c => c,
                                };
                                out.push(InputEvent::Key {
                                    code,
                                    pressed: ev.value != 0,
                                });
                            }
                        }
                        Kind::Rotation => {
                            if ev.type_ == EV_ABS && ev.code == ABS_PRESSURE {
                                if let Some(upside_down) = orientation_from_gyro(ev.value) {
                                    out.push(InputEvent::Rotation { upside_down });
                                }
                            }
                        }
                    }
                }
            }
            if self.voyage {
                // 起点落在压感键冷区的手势整个丢掉 (KOReader 把它们改写成 "none")
                let mut i = start;
                while i < out.len() {
                    if matches!(&out[i], InputEvent::Gesture(g) if in_voyage_cold_spot(g.start.0, g.start.1)) {
                        out.remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
            Ok(())
        }

        pub fn grab(&mut self) -> std::io::Result<()> {
            for dev in self.devices.iter_mut() {
                if matches!(dev.kind, Kind::Touch { .. }) {
                    let rc = unsafe { libc::ioctl(dev.fd, eviocgrab() as _, 1i32) };
                    if rc == 0 {
                        dev.grabbed = true;
                    }
                }
            }
            Ok(())
        }

        pub fn ungrab(&mut self) -> std::io::Result<()> {
            for dev in self.devices.iter_mut() {
                if dev.grabbed {
                    unsafe {
                        libc::ioctl(dev.fd, eviocgrab() as _, 0i32);
                    }
                    dev.grabbed = false;
                }
            }
            Ok(())
        }

        pub fn reset(&mut self) {
            for dev in self.devices.iter_mut() {
                if let Kind::Touch { recognizer, .. } = &mut dev.kind {
                    recognizer.reset();
                }
            }
        }

        pub fn touch_active(&self) -> bool {
            self.devices.iter().any(|d| match &d.kind {
                Kind::Touch { recognizer, .. } => recognizer.touch_active(),
                _ => false,
            })
        }
    }

    impl Drop for InputReader {
        fn drop(&mut self) {
            for dev in &self.devices {
                unsafe {
                    libc::close(dev.fd);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn feed_touch_event(
        info: &DeviceInfo,
        recognizer: &mut GestureRecognizer,
        x_min: i32,
        x_max: i32,
        y_min: i32,
        y_max: i32,
        ev: &RawInputEvent,
        out: &mut Vec<super::Gesture>,
    ) {
        // 每个事件都带内核时间戳: 先用它更新识别器的时钟。只在 SYN 时更新会让按下 (tracking id/坐标,
        // 位于 SYN 之前) 用上"上一次 SYN"的时间 —— 可能是几分钟前, 导致所有点击都被判为长按。
        // 用事件自身时间而非读取时间, 主循环忙 (渲染中) 时积压的事件也能得到正确时长。
        let ts = ev.tv_sec as u32 as u64 * 1000 + ev.tv_usec as u32 as u64 / 1000;
        let mut push = |s: TouchSample| recognizer.feed(s, out);
        push(TouchSample::Sync(ts));
        match ev.type_ {
            // SYN_REPORT (code 0); SYN_DROPPED 等忽略
            t if t == EV_SYN => {
                if ev.code == 0 {
                    push(TouchSample::Report);
                }
            }
            t if t == EV_ABS => match ev.code {
                c if c == ABS_MT_SLOT => push(TouchSample::Slot(ev.value)),
                c if c == ABS_MT_TRACKING_ID => push(TouchSample::TrackingId(ev.value)),
                c if c == ABS_MT_POSITION_X || c == ABS_X => push(axis_sample(info, true, ev.value, x_min, x_max)),
                c if c == ABS_MT_POSITION_Y || c == ABS_Y => push(axis_sample(info, false, ev.value, y_min, y_max)),
                _ => {}
            },
            t if t == EV_KEY => {
                if ev.code == BTN_TOUCH {
                    push(TouchSample::BtnTouch(ev.value != 0));
                }
            }
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux_impl::InputReader;

#[cfg(not(target_os = "linux"))]
pub struct InputReader;

#[cfg(not(target_os = "linux"))]
impl InputReader {
    /// 发现并打开输入设备。`info` 提供屏幕尺寸与触摸变换, `model_keys` 决定翻页键映射方向。
    pub fn open(_info: &DeviceInfo, _config: GestureConfig) -> std::io::Result<Self> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    /// 需要 poll 的 fd 列表。
    #[cfg(unix)]
    pub fn fds(&self) -> Vec<std::os::fd::RawFd> {
        Vec::new()
    }

    /// 读取所有就绪事件 (不阻塞), 识别出的事件追加到 `out`。
    pub fn read(&mut self, _out: &mut Vec<InputEvent>) -> std::io::Result<()> {
        Ok(())
    }

    pub fn grab(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    pub fn set_upside_down(&mut self, _upside_down: bool) {}

    pub fn ungrab(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    /// 丢弃进行中的触摸状态。
    pub fn reset(&mut self) {}

    pub fn touch_active(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(swap: bool, mx: bool, my: bool) -> DeviceInfo {
        DeviceInfo { width: 600, height: 800, touch_swap_axes: swap, touch_mirror_x: mx, touch_mirror_y: my, ..Default::default() }
    }

    #[test]
    fn touch_axes_scale_swap_and_mirror() {
        // 原始范围 0..4095, 屏幕 600x800
        let r = (0, 4095);
        assert_eq!(to_screen(&info(false, false, false), (2048, 1024), r, r), (300, 200));
        // 交换轴: 原始 X 是屏幕 Y (按高度缩放), 原始 Y 是屏幕 X
        assert_eq!(to_screen(&info(true, false, false), (2048, 1024), r, r), (150, 400));
        // 交换后镜像屏幕 X
        assert_eq!(to_screen(&info(true, true, false), (2048, 1024), r, r), (449, 400));
        assert_eq!(to_screen(&info(false, false, true), (0, 4095), r, r), (0, 0));
        assert!(matches!(axis_sample(&info(true, false, false), true, 4095, 0, 4095), TouchSample::Y(799)));
    }

    #[test]
    fn gyro_and_voyage_cold_spots() {
        assert_eq!(orientation_from_gyro(19), Some(false));
        assert_eq!(orientation_from_gyro(20), Some(true));
        assert_eq!(orientation_from_gyro(21), None);
        assert!(in_voyage_cold_spot(1071, 485) && in_voyage_cold_spot(0, 910));
        assert!(!in_voyage_cold_spot(536, 724) && !in_voyage_cold_spot(1071, 1300));
    }
}
