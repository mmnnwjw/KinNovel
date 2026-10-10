# Kindle 设备差异化处理研究报告（KOReader + FBInk）

来源：`koreader/koreader`（AGPL-3.0）`frontend/device/kindle/device.lua`、`powerd.lua`、`platform/kindle/koreader.sh`（master 分支，2026-10 抓取）；`NiLuJe/FBInk`（GPL-3.0）`fbink.h`、`README.md`。标注"不确定"处均为代码未直接验证、需上机测试的内容。

## 1. 触屏机型表

| 机型 (device.lua model) | 平台/EPDC 家族 | 分辨率DPI | 触摸轴异常 | 翻页键 evdev code | 陀螺仪/旋转 | 暖光(warmth) |
|---|---|---|---|---|---|---|
| KindleTouch / PaperWhite / PW2 | mxcfb (i.MX5/6, Wario) | 212 DPI (PW1) | 无特殊标记 | 无按键(纯触摸) | 无 | 无 |
| KindleBasic(KT2)/KV(Voyage) | mxcfb, Wario+ | 300 DPI | 无 | Voyage: `[104]="LPgBack",[109]="LPgFwd"` (WhisperTouch 压力键, device.lua:1368-1369) | 无 | 无 |
| KindlePaperWhite3 | mxcfb | 300 DPI | 无 | 无 | 无 | 无 |
| KindleOasis (KOA) | mxcfb | 300 DPI | 无 | `[104]="RPgFwd",[109]="RPgBack"`（与非 Oasis 机型相反，device.lua:1488-1491） | 有，`OasisGyroTranslation`：EV_ABS/ABS_PRESSURE 值 15-22 映射到 `MSC_GYRO`（device.lua:1426-1465） | 无 |
| KindleOasis2 (KOA2) | mxcfb | 300 DPI | 无 | 同上 (1592-1595) | `KindleGyroTransform`：值 15-18，对应 `drivers/input/misc/accel/bma2x2.c`（1537-1571） | 无 |
| KindleOasis3 (KOA3) | mxcfb | 300 DPI | 无 | 同上 (1669-1672) | 同 KOA2 gyro | 有：`fl_intensity_files`(lm3697-bl1) + `warmth_intensity_files`(lm3697-bl0)（1658-1659） |
| KindleBasic2/3(KT3/KT4)、PW4 | mxcfb (Rex SoC: bd71827/bd7181x PMIC) | 300 DPI | KindleBasic3 用 "snow protocol"：`self.input.snow_protocol = true`，因设备不发送 `ABS_MT_TRACKING_ID:-1`（1760） | 无 | 无 | 无 |
| KindlePaperWhite5/5SE/6, Basic4/5 | **MTK (Bellatrix)**, `isMTK=yes` | 300 DPI | 无特殊标记 | 无(纯触摸) | 无（5SE 有光线感应 `hasLightSensor`） | PW5/5SE/6, Basic4/5 均 `hasNaturalLight`+`hasNaturalLightMixer` |
| KindleScribe/Scribe3/ColorSoft | **MTK**, `isMTK=yes`, Wacom 数位笔 | 300 DPI | `self.input.wacom_protocol = true`（1896, 1959, 2022） | 无 | `hasGSensor=yes`，用 `KindleGyroTransform`（同 KOA2） | `hasNaturalLight`+Mixer+`hasLightSensor` |
| K11 (KindleBasic5) | MTK | 300 DPI | 无 | 无 | 无 | 无 |

机型探测：解析 `/proc/usid`（device.lua:2116起）。v1 格式（首字符 'B'/'9'，取第3-4位）查表 `k2_set/pw_set/kv_set` 等；v2 格式取第4-6位查 `pw5_set/ks_set/kcs_set` 等（device.lua 约2170-2220附近，未逐一列出所有三字符代码，建议直接复制该查表逻辑而非重新推导——**不确定**完整代码表是否已收录所有新机型，需要以最新 master 为准）。

关键字段含义：
- `isTouchDevice`/`hasFrontlight`/`hasGSensor`/`hasNaturalLight(Mixer)`/`hasLightSensor`：决定 UI 功能开关。
- `canHWInvert = yes`（Kindle 基类默认，device.lua:396）：mxcfb 机型普遍支持硬件反色（夜间模式），legacy einkfb 机型设为 `no`（922/928/937/943/952/958/966/971附近，多个 legacy 型号都显式关闭）。
- `isREAGL`：新机型默认 `yes`（396附近 Kindle 基类 405），legacy 机型显式 `no`。
- `isMTK`：MTK 机型（PW5+/Basic4+/Scribe系列）特有，解锁 `canDoSwipeAnimation`。
- 未发现 `touch_mirrored_x`/`touch_switch_xy` 这类字段存在于 koreader `device.lua` 中——**轴交换/镜像实际是 FBInk 层处理的**（见下节 `touch_swap_axes`/`touch_mirror_x`/`touch_mirror_y`，fbink.h:615-617），KOReader 自身未重复实现，这点对我们很重要：触摸坐标矫正应交给 FBInk/底层驱动,而不是在应用层按机型硬编码。

## 2. 输入设备发现方式（建议采用）

KOReader **不是**手工枚举 `/dev/input/event*`，而是调用 FBInk 自带的 `libfbink_input`（`fbink_input_scan`，声明于 fbink.h:1770，实现随 FBInk 静态/动态库一起分发）：

```lua
-- device.lua:535-559 Kindle:openInputDevices()
local match_mask = bit.bor(C.INPUT_TOUCHSCREEN, C.INPUT_SCALED_TABLET,
    C.INPUT_PAGINATION_BUTTONS, C.INPUT_HOME_BUTTON, C.INPUT_DPAD, C.INPUT_KINDLE_FRAME_TAP)
local devices = FBInkInput.fbink_input_scan(match_mask, 0, 0, dev_count)
-- 对每个匹配设备: self.input:fdopen(fd, path, name)
```

陀螺仪/旋转事件设备单独扫描（device.lua:575-592），用 `C.INPUT_ROTATION_EVENT` 并排除平板/触屏类型，避免误判。

自动探测失败时回退到硬编码路径（device.lua:563-569）：优先 `self.touch_dev`（如 KOA 系列的 `/dev/input/by-path/platform-...-event`），否则 `/dev/input/event0`+`event1`。

**对我们 Rust 项目的建议**：
1. 既然已静态链接 FBInk，直接 FFI 绑定 `fbink_input_scan`/`fbink_input_check`（fbink.h:1770/1780），用能力位掩码（`INPUT_TOUCHSCREEN|INPUT_PAGINATION_BUTTONS|INPUT_DPAD|INPUT_HOME_BUTTON`）做设备发现，无需自行维护每机型路径表——**这是 FBInk 已经做过的脏活，值得直接复用**（GPL-3.0 对 GPL-3.0 项目可直接拿 FFI 绑定调用，无需重写逐机型表）。
2. 回退路径（硬编码 `/dev/input/eventN` 或 `by-path`）仅作为 `fbink_input_scan` 失败时的 fallback，不作为首选策略。
3. 旋转/陀螺仪事件需要单独扫描并记录 fd 集合，防止其它上报 `ABS_PRESSURE` 的设备（如数位笔）被误当作方向传感器（device.lua:1437-1438 的注释明确提到这个陷阱）。

## 3. 电源/挂起事件处理建议

来源：`lipc`（Lab126 IPC）属性监听 + `powerd.lua`。关键事件源码（device.lua:749-902）：

| lipc 事件/属性 | 触发时机 | KOReader 动作 |
|---|---|---|
| `goingToScreenSaver` (source 2=按键, 4=HALL磁感) | 进入睡眠前 | `Kindle:intoScreenSaver()`：显示自绘 Screensaver（非广告机型），调用 `powerd:beforeSuspend()` → `device:_beforeSuspend()`（屏蔽输入 + 广播 `Suspend` 事件）（756-779） |
| `outOfScreenSaver` (source 1=按键, 6=HALL) | 唤醒 | `Kindle:outofScreenSaver()`：关闭 Screensaver 部件，必要时整屏刷新；调用 `powerd:afterResume()` → `_afterResume()`（恢复输入 + 广播 `Resume` 事件），之后恢复背光（781-833） |
| `exitingScreenSaver` | stock UI 实际关闭截屏后 | 目前是空函数（836），区分"请求关闭"与"确实关闭" |
| `readyToSuspend`/`com.lab126.powerd rtcWakeup` | 真正挂起前，Kindle 只允许在此状态设置 RTC 唤醒 | `setRtcWakeup(seconds_from_now)`（powerd.lua:251-255, 295-307），用于定时任务唤醒 |
| 唤醒后 `checkUnexpectedWakeup` | resume 15 秒后检查 powerd 状态 | 区分"用户按键唤醒"还是"我们自己设的 RTC alarm 唤醒"（powerd.lua:264-276, 287-293） |
| 充电事件 `Charging`/`NotCharging` | USB 插拔 | `_beforeCharging`/`usbPlugIn`/`usbPlugOut`/`_afterNotCharging`（887-894） |

挂起触发：`powerd:toggleSuspend()` 通过 `lipc_handle:set_int_property("com.lab126.powerd","powerButton",1)`，无 lipc 时回退 `os.execute("powerd_test -p")`（powerd.lua:242-248）。

**Framework（awesome/cvm/X11 GUI）暂停与恢复**（`platform/kindle/koreader.sh`）：
- 启动时若 `--framework_stop`：Upstart 系统用 `stop lab126_gui`（先 `trap "" TERM` 避免被杀），SysV 系统用 `/etc/init.d/framework stop`（koreader.sh:219-233）。
- 不完全停止框架时（仅想要全屏但保留后台）：对 Upstart 系统尝试禁用 pillow 状态栏，FW≥5.7.2 时 `killall -STOP awesome`（SIGSTOP 冻结而非杀死，koreader.sh:265-280），退出时 `killall -CONT awesome` 恢复（koreader.sh:381-383）。
- SysV 系统用同样套路 SIGSTOP/CONT `cvm`（koreader.sh:307-310, 354-358）。
- device.lua 对应：Oasis 系列在访问 lipc 获取方向信息前，临时 `killall -CONT awesome` 唤醒框架完成 IPC 调用，随后 `killall -STOP awesome` 重新冻结（1467-1471, 1523-1526 等多处重复）——**提示 lipc 调用依赖 framework 进程存活，纯 SIGSTOP 冻结时某些 lipc 属性读取会卡死，需要临时唤醒**。

**对我们的建议**：
1. 用等价于 lipc 的机制监听电源事件——Kindle 没有公开文档化的 lipc C API 替代品，必须通过 `liblipclua`/`lipc-wait-event` 或直接 dlopen `liblipc.so` 监听 `com.lab126.powerd` 的 `goingToScreenSaver`/`outOfScreenSaver`/`readyToSuspend` 属性变化。**不确定**：是否有不依赖 lipc 的纯 sysfs/evdev 挂起事件源，需进一步验证（旧 K2/K3 用 `powerd_test`/`/proc` 接口，新机型几乎全靠 lipc）。
2. 挂起前调用等价 `beforeSuspend`：停止渲染、暂停输入扫描线程；恢复后 `afterResume`：重新打开输入设备 fd（Linux 挂起/恢复后 evdev fd 通常仍然有效，但要重新读取电量/背光状态）。
3. RTC 定时唤醒只能在 `readyToSuspend` 状态设置，意味着我们如果要支持"定时自动翻页/关闭"等功能，需要抓住这个窗口期写入 `rtcWakeup`。
4. 冻结 framework 时如需调用任何 lipc 属性（如旋转方向），要先 `SIGCONT` 唤醒再 `SIGSTOP` 冻结回去，否则调用会挂起。
5. **实测 (KPW5, FW 5.17.1, 2026-10-10)**：Xorg/awesome/blanket 处于 SIGSTOP 时，`powerd_test -p` 无法让设备进入屏保 —— 进入屏保的流程要向 `com.lab126.winmgr`（awesome）读 `isScreenSaverLayerWindowActive`/`ASRMode`，调用挂到 10 s 超时后放弃，`powerd` 一直停在 `active`。框架运行时同样的操作正常进入 `screenSaver`。向 `/dev/input/event0` 写入 KEY_POWER 不会触发 powerd（它不从 evdev 读电源键），只能用 `powerd_test -p` 或真实按键测试。KinNovel 1.0 的做法：应用收到电源键按下 (event0) 立即 SIGCONT 框架，让 powerd 的流程走完；15 s 内没有 `goingToScreenSaver` 就重新 SIGSTOP 并整屏重画（`kn-ui` run 循环）。Python 0.8.x 同样暂停这些进程，很可能也受影响（未用实体按键验证）。

## 4. FBInk API 子集与 waveform 映射建议

来源：`fbink.h`。关键结构 `FBInkState`（fbink.h:575-626）字段（通过 `fbink_get_state` 填充）：
- 设备识别：`device_name`/`device_codename`/`device_platform`/`device_id`（585-588）
- 触摸轴校正：`touch_swap_axes`（先做）、`touch_mirror_x`、`touch_mirror_y`（615-617，均由 FBInk 的 deviceQuirks 表预先判定，无需我们自己维护机型表）
- 平台标记：`is_mtk`/`is_sunxi`/`is_tolino`/`is_kindle_legacy`（603-609）
- 旋转：`can_rotate`（有陀螺仪）、`current_rota`、`rotation_map`、`ntx_boot_rota`、`ntx_rota_quirk`（612-619）
- 反色/夜间：`can_hw_invert`、`has_eclipse_wfm`（支持 nightmode 专用 waveform）（621-622）
- 其它：`can_wake_epdc`、`unreliable_wait_for`（MXCFB_WAIT_FOR_UPDATE_COMPLETE 可能超时）、`can_wait_for_submission`（610-611, 625）

`fbink_reinit(fbfd, cfg)`：framebuffer 状态变化（旋转、位深）后应调用而非重新 `fbink_init`（fbink.h:1053-1077）。

Waveform 模式常量（`WFM_MODE_INDEX_E`, fbink.h:374-459）与我们需要的场景映射建议：

| 用途场景 | 建议 WFM 模式 | 说明 |
|---|---|---|
| 翻页（全页刷新，追求清晰无残影） | `WFM_GC16`（387，~450ms，最高保真）或 `WFM_GC16_FAST`（418，略快，略低保真） | 普通机型；MTK 机型可配合 `fbink_mtk_toggle_auto_reagl` 让大区域自动升级为 REAGL |
| 翻页（需要防鬼影、不想整页闪烁） | `WFM_REAGL`（408）/`WFM_REAGLD`（413，更强去鬼影但闪烁抑制较弱）；MTK 上 `WFM_GC16HQ`（438，i.MX专属，REAGL别名）**不适用于 MTK**，MTK 应直接用 auto-REAGL 升级机制 | 仅 i.MX/mxcfb 机型可用 REAGL/REAGLD/GC16HQ |
| UI 局部更新（菜单/状态栏，非全页） | `WFM_AUTO`（374，EPDC 自动选择）或 `WFM_DU`（382，快速黑白，~260ms，有轻微鬼影） | 小区域、低延迟优先 |
| 按钮按下即时反馈 | `WFM_A2`（395，~120ms，仅黑白间切换）或 `WFM_DU4`（420，支持灰度） | 追求最低延迟，可接受轻微鬼影；MTK/部分设备另有 `fbink_mtk_toggle_pen_mode`（手写笔模式专用 DU/DUNM） |
| 整页强制闪烁刷新（消除残影，如打开书/定期清屏） | 任意模式 + `FBInkConfig.is_flashing = true`（触发 `UPDATE_MODE_FULL`，fbink.h:637），或显式 `WFM_GC16` + flashing | 典型用法是 `WFM_INIT`（431，~2000ms，多次闪烁后到全白，仅用于真正的初始化场景，不建议常规使用） |
| 夜间反色模式下刷新 | `WFM_GL16_INV`（422，为黑底文字优化）或 `WFM_GLKW16`/`WFM_GCK16`（428-429，部分平台专属），需配合 `is_nightmode=true`（走 `EPDC_FLAG_ENABLE_INVERSION`） | 仅 `has_eclipse_wfm=true` 的设备支持专属 nightmode 波形；否则退回软件反色 `is_inverted` |

MTK（Bellatrix，PW5+/Scribe）专属 API（fbink.h:1646-1681）：
- `fbink_mtk_set_swipe_data(direction, steps)`：配置滑动翻页动画方向和步数，Bellatrix 上 steps 上限 60，且该维度（宽/高）必须 ≥ steps 否则动画被禁用（1650-1653）。
- `fbink_mtk_toggle_auto_reagl(fbfd, toggle)`：控制大区域 DU/GL16/GC16 PARTIAL 更新是否自动升级为 REAGL；关闭后即所谓"fast mode"，但会导致 `fbink_wait_for_any_complete` 永远超时（1659-1661, 1674-1678）。
- `fbink_mtk_set_halftone(...)`：设置方格图案遮罩区域（Kindle专属，非必需功能）。
- `fbink_mtk_toggle_pen_mode`：手写笔模式更新必须用 DU 或 DUNM（夜间模式下），(1689-1691)。

等待刷新完成：`fbink_wait_for_submission`（976，等更新提交到 EPDC 队列）、`fbink_wait_for_complete`（987，等实际刷新完成，对 `unreliable_wait_for=true` 的设备可能超时需要容错）、`fbink_wait_for_any_complete`（1662，MTK 专属，等待所有 pending 更新，fast mode 下不可用）。

硬件反色：`is_nightmode`（需 `can_hw_invert`/`has_eclipse_wfm`）vs 软件反色 `is_inverted`（draw 时反色，始终可用，二者不互斥，fbink.h:635-636, 668-672）；注意需要设置环境变量 `FBINK_ALLOW_HW_INVERT` 才能绕过某些安全检查（672）。

Dithering（抖动）：`dithering_mode`（`HWD_PASSTHROUGH`/`FLOYD_STEINBERG`/`ATKINSON`/`ORDERED`/`QUANT_ONLY`/`HWD_LEGACY`，fbink.h:465-470），`HWD_ORDERED` 通常是 EPDC v2 唯一支持的硬件抖动变体；旧设备可能只有软件/legacy 路径。

**Rust 封装建议子集**：`fbink_open/close/init/reinit/get_state`、`fbink_refresh`/`fbink_refresh_rect`、`fbink_wait_for_submission`/`fbink_wait_for_complete`（MTK 再加 `fbink_wait_for_any_complete`）、`fbink_mtk_set_swipe_data`/`fbink_mtk_toggle_auto_reagl`（仅 `is_mtk` 时启用）、`fbink_input_scan`/`fbink_input_check`、`fbink_invert_screen`/`fbink_invert_rect`。

## 5. 许可证说明

- **KOReader**（AGPL-3.0）：GPLv3 第 13 条允许与 AGPLv3 代码组合，但组合后被并入的 AGPL 部分仍保留其网络条款。为保持本项目纯 GPL-3.0、避免许可混杂，**约定只参考其做法与事实**（机型按键 evdev code、lipc 属性名、流程顺序），用 Rust 重新实现，不翻译/复制 Lua 源码片段。
- **FBInk**（GPL-3.0）：与我们目标 License（GPL-3.0）兼容，`fbink.h` 声明的 API 以及 FBInk 的 `.a`/`.so` 静态/动态链接可以直接使用（本项目已经这样做）；如果未来需要搬运 FBInk *内部实现代码*（而非仅链接其库），同为 GPL-3.0 时是允许的（需保留版权声明、提供源码），但目前的"调用现成 API"模式不涉及复制其实现代码，无需额外处理。
- 本项目为 GPL-3.0：FBInk（GPL-3.0）可直接静态链接，必要时可摘抄其实现（保留版权声明）；KOReader 部分按上条约定只借鉴思路、重新编写。

## 不确定/待验证项
- `/proc/usid` 型号判定表是否已覆盖所有新机型（如 K12/未来机型），需以当前 master 为准动态核对。
- 是否存在不依赖 `liblipclua`/lipc 协议的纯 sysfs 挂起事件通道；目前看到的所有可靠事件源都经过 lipc。
- `touch_swap_axes`/`touch_mirror_x/y` 具体每机型取值未在 `fbink.h` 中列出（仅字段定义），需运行 `fbink_get_state` 在实机或已知设备 ID 表（`fbink_device_id.c`，未抓取）中查询确认，建议后续直接读取该源文件获取完整 deviceQuirks 表。
