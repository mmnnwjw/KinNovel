# KinNovel 1.0 (Rust) — 架构设计草案

> 状态: Phase 0 (可行性验证) 进行中。Python 版 0.8.x 在 `legacy` 分支维护, 作为行为规范与对拍基准。

## 目标

| 指标 | Python 0.8.0 (KPW5 实测) | 1.0 目标 |
|---|---|---|
| 冷启动到首页 | 导入模块 ~2 s + 初始化 | < 0.5 s |
| 打开章节 (66k 字) | ~1.5 s (字体 0.47 + 分页 0.87) | < 0.3 s 首屏, 其余后台分页 |
| 翻页 (预渲染命中 / 冷) | 41 ms / ~200 ms | < 30 ms / < 60 ms |
| 待机 CPU | 9 ticks / 30 s | ≈ 0 (只在分钟时钟醒) |
| 安装体积 / 依赖 | 39 MB + 需要 πthon 37 MB | 单个静态二进制 < 10 MB, 无依赖 |
| 内存 | ≥ 23 MB 仅导入 | < 15 MB 常驻 (不含图片缓存) |
| 机型 | 4 套自写 EPDC 协议, 3 套未实测 | FBInk 覆盖全部 Kindle 触屏机型 |

## 总体结构 (cargo workspace `rust/`)

```
crates/
  kn-platform   设备层: FBInk FFI(显示/机型识别)、evdev 输入、LIPC 电源事件、电池/时间
  kn-render     L8 位图、字形缓存(atlas)、文字/图形绘制、图片缩放
  kn-text       章节 HTML → 块 → 断行(禁则/字体回退) → 分页; 进度 XPath 锚点
  kn-net        rustls + tungstenite 的 SignalR 客户端、REST 鉴权、限流、优先级队列
  kn-store      配置/会话/缓存(兼容 Python 版 cache/ 目录与 config.json)
  kn-ui         组件树、脏区域、刷新调度、手势分发、主题
  kn-app        页面(首页/书架/历史/排行/分类/详情/系列/阅读/目录/设置/账号/公告...)与 main
```

依赖方向: app → ui → render/platform; app → text/net/store。text 与 net 不依赖 UI, 可在主机上单测。

## 线程模型

- **UI 线程** (主线程): `poll()` 等待 {触摸 fd, 电源键 fd, 唤醒 eventfd, 定时器}; 处理手势 → 更新状态 → 标脏 → 刷新调度。所有 UI 状态只在此线程修改。
- **网络线程** (1 个): 持有唯一 Hub 连接, 按优先级(交互 > 预取)串行执行请求, 结果经 channel 投递并写 eventfd 唤醒 UI。
- **工作池** (2 个): 章节解析/分页、图片下载解码、字体解码。结果同样经 channel 回 UI。
- 空闲任务: UI 线程无事件时执行(预渲染前后页、后台续排版), 每个任务有时间片上限, 来触摸立即让路。

## 显示与刷新 (参考 KOReader UIManager + FBInk)

- 组件 `render(&mut Canvas, rect)`; 状态变化时 `mark_dirty(rect, RefreshKind)`。
- 刷新类型 (对应 KOReader): `Ui`(GC16 局部)、`Partial`(翻页, MTK 用 REAGL)、`Fast`(按下反馈/菜单, DU/A2)、`Flash`(GC16 全刷)。
- 调度器每帧合并脏区域 (相交/相邻合并, 面积过大升级为整屏), 沿用 0.8.0 的残影预算策略。
- 按钮按下立即 `Fast` 反相反馈 (~20 ms), 再执行动作 —— 解决"点了没反应"的墨水屏通病。
- 帧缓冲写入: 直接写 FBInk 映射的 framebuffer (考虑 line_length 填充), 只写脏矩形。

## 文字渲染

- 章节字体 WOFF2 → TTF (`woff2-patched`) 解码后缓存到磁盘, 下次直接 mmap; 解析/光栅化用 `skrifa` + `zeno`。
- 字形缓存: (字体, 字号, 字形 id) → 8bit coverage 位图, LRU; 绘制 = 查表 + alpha blit。
- 每字回退: 章节字体 cmap 缺字 → 系统字体 (`/usr/java/lib/fonts/STHeitiMedium.ttf`)。
- 分页增量化: 先排到目标页即可显示, 其余作为空闲任务继续, 页码显示为 "x/…" 直到完成。

## 兼容性

- 机型/平台: 交给 FBInk 识别 (MTK / Rex / Zelda / 经典 mxcfb), 不再自写 ioctl 结构体。
- 输入: 用 FBInk 的 `fbink_input_scan()` 发现触摸/按键设备, 触摸坐标变换取 `FBInkState` 的 `touch_swap_axes` / `touch_mirror_x/y` (KOReader 也是这样做的, 机型表在 FBInk 内); 物理翻页键 evdev code 参考 KOReader (Voyage: 104=后退/109=前进, Oasis 系列相反); Oasis 重力感应旋转后续支持。
- 电源: 监听 `com.lab126.powerd` 的 `goingToScreenSaver` / `outOfScreenSaver` / `readyToSuspend` (先沿用 `lipc-wait-event` 子进程, 后续可 dlopen liblipc); 框架进程只 SIGSTOP/SIGCONT 不杀; 调用 lipc 前需短暂 SIGCONT 框架。详见 `research-kindle-devices.md`。
- 数据: 读取 Python 版的 `bin/config.json`、`cache/session.json`、`cache/progress/*.json`、章节/字体缓存, 升级不丢进度与登录。

## 验证策略

- Python 版作为基准: 导出断行/分页/进度锚点的 golden 数据, Rust 逐项对拍。
- 每个阶段都在 KPW5 实机验收 (性能数字 + 截图), 网络测试严格限频。
- CI: GitHub Actions 构建 armv7 musl 产物 + 主机单测。

## Phase 0 结论 (KPW5 实测)

| 验证项 | 结果 | 选型 |
|---|---|---|
| 静态二进制 | 351 KB, 启动到输出 ~1 ms | `armv7-unknown-linux-musleabihf` + cargo-zigbuild; 主机构建也走 `cargo zigbuild --target x86_64-pc-windows-gnu`(ring 需要 C 编译器) |
| 章节字体 | 真实 WOFF2 纯 Rust 解码成功, 字形与 Pillow 一致; 两套字体加载 355 ms (Python 仅章节字体 ~470 ms); 冷页渲染 125 ms, 热页 44 ms(未优化 blit); 18 MB 峰值 | `woff2-patched` + `skrifa`/`read-fonts` + `zeno`, 全部 MIT/Apache, 无 FreeType |
| 网络 | TLS+negotiate+WS+握手+一次调用共 0.62 s, 1.6 MB 二进制, 1.7 MB RSS | 同步阻塞单连接: `rustls`(ring) + `webpki-roots` + `tungstenite` + `flate2`(miniz_oxide); JSON Hub 协议 + base64 gzip 即可, 不需要 MessagePack |
| 显示/输入 | FBInk 识别为 PW5/Bellatrix(id 1791), stride 1248; 整屏写入 4.7 ms + 提交 0.6 ms; REAGL/GL16/DU/A2 局部与 MTK 翻页动画均可用 (完成等待: A2 ~120 ms, DU ~210 ms, REAGL/GL16 ~300 ms, GC16 闪刷 ~365 ms); `fbink_input_scan` 正确识别触摸屏与电源键; 389 KB | FBInk 静态链接 (`FBINK_FOR_KINDLE`+`FBINK_MINIMAL`+`FBINK_WITH_INPUT`), C shim 暴露扁平结构体; 直接写 `fbink_get_fb_pointer` 映射内存 + `fbink_refresh`; 触摸用 libc 直读 evdev (注意 32 位 `timeval`); FBInk 日志关掉或走 syslog |

注意: 匿名可用的是 `GetLatestBookList`; `GetBookList` 需要登录。

后续优化点: 字形 blit 改为按行切片批量混合 (目标热页 < 10 ms); 解码后的 TTF 落盘缓存避免重复解码。

## 阶段

0. 可行性验证: 静态二进制 ✅、FBInk 显示/输入、WOFF2+CJK 渲染、TLS+SignalR。
1. 平台层 + 渲染层 + 刷新调度 (能在屏幕上画出首页并响应点击)。
2. 文字引擎 (对拍 Python) + 阅读页。
3. 网络/存储 + 全部页面。
4. 打包 (KUAL 扩展, 与 Python 版缓存兼容)、迁移说明, 发布 1.0.0。
