# KinNovel (Rust) — 架构设计

> 状态: 已发布 (当前 v1.3.0)。Python 版 0.8.x 在 `legacy` 分支维护, 作为排版对拍基准。
> 各页面的界面规范见 [`UI-DESIGN.md`](UI-DESIGN.md), 机型差异见 [`research-kindle-devices.md`](research-kindle-devices.md)。

## 目标与现状

| 指标 | Python 0.8.0 (KPW5 实测) | 目标 | Rust 现状 (KPW5 实测) |
|---|---|---|---|
| 打开章节 (66k 字) | ~1.5 s (字体 0.47 + 分页 0.87) | < 0.3 s 首屏, 其余后台分页 | 首次解码字体 ~365 ms (之后读缓存的 `.ttf`) + 排版 ~56 ms; 离线/服务器故障时已缓存章节 ~70 ms 打开 |
| 翻页 | 41 ms (预渲染命中) / ~200 ms (冷) | < 30 ms / < 60 ms | 渲染 ~4 ms + 提交 6.5–9 ms (REAGL) |
| 待机 CPU | 9 ticks / 30 s | ≈ 0 | 单个 `poll()` 循环, 无事件时睡到下一分钟 (刷新时钟) |
| 安装体积 / 依赖 | 39 MB + 需要 πthon 37 MB | 单个静态二进制 < 10 MB | 静态 musl 二进制 ~5.5 MB (保留符号表供崩溃回溯), 发布包 ~2.8 MB, 无运行时依赖 |
| 机型 | 4 套自写 EPDC 协议, 3 套未实测 | FBInk 覆盖全部 Kindle 触屏机型 | FBInk + KOReader 机型表适配; 只有 KPW5 实机验证 |

## 总体结构 (cargo workspace `rust/`)

```
crates/
  kn-platform   设备层: FBInk FFI (显示/机型识别)、evdev 输入与手势、LIPC 电源事件、前光、机型差异 (model.rs)
  kn-render     L8 位图、字体 (WOFF2 解码 + 光栅化)、字形缓存、文字/图形绘制、图片解码与缩放 (设备上 WebP 走 libwebp)
  kn-text       章节 HTML → 块 → 断行 (禁则/字体回退) → 渐进分页; 进度 XPath 锚点
  kn-net        rustls + tungstenite 的 SignalR 客户端、会话、限流、交互/预取优先级、服务器熔断
  kn-ime        屏幕键盘的拼音输入引擎 (内嵌 Brotli 词库)
  kn-ui         立即模式页面、刷新调度 (RefreshScheduler)、命中区域、工作池、屏幕键盘、run 循环
  kn-app        二进制 `kinnovel`: 全部页面、存储 (store.rs, 兼容 Python 版目录与格式)、接口解析 (api.rs)、
                漫画缓存、书签/统计/更新检查/休眠封面/崩溃日志
```

依赖方向: app → ui → render/platform; app → text/net/ime。text、net、ime 不依赖 UI, 可在主机上单测。
原计划独立的 `kn-store` 最后放进了 `kn-app/src/store.rs`。`rust/spike-*` 是 Phase 0 的验证程序, 不在 workspace 内, 仅供参考。

## 线程模型

- **UI 线程** (主线程): `poll()` 等待 {触摸/按键 fd, 电源事件 fd, 工作池唤醒 eventfd, 下一分钟/计时器}; 处理手势 → 更新页面状态 → 渲染 → 刷新调度。所有 UI 状态只在此线程修改, 页面状态在页面结构体里, 没有全局变量。
- **工作池** (2 个线程): 网络请求、章节解析/分页、图片下载解码、字体解码。任务结果送回**发起它的那个页面实例** (`Page::on_message`), 页面已经离开时直接丢弃。
- **网络**: `kn-net` 全部是阻塞调用, 由工作池线程调用。只有一条 SignalR 连接, 交互请求优先于预取; 连续网络错误/超时/502–504 时熔断, 30 s 后试探, 间隔翻倍到 5 min, 熔断期间章节读缓存、跳过进度上传和预取。
- 其它后台线程: 前光读写 (`lipc-set-prop` 合并)、每日更新检查、休眠封面预取。

## 显示与刷新 (参考 KOReader UIManager + FBInk)

- 页面每帧整屏重画到 L8 位图 (立即模式), 调度器和上一帧做差分, 只提交变化的区域。
- 刷新类型: `Ui` (GC16 局部)、`Turn` (翻页, MTK 用 REAGL)、按下反馈 (反相 + A2/DU)、`Flash` (GC16 闪刷)、`Clean` (区域闪刷, 关闭弹窗时)。
- 调度器把差分按纵向间隔拆成最多 4 个区域分别刷新, 面积过大升级为整屏, 沿用 0.8.0 的残影预算; 实心深色块被清回背景色时对该块单独闪刷 (KOReader 关闭弹窗的 `flashui`)。细则见 `kn-ui/src/refresh.rs` 模块说明。
- 按钮按下立即反相反馈, 再执行动作 —— 解决"点了没反应"的墨水屏通病。
- 进入新页面、切换主题、唤醒时整屏闪刷; 阅读页在章节排好后自己闪刷一次。
- 帧缓冲写入: 直接写 FBInk 映射的 framebuffer (考虑 line_length 填充); 24/32 bpp (ColorSoft) 展开灰度。

## 文字渲染

- 章节字体是站点下发的混淆字体 (整套字形的码位置换), 只能用它本身来画正文。WOFF2 → TTF (`woff2-patched`) 解码后存成 `.ttf` 旁文件, 下次直接读; 解析/光栅化用 `skrifa` + `zeno`, 不用 FreeType。
- 字形缓存: (字体, 字号, 字形 id) → 8 bit 覆盖率位图; 绘制 = 查表 + alpha 混合 (NEON 自动向量化)。
- 每字回退: 章节字体缺字 → 系统字体 (`/usr/java/lib/fonts/STHeitiMedium.ttf` 等)。
- 分页渐进: 先排到目标页即可显示, 其余在后台继续, 页码显示为 "x/…" 直到排完。
- 排版必须与 Python 版一致: `tools/kn_text_golden.py` 生成的 golden 文件在主机测试里逐项对拍。

## 兼容性

- 机型/平台: FBInk 识别平台与触摸轴变换; 按键码、前光/色温、REAGL、方向感应等差异按 KOReader `device.lua` / `powerd.lua` 整理成 `kn-platform/src/model.rs` 的机型表, 未知机型回退到探测。
- 电源: 先沿用 `lipc-wait-event` 子进程监听 `com.lab126.powerd`; 框架进程只 SIGSTOP/SIGCONT 不杀; 电源键按下时先恢复框架, 让系统进入屏保的流程能走完 (见 `research-kindle-devices.md` §3)。
- 数据: 读写与 Python 版相同的 `bin/config.json`、`cache/session.json`、`cache/progress/*.json`、章节/字体缓存, 从 0.8.x 升级不丢进度与登录。新增的书签、阅读统计等放在 `data/`, 清理缓存不会删掉它们。

## 验证与发布

- Python 版作为基准: 断行/分页/进度锚点的 golden 数据, Rust 逐项对拍。
- 主机测试: `./build.sh host-test`; 主机预览: `kinnovel --preview out.pgm`, 配合 `KN_FAKE_DIR` 的模拟接口数据驱动真实页面。
- 每项功能在 KPW5 实机验收 (性能数字 + 截图), 联网测试严格限频。
- CI (`.github/workflows/ci.yml`): 每次推送跑主机单测并构建 armv7 程序。推送 `vX.Y.Z` 标签时 `release.yml` 构建发布包, 以 `docs/releases/vX.Y.Z.md` 为说明创建 GitHub Release。正式发布包只来自 CI (Windows 本地构建与 CI 构建的二进制不会逐字节相同)。

## Phase 0 结论 (KPW5 实测, 留作记录)

| 验证项 | 结果 | 选型 |
|---|---|---|
| 静态二进制 | 351 KB, 启动到输出 ~1 ms | `armv7-unknown-linux-musleabihf` + cargo-zigbuild; 主机构建也走 `cargo zigbuild --target x86_64-pc-windows-gnu`(ring 需要 C 编译器) |
| 章节字体 | 真实 WOFF2 纯 Rust 解码成功, 字形与 Pillow 一致; 两套字体加载 355 ms (Python 仅章节字体 ~470 ms); 冷页渲染 125 ms, 热页 44 ms(未优化 blit); 18 MB 峰值 | `woff2-patched` + `skrifa`/`read-fonts` + `zeno`, 全部 MIT/Apache, 无 FreeType |
| 网络 | TLS+negotiate+WS+握手+一次调用共 0.62 s, 1.6 MB 二进制, 1.7 MB RSS | 同步阻塞单连接: `rustls`(ring) + `webpki-roots` + `tungstenite` + `flate2`(miniz_oxide); JSON Hub 协议 + base64 gzip 即可, 不需要 MessagePack |
| 显示/输入 | FBInk 识别为 PW5/Bellatrix(id 1791), stride 1248; 整屏写入 4.7 ms + 提交 0.6 ms; REAGL/GL16/DU/A2 局部与 MTK 翻页动画均可用 (完成等待: A2 ~120 ms, DU ~210 ms, REAGL/GL16 ~300 ms, GC16 闪刷 ~365 ms); `fbink_input_scan` 正确识别触摸屏与电源键; 389 KB | FBInk 静态链接 (`FBINK_FOR_KINDLE`+`FBINK_MINIMAL`+`FBINK_WITH_INPUT`), C shim 暴露扁平结构体; 直接写 `fbink_get_fb_pointer` 映射内存 + `fbink_refresh`; 触摸用 libc 直读 evdev (注意 32 位 `timeval`); FBInk 日志关掉或走 syslog |

注意: 匿名可用的是 `GetLatestBookList`; `GetBookList` 需要登录。

## 阶段与版本

0. 可行性验证: 静态二进制、FBInk 显示/输入、WOFF2+CJK 渲染、TLS+SignalR。✅
1. 平台层 + 渲染层 + 刷新调度。✅
2. 文字引擎 (对拍 Python) + 阅读页。✅
3. 网络/存储 + 全部页面。✅
4. 打包 (KUAL 扩展, 与 Python 版缓存兼容), 发布 1.0.0。✅ (2026-10-10)

之后的版本:
- 1.1.0: 断网/服务器故障降级 (熔断、章节先显示缓存)、插图全屏查看、页面刷新策略调整。
- 1.2.0: 漫画、搜索 + 屏幕键盘拼音输入、阅读时调前光、多机型适配。1.2.1: 分区刷新与局部闪刷、圆角按下反馈。
- 1.3.0: 书签与章内跳页、书架文件夹与编辑、阅读统计、存储空间、休眠封面、更新检查、段间距、libwebp 解码、崩溃日志、Release 工作流。

未完成: 轻书架服务器恢复后用真实接口核对漫画、搜索、书架写入等 (目前只用模拟数据测过); 新建文件夹、在文件夹之间移动书。
