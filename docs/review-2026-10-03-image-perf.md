# KinNovel 代码审查报告（2026-10-03）

审查对象：`mmnnwjw/KinNovel` @ `aae5c8d`（38 commits）
参考项目：`LightNovelShelf/Web` `c53d0e8`、`koreader` `06c87b8`、
`NiLuJe/FBInk` `886f25f`、`lxdklp/kComics` `00316a2`
重点：项目架构梳理、图片加载性能、潜在 Bug。

结论标注：**已复核** = 本次直接读码/跑码确认；**静态阅读** = 由代码路径推理，
逻辑闭环但未真机复现；**待实机** = 依赖 Kindle 硬件/EPDC/真实服务端。

---

## 0. 审查方法

- 完整克隆 KinNovel，通读 `bin/`、`bin/src/`、`tests/`、`tools/`、`docs/`。
- 5 条并行线索：传输/缓存、阅读器排版、平台帧缓冲、UI/页面、参考项目对照。
- 对高危结论逐条回读源码，并与参考项目真实头文件/实现交叉验证；
  其中两条“疑似 P1”被复核为误报（见 §7）。

---

## 1. 项目架构

### 1.1 定位

KinNovel 是运行在**已越狱 Kindle**（FW 5.16.3+、Python 3.14 armhf）上的
轻书架小说阅读客户端。直接操作 `/dev/fb0` 与 EPDC ioctl、用 `evdev` 读触摸、
用 Pillow 做灰阶渲染，不依赖浏览器/Qt/桌面环境。

### 1.2 分层

| 层 | 目录/文件 | 职责 |
|---|---|---|
| 启动/生命周期 | `bin/start.sh`、`bin/app.py` | KUAL 启动、单实例锁、暂停占屏进程、快照/恢复、信号处理 |
| 平台层 | `bin/src/screen/`、`bin/src/kinnovel/power.py` | framebuffer/EPDC 输出、evdev 输入解析、休眠/唤醒 |
| 传输层 | `bin/src/kinnovel/transport.py`、`api.py` | SignalR over WebSocket、JSON 记录、gzip、限流、重连、token |
| 阅读层 | `bin/src/kinnovel/reader.py` | HTML 清洗、块提取、字体解析/回退、排版分页、XPath |
| UI 层 | `bin/src/kinnovel/ui.py`、`pages/*.py` | 立即模式 Pillow 渲染、页面路由、命中区域、弹窗/键盘 |
| 存储层 | `bin/src/kinnovel/utils.py`、`config.py`、`image_worker.py` | 原子写、SHA 文件名、LRU 清理、图片子进程下载/解码 |

### 1.3 启动与生命周期

1. `start.sh` 建 `/tmp/kinnovel.lock`，扫描 `/proc/*/fd` 找占用 `/dev/fb0`
   的进程并 `SIGSTOP`，PID 写入 `/tmp/kinnovel_paused_pids`。
2. 保存 `/dev/fb0` 快照到 `/tmp`，设置 `LD_LIBRARY_PATH`/`PYTHONPATH`。
3. `app.py` 初始化日志、`Config`、`ApiClient`、`ImageCache`、framebuffer、
   evdev，注册页面，`context.home()` 后启动 `PowerManager`。
4. 主线程阻塞在 `screen.input.listen(on_gesture)`，手势直接进入
   `PageContext.handle()`；退出时恢复 framebuffer 与暂停进程。

### 1.4 线程模型（实际，与设计文档不一致）

设计文档 `docs/architecture.md` 要求“UI 对象只在主线程变更、跨线程统一走
AppEvent 队列”。**实现并非如此**：

- 主线程只跑 evdev 读循环。
- `PageContext.run_async()`（`bin/src/kinnovel/ui.py:486`）对每个操作新建一个
  **无上限 daemon 线程**，且 **`on_success`/`on_error` 回调仍在工作线程执行**，
  随后直接 `self.show()`，即在工作线程里渲染并写 framebuffer。
- 因此各 `pages/*.py` 的模块级 `STATE` 被工作线程与输入线程并发读写，仅靠
  `EInkDisplay._show_lock` 串行化最终输出，没有共享状态锁。

这是后面许多竞态/重绘问题的根因。

### 1.5 数据流

- **登录**：`Config` 明文读账号密码 → `ApiClient` REST 登录 → token 落
  `session.json`。
- **列表/详情/章节**：`ApiClient.invoke()` → SignalR WebSocket → JSON 记录
  （`0x1e` 分隔）→ 业务对象。
- **阅读**：`pages/reader.py:enter()` → `_load_chapter()`（磁盘缓存 12h TTL）
  → `_prepare_document()`（lxml 清洗 + Pillow 逐字排版）→ `STATE["doc"]`，
  渲染只画 `doc.pages[page]`。
- **图片**：`<img src>` → `absolute_url` → `ImageCache.prefetch()` →
  `bin/image_worker.py` 子进程下载 → 解码为 `L` → 存 PNG → 渲染时
  `ImageCache.get()` 再从 PNG 解码 → `_fit_image()` → paste 到整页图。

---

## 2. 各模块实现要点

### 2.1 `app.py`（`bin/app.py`）

- `load_fonts()` 按分辨率缩放加载 4 组 UI 字号，缺失字体回退
  `ImageFont.load_default()`。
- `initialize_screen()` 按 `mtk→rex→zelda→mxcfb` 探测，逐协议 `probe()`。
- `on_gesture()` 直接调 `context.handle()`，返回字符串时 `replace()`。
- `shutdown()` 关 hub/输入/framebuffer/日志，但**不等在跑的 worker 线程结束**。

### 2.2 `ui.py`（`bin/src/kinnovel/ui.py`）

- `Canvas`：`text`/`text_fallback`/`wrap`/`button`/`header` 等立即模式绘制。
- `ImageCache`（`:221`）：内存 `OrderedDict` 上限 **24 张**，磁盘路径
  `cache/covers/<sha256(url)>.png`。
- `PageContext`：页面栈（上限 20）、`navigate/replace/back/home`、
  `render()`（每次新建整屏 `L` 图）、`show()`（`_show_lock` 内渲染+输出）、
  `handle()`、`run_async()`、`prune_cache()`。

### 2.3 `transport.py` / `api.py`

- `SignalRClient`：negotiate → 裸 RFC6455 WebSocket → JSON handshake
  `{"protocol":"json","version":1}\x1e` → invocation。
- `_dispatch()` 处理 completion/StreamItem/Close；gzip 仅当 `Response` 是
  base64 字符串时尝试解压。
- `_invoke_once()`（`:408`）**全程持有 `RLock`**：连接、发送、接收、等
  completion 都在锁内，默认超时可达 60s，即所有 Hub 调用完全串行。
- `RateLimit(9, 5.5)` 只作用于 Hub invocation；REST/negotiate/图片不走它。
- `ApiClient.invoke()` 对 401 清 token → refresh → 关 hub → 重试一次。

### 2.4 `reader.py`（`bin/src/kinnovel/reader.py`）

- `sanitize_html()`：lxml 解析，去 `script/style/iframe/...`、`on*`、`style`、
  `srcset`，过滤 `javascript:`/`data:` href。
- `extract_blocks()`：递归产出 `text/heading/image/footnote` 块并记录相对
  XPath 与 offset。
- `ensure_font()`：URL hash 命名，`Content-Length` 校验，原子写，WOFF1 归一化。
- `FontResolver`：按 (font,size) 缓存，缺失字形用 `glyph_available()` 判定后
  回退系统字体。
- `ReaderDocument._paginate()`：逐字测量做禁则换行，图片块预留
  `min(usable_height*0.62, usable_width*0.72)` 固定高度。

### 2.5 `pages/*.py`

- `home`：按 `home_order` 布局入口。
- `rank/browse/history/announcements/account/...`：模块级 `STATE` + `run_async`
  的立即模式页面，部分带 generation 防旧响应。
- `book`：详情 + 章节分页 + 后台预热目标章节。
- `reader`：compact/控件层双态、翻页、目录、全屏插图、进度保存/上传。
- `shelf`：文件夹树 + 分页 + 长按操作；**渲染只画文字行，不画封面**。

### 2.6 平台层（`bin/src/screen/`）

- `EInkDisplay` 用四套 ctypes 结构体对应 mtk/rex/zelda/mxcfb，ioctl 号按
  `sizeof` 动态算。
- `mxc_update()`：8px 对齐、full/partial、dither 强制 full、REAGL/AUTO 波形、
  夜间反色、marker 与等待策略。
- `write_image()`：8bpp 走 `tobytes()`+整块/逐行切片；1bpp 走 `convert("1")`。
- 输入：扫描 `event*` 找多点触控设备，`MultiTouchParser` 解析 slot 与手势。

---

## 3. 图片加载性能问题（重点）

按影响排序，行号来自当前 commit。

### P0-1 图片失败会进入“无限重试 + 每次全屏刷新”热循环（已复核）

闭环：

1. `ImageCache.prefetch()`（`ui.py:254`）在子进程超时/非零返回时**不抛异常、
   返回 `None`**（`:271`、`:273`）。
2. `run_async()`（`ui.py:486`）把“无异常”当成功，执行 `on_success` →
   `STATE["image_pending"].discard(url)`（`pages/reader.py:110`）。
3. 紧接着 `run_async` 无条件 `self.show()`（`ui.py:501`）触发整页重绘。
4. `render()` 发现图仍不存在且 `url not in image_pending`，于是**再次**
   `add` + `run_async`（`pages/reader.py:466-477`；全屏预览同理 `:419-430`）。

对 404/坏图这种快速失败，会变成“不断 fork 子进程 + 不断整屏 EPDC 刷新”，
既费电又持续闪屏，是当前最严重的性能/体验缺陷。

### P0-2 整章插图预取无并发上限，且每张图 fork 一个 Python 子进程（已复核）

- `_prefetch_images()`（`pages/reader.py:99`）遍历**整章所有图片块**，每个 URL
  调一次 `run_async`；每章调用一次（`:340`）。
- `run_async` 每个操作起一条线程（`ui.py:508`）。
- `prefetch()` 在每条线程里 `subprocess.run([sys.executable, worker, ...])`
  （`ui.py:262`），即**每张图一个新 Python 解释器**，重新 import PIL 并建 TLS。

含 N 张图的章节 = N 条线程 + N 个子进程同时下载/解码。Kindle 上 Python 冷启动
加 PIL import 本身就要数百毫秒到秒级，叠加内存/网络争抢，既拖慢首屏也易 OOM。
参考实现（kComics、KOReader、Web）都有明确并发上限（3、单任务、批量 6）。

### P1-3 每张图下载完成都会整页重绘 + 全屏刷新（已复核）

`run_async` 在 `on_success` 后无条件 `self.show()`（`ui.py:501`），而 `show()`
永远新建整屏图并提交整屏更新（`:370`、`:393`、`:404`）。`_prefetch_images` 的
`on_success` 只做 `discard`，不判断“当前页是否需要”。因此每张后台图完成都会
引发一次整屏 e-ink 刷新。正确做法（见 KOReader）是只刷新该插图矩形并合并刷新。

### P1-4 `image_worker` 全尺寸解码后转 `L` 再重编码为 PNG（已复核）

`bin/image_worker.py`：`response.read(8MB)` → `Image.open` → `image.load()`
**全尺寸解码**（`:34-38`）→ `convert("L")` 复制（`:40`）→ `save(temp,"PNG")`
（`:44`）。对照片类插图，PNG 编码/解码都比 JPEG/WebP 慢得多、文件大得多；
下一帧 `get()` 还要再解码一次 PNG。kComics 的 `_fit_to_screen()` 更合适：
先读 header 拿尺寸，非超限直接用原图；超限才解码并按目标尺寸 LANCZOS 缩放，
最后存 JPEG q70。

### P1-5 没有按用途做服务端尺寸协商（已复核）

`_scaled_url()`（`ui.py:213`）只有 URL 已含 `size=` 且不含 `height=` 时才追加
`height=1024`，且**所有场景都用 1024**。Web 端则是：书卡封面 `height=512`
（`refs/lighnovelshelf-web/src/components/BookCard.vue:5`）、详情 `1024`
（`SystemImage.vue:25`）、阅读插图横屏 `1024`/竖屏 `2048`
（`src/utils/url.ts:32`）。KinNovel 书架行内封面只显示约屏宽 23%，却可能
下载/解码 1024 高整图；URL 不含 `size=` 时更直接拉原图。尺寸不匹配同时放大
下载、解码、内存三项成本。

### P1-6 图片内存缓存按“张数”而非“字节”计（已复核）

`ImageCache(maximum=24)`（`ui.py:222`）按条目数淘汰。worker 允许
`MAX_IMAGE_PIXELS=20_000_000`（`image_worker.py:19`），一张 20MP 的 `L` 图约
20MB 常驻，24 张理论可达数百 MB，再叠加 `_fit_image` 的 8 张与阅读文档，
低内存 Kindle 容易 swap/OOM。KOReader 的 `frontend/cache.lua` +
`imagewidget.lua` 按**位图字节数**预算，并在内存压力下丢掉一半缓存。

### P1-7 解码与 LANCZOS 缩放发生在渲染线程（已复核）

- `ImageCache.get()`（`ui.py:241-243`）同步 `Image.open`+`load`+`convert("L")`。
- `ImageCache.cover()`（`:286`）每次渲染都重跑 `ImageOps.fit(..., LANCZOS)`，
  **fit 结果不缓存**。
- `_fit_image()`（`pages/reader.py:184`）缓存上限仅 8，到顶后**整体 clear**。

首页翻页/打开详情时，这些都在输入线程的 `handle→show→render` 路径上同步执行，
直接造成可感知卡顿。

### P1-8 预取完成不预热内存缓存；已缓存章节会逐张全解码（已复核）

- `prefetch()` 成功后直接返回（`ui.py:254-277`），**不把解码图放进 `_memory`**；
  下一次渲染仍要在渲染线程重新打开 PNG、解码。
- `_prefetch_images()` 开头对每张图调 `ctx.images.get(url)`
  （`pages/reader.py:104`）判断是否已缓存；若磁盘已有，会在工作线程里
  **逐张全解码整章插图**，并把 24 张之上的旧图挤出缓存。

### P1-9 书架预取了自己根本不画的封面（已复核）

`pages/shelf.py:45-48` 加载后对前 8 本书启动封面预取，但 `render()`
（`:86-100`）只画文字行、从不粘贴封面。结果是 8 次无意义下载 + 每次完成各触发
一次整屏重绘。

### P2-10 缓存目录混用、配额均分、无 HTTP 复用/ETag/Range

- 封面与正文插图共用 `cache/covers`（`ui.py:229`），`cache/images` 实际闲置。
- `prune_cache()` 把总配额按目录均分（`ui.py:510-516`），插图与封面互相挤占。
- 图片/字体/negotiate/REST 全是一次性 `urllib` 请求，无连接复用与
  `ETag`/`If-None-Match`/`Range`（`image_worker.py:34`、`reader.py:185`）。

### P2-11 页面始终全屏刷新，`show(region=...)` 从未被使用

framebuffer 层已支持 `region`（`framebuffer.py:554-569`），但
`PageContext.show()` 从不传区域，因此所有翻页/插图到达都是全屏 PARTIAL。

---

## 4. 潜在 Bug

### 4.1 图片与异步状态

1. **`image_pending` 跨页面永久泄漏**（已复核）。`run_async` 仅在
   `self.page_name == owner_page` 时执行回调（`ui.py:491`、`:501`）。若用户在
   下载期间进入目录/设置，discard 回调被跳过；返回后该 URL 永远 pending，
   `render()` 认为正在加载，插图永久显示 `[图片]`。
2. **回调在工作线程改 UI/STATE**（已复核）。`on_success` 在工作线程执行并调
   `show()`；`STATE["data"]/doc/page/image_pending/fitted_cache` 与输入线程
   并发读写，存在撕裂与丢更新风险。
3. **账号自动登录可永久卡在“正在登录…”**（已复核）。`account.py:18` 只在
   `not loading and not attempted` 时重试；登录中离开页面会跳过回调，
   `loading` 永远 True，回来也没有重试按钮。
4. **书架/历史缺 generation 防旧响应**（静态阅读）。`shelf._load()` 与
   `history` 的加载/清空没有类似 `book.py:28` 的代际校验，快速切换文件夹时
   旧响应可能覆盖新状态。
5. **书籍详情切换时不清 `STATE["data"]`**（已复核）。`book.py:26-29` 只改
   `book_id` 与 `loading`，`render()`（`:146`）仍用旧 `STATE["data"]`，
   加载窗口内章节/阅读按钮会操作上一本书的数据。
6. **书籍预热使用全局 `STATE["book_id"]`**（已复核）。`book.py:96-98` 的
   `operation()` 捕获了 `sort_num` 却在执行时读 `STATE["book_id"]`；期间切书
   会把旧书章节写到新书缓存键下。
7. **公告详情失败后无终态**（静态阅读）。`announcements.enter_detail()` 失败
   只弹窗，`STATE["data"]` 仍为 `None`，关掉弹窗后一直显示“加载中…”。
8. **目录空白行可点击**（静态阅读）。`render_catalog()` 对越界行也写 rect，
   `handle_catalog()` 未复检边界。
9. **章节导航假定 `SortNum` 连续**（已复核）。`book.py:92` 用真实 `SortNum`
   打开章节，而 `_change_chapter()`（`pages/reader.py:405`）与目录跳转
   （`:638`）按 `index+1` 与 `len(chapters)` 比较；SortNum 不连续时会跳错章。
10. **服务端进度丢失段落内偏移**（已复核）。本地 `_save_progress()` 存
    `(path, offset)`，但 `upload_progress()` 只发 `first_path_on_page()`
    （`pages/reader.py:384`），服务端恢复只能回到块首页。

### 4.2 图片缓存正确性

11. **损坏缓存文件会“粘住”**（已复核）。`get()` 解码失败返回 `None`
    （`ui.py:244`），但 `prefetch()` 只要文件非空就提前返回（`:258`），
    永远不会重下。
12. **同一 URL 并发预取会撞同一临时文件**（静态阅读）。`prefetch()` 无 per-URL
    in-flight 锁；`image_worker.py:45` 临时名固定为 `<hash>.png.tmp`，
    两个 worker 同写会互相覆盖。
13. **失败无负缓存**（已复核）。`prefetch` 失败后无记录，结合 P0-1 即重试热循环。
14. **`prune_cache`/清缓存与下载 worker 无协调**（静态阅读），且 `_memory`
    不随磁盘淘汰失效。

### 4.3 平台层

15. **`MxcfbRect` 字段顺序与 FBInk 不一致（已复核，潜在）。**
    `framebuffer.py:121` 定义 `(left, top, width, height)`，而 FBInk
    `refs/FBInk/eink/mxcfb-kindle.h:117` 与 `mtk-kindle.h:248` 都是
    `(top, left, width, height)`。`_send_update()`（`:433-434`）按属性名赋值，
    于是经典 mxcfb 结构体 offset0=left=x、offset4=top=y，内核按 top-first 读取
    时会**把 x/y 对调**。当前更新都是全屏 `(0,0,W,H)`，x=y=0 掩盖了该 bug；
    一旦实现真正局部刷新就会暴露。
16. **`68 字节` 注释是错的**（已复核）。`framebuffer.py:159` 注释写 68 字节，
    实际与 FBInk `struct mxcfb_update_data` 都是 **72 字节**
    （rect16 + 5×4 + temp4 + flags4 + alt_buffer28）。ioctl 用 `sizeof` 动态算，
    行为正确，仅注释误导。
17. **暂停 PID 列表跨进程持久且只按 PID**（已复核）。`power.py:24` 与
    `start.sh` 共用 `/tmp/kinnovel_paused_pids`；崩溃后残留，重启时 PID 复用可能
    把 `SIGSTOP/SIGCONT` 发给无关进程。
18. **休眠看门狗可能误判**（静态阅读）。`power.py:210` 把任何含
    `screensaver`/`suspend` 的状态当休眠，短瞬态可能触发强制唤醒。
19. **单点触控检测得到但无法解析**（静态阅读）。`_has_touch_caps` 接受只有
    `ABS_X/ABS_Y` 的设备，但 `MultiTouchParser._slot_has_coords` 要求
    `tracking_id != -1`，该路径永不产生手势。
20. **`_ioctl` 超时遗留阻塞线程**（静态阅读）。`framebuffer.py:371-389` 每次
    ioctl 新建 daemon 线程，超时后线程仍阻塞在内核。
21. **framebuffer 格式未校验**（静态阅读）。`write_image` 只区分 1bpp 与“其它”，
    16/24/32bpp 会按每像素一字节写入，可能花屏。
22. **全局状态字典无上限**（静态阅读）。`utils._ATOMIC_WRITE_LOCKS` 与
    `pages/reader._CHAPTER_LOCKS` 按路径/章节无限增长。

### 4.4 其它

23. `settings.py:88` 每次渲染都 `cache_size(CACHE_DIR)` 全树遍历，设置页会卡。
24. `Config.save()` 每次 Stepper/Toggle 都 fsync，设置页连点会卡。
25. `transport.gunzip_limited()` 未校验 `obj.eof`，截断 gzip 可能被接受。
26. WebSocket 握手失败路径未在 `finally` 关 socket（`transport.py:88-117`）。
27. `_xor_mask()` 用大整数异或整帧，接近上限时 CPU/瞬时内存尖峰。

---

## 5. 参考项目可借鉴点（按收益排序）

1. **按用途的尺寸变体**（Web `src/utils/url.ts:25,32`、`SystemImage.vue:25`）。
   把 `height` 提为 `ImageCache` 参数并纳入缓存键：书架/列表 `512`，详情
   `1024`，阅读插图横屏 `1024`/竖屏 `2048`。同时降低下载量、解码时间、
   磁盘与内存占用，投入产出比最高。
2. **有界图片 worker 池 + in-flight 去重**。参考 kComics
   `ThreadPoolExecutor(max_workers=concurrency)`（`info.py:583`，默认 3）、
   Web `loadingBatches`/`loadedBatches`（`Manga/Reader.vue:461,517`）、
   KOReader“一次只跑一个缩略图子进程”（`readerthumbnail.lua:287`）。
   替换当前“一图一线程一子进程”。
3. **解码一次到显示桶**。参考 kComics `_fit_to_screen()`（`mobi_convert.py:258`）
   与 FBInk（解码时直接选目标通道/尺寸，`fbink.c:11875,11994`）。worker 接受
   目标高度，JPEG 可用 `Image.draft()`；输出建议 JPEG/WebP 而非 PNG。
4. **按字节预算的 LRU**。参考 KOReader `frontend/cache.lua:38,79,140` 与
   `imagewidget.lua:41,147,242`：按位图字节计费、单对象超限拒绝、内存压力下
   丢弃一半。替换“最多 24 张”。
5. **到达即区域刷新**。参考 KOReader `textboxwidget.lua:1103,1125` 与
   `uimanager.lua:1241,1266,1348`（合并相交区域、延迟合并、累计 N 次后升全刷）。
   需要先给 `PageContext.show()` 加 region/dirty 参数。
6. **条件请求与缓存元数据**。参考 KOReader `bookinfomanager.lua:33,1013`
   （size/mtime + `cover_sizetag`）。至少加 `ETag`/`Last-Modified` 与
   “URL+尺寸变体”键。
7. **漫画式批量预取**：Web `Manga/Reader.vue:343,407,517,536,550`——首批 6 页、
   当前跨页 + 前向 4 页、按 URL 去重、按 generation 取消过期任务。
8. **软件抖动**：参考 FBInk `fbink.c:10880` 的 8×8 有序抖动；插图区域用
   dither hint，A2 仅用于纯黑白 UI。

---

## 6. 建议修复顺序

**P0（先止血）**

1. 让 `prefetch()` 用返回值/异常明确区分成功失败；失败写负缓存并
   **停止重试循环**（P0-1）。
2. 引入有界图片任务队列（2–3 并发），去掉“一图一子进程”；子进程改常驻/线程池
   （P0-2）。
3. `run_async` 成功后只在“变化确实影响当前可见内容”时重绘；图片到达走区域刷新
   （P1-3）。

**P1**

4. `image_worker` 接收目标尺寸，`draft`/缩放到显示桶后再存，输出改 JPEG/WebP
   （P1-4）。
5. `ImageCache` API 增加尺寸变体参数并纳入磁盘/内存键；封面/插图按用途取
   `512/1024/2048`（P1-5）。
6. 内存缓存改为字节预算 LRU（P1-6）。
7. `prefetch` 成功后预热 `_memory`；`_prefetch_images` 不再用 `get()` 全解码探测
   （P1-8）。
8. 移除 `shelf` 无用封面预取；修 `image_pending` 泄漏与回调线程问题
   （P1-9、§4.1/4.2）。
9. 修 `MxcfbRect` 字段顺序，为局部刷新铺路（§4.15）。

**P2**

10. 缓存目录分离与配额、HTTP 复用/ETag、settings 页缓存统计异步化、平台层
    健壮性（§4.16-4.27）。

完整改造可参考 `docs/architecture.md` 已有的线程模型与缓存设计，但需要把
“主线程唯一变更 UI 状态”真正落地。

---

## 7. 复核结论：两条疑似 P1 被判定为误报

1. **“mxcfb 结构体 68 字节、当前 72 字节导致 ioctl 不兼容”不成立。**
   对照 `refs/FBInk/eink/mxcfb-kindle.h:268`，`struct mxcfb_update_data`
   含 `hist_bw_waveform_mode`/`hist_gray_waveform_mode` 后就是 72 字节
   （rect16 + 5×4 + temp4 + flags4 + alt_buffer28）；FBInk 也是
   `_IOW('F',0x2E,struct mxcfb_update_data)`。KinNovel 用 `sizeof` 动态计算，
   与参考一致。真正的问题只是 `framebuffer.py:159` 注释写错（见 §4.16）。
2. **“`fcntl.ioctl` 不接受 `ctypes.c_uint32` 标量，导致 set_pwrdown_delay /
   wait_submission / poweron 失效”不成立。** 实测 `ctypes` 标量支持 buffer
   协议（`memoryview(c_uint32(0))` 可写、连续、4 字节），`fcntl.ioctl` 对非 int
   参数走 buffer 路径；对 `_IOW(...,4)` 传 4 字节缓冲区正是正确用法。
   当前 3 处标量调用无需修改。

影响：§4.15（`MxcfbRect` 字段顺序）是本次复核中新确认的真实平台层缺陷，
比原报告提出的两个“结构体/ioctl”问题更值得修。

---

## 8. 验证记录

- `python -m compileall -q bin`：通过。
- `python -m unittest discover -s tests`：33 个用例中 29 通过、1 跳过、3 个因
  **vendored PIL 是 armhf 二进制**在本机（Windows/x86）无法 import `_imaging`
  而报错（`test_reader`/`test_pages`/`test_utils`）。纯 Python 的
  `test_transport`/`test_parser`/`test_power` 通过。
- 结构体尺寸：本机 ctypes 实测 `MxcfbUpdateData=72`、`Rex=80`、`Zelda=88`、
  `Mtk=96`，与 FBInk 头文件逐字段核对一致。
- 参考项目均已 clone 到 `E:\Downloads\KinNovel_Perf\refs\`（Web `c53d0e8`、
  kComics `00316a2`、FBInk `886f25f`、koreader `06c87b8`）。

未验证项：所有 EPDC ioctl 真机行为、evdev 坐标映射、休眠/唤醒、真实服务端
大图解码与 WOFF2 字体加载，仍需在已越狱 Kindle 上回归。

