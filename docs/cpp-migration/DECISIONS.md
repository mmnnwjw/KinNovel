# KinNovel C++ 重构技术决策记录 (DECISIONS.md)

本文档记录 KinNovel 从 Python (3.14 + Pillow + lxml + evdev) 重构为 C++17 独立二进制过程中的关键技术选型与架构决策。

---

## 决策 1：硬件抽象层 (HAL) 与双后端架构

- **背景**：Kindle 真机直接通过 `/dev/fb0` ioctl (FBInk/EPDC) 和 `/dev/input/event*` 驱动屏幕与触摸，而开发机 (x86_64 Linux / Termux) 没有电子墨水屏和电容触摸硬件。
- **决策**：
  - 定义纯虚接口 `IDisplay`、`IInput`、`IPower`，上层应用与 UI 完全面向接口编程。
  - Kindle 后端：`FbinkDisplay`（封装 FBInk 电子墨水屏控制器 ioctl）、`EvdevInput`（直接解析 Linux evdev 事件流并抽象为 `TouchGesture` 单击、长按、滑动）、`LipcPowerManager`（LIPC 电源事件与休眠看门狗）。
  - PC / 模拟后端：`MockDisplay`（在内存中维护 8bpp 灰度帧缓冲，支持导出 PNG 截图）、`MockInput`（支持注入手势事件）、`MockPowerManager`（模拟休眠/唤醒）。
- **结果**：所有单元测试、Golden 测试和 UI 截图测试均可在无 Kindle 设备的环境下 100% 自动化运行；真机代码保持整洁隔离。

---

## 决策 2：排版与分词引擎 (LayoutEngine)

- **背景**：原版 Python 使用 Pillow + 自定义标点禁则进行分行与分页，需保证重构后分页位置与字符排列和 Python 黄金数据完全一致。
- **决策**：
  - 实现基于 FreeType2 的 `FontEngine`，通过 glyph advance 矩阵进行高精度排版测距。
  - 严格实现标点禁则集合：
    - 行首禁则（`LINE_START_FORBIDDEN`）：逗号、句号、问号、感叹号、右括号等禁止出现在行首；遇违规时将前一字符连带移至下一行或触发避头折行。
    - 行尾禁则（`LINE_END_FORBIDDEN`）：左括号、左引号等禁止出现在行尾。
  - 支持首行缩进（全角空格/两个字符宽度）、两端对齐计算与空白压缩。
- **结果**：`test_golden` 黄金测试 100% 逐行匹配 Python 版基准数据。

---

## 决策 3：轻量级 UI 框架（自研 Canvas + PageContext）

- **背景**：若引入外部 GUI 框架（如 LVGL、Qt、nanovg），会导致二进制体积暴增、引入复杂消息调度，且难以适配 Kindle 电子墨水屏的局部刷新与波形模式（Waveform GC16/DU/Auto）。
- **决策**：
  - 继承原版直观的页面模型，设计轻量级 `Canvas`（2D 灰度图元绘制、抗锯齿字形栅格化、按钮、标题栏、弹窗）。
  - 实现 `PageContext` 导航堆栈与页面上下文管理（`navigate`, `replace`, `back`, `home`，多层 Modal 对话框，自动淡出 Toast，后台异步工作线程 `runAsync`）。
  - 页面全部继承自接口 `IPage`，生命周期清晰（`enter`, `render`, `handle`, `exit`）。
- **结果**：全套 17 个页面与 UI 控件库代码量小、无外部 GUI 依赖，支持白天/夜间模式即时切换与屏幕分辨率自适应等比缩放。

---

## 决策 4：网络层与会话保持

- **背景**：轻小说站 API 使用 REST 接口获取书籍与章节，并采用 SignalR 实时长连接推送通知。
- **决策**：
  - HTTP 传输层基于 `libcurl`（支持 TLS 1.3 / OpenSSL / mbedTLS，可配置 CA 证书路径与 `strict_tls` 校验）。
  - JSON 序列化/反序列化统一采用极高性能的 `yyjson`（相比 nlohmann/json 节省大量内存和 CPU，且无异常开销）。
  - 严格遵循站点限流要求，内置滑动窗口限流器（默认 9 次请求 / 5500ms），防止高频突发请求导致封号。
  - 会话管理存储于 `SessionStore`，自动刷新 Access Token，支持离线章节与封面磁盘 LRU 缓存。
- **结果**：网络层端到端稳定性高，支持离线回退阅读已缓存章节。

---

## 决策 5：系统级单二进制集成与 KUAL 打包

- **背景**：Python 版由多个 Python 脚本、20+ 共享库 `.so` 和 Python 3.14 解释器组成，安装包超过 25MB，冷启动耗时超过 5 秒。
- **决策**：
  - C++ 版编译为单一静态链接可执行文件 `kinnovel`，无 Python 运行时依赖。
  - 内置基于 `flock` 的单实例进程锁 (`/tmp/kinnovel.lock`)。
  - 内置 Kindle 帧缓冲快照（保存原 Kindle 屏幕到 `/tmp/kinnovel_fb.bin`，退出时写回并刷新恢复），支持 CLI 命令 `kinnovel snapshot save/restore`。
  - 编写 `tools/package_kual.sh` 脚本，输出兼容标准 KUAL 扩展规范的 Release ZIP，压缩包体积由 25MB 大幅降至约 4.5MB，冷启动由 5 秒降至毫秒级。
- **结果**：启动极其迅速，内存占用大幅降低，完全摆脱对 Kindle 上 Python 环境的依赖。
