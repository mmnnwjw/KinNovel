//! 前光 (亮度) 与色温, 照搬 KOReader `frontend/device/kindle/powerd.lua`。
//!
//! 走系统 powerd 的 LIPC 属性 (与系统快捷设置同一来源, 改动在退出后保留):
//! - 亮度 `flIntensity` 0..=24, 色温 `currentAmberLevel` 0..=24 (KOReader 的 fl_max / fl_warmth_max)。
//! - 哪些机型有前光 / 暖光由 KOReader 的机型表决定 (`ModelCaps`); 不在表里的新机型才运行时探测。
//! - PW1/PW2/Voyage/PW3 (`!canTurnFrontlightOff`): lipc 的 0 档并不关灯。KOReader 在前面加一个
//!   "真关灯" 档: 界面 0 = lipc 0 + 直接往 sysfs 写 0; 界面 n = lipc n-1 (所以这些机型是 0..=25)。
//!   读取时 lipc 为 0 再看 sysfs, 非 0 说明灯其实亮着, 算作 1。
//!
//! `lipc-get-prop` / `lipc-set-prop` 是子进程 (设备上 ~20 ms), 都放在后台线程:
//! 启动时读一次当前值, 打开阅读菜单时 `refresh()` 重读 (有环境光传感器的机型会自动调亮度);
//! 设置时只发给写入线程, 连续点击只写最后的值。主机构建 (预览) 用内存里的假值, 便于看界面。

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use crate::model::ModelCaps;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightProp {
    Brightness,
    Warmth,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LightLevel {
    pub value: i32,
    pub max: i32,
}

#[derive(Default)]
struct State {
    brightness: Option<LightLevel>,
    warmth: Option<LightLevel>,
}

/// 读写方式 (由机型决定, 读写线程共用)。
#[derive(Clone, Copy, Debug)]
struct Mode {
    /// None = 未知机型, 运行时探测
    caps: Option<ModelCaps>,
}

impl Mode {
    /// 加了 "真关灯" 档的机型 (界面值 = lipc 值 + 1)
    fn synthetic_off(&self) -> Option<&'static str> {
        self.caps.filter(|c| c.frontlight && !c.fl_off_at_zero).and_then(|c| c.fl_sysfs)
    }

    fn read(&self) -> State {
        let Some(caps) = self.caps else {
            // 未知机型: 有 flMaxIntensity 才有前光, 读得到 currentAmberLevel 才有暖光
            let max = lipc_get("flMaxIntensity").filter(|m| *m > 0);
            return State {
                brightness: max.and_then(|max| Some(LightLevel { value: lipc_get("flIntensity")?.clamp(0, max), max })),
                warmth: lipc_get("currentAmberLevel").map(|v| LightLevel { value: v.clamp(0, MAX_LEVEL), max: MAX_LEVEL }),
            };
        };
        let brightness = caps.frontlight.then(|| {
            // KOReader: 读失败按 fl_min (0) 处理
            let v = lipc_get("flIntensity").unwrap_or(0).clamp(0, MAX_LEVEL);
            match self.synthetic_off() {
                Some(sysfs) => {
                    let value = if v == 0 { i32::from(sysfs_get(sysfs).unwrap_or(0) > 0) } else { v + 1 };
                    LightLevel { value, max: MAX_LEVEL + 1 }
                }
                None => LightLevel { value: v, max: MAX_LEVEL },
            }
        });
        let warmth = caps
            .natural_light
            .then(|| LightLevel { value: lipc_get("currentAmberLevel").unwrap_or(0).clamp(0, MAX_LEVEL), max: MAX_LEVEL });
        State { brightness, warmth }
    }

    fn write(&self, prop: LightProp, value: i32) {
        match (prop, self.synthetic_off()) {
            (LightProp::Brightness, Some(sysfs)) => {
                lipc_set(prop_name(prop), (value - 1).max(0));
                if value == 0 {
                    // lipc 0 在这些机型上不关灯, 直接写 sysfs (KOReader setIntensityHW)
                    let _ = std::fs::write(sysfs, "0");
                }
            }
            _ => lipc_set(prop_name(prop), value),
        }
    }
}

/// 廉价克隆的句柄; 状态未读到 (或机型不支持) 时对应项为 None。
#[derive(Clone)]
pub struct Frontlight {
    state: Arc<Mutex<State>>,
    writer: Option<Sender<(LightProp, i32)>>,
    mode: Option<Mode>,
}

const MAX_LEVEL: i32 = 24;

impl Frontlight {
    /// 设备上: 后台读取当前值并启动写入线程 (`caps` 为 None 时运行时探测)。主机上: 内存假值。
    pub fn detect(caps: Option<ModelCaps>) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        if cfg!(target_os = "linux") {
            let mode = Mode { caps };
            let reader = state.clone();
            let _ = std::thread::Builder::new().name("light-read".into()).spawn(move || {
                let fresh = mode.read();
                *reader.lock().unwrap_or_else(|e| e.into_inner()) = fresh;
            });
            let (tx, rx) = channel::<(LightProp, i32)>();
            let _ = std::thread::Builder::new().name("light-write".into()).spawn(move || {
                while let Ok(first) = rx.recv() {
                    // 合并排队中的请求, 每个属性只写最后一次
                    let mut latest = [None, None];
                    let mut put = |(prop, v): (LightProp, i32)| latest[prop as usize] = Some(v);
                    put(first);
                    while let Ok(next) = rx.try_recv() {
                        put(next);
                    }
                    for (prop, v) in [LightProp::Brightness, LightProp::Warmth].into_iter().zip(latest) {
                        if let Some(v) = v {
                            mode.write(prop, v);
                        }
                    }
                }
            });
            Frontlight { state, writer: Some(tx), mode: Some(mode) }
        } else {
            {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.brightness = Some(LightLevel { value: 12, max: MAX_LEVEL });
                s.warmth = Some(LightLevel { value: 6, max: MAX_LEVEL });
            }
            Frontlight { state, writer: None, mode: None }
        }
    }

    /// 不支持前光的句柄 (测试用)。
    pub fn none() -> Self {
        Frontlight { state: Arc::new(Mutex::new(State::default())), writer: None, mode: None }
    }

    /// 同步重读当前值 (设备上每个属性 ~20 ms)。系统可能改过亮度 (自动亮度、休眠唤醒)。
    pub fn refresh(&self) {
        let Some(mode) = self.mode else { return };
        let fresh = mode.read();
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // 读失败 (None) 时保留旧值, 不让面板闪没
        s.brightness = fresh.brightness.or(s.brightness);
        s.warmth = fresh.warmth.or(s.warmth);
    }

    pub fn get(&self, prop: LightProp) -> Option<LightLevel> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match prop {
            LightProp::Brightness => s.brightness,
            LightProp::Warmth => s.warmth,
        }
    }

    /// 设置 (钳到 0..=max), 返回新值; 不支持时 None。
    pub fn set(&self, prop: LightProp, value: i32) -> Option<i32> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let level = match prop {
            LightProp::Brightness => s.brightness.as_mut(),
            LightProp::Warmth => s.warmth.as_mut(),
        }?;
        let value = value.clamp(0, level.max);
        if level.value != value {
            level.value = value;
            if let Some(tx) = &self.writer {
                let _ = tx.send((prop, value));
            }
        }
        Some(value)
    }
}

fn prop_name(prop: LightProp) -> &'static str {
    match prop {
        LightProp::Brightness => "flIntensity",
        LightProp::Warmth => "currentAmberLevel",
    }
}

fn sysfs_get(path: &str) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn lipc_get(name: &str) -> Option<i32> {
    let out = std::process::Command::new("lipc-get-prop")
        .args(["com.lab126.powerd", name])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn lipc_set(name: &str, value: i32) {
    let _ = std::process::Command::new("lipc-set-prop")
        .args(["-i", "-q", "--", "com.lab126.powerd", name, &value.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn host_fake_clamps_and_none_is_unsupported() {
        let fl = Frontlight::detect(None);
        assert_eq!(fl.set(LightProp::Brightness, 99), Some(24));
        assert_eq!(fl.get(LightProp::Brightness).unwrap().value, 24);
        assert_eq!(fl.set(LightProp::Warmth, -3), Some(0));
        fl.refresh();
        assert_eq!(fl.get(LightProp::Warmth).unwrap().value, 0);
        let none = Frontlight::none();
        assert_eq!(none.get(LightProp::Warmth), None);
        assert_eq!(none.set(LightProp::Brightness, 3), None);
    }

    #[test]
    fn synthetic_off_step_only_on_old_frontlights() {
        let mode = |n| Mode { caps: ModelCaps::for_kindle(n) };
        assert!(mode("Voyage").synthetic_off().is_some());
        assert!(mode("PaperWhite").synthetic_off().is_some());
        assert!(mode("PaperWhite 5").synthetic_off().is_none());
        assert!(mode("Basic 2").synthetic_off().is_none());
        assert!(mode("Kindle 99").synthetic_off().is_none());
    }
}
