//! 前光 (亮度) 与色温。
//!
//! 走系统 powerd 的 LIPC 属性 (与系统快捷设置同一来源, 改动在退出后保留):
//! - 亮度 `flIntensity` 0..=`flMaxIntensity` (KPW5: 24)
//! - 色温 `currentAmberLevel` 0..=24 (没有暖光的机型读取失败 → 不支持; 上限与 KOReader 一致)
//!
//! `lipc-get-prop` / `lipc-set-prop` 是子进程 (设备上 ~20 ms), 都放在后台线程:
//! 启动时读一次当前值; 设置时只发给写入线程, 连续点击只写最后的值。
//! 主机构建 (预览) 用内存里的假值, 便于看界面。

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

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

/// 廉价克隆的句柄; 状态未读到 (或机型不支持) 时对应项为 None。
#[derive(Clone)]
pub struct Frontlight {
    state: Arc<Mutex<State>>,
    writer: Option<Sender<(LightProp, i32)>>,
}

const MAX_WARMTH: i32 = 24;

impl Frontlight {
    /// 设备上: 后台读取当前值并启动写入线程。主机上: 内存假值。
    pub fn detect() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        if cfg!(target_os = "linux") {
            let reader = state.clone();
            let _ = std::thread::Builder::new().name("light-read".into()).spawn(move || {
                let max = lipc_get("flMaxIntensity").filter(|m| *m > 0);
                let brightness = max.and_then(|max| Some(LightLevel { value: lipc_get("flIntensity")?.clamp(0, max), max }));
                let warmth = lipc_get("currentAmberLevel").map(|v| LightLevel { value: v.clamp(0, MAX_WARMTH), max: MAX_WARMTH });
                let mut s = reader.lock().unwrap_or_else(|e| e.into_inner());
                s.brightness = brightness;
                s.warmth = warmth;
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
                            lipc_set(prop_name(prop), v);
                        }
                    }
                }
            });
            Frontlight { state, writer: Some(tx) }
        } else {
            {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.brightness = Some(LightLevel { value: 12, max: 24 });
                s.warmth = Some(LightLevel { value: 6, max: MAX_WARMTH });
            }
            Frontlight { state, writer: None }
        }
    }

    /// 不支持前光的句柄 (测试用)。
    pub fn none() -> Self {
        Frontlight { state: Arc::new(Mutex::new(State::default())), writer: None }
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
        let fl = Frontlight::detect();
        assert_eq!(fl.set(LightProp::Brightness, 99), Some(24));
        assert_eq!(fl.get(LightProp::Brightness).unwrap().value, 24);
        assert_eq!(fl.set(LightProp::Warmth, -3), Some(0));
        let none = Frontlight::none();
        assert_eq!(none.get(LightProp::Warmth), None);
        assert_eq!(none.set(LightProp::Brightness, 3), None);
    }
}
