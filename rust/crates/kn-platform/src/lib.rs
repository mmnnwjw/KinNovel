//! kn-platform: Kindle 设备层。
//!
//! - `display`: FBInk 显示 (仅 Linux 目标编译 FBInk), 主机上用 `MemoryDisplay` 代替以便测试。
//! - `input`: evdev 读取 + 手势识别 (移植自 Python `screen/input/parser.py`)。
//! - `light`: 前光亮度/色温 (powerd LIPC 属性)。
//! - `power`: LIPC 电源事件 (`lipc-wait-event` 子进程) 与框架进程暂停/恢复。
//!
//! 所有会阻塞的东西都暴露 fd, 由 kn-ui 的主循环统一 `poll()`。

// 设备专用代码 (evdev 常量、MTK 换页方向等) 在主机构建里用不到
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

pub mod display;
pub mod gesture;
pub mod input;
pub mod light;
pub mod power;

#[cfg(target_os = "linux")]
mod ffi;

pub use display::{DeviceInfo, Display, MemoryDisplay, RefreshRequest, SwipeDir, Waveform};
pub use gesture::{Gesture, GestureConfig, GestureKind, GestureRecognizer, TouchSample};
pub use input::{InputEvent, InputReader, KeyCode};
pub use light::{Frontlight, LightLevel, LightProp};
pub use power::{PowerEvent, PowerMonitor};

#[cfg(target_os = "linux")]
pub use display::FbinkDisplay;
