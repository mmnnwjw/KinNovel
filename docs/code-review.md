# Code review and resolution

The implementation was reviewed by a separate agent with a focus on bugs,
Kindle resource use, and missing tests. The initial findings and their current
resolution are recorded below.

| Severity | Finding | Resolution |
|---|---|---|
| P0 | Keyboard expected integer font keys while the app exposed named keys. | `bin/app.py` now exposes both key sets; a keyboard render test was added. |
| P1 | All non-tap gestures were discarded globally, so shelf long-press actions were dead. | `PageContext` now forwards `tap` and `long`; a long-gesture test was added. |
| P1 | Header back/home labels had no hit handling. | Header left and default right actions are handled centrally; page-specific right actions remain page-owned. |
| P1 | Several list pages requested more rows than they rendered and advanced the server page, skipping data. | Request sizes now match visible page sizes; notifications received previous/next controls. |
| P1 | Reader pagination height differed from the actual rendered content height. | Pagination and clipping now use the same header/content/footer geometry. |
| P1 | Clearing cache removed `session.json` and `visitor-id`. | Cache clearing now targets only covers, fonts, images, and content directories. |
| P1 | TLS verification was disabled by default. | `strict_tls` now defaults to `true`; disabling it remains an explicit compatibility option. |
| P2 | A WebSocket frame arriving with the HTTP 101 response could be dropped. | Remaining bytes after the HTTP headers are retained in the socket receive buffer. |
| P2 | Older asynchronous responses could overwrite newer user selections. | Catalogue, search, series, rank, announcement, book and notification loads use generation checks. |
| Perf | Cache pruning repeatedly rescanned the tree and the limit was not applied. | Pruning scans once per cache directory and runs on a background thread at startup. |
| Perf | Missing covers/images could block the render thread. | Image drawing no longer performs synchronous downloads; prefetch is asynchronous. |
| Perf | Character-by-character wrapping recalculated the full line width. | Wrapping now accumulates per-character width. A 112,007-character benchmark dropped to 0.593 seconds on the host. |

## Verification after fixes

```text
python -m unittest discover -s tests -v
Ran 16 tests: OK

python -m compileall -q bin
OK

python tools/render_preview.py
home.png, browse.png, reader.png generated

Live Hub GetLatestBookList with strict TLS:
Total=11568, returned=6
```

True-device framebuffer/EPDC, evdev coordinate mapping, Kindle sleep/resume and
the full authenticated API flow still require testing on a jailbroken device.

## Live account smoke test

The workspace test account was used with a dedicated 10-second-per-request
script. The test covered login, `GetMyInfo`, shelf, history, latest books, book
details, one real chapter, the chapter font, announcements and notifications.
It remained serial and did not request images or batches.

- Login and authenticated Hub calls succeeded.
- The selected real chapter contained 521 characters.
- The live chapter font was a 1,058,952-byte WOFF2 file.
- Host Pillow 11.3 loaded the WOFF2 directly and rendered correct Chinese
  glyphs; the bundled Kindle Pillow is version 12.3 with FreeType and needs
  final on-device WOFF2 confirmation.
- The font contained no PUA code points, but the chapter-font and system-font
  renders differed by about 7.94% of pixels on the same text sample, proving
  that the visual mapping comes from the chapter font itself.
- One long request caused the server to close the WebSocket before the next
  call; this exposed and fixed a missing reconnect-and-retry path.
- The final full run completed without authorization or rate-limit errors.

## Second review round (2026-09)

| Severity | Finding | Resolution |
|---|---|---|
| P0 | `browse` page 2+ rendered blank: server-paginated `Data` was indexed by the global `(page-1)*per_page` offset. | Row index is now the in-page index; numbering keeps the global offset for display only. Regression test added. |
| P0 | Reader chapter load had no staleness check: switching chapters quickly let an older response overwrite the newer chapter state. | `enter` success/error closures now verify `book_id`/`sort_num` before applying. |
| P0 | The `mxcfb` protocol used NXP upstream numbers (ioctl `0x46`, 64-byte struct), which no Kindle lab126 kernel accepts. | Struct, ioctls (`0x2E/0x2F/0x30/0x37`), waveform and flag enums now follow KOReader `mxcfb-kindle.h` (68-byte struct with `hist_bw/hist_gray` fields). |
| P1 | EPDC region alignment floored width/height, so e.g. 758px-wide screens never refreshed the right 6 pixels. | Start aligns down, end aligns up, then clamps to screen bounds. |
| P1 | `auto` protocol detection always picked `mtk` because ioctl errors were swallowed, leaving mxcfb devices with a dead screen. | Initialization now submits a small probe update; failure falls through to the next protocol and closes the fd. |
| P1 | `ApiError` (business failure, 401) was retried like a transport error, closing a healthy socket and doubling rate-limit pressure. | `invoke` re-raises `ApiError` without retry. Test added. |
| P1 | A SignalR record split across two WebSocket messages failed JSON parsing and the completion was lost. | Records now accumulate in a byte buffer split on `0x1e`. Test added. |
| P1 | `wrap_line` moved trailing closing punctuation to the start of the next line (inverted kinsoku rule). | Closing punctuation now squeezes onto the current line; opening punctuation moves to the next line. Tests added. |
| P1 | Chapter font cache used the URL basename, so same-named fonts overwrote each other, and truncated downloads were cached permanently. | Cache key is now a URL hash; downloads validate `Content-Length` before persisting. |
| P1 | Announcements list had no pager (pages > 1 unreachable) and shared the browse indexing bug. | Local indexing plus a bottom pager; empty state added. |
| P1 | Book detail `bound` state raced between books and compared `int` against raw shelf ids. | Result guarded by `book_id`; ids normalized to `int`. |
| P1 | Long-press triggered tap actions everywhere (including exiting the app from home). | Pages that do not use long-press now ignore non-tap gestures. |
| P1 | History could only be cleared via an invisible top-right hotspot, and rows beyond the first screen were unreachable. | Bottom bar with visible pager and explicit "清空" button. |
| P2 | Shop items beyond index 8 were unreachable. | Dynamic rows per screen height plus a pager. |
| P2 | gzip responses had no decompression limit. | Streaming gunzip with an 8MB cap. Test added. |
| P2 | WebSocket handshake did not validate `Sec-WebSocket-Accept`; handshake socket timeouts bypassed retry logic. | Accept hash verified; handshake errors wrapped as `TransportError`. |
| P2 | Successive toasts raced and cleared each other early. | Toast clearing is generation-guarded. |
| P2 | Reader re-ran LANCZOS contain on every render for the same illustration. | Fitted images cached per (url, size) with a small bound. |
| P2 | Headings used the body line height and could overlap. | Line height now derives from each run's font size. |
| P2 | `FontResolver` bypassed the system font cache; glyph cache grew unbounded. | System fonts cached per size; glyph cache capped. |
| P2 | Touch edge pixels mapped to `render_w` (out of range); scanned evdev devices leaked fds; press durations between tap and long thresholds were dropped. | Pixel clamp, scan handles closed, duration dead zone removed. |
| P2 | `fb_snapshot restore` crashed when the snapshot was larger than `smem_len`. | Writes truncate to the smaller length. |
| P2 | `write_image` copied row-by-row even for full-screen writes; `show(region=...)` wrote the whole image at the region origin; log file grew forever; framebuffer fd/mmap were never closed. | Single-slice fast path, region cropping, startup log rotation, `close()` on shutdown. |
| P2 | Shelf folder deletion copy implied the contents would be deleted; the actual (spec-correct) behavior promotes contents to the parent level. | Confirm copy updated to match behavior. |

Deferred: main-loop rendering still runs synchronously inside the evdev read
loop; ioctl timeout threads can linger in the kernel; partial/dirty-region
refresh is not yet wired into `PageContext.show()`. These need on-device
validation before changing.

## Third review round (2026-10, v0.7.0)

本轮聚焦图片加载性能，并在 Kindle 实机（1236x1648 / MTK / Python 3.14）上
完成验证。完整报告见 `docs/review-2026-10-03-image-perf.md`。

| Severity | Finding | Resolution |
|---|---|---|
| P0 | 图片失败被当作成功：`prefetch` 返回 `None`、调用方丢弃 pending 后立即重绘，形成“反复 fork + 反复全屏刷新”的热循环。 | `prefetch` 改为明确返回状态并记录失败退避；渲染不再因失败重新入队。`tests/test_images.py::test_failure_backoff_prevents_retry_storm` 覆盖。 |
| P0 | 整章插图预取无并发上限，且每张图一个 Python 子进程（实测冷启动约 1.07s / 47MB）。 | 改为 2 个常驻 daemon worker + 每 URL+尺寸 in-flight 去重，只预取当前页及后两页。 |
| P1 | 每张图完成都整页重绘 + 全屏刷新。 | 新增 UI 回调队列与合并刷新，插图到达只刷新其矩形。 |
| P1 | 全尺寸解码后转 `L` 再存 PNG；二次显示再解码一次。 | 按目标尺寸缩放后存灰度 JPEG，成功后预热内存缓存。 |
| P1 | 不做用途化尺寸协商，所有图都 `height=1024`。 | 封面 512、插图按显示高度取桶，仅对 `size=WxH` 图床 URL 改写。 |
| P1 | 图片内存缓存按条目数（24）而非字节计。 | 48MB 字节预算 + 条目上限双重 LRU；封面缩放结果单独缓存。 |
| P1 | 已缓存章节会逐张 `get()` 全解码；`image_pending` 跨页面永久泄漏。 | 预取改为 `is_cached` 探测 + 有界队列；去掉 `image_pending`，改由 `ImageCache` 统一去重。 |
| P1 | 书架预取自己不渲染的封面。 | 删除该预取。 |
| P1 | 异步回调在工作线程修改 UI 状态并渲染。 | 新增 `PageContext.post/request_show/drain_ui_queue`，输入循环 50ms 轮询，在主线程执行回调。 |
| P1 | 账号自动登录离开页面后永久卡在“正在登录…”。 | `run_async(sticky=True)` + 回调内按当前页刷新。 |
| P1 | 书籍详情切书残留旧数据；章节预热使用全局 `book_id`。 | 清空 `STATE["data"]/bound`；预热显式传入 `book_id`。 |
| P2 | 插图排版用固定高度，浪费版面。 | 解析 `<img width/height>`，按真实宽高比预留高度。 |
| P2 | 目录空白行可点击；章节导航假定 `SortNum` 连续。 | 只为有效行注册命中区域；新增 `_chapter_sort/_chapter_index`。 |
| P2 | 损坏缓存文件不再重下；失败无负缓存。 | `get()` 删除无法解码的文件；失败退避表有上限。 |
| P2 | 经典 mxcfb 的 `mxcfb_rect` 字段顺序为 `left, top`，与 FBInk 内核头文件不符。 | 改为 `top, left`；当前全屏刷新下不可见，为局部刷新铺路。 |

Device verification:

```text
Kindle (armv7l, Python 3.14.3, 1236x1648, mtk)
python -m unittest discover -s tests  -> Ran 101 tests, OK (skipped=1)
live image bench: cover 512 -> 359x512 grayscale JPEG, 55 KB, RSS +1 MB
device_boot_test.sh: SIGTERM -> exited in 3s, lock removed, screen restored
```

## Fourth review round (2026-10, v0.7.1)

| Severity | Finding | Resolution |
|---|---|---|
| P0 | 阅读历史/最近闪退：`GetBookListByIds` 对失效书籍返回 `null` 元素，页面 `item.get()` 抛异常，错误弹窗重绘同一破损页面，异常最终冲出输入循环终止应用。 | 列表接口统一过滤非 `dict` 元素（含裸数组返回）；`PageContext.show()` 渲染失败降级为错误页；`screen.listen` 单个手势异常不再终止循环；`app.on_gesture` 错误提示自身异常也被兜住。 |
| P0 | 阅读历史一直加载不出内容：`GetBookListByIds` 传了 `Type=Novel`，服务端返回空列表。 | 小说省略 `Type`，仅漫画传 `Type=Comic`，与 Web 端一致。 |
| P1 | 当前页/翻回页的插图会被排在大量后台预取之后，且失败后不会自动重试。 | ImageCache 改为优先队列（可见 0 / 邻近 3 / 后台 6），可见请求插队；失败按 2/4/8… 秒退避自动重试；失败不再触发重绘循环。 |
| P1 | 长时间闲置后首个 Hub 调用可能等满 60 秒。 | 闲置超过 20 秒主动重连；Hub 调用超时收紧到 25 秒。 |
| P2 | 关于页参考项目列表缺 FBInk。 | 已补上。 |

Device verification (v0.7.1):

```text
python -m unittest discover -s tests  -> Ran 113 tests, OK (skipped=1)
live_page_probe: history raw 24 ids, 1 null filtered -> render OK;
                 browse 8 items, rank 48 items -> render OK
device_boot_test.sh: started to touch-listening, SIGTERM -> 2s, lock removed
```

## Fifth review round (2026-10, v0.7.2)

| Severity | Finding | Resolution |
|---|---|---|
| Feature | 漫画会出现在排行榜/最近等小说列表里。 | 在 `api.py` 集中过滤：`is_comic()` 同时识别书籍列表的 `Type` 与书架的 `type`，`novel_items()`/`_novel_data()` 兼容裸数组与 `{Data:[...]}`；排行榜、最近、按 ID 取书、系列、书架全部只保留小说，阅读历史本就只读 `Novel` 字段。 |

Device verification (v0.7.2):

```text
python -m unittest discover -s tests  -> Ran 119 tests, OK (skipped=1)
live_type_probe: rank raw 48 (44 Novel + 4 Comic) -> filtered 44, comic 0
                 browse 10 -> 10 (comic 0); shelf 5 -> 5 (comic 0)
```

## Sixth review round (2026-10, v0.7.3)

本轮针对实机测量出的排版瓶颈做优化；旧实现自 v0.2.0 引入逐字探测、v0.3.1
加入 notdef 位图比对后一直沿用。

| Severity | Finding | Resolution |
|---|---|---|
| Perf | `glyph_available()` 对每个新字符执行 `getmask()` + 额外渲染一张位图与 `.notdef` 逐字节比对，实测 1.4–2.4ms/字，占分页时间约 91%。 | 改用 FreeType `FT_Get_Char_Index` 做 cmap 覆盖查询（0.011ms/字），与浏览器回退同一机制；栅格化判定保留为库不可用时的兜底。 |
| Perf | 每章新建 `FontResolver`，1MB 级 WOFF2 每章重复 `truetype` 约 450ms。 | `cached_font()` 按 (路径, 字号) 全局复用 FreeTypeFont。 |
| Perf | 每次 `show()` 都重新栅格化整页文字，翻页回看/控件层显隐/插图到达都要重付一次。 | 新增页面内容位图缓存（3 页 LRU），缓存键含文档版本、页码、插图代际、分辨率、夜间模式。 |

Device verification (v0.7.3) — 渲染逐像素一致：

```text
python -m unittest discover -s tests  -> Ran 121 tests, OK (skipped=1)
layout   : ch1 1620->799ms(cold)/80ms(warm); ch3 7259->264ms; pages identical
cmap     : 605 sample chars, 0 mismatch vs raster fallback
render   : text page 128->13ms; image page 104->10ms; chrome toggle 21/13ms
pixel    : old vs new pagination 0/41M diff; cached vs uncached 0 diff
```

## Seventh review round (2026-10-04, v0.7.4)

本轮完成一次全项目复审，并优先修复会影响日常阅读与设备稳定性的缺陷。

| Severity | Finding | Resolution |
|---|---|---|
| P1 | 书籍详情封面按固定 `height=512` 预取，渲染却按显示高度的尺寸桶读取；600×800、758×1024、1072×1448 等分辨率下封面永久空白。 | 预取与渲染统一使用 `height_bucket(cover_height)`，`cover()` 还能复用最接近的已缓存尺寸桶。 |
| P1 | 阅读历史底部按 4 列排版 5 个按钮，“清空”越界且无法可靠点击。 | 按按钮数量计算列宽，并补齐 `count` 命中处理。 |
| P1 | 弹窗高度按未换行行数计算，长错误信息溢出并压住按钮。 | 先换行再按真实行数计算高度，超过一屏时截断加省略号。 |
| P1 | `start.sh` 的 mkdir 锁在写入 pid 前存在竞态，两个实例可能同时操作 framebuffer。 | 启动锁增加空 pid 重试与 `/proc` cmdline 校验，避免误删新锁和 PID 复用误判。 |
| P1 | 打开书籍详情无条件预热目标章节，会在服务端产生额外阅读记录。 | 新增 `prefetch_reading_target` 设置（默认关闭，设置页开启前提示），关闭时不做任何章节预取。 |
| P1 | 所有列表页从详情返回后都重置到第一页。 | `PageContext.returning` 区分返回与从主页新进入；书架/历史/排行/最近/系列/公告返回时保留页码。 |
| P1 | `SignalRClient.invoke` 对所有传输错误自动重试，可能重复购买、签到或保存书架。 | 增加 `retry` 开关，`BuyShopItem`、`SignIn`、`SaveBookShelf`、`MarkNotifications`、`ClearReadHistory` 等非幂等方法不自动重试。 |
| P2 | 截断 gzip 被静默接受；连接/发送阶段的 `OSError` 绕过重试。 | 校验 `decompressobj.eof`；建连和发送统一包装为 `TransportError` 以进入重试路径。 |
| P2 | 正文图片未限制协议，`file://` 等地址会进入 `urlopen`；root-relative 路径拼接错误。 | 只接受 http/https 或相对地址；`absolute_url` 保留前导斜杠的站点根语义。 |
| P2 | WOFF1 表目录的 `origLength` 无上限，伪造字体会诱导超大解压。 | 单表 16MB、总量 32MB 上限，超限直接拒绝。 |
| P2 | 未知服务端 XPath 被当作第 0 页；本地进度不区分简繁转换。 | `page_for_path(..., missing=None)` 供进度恢复使用；本地进度文件名包含 convert。 |
| P2 | 图片 worker 并发自增 `image_generation`，正文缓存被逐张图重复失效。 | 计数改到 UI 线程回调内执行。 |
| P2 | `run_async` 每个操作新建无上限线程。 | 使用 6 槽信号量限制并发工作线程。 |

验证（v0.7.4）：

```text
python -m unittest discover -s tests  -> Ran 133 tests, OK (skipped=2)
python -m compileall -q bin            -> OK
python tools/render_preview.py         -> home/browse/reader/settings/about OK
```

## Eighth review round (2026-10-04, v0.7.5)

本轮优化 SignalR 传输层。应用不需要通知/成长值/私信的实时推送，因此不引入
服务器调用分发和读线程，而是先解决重连、排队、退出和重复请求问题。

| Severity | Finding | Resolution |
|---|---|---|
| P1 | 空闲 20 秒主动断开，长时间阅读后首次操作要重新 negotiate + TLS + 握手。 | 改为协议 Ping（type 6）保活，每 10 秒检查一次；实测空闲 45 秒后 socket 未变化、连接次数仍为 1。 |
| P1 | 后台 `GetNovelContent` 预取和用户操作共用一把全局锁，慢预取会阻塞交互请求。 | 增加 interactive(0)/prefetch(1) 调度；后台请求只有在没有交互请求等待时才会开始，`prefetch_chapter` 以 priority=1 调用。 |
| P1 | `close()` 需要等待持有全局锁的接收循环，网络卡死时退出最长要等 25 秒。 | 新增 `shutdown()`：先 `socket.shutdown()` 打断阻塞 recv，再关闭连接；应用退出改用 `ApiClient.shutdown()`。 |
| P1 | 连接/握手/单次调用超时叠加，最坏接近 90 秒。 | 连接与握手各 10 秒，接收循环按剩余 deadline 设置 socket timeout，单次调用最坏约 45 秒。 |
| P2 | 相同只读请求会重复发送，分类/公告页面反复请求。 | 只读调用按 (method, params) 合并 in-flight；分类缓存 300 秒、公告缓存 60 秒。 |
| P2 | REST/negotiate 不经过限流，429 没有退避。 | REST 与 Hub 共用 RateLimit；429 按 `Retry-After` 退避后重试一次。 |
| P2 | 截断 gzip、`type=7 Close`、非对象消息、64MB 记录缓冲。 | 校验 gzip eof；显式处理 Close/握手 error；忽略非对象消息；缓冲上限收紧到 16MB。 |
| P2 | `get_access_token()` 吞掉所有刷新异常并静默转为匿名连接。 | 只有 refresh token 明确失效（-100/401/404）才清理并匿名；临时网络错误向上抛出触发重试。 |

验证（v0.7.5）：

```text
python -m unittest discover -s tests  -> Ran 141 tests, OK (skipped=2)
python -m compileall -q bin            -> OK
live_account_smoke                     -> login/shelf/history/book/chapter/font/announcement/notification OK
live_keepalive                         -> 45s idle, connects=1, socket unchanged
```
