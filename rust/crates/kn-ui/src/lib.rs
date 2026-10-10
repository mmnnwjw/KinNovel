//! kn-ui: 墨水屏 UI 运行时。
//!
//! 模型 (沿用 Python 0.8.0 已在 KPW5 上验证的做法, 换成原生实现):
//! - **立即模式渲染**: 页面每次 `render` 把整帧画进后备缓冲, 同时登记命中区域 (`Hits`)。
//! - **差分刷新**: `RefreshScheduler` 把新帧与屏幕上的帧做 `diff_bbox`, 只提交变化矩形;
//!   画面无变化则不刷新。页面通过 `RefreshHint` 告诉调度器这次变化的性质, 调度器决定波形与是否闪刷。
//! - **残影预算**: 局部刷新按面积累计 (翻页在支持 REAGL 时按 1/4 计入), 达到 `full_refresh_every` 屏后,
//!   下一次大面积 (>50% 屏) 刷新升级为闪刷。
//! - **按下反馈**: 手指按下 (`GestureKind::Down`) 落在带 `feedback` 的命中区域时, 立即把该矩形反相并用 A2 刷新,
//!   抬起后执行动作并正常重绘 —— 墨水屏上"点了有反应"。
//! - **单线程状态**: 所有页面状态只在 UI 线程修改, 且就放在页面结构体里。耗时工作交给 `Tasks` 线程池,
//!   结果按页面实例路由回 `Page::on_message` (页面已关闭则丢弃)。
//! - **主循环**: `poll()` 等待 {输入 fd, 电源 fd, 唤醒 fd}; 超时 = min(下一分钟时钟, 电源看门狗, 有空闲任务时 0)。
//!   没有事件时调用当前页面的 `on_idle` (如预渲染下一页), 有手指按着时不调用。

mod app;
mod hits;
mod refresh;
mod tasks;
mod theme;
pub mod widgets;

pub use app::{render_once, run, App, Cx, Delivery, Headless, Page, PageId, PagePoster, Transition};
pub use hits::{HitId, Hits};
pub use refresh::{RefreshHint, RefreshPolicy, RefreshScheduler};
pub use tasks::{Poster, Tasks, Waker};
pub use theme::Theme;
