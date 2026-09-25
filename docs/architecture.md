# Kindle 版轻书架架构方案

## 0. 目标、结论与明确排除范围

目标是做一个运行在越狱 Kindle 5.16.3+、Python 3.14 环境中的前台应用，尽量贴近 kComics 已验证的技术路线：`/dev/fb0` + EPDC ioctl + Pillow 灰阶渲染 + evdev 触控 + SQLite/文件缓存。业务上优先实现小说首页、分类、排行、搜索、详情、章节目录、正文阅读、进度同步和书架；之后再按收益补公告、通知、评论、签到/积分、私信、商城和漫画在线阅读。

关键结论：

- 实时与业务调用使用 ASP.NET Core SignalR 的 MessagePack Hub 协议，而不是优先 JSON。原因是 Web 端 `invokeHub(method, params, options)` 实际发送两个参数，第二个参数是 `{ UseGzip: true }`；服务端返回 `ApiEnvelope.Response` 时，`UseGzip: true` 依赖 MessagePack 的 `bin` 类型承载 gzip 字节。若改用 JSON Hub 协议，二进制大概率会被 JSON 序列化为 base64 字符串，与 Web 端 `Uint8Array -> ungzip` 的语义不一致。
- REST 只保留登录、注册、邮件验证码、密码重置和 token 刷新等少数接口。业务查询走 Hub，和 Web 端保持一致。
- 阅读器用 lxml 做 HTML 安全子集解析，用 Pillow/FreeType 加载服务端 `Font` 字体，自行排版分页；进度继续生成并上传兼容 Web 端的 XPath 字符串。
- 书架沿用远端扁平树模型：`NOVEL`、`COMIC`、`FOLDER` 三种条目类型，`parents` 表示从根到父级的路径。本地必须显式做三份状态：远端基线、主状态、编辑 draft，并在保存前重新拉取远端做合并，不能静默覆盖。
- 不复用 kComics 的 Calibre/MOBI 导出链路，因为下载和导出被排除。若复制 kComics 的 framebuffer/evdev/键盘实现，应用整体应按 GPLv3 发布并保留版权声明。

明确排除：

| 功能 | 决策 | 理由 |
|---|---|---|
| 上传、图片上传、书籍/章节编辑、发布、删除、重排 | 排除 | 用户明确排除；Kindle 输入体验和 HTML/Markdown 编辑器成本过高，收益低。 |
| `/api/book/download`、`/api/book/download_chapter`、EPUB/CBZ 导出、MOBI 生成 | 排除 | 用户明确排除下载；带宽、存储、版权和内存开销大，还会引入 Calibre 大依赖。 |
| 论坛/社区 | 排除 | 用户明确排除；复杂 UI、分页、富交互和实时状态对 Kindle 收益低。 |
| 全屏浏览器式外链、Web/PWA 行为、裁剪器、瀑布流等 | 排除 | 不适合 framebuffer 前台应用。 |
| 用户头像设置 | 排除 | Web 端需要已有图片 URL 或上传图片，Kindle 端无实际输入收益。 |

需要特别说明：应用仍会缓存已读取章节、封面、字体和图片，用于离线续读、减少流量和降低 e-ink 页面等待时间，但不会调用站点“下载/导出”接口，也不会生成 MOBI。

## 1. 总体分层、目录结构、生命周期、线程模型和状态机

### 1.1 分层

架构分为六层：

1. 平台层：framebuffer、EPDC ioctl、evdev 输入、设备型号探测、文件权限。
2. 传输层：REST、WebSocket、SignalR MessagePack、gzip 解包、限流、重连、token。
3. 领域层：用户、书籍、章节、书架、评论、通知、积分、商城、私信、漫画。
4. 阅读层：HTML 安全解析、样式子集、字体、排版、分页、XPath、脚注和插图。
5. 存储层：SQLite WAL、原子文件写、磁盘缓存、离线 outbox、路径安全。
6. UI 层：页面协议、组件、键盘、弹窗、灰阶渲染、脏区刷新、夜间模式。

### 1.2 目录结构

```text
LightNovelShelfKindle/
  manifest.json
  launcher.sh
  install.sh
  uninstall.sh
  LICENSE
  THIRD-PARTY-NOTICES.md
  bin/
    app.py
    fb_snapshot.py
    config.json
    src/
      app/
        main.py
        router.py
        events.py
        state.py
        lifecycle.py
      platform/
        framebuffer.py
        input.py
        device.py
        power.py
      transport/
        http.py
        websocket.py
        msgpack_codec.py
        signalr.py
        rate_limit.py
        visitor_id.py
      auth/
        session.py
        token_store.py
      domain/
        user.py
        book.py
        chapter.py
        shelf.py
        comment.py
        points.py
        shop.py
        direct_message.py
        manga.py
      reader/
        html_sanitizer.py
        html_tree.py
        style.py
        font.py
        layout.py
        pager.py
        xpath.py
        footnote.py
        image_loader.py
      ui/
        page.py
        widgets.py
        keyboard.py
        render.py
        theme.py
        pages/
          home.py
          book_list.py
          rank.py
          search.py
          book_info.py
          reader.py
          shelf.py
          login.py
          settings.py
          announcement.py
          notification.py
          comment.py
          direct_message.py
          points.py
          shop.py
      storage/
        db.py
        cache.py
        atomic.py
        outbox.py
    vendor/
      PIL/
      lxml/
      fonttools/
      pinyin/
      licenses/
  tests/
    unit/
    integration/
    devices/
```

`vendor` 内只保留 Kindle armhf 可用的二进制依赖和许可证。默认不包含 Calibre；如果未来恢复 MOBI 导出，才单独评估 GPLv3/依赖体积。

### 1.3 启动与退出

启动流程：

1. `launcher.sh` 获取单实例锁，例如 `flock /tmp/lightnovel-shelf.lock`。
2. 记录并轮转日志，限制日志文件大小。
3. 扫描 `/proc/*/fd`，找出占用 `/dev/fb0` 的进程；排除自身后发送 `SIGSTOP`，把 PID 写入 `/tmp/lightnovel_shelf_paused_pids`。
4. 保存当前 framebuffer 到 `/tmp/lightnovel_shelf_fb.bin`。
5. 设置 `PYTHON=/mnt/us/python3/bin/python3.14`、`LD_LIBRARY_PATH`、`SSL_CERT_FILE`。
6. 启动 `app.py`。
7. `app.py` 初始化日志、配置、SQLite、字体缓存、framebuffer、evdev、页面栈和传输层。
8. 恢复本地设置和书架缓存，先渲染可用的离线首页，再异步连接远端。

退出流程：

1. 捕获 `SIGINT`、`SIGTERM`、正常退出和致命异常。
2. 停止接收新事件；flush 进度、设置和 outbox。
3. 关闭 WebSocket、HTTP 超时取消、SQLite checkpoint。
4. 关闭 framebuffer mmap 和 evdev。
5. 恢复 framebuffer 快照，恢复被暂停进程。
6. 删除锁文件和暂停 PID 列表，返回退出码。

### 1.4 前后台生命周期

Kindle 上没有可靠的系统级后台 UI 权限，应用定义为前台应用：

- 应用运行时持有 framebuffer 和 evdev，SignalR、缓存和通知线程持续工作。
- 退出即断开实时连接并恢复系统界面，不保留常驻后台服务。
- 不承诺拦截系统休眠、锁屏或电源事件。可以尝试读取 powerd/lipc 状态，但必须作为可选能力处理。
- 若收到外部 `SIGSTOP`，状态不迁移，只保证恢复后能重新对账；收到 `SIGCONT` 后重新刷新 WebSocket 状态并执行 `GetMyInfo`、书架和私信 resync。

### 1.5 线程模型

不引入 asyncio。Kindle 上线程 + 队列比事件循环更容易控制内存和依赖。

| 线程 | 职责 | 数量 |
|---|---|---|
| Main/UI thread | evdev 事件、页面状态机、渲染调度、事件分发 | 1 |
| SignalR receive | negotiate、WebSocket 帧、Hub 消息、ping | 1 |
| Request workers | REST 请求、图片下载、字体下载 | 2 |
| Layout/render worker | 章节排版、页图渲染、封面转灰阶 | 1-2 |
| Storage worker | SQLite 写入、outbox flush、LRU 清理 | 1 |
| Timer/scheduler | 超时、重连、进度 debounce、缓存清理 | 1 |

所有跨线程状态通过统一 `AppEvent` 队列进入主线程：

```text
InputEvent -> main queue
NetworkEvent -> main queue
LayoutFinished -> main queue
StorageFinished -> main queue
TimerEvent -> main queue
```

规则：

- UI 对象只在主线程变更。
- `Pillow` 图像只由 render worker 创建，渲染结果或磁盘文件传回主线程。
- WebSocket 发送必须持锁串行化，避免多个 invocation 的 MessagePack 帧交错。
- SQLite 使用一个连接加 WAL，或者每个线程独立连接但所有写操作经 storage worker。
- 请求队列限制并发，防止 Kindle CPU 和网络抖动导致界面卡死。

### 1.6 状态机

页面路由栈：

```text
Route = (page_name, params, immutable_state, created_at)
Router:
  push(route)
  replace(route)
  pop()
  reset_to(route)
  modal_push(modal)
  modal_pop()
```

应用页面状态：

```text
booting -> home_ready
home_ready -> browsing/searching/detail/reader/shelf/settings/account
browsing -> detail -> reader
reader -> detail / previous_chapter / next_chapter
any -> modal(loading/error/confirm/keyboard)
any -> stopping
stopping -> exited
```

阅读器状态：

```text
idle
  -> loading_metadata
  -> loading_content
  -> loading_font
  -> layouting
  -> ready
  -> turning_page
  -> ready
  -> saving_progress
  -> offline_ready
  -> error
```

书架状态：

```text
viewing
  -> syncing
  -> viewing
  -> editing_draft
  -> validating_merge
  -> conflict
  -> saving
  -> viewing
```

## 2. SignalR Hub 客户端方案

### 2.1 端点与 negotiate

默认 API server：

- `https://api.lightnovel.life`
- `https://cf-api.lightnovel.life`

Hub endpoint：

```text
{api_server}/hub/api
```

negotiate 请求：

```http
POST {api_server}/hub/api/negotiate?negotiateVersion=1
Accept: application/json
Authorization: Bearer {access_token}
x-id: {visitor_id}
```

可选支持 `negotiateVersion=0`。成功响应应解析：

```json
{
  "negotiateVersion": 1,
  "connectionId": "...",
  "availableTransports": [
    {
      "transport": "WebSockets",
      "transferFormat": "Binary"
    }
  ],
  "accessToken": "..."
}
```

如果返回 `accessToken`，后续 WebSocket 使用该 token 覆盖当前 access token；这能处理服务器在 negotiate 阶段刷新短期 token 的情况。

### 2.2 WebSocket 连接

首选 transport 为 `WebSockets`，transfer format 为 `Binary`。

URL：

```text
wss://host/hub/api?id={urlquote(connection_id)}&access_token={urlquote(accessToken)}
```

必须使用 RFC 3986 URL quote。纯 Python socket 可以在 HTTP Upgrade 头中携带 `Authorization: Bearer ...`，但为了最大程度贴近浏览器客户端和 Cloudflare 兼容性，优先使用 `access_token` 查询参数。

HTTP Upgrade 请求需要：

```text
Upgrade: websocket
Connection: Upgrade
Sec-WebSocket-Key: 16-byte random base64
Sec-WebSocket-Version: 13
Host: api host
User-Agent: LightNovelShelfKindle/{version}
```

可选 `Origin` 不伪造站点 Origin，除非实测 Cloudflare 要求；默认不发送。不请求 `permessage-deflate`，降低 Kindle CPU 与内存成本。

TLS：

- 使用 Python `ssl.create_default_context()`。
- 必须校验 CA，禁止照抄 kComics 更新接口中的 `_create_unverified_context()`。
- SNI 必须等于 API host。
- 连接超时 8 秒，读超时 60 秒。

### 2.3 WebSocket 增量帧

RFC 6455 支持分片：

```text
FIN=0, opcode=0x2 -> continuation
FIN=0, opcode=0x0 -> continuation
FIN=1, opcode=0x0 -> complete binary message
```

客户端实现必须：

1. 对 opcode `0x2` 开启消息缓冲。
2. 保存 opcode，直到 `FIN=1`。
3. 拼接 continuation payload。
4. 将完整二进制消息交给 SignalR MessagePack 解码器。

SignalR 本身没有“半个 Hub 消息”的增量协议；一个完整 WebSocket message 对应一个 Hub message。所谓增量帧只发生在 RFC 6455 层，不能在 MessagePack 未解完前调用回调。

控制帧：

- 收到 opcode `0x9` ping，立即回 `0xA` pong。
- 收到 opcode `0x8` close，按正常断线处理。
- 发送方必须掩码 payload。
- 单帧建议不超过 256 KiB；大响应由服务端 gzip 降低体积，客户端解压后再进入业务对象。

### 2.4 SignalR handshake

WebSocket 打开后发送：

```text
b'{"protocol":"messagepack","version":1}\x1e'
```

即 JSON handshake 加 Record Separator `0x1e`。

接收握手响应：

- `{}\x1e` 表示成功。
- 包含 `error` 的 JSON 表示失败，例如协议不支持或版本不匹配。
- 只有收到成功响应后才允许发送 invocation。

备选方案：

- 若服务器拒绝 `messagepack`，可尝试 `b'{"protocol":"json","version":1}\x1e'`。
- JSON 模式只作为诊断/降级路径，不承诺 `UseGzip` 可用；当 `ApiEnvelope.Response` 是字符串时尝试 base64 解码并 gzip 解压，但必须以真实服务端响应 fixture 验证。
- 禁止自动把 JSON 降级模式当成长期默认，因为会导致 Web 端与 Kindle 端行为不一致。

### 2.5 MessagePack Hub 协议帧

MessagePack 协议消息是 MessagePack array，不是 JSON。需要实现的核心类型：

| type | 名称 | 格式 |
|---|---|---|
| 1 | Invocation | 无 invocationId：`[1, headers, target, arguments]`；有 invocationId：`[1, headers, invocationId, target, arguments]` |
| 2 | StreamItem | `[2, headers, invocationId, item]` |
| 3 | Completion | 成功：`[3, headers, invocationId, result, nil]`；失败：`[3, headers, invocationId, nil, error]` |
| 4 | StreamInvocation | `[4, headers, invocationId, target, arguments]` |
| 5 | CancelInvocation | `[5, headers, invocationId]` |
| 6 | Ping | `[6]` |
| 7 | Close | `[7, error, allowReconnect]` |

字段说明：

- `headers` 编码为 string key 的 MessagePack map；无 headers 时编码空 map。
- `invocationId` 为 UTF-8 字符串，客户端使用递增字符串或 UUID。
- `target` 为 Hub 方法名，例如 `GetNovelContent`。
- `arguments` 为 array。
- `result` 可以是任意 MessagePack object。
- `error` 为字符串。

客户端 invocation 必须按下面生成：

```python
frame = [1, {}, invocation_id, method_name, [params, {"UseGzip": True}]]
payload = msgpack_encode(frame)
```

Web 端 `invokeHub(methodName, params, options)` 中 `params` 默认 `{}`，`options` 默认 `{"UseGzip": True}`。因此即使方法没有业务参数，也要发送：

```text
[1, {}, invocationId, "GetMyInfo", [{}, {"UseGzip": true}]]
```

不能把 `params` 和 `options` 合并成一个对象，也不能漏掉 options。

Completion 处理：

- 第 3 个元素为 `invocationId`。
- 成功时第 4 个元素是 result，第 5 个元素为 nil。
- 失败时第 4 个元素为 nil，第 5 个元素为 error。
- 必须校验 invocationId 与 pending 请求一致。
- `result` 预期是 `ApiEnvelope`：`{"Success": bool, "Response": value, "Status": int, "Msg": str}`。

### 2.6 MessagePack 解码器要求

可以采用两种实现：

1. 优先 vendor 一个 armhf 版 `msgpack-python`，并验证 Python 3.14 ABI。
2. 若无法打包，则实现受限解码器，只覆盖 nil、bool、int、uint、float、str、bin、array、map、timestamp ext、未知 ext 的保留。

必须支持：

- `bin 8/16/32`：解为 `bytes`，这是 `UseGzip` 的关键。
- UTF-8 string：解为 Python `str`。
- int/uint 所有宽度。
- bool、nil。
- array 和 map。
- MessagePack timestamp extension type `-1`：SignalR/MessagePack 客户端会把 DateTime 解成时间对象。Kindle 端统一转换为 ISO-8601 字符串。
- 未知 ext：至少保留类型和字节数，不能直接崩溃。

编码要求：

- 对 PascalCase 字段名不做大小写转换。
- `UseGzip` 编码为 boolean。
- 请求参数使用 map，不使用 tuple/object。
- datetime 不主动编码；请求中没有 datetime。
- 字符串长度、数组长度、map 长度必须使用正确格式族。

### 2.7 UseGzip 与响应解码

流程：

1. 收到 completion result，读取 `ApiEnvelope`。
2. `Success == False` 时抛出 `ServerError(Msg, Status)`。
3. `Response` 是 `bytes` 时执行 `gzip.decompress(response)`，再解析 UTF-8 JSON。
4. `Response` 不是 `bytes` 时直接作为对象返回。
5. 解析后的 JSON 仍是 PascalCase，不做字段名转换。

安全限制：

- gzip 解压前检查前 10 字节，最大解压后大小建议 8 MiB。
- 使用流式 `zlib.decompressobj(wbits=16 + MAX_WBITS)`，超过限制立即失败。
- 捕获 CRC 错误、截断错误和 JSON 解析错误，统一变成 `TransportError`。

`UseGzip` 是 LightNovelShelf 自定义的 Hub 参数，不是 SignalR Hub 协议压缩，也不是 WebSocket `permessage-deflate`。客户端可提供配置项，但默认必须为 true，与 Web 端一致。

### 2.8 重连、断线和 invocation 状态

Hub 连接状态机：

```text
stopped
  -> negotiating
  -> connecting_ws
  -> handshaking
  -> connected
  -> reconnecting
  -> negotiating
```

重连策略与 Web 端一致：

```text
自动重连等待：0s, 5s, 10s, 20s，之后固定 30s
完全 close 后初始重连等待：15s
```

建议 Kindle 端加抖动，例如 `delay + random(0, 2000ms)`，避免网络恢复时集中重连。

断线时：

1. 所有 pending invocation 标记为 `CancelledByReconnect`，上层可以选择重试。
2. UI 显示 `reconnecting` 状态，不阻止离线内容使用。
3. 重建连接必须重新 negotiate 和 handshake。
4. 新连接对象重新注册客户端事件回调：`OnDirectMessage`、`OnDirectMessageRead`、`OnDirectMessageBlockChanged`。
5. 连接成功后执行领域对账：`GetMyInfo`、书架远端拉取、私信列表和已打开会话的 resync。

invocation 分类：

| 类型 | 断线/超时行为 | 理由 |
|---|---|---|
| 查询类 | 可自动重试 | 幂等 |
| `SaveReadPosition` | 可重试，outbox 只保留最新一条 | 幂等，后写覆盖 |
| `SaveBookShelf` | 只能按明确合并结果重试 | 无服务端 revision |
| `PostComment`、`ReplyComment` | 不自动重试 | 可能重复发评论 |
| `BuyShopItem`、使用道具 | 不自动重试 | 资产类操作必须避免重复 |
| `SendDirectMessage` | 用原 `ClientMessageId` 重试 | 服务端幂等 |

### 2.9 token 刷新与实时连接

规则：

1. negotiate 前调用 `get_access_token()`。
2. 如果本地 access token 超过有效窗口，先调用 REST refresh。
3. negotiate 响应返回新 `accessToken` 时，更新内存中的 access token。
4. WebSocket 建立后不再主动替换 token；token 过期只能断开重建。
5. 收到 unauthorized、Hub 错误文本包含 `user is unauthorized`、HTTP 401/-100 时：清理用户、重启 Hub 为匿名连接，并提示登录。
6. refresh token 无效时：清除 `RefreshToken` 和用户状态，不反复请求。

不要试图在单个 WebSocket 上发送认证更新或重新 handshake；SignalR 标准做法是重建连接。

### 2.10 请求限流

Web 端使用共享队列：9 个请求 / 5.5 秒。Kindle 端必须保留同一限制，所有 REST 和 Hub invocation 使用共享 limiter。

建议实现为时间戳窗口：

```text
while len(window) >= 9 and now - window[0] < 5.5s:
    sleep((5.5s - (now - window[0])) + jitter)
window.append(now)
send()
```

补充规则：

- 图片、封面、字体下载使用独立并发限制：最多 2 个并发，默认 2 个请求/秒，可配置到 1。
- 排队时 UI 必须可取消，或允许继续阅读离线页。
- 超时：Hub invocation 20 秒；REST 15 秒；图片 30 秒。
- 重试只针对查询和幂等操作，使用指数退避：1s、3s、8s，最多 3 次。
- 429 响应按 `Retry-After` 或至少 5 秒处理。

### 2.11 依赖备选

| 方案 | 优点 | 风险 |
|---|---|---|
| 自写 RFC6455 + 自写 MessagePack 子集 | 无额外二进制依赖，行为可控 | 需要协议测试充分 |
| vendor `websocket-client` + `msgpack-python` | 开发快 | 需要 armhf/Python 3.14 兼容包 |
| vendor asyncio `websockets` | 功能完整 | 引入 asyncio，不适合本应用线程模型 |

建议 P0 使用 `msgpack-python` 加自写最小 WebSocket；如果 `msgpack-python` 没有 Python 3.14 armhf 包，则自写 MessagePack 子集，并用真实服务端响应 fixture 做回归。

## 3. 认证、会话与持久化

### 3.1 REST API

固定路径：

```text
POST /api/user/login
POST /api/user/refresh_token
POST /api/user/register
POST /api/user/reset_password
GET  /api/user/send_reset_email?email=...
GET  /api/user/send_register_email?email=...
```

REST 请求格式：

- `Accept: application/json`
- `Content-Type: application/json`
- `x-id: {visitor_id}`

`visitor_id` 不实现浏览器指纹，改为首次启动生成的 UUIDv4 并持久化。这样满足服务端 `x-id` 需求，同时避免 FingerprintJS 的成本和隐私问题。

### 3.2 登录与注册

登录响应中的 `Response`：

```json
{
  "Token": "...",
  "RefreshToken": "..."
}
```

登录流程：

1. `POST /api/user/login`。
2. 保存 `Token` 和 `RefreshToken`。
3. 重启 SignalR 连接。
4. 调用 Hub `GetMyInfo` 获取用户资料、未读数和 Growth。
5. 拉取书架、阅读历史和通知摘要。

注册流程：

1. 输入用户名、邮箱、密码、邮件验证码、邀请码。
2. 可选提供“发送注册邮件”。
3. `POST /api/user/register`。
4. 与登录一样完成认证并进入首页。

Kindle 输入体验差，注册和密码重置放在 P1/P2，不阻塞 P0 登录阅读。

### 3.3 RefreshToken 存储与 token 生命周期

Web 端 access token 只缓存约 3 秒，之后由 refresh token 换取新 access token。Kindle 端保持同样模型：

```text
login -> store RefreshToken + access token
access token older than 3s -> POST /api/user/refresh_token
refresh success -> replace access token
refresh invalid -> clear credentials + anonymous mode
```

RefreshToken 存储在 SQLite `auth` 表：

```sql
CREATE TABLE auth (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  refresh_token TEXT NOT NULL,
  access_token TEXT NOT NULL,
  access_token_updated_at INTEGER NOT NULL,
  user_json TEXT,
  user_updated_at INTEGER NOT NULL
);
```

注意：Kindle 用户存储多为 FAT/exFAT，没有可靠 UNIX 权限。默认方案不能承诺 token 加密安全；它与 Web IndexedDB 一样是本地明文存储，必须在文档中明示风险。若要求更强安全，需要用户口令派生密钥加密 token，但会牺牲免登录体验。

### 3.4 会话状态与错误处理

统一 `ServerError`：

```python
ServerError(msg: str, status: int)
```

错误来源：

- REST HTTP 非 2xx：读取 `ApiEnvelope.Msg` 和 `ApiEnvelope.Status`。
- Hub envelope `Success == False`：使用 `Msg` 和 `Status`。
- Hub completion error：保留完整字符串。
- 网络、DNS、TLS、超时：`NetworkError`，UI 显示离线状态。

无效 refresh token 状态：

- `ServerError.status == -100`
- `ServerError.status == 404`

处理：

1. 清空 `auth` 表中的 token 和 user。
2. 设置 session 为 anonymous。
3. 重启 SignalR 为匿名连接。
4. 显示“登录已失效”，允许继续浏览公开内容。

### 3.5 用户状态

内存中的 `CurrentUser` 与服务端 `GetMyInfo` 响应保持一致，字段包括：

- `Id`、`UserName`、`Email`、`Avatar`、`Role.Name`
- `RegisterAt`、`InviteCode`
- `Level`、`InteriorLevel`
- `UnreadNotificationCount`
- `UnreadDirectMessageCount`
- `Growth`

用户状态可持久化为 JSON，但只作为启动离线展示；每次联网后必须重新 `GetMyInfo` 校准。通知和私信未读数不能只依赖本地缓存。

## 4. 小说业务与阅读器

### 4.1 业务接口映射

首页：

- `GetOnlineInfo`
- `GetAnnouncementList({Page, Size})`
- `GetLatestBookList({IgnoreJapanese, IgnoreAI})`
- `GetBanInfoList()`

分类与列表：

- `GetBookCategories({Type: "Novel"})`
- `GetBookList({Page, Size, Order, IgnoreJapanese, IgnoreAI, CategoryId})`
- `GetSeriesList(...)`
- `GetBooksBySeries({SeriesName, ...})`

排行：

- 日榜：`GetRank({Days: 1})`
- 周榜：`GetRank({Days: 7})`
- 月榜：`GetRank({Days: 31})`

搜索：

| 模式 | 调用 |
|---|---|
| fuzzy | `GetBookList({KeyWords: key})` |
| exact | `GetBookList({KeyWords: '"' + key + '"'})` |
| title | `GetBookListByTitle` |
| author | `GetBookListByAuthor` |
| name | `GetBookListByName` |
| tags | `GetBookListByTags` |

详情：

- `GetBookInfo({Id})`
- `GetBookListByIds({Ids})`，单次最多 24 个。

正文：

- `GetNovelContent({Bid, SortNum, Convert})`

进度：

- `SaveReadPosition({Bid, Cid, XPath})`
- `GetReadPosition({Id})`

列表每页建议 12 或 16，不直接复用 Web 的大网格。接口仍传 `Page` 和 `Size`；UI 用 e-ink 友好的行式列表或小封面网格。

### 4.2 详情页状态

详情页必须展示：

- 封面、书名、作者、系列名/中文名、标签、简介。
- 最后更新章节、更新时间、收藏数、浏览数。
- 章节目录，支持正序/倒序。
- 当前进度章节，按钮显示“继续阅读”或“开始阅读”。
- 同系列书籍列表。
- 加入/移出书架。
- 可选评论入口。

进度优先级：

```text
本地进度 -> GetBookInfo.ReadPosition -> 第一章
```

若本地进度存在但章节已不存在，回退到服务端进度；两者都无效时回退第一章。

### 4.3 HTML 安全子集解析

使用 `lxml.etree.HTMLParser(recover=True, huge_tree=False)` 解析正文，再做白名单清洗。

允许的元素：

- 文本结构：`p`、`br`、`hr`、`span`、`div`
- 标题：`h1`、`h2`、`h3`、`h4`、`h5`、`h6`
- 强调：`strong`、`b`、`em`、`i`、`u`、`s`、`sup`、`sub`
- 列表：`ul`、`ol`、`li`
- 引用：`blockquote`
- 代码：`code`、`pre`
- 图片：`img`
- 链接：`a`
- 表格：`table`、`thead`、`tbody`、`tr`、`td`、`th`
- 注音：`ruby`、`rt`、`rp`

必须删除：

- `script`、`style`、`iframe`、`frame`、`form`、`input`、`button`
- `object`、`embed`、`link`、`meta`、`base`
- 所有 `on*` 事件属性
- `javascript:`、`data:`、`file:`、`vbscript:` URL
- 未知标签降级为保留其文本或删除，不允许原样输出

允许的 class 子集：

- 对齐：`left`、`right`、`center`
- 强调：`bold`、`stress`、`ita`
- 字号：`em05` 到 `em30`
- 注释：`duokan-footnote`、`footnotes`
- 插图：`illus`、`illu`、`duokan-image-single`、`image-preview`
- 其他站点阅读 class，如 `author`、`message`、`cut-line`

URL 处理：

- 相对 URL 以 API server 为 base 解析。
- 只允许 `http` 和 `https`。
- 图片 URL 记录原始 URL 和规范化后的 URL。
- 链接只识别站内路由和 `#fragment`，外链显示确认弹窗或直接提示不支持浏览器。

### 4.4 阅读样式子集

必须实现的 Web `read.scss` 语义：

- `p` 默认 `line-height: 1.8em`，首行缩进可配置为 2em 或 0。
- `blockquote` 左缩进、上下留白、左边框，在灰阶下用浅灰线。
- `h1`、`h2`、`h3`、`h4` 分别映射到相对字号和居中/加粗。
- `pre` 保持换行，使用等宽或系统字体，不横向滚动，超出时截断并提供“查看原文”提示。
- 图片最大宽高不超过页面内容区，等比缩放。
- `duokan-footnote` 渲染为可点击上标；`.footnotes` 默认不进入正文分页，脚注内容以弹窗显示。
- 颜色类全部映射为灰阶：红/绿/蓝等不做彩色，避免 e-ink 抖动。
- `float`、瀑布流、动画和 CSS 着重号不承诺像素级一致。

### 4.5 字体混淆与 TrueType cmap/字形映射

服务端 `Chapter.Font` 是字体 URL；相对路径以 API server 为 base。Web 端只是生成：

```css
@font-face {
  font-family: read;
  src: url(font_url);
}
```

Kindle 端的等价实现：

1. 下载字体到磁盘缓存。
2. 校验 magic：`TTF/OTF` 直接支持；`WOFF` 可尝试 Pillow/FreeType；`WOFF2` 需要额外解压能力，默认不支持。
3. 用 `PIL.ImageFont.truetype(font_path, size)` 加载。
4. 对正文字符做 cmap 覆盖检查。
5. 渲染时按字符调用 `font.getlength(char)` 和 `draw.text(..., font=font)`。

为什么常规字体混淆可以直接迁移：

- Pillow 的 FreeType 后端同样通过字体 `cmap` 把 Unicode codepoint 解析到 glyph id。
- Web 的 `@font-face` 没有额外 JavaScript 解码，也没有显式 glyph 映射表。
- 因此如果反爬字体只是把 codepoint 映射到自定义 glyph，Pillow 和浏览器会走同一个 cmap 语义。

必须做验证：

- 用 `fontTools.ttLib` 读取 `cmap`，确认正文所有 codepoint 都有映射。
- 抽样渲染，检查 glyph mask 是否为空或“豆腐”。
- 记录字体 hash，同一章节内容与字体 hash 绑定，避免更新字体后沿用旧分页。

Pillow 的限制：

- Pillow 不提供 HarfBuzz shaping，不支持复杂 OpenType `GSUB/GPOS` 替换。
- 若字体混淆依赖 ligature、上下文替换、变体选择符或 PUA 加 GSUB，而不是直接 cmap，Pillow 无法保证正确。
- Pillow 不提供按 glyph id 渲染的公开 API；只靠 `fontTools` 不能完成 glyph 渲染。

分阶段决策：

1. P0：只用 Pillow/FreeType 直接渲染。对缺失字符回退系统字体，并在日志记录缺失集合。
2. P1：建立真实章节字体样本库，统计 cmap 覆盖和渲染差异。
3. P2：如果样本证明需要 shaping，才引入 `uharfbuzz` + `freetype-py` 的 armhf 包，按 glyph id 渲染。不得在 P0 盲目引入。

### 4.6 排版分页

排版流程：

```text
sanitized HTML
  -> BlockNode tree
  -> TextRun / ImageRun / FootnoteRun
  -> line fragments
  -> Page objects
  -> current page bitmap
```

BlockNode 属性：

- 字号倍率：来自 h1-h6 或 emXX class。
- 行高倍率：默认 1.8。
- 首行缩进：默认设置。
- 对齐：left/center/right；justify 作为左对齐处理，避免 e-ink 大量空白调整。
- 上下 margin。
- 加粗/斜体。

行断规则：

- Web 竇 CSS `line-break: anywhere`，因此可以按字符断行。
- CJK 允许任意字符断行。
- Latin 优先在空格、连字符后断；行尾放不下时允许字符内断行。
- 避免标点出现在行首，至少处理 `，。、；：？！）》”’`。
- 禁止双空行；空块只保留一个行高。
- 每行使用 `font.getlength(text)` 计算，不用像素估算。

分页：

- 页面内容区 = framebuffer 高度减状态栏、页脚、上下边距。
- Kindle 阅读页默认单栏，不实现 Web 大屏双栏。
- 只保留当前页、上一页、下一页的布局结果；页图按需渲染。
- 字号、字体、行高、边距、窗口方向或灰阶变化时必须重新 layout。
- 分页完成后生成 `Page -> first_text_element_xpath` 映射，用于进度保存。
- 插图先从 URL `size` 参数取得宽高，未提供时使用预设比例；实际解码后若尺寸不同，只重排当前章节并保留远端进度。

性能目标：

- 已缓存章节翻页：小于 500ms，目标 250ms。
- 冷加载加排版：小于 1.5s。
- 正文章节解析后内存中的原始 HTML 不超过 2 MiB；超过时提示“章节过大，使用保守排版”。

### 4.7 图片与脚注

插图策略：

- 站点系统图床 URL 含 `size` 和 `placeholder` 时，读取宽高并添加 `height` 参数。
- 横图请求高度 1024，竖图请求高度 2048，与 Web 端一致。
- 封面列表请求高度 256，详情页请求高度 512。
- 图片先显示灰色占位，加载完成后局部刷新。
- 解码使用 `Image.open(...).thumbnail(...)` 或 `draft(...)`，立即 `close()` 释放对象。
- 不实现 blurhash 的彩色占位；灰阶下用简单灰色块和加载图标即可。

脚注策略：

1. 查找 `a.duokan-footnote`，读取 `href="#id"`。
2. 用 `id` 在当前树内查找脚注节点。
3. marker 渲染为上标序号或小图标。
4. 点击 marker 弹窗显示脚注 HTML 的纯文本/有限排版。
5. `.footnotes` 不进入正文分页。

### 4.8 阅读进度 XPath 兼容

服务端进度格式是字符串 XPath。必须实现与 Web `history.ts` 相同的生成算法：

```python
def canonical_xpath(element, root):
    element_id = element.attrib.get("id")
    if element_id:
        return f'//*[@id="{element_id}"]'
    if element is root:
        return "."

    parent = element.getparent()
    index = 1
    for sibling in parent:
        if sibling is element:
            break
        if sibling.tag == element.tag:
            index += 1
    local_name = element.tag.lower()
    return f"{canonical_xpath(parent, root)}/{local_name}[{index}]"
```

保存规则：

- 只对包含非空文本的元素生成 XPath。
- 每页保存“第一个可见文本元素的 XPath”。
- 页面翻页后 debounce 300ms 保存。
- 本地记录同时保存：`user_id`、`bid`、`cid`、`sort_num`、`xpath`、`page`、`top`、`chapter_hash`、`font_hash`、`updated_at`。
- 上传时只发服务端需要的 `Bid`、`Cid`、`XPath`。
- outbox 中同一 `user_id + bid` 只保留最新一条，成功后删除。

解析远端 XPath：

1. 优先支持 `//*[@id="..."]`。
2. 支持 `.` 开头的相对路径。
3. 支持有限 `//tag[n]` 全局路径。
4. 解析失败时按章节 sort、本地 `page` 和文本内容相似度回退。
5. 不能解析时不覆盖本地进度，只提示“远端进度无法精确定位”。

兼容风险：浏览器 HTML 规范化和 lxml HTML 规范化可能造成标签层级或自闭合差异。为降低风险：

- 清洗阶段保留原始元素顺序、tag 名和 id。
- 不主动包裹或删除允许的元素。
- 对无 id 的 XPath建立样本库回归测试。
- 本地额外保存字符 offset，只用于恢复，不上传。

## 5. 书架模型与冲突策略

### 5.1 远端数据模型

服务端 `GetBookShelf` 返回：

```json
{
  "data": [
    {
      "type": "NOVEL",
      "id": 123,
      "index": 0,
      "parents": [],
      "updateAt": "2026-09-25T00:00:00Z"
    },
    {
      "type": "FOLDER",
      "id": "abc",
      "index": 1,
      "parents": [],
      "title": "待读",
      "updateAt": "2026-09-25T00:00:00Z"
    }
  ],
  "ver": "20260921"
}
```

说明：

- `type` 只有 `NOVEL`、`COMIC`、`FOLDER` 三类。
- 书籍 id 是数字，文件夹 id 是字符串。
- `parents` 是从根到父级的完整路径，不是单个 parent id。
- 服务端没有层级深度限制，树是任意深度的扁平数组。
- `index` 表示当前父层内的顺序。
- `ver` 当前版本为 `20260921`；版本不匹配时必须拒绝写入并重新拉取。

### 5.2 本地三份状态

```text
remote_base:  编辑开始时拉取的远端快照
main:         当前显示的主状态
draft:        整理模式中的可丢弃编辑副本
```

SQLite 表：

```sql
CREATE TABLE shelf_items (
  item_id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  parent_index INTEGER NOT NULL,
  parents_json TEXT NOT NULL,
  title TEXT,
  update_at TEXT NOT NULL,
  main_json TEXT NOT NULL,
  base_json TEXT,
  draft_json TEXT,
  dirty INTEGER NOT NULL DEFAULT 0
);
```

`item_id` 统一转字符串存储：书籍 `str(id)`，文件夹本身就是字符串。加载时保留原始类型。

### 5.3 索引压缩与排序

所有移动、删除、插入后必须执行 `squeeze_index`：

1. 对全部项目按 `(index, parents.length)` 稳定排序。
2. 从 0 开始按父层重新分配连续 index。
3. 父层 key 用 `parents[-1] or ""`。

排序返回值约定：

- 小于 0：a 在前。
- 等于 0：保持稳定排序。
- 大于 0：b 在前。

### 5.4 书架操作

加入书架：

```python
item = {
    "type": "NOVEL",
    "id": book_id,
    "index": 0,
    "parents": [],
    "updateAt": now_iso(),
}
```

根层已有项目 `index += 1`，再插入新项目。

移出书架：

- 删除传入的项目。
- 若传入文件夹 id，则删除所有 `parents` 包含该文件夹 id 的项目。
- 删除后执行 `squeeze_index`。

移动项目：

1. 目标路径不能经过任何被移动的文件夹，否则拒绝，防止树环。
2. 若目标路径等于当前路径，跳过。
3. 若移动集合包含祖先，后代跳过，由祖先整体移动。
4. 被移动项目写入新 `parents`，按原相对顺序放在目标层开头。
5. 目标层原项目整体后移。
6. 对后代项目重写路径前缀：找到第一个被移动祖先，用 `new_parents + old_parents[anchor:]`。

新建文件夹：

- name trim 后不能为空，不能等于“根文件夹”。
- 同层禁止同名文件夹。
- id 使用 nanoid 兼容格式：21 位，字符集 `A-Za-z0-9_-`，用 `secrets` 生成。
- 插入到目标层开头。

重命名：

- 仅修改文件夹 title。
- 同层重名校验。

删除文件夹：

- 内容提升到该文件夹的上一层。
- 直接子项提升到上一层末尾，深层相对层级保持。
- 从所有路径中摘掉该文件夹 id。
- 删除文件夹本身并压缩 index。

### 5.5 离线缓存与冲突策略

远端没有 revision 或 version，`SaveBookShelf` 是整棵覆盖。不能照抄 Web 的静默覆盖。

编辑开始：

1. 拉取远端为 `remote_base`。
2. 复制到 `draft`。
3. 所有本地操作只修改 draft。

保存：

1. 重新拉取远端 `remote_now`。
2. 若 `remote_now == remote_base`，直接保存 draft。
3. 若不同，执行三方合并：
   - 远端新增且本地 base 没有：保留。
   - 本地新增且远端 base 没有：保留。
   - base 存在、本地删除、远端未改：删除。
   - base 存在、远端删除、本地未改：删除。
   - 同一 id 两边都修改：按 item 生成冲突项。
   - 文件夹移动导致路径不同：优先保留移动方；双方都移动则冲突。
4. 可自动合并时展示合并摘要，用户确认后保存。
5. 不可自动合并时提供三个选择：保留本地、保留远端、逐项选择。

离线：

- draft 可继续整理，但保存必须进入 outbox。
- 恢复网络后先执行同一套三方合并，不能直接上传。
- outbox 中书架类型只允许一条任务，保存最新完整结果。

书架内容的离线展示：

- 本地保存 shelf item。
- 书籍摘要通过 `GetBookListByIds` 分批拉取并缓存，单批最多 24 个。
- 封面按需加载并缓存。
- 若书籍摘要拉取失败，显示占位卡，不删除 shelf item。

## 6. 小说之外功能取舍

按收益/复杂度排序：

| 优先级 | 功能 | 结论 | 理由 |
|---|---|---|---|
| P1 | 公告列表与详情 | 保留 | 接口简单，信息价值高；HTML 渲染可复用阅读器子集。 |
| P1 | 通知列表、详情动作、标记已读 | 保留 | 用户状态完整性高，`GetNotifications`/`MarkNotifications` 直接可用。 |
| P1 | 阅读历史 | 保留 | `GetReadHistory` 简单，帮助续读；`ClearReadHistory` 可放在设置页。 |
| P2 | 评论读取、发布、回复、删除 | 保留 | 复杂度中等；Kindle 键盘可用，但输入成本高。发布必须确认且不自动重试。 |
| P2 | 积分/签到/签到日历/积分流水/金币流水 | 保留 | 接口简单，收益明显；不做复杂动画。 |
| P2 | 私信 | 保留 | 实时性强，需 SignalR 订阅、会话状态和幂等发送；比评论复杂但价值高。 |
| P2 | 漫画列表、搜索、详情、在线阅读 | 保留但放 P2 | kComics 已验证图片渲染技术，`GetComicContent` 每批 6 张可控；不进入 P0，避免影响小说核心。 |
| P3 | 商城货架、购买、我的道具、使用补签卡/漫画额度卡 | 保留但放 P3 | 涉及资产操作，必须强确认且不自动重试；收益低于阅读主线。 |
| P3 | 公开用户摘要 | 保留 | `GetUserSummary` 简单，可用于评论和详情展示。 |
| P3 | 在线统计、处刑列表、贡献者列表 | 可选展示 | 信息价值低，只做文本化展示。 |
| 排除 | 论坛/社区 | 不做 | 用户明确排除。 |
| 排除 | 上传/编辑/发布 | 不做 | 用户明确排除，Kindle 输入成本过高。 |
| 排除 | 下载/导出/MOBI | 不做 | 用户明确排除，资源和存储成本高。 |

### 6.1 通知

接口：

- `GetNotifications({Page, Size})`
- `MarkNotifications({Ids})`

UI：列表显示头像、标题、正文摘要、时间、未读状态；点击进入详情，必要时按 `Action.Type` 跳转；长按或菜单标记已读。

### 6.2 评论

接口：

- `GetComments({Type, Id, Page})`
- `PostComment({Type, Id, Content, ReplyId?, ParentId?})`
- `ReplyComment(...)`
- `DeleteComment({Id})`

策略：

- 详情页底部懒加载，避免启动时请求过多。
- 发布前显示确认弹窗。
- 网络失败保留草稿，不自动重试。
- 服务端响应结构比较宽松，解析时必须容忍缺失字段。

### 6.3 积分与商城

接口：

- `SignIn({})`
- `GetPointLog({Page, Size})`
- `GetCoinLog({Page, Size})`
- `GetSignInCalendar({Year, Month})`
- `GetShop({})`
- `GetMyItems({})`
- `BuyShopItem({Key, Quantity})`
- `UseSignMakeupCard({Date})`
- `UseComicQuotaCard({})`

资产操作规则：

- 购买和使用前显示价格、余额、持有量、限购结果。
- 不自动重试。
- 失败后必须重新 `GetMyInfo`/`GetShop` 对账。

### 6.4 私信

接口：

- `GetDirectConversations({BeforeMessageId, Size})`
- `GetDirectMessages({PeerUserId, BeforeMessageId, Size})`
- `SendDirectMessage({RecipientUserId, ClientMessageId, Content})`
- `MarkDirectMessagesRead({PeerUserId, ThroughMessageId})`
- `SetDirectMessageBlock({UserId, IsBlocked})`

实时订阅：

- `OnDirectMessage`
- `OnDirectMessageRead`
- `OnDirectMessageBlockChanged`

实现要点：

- `ClientMessageId` 用 UUIDv4，重试必须复用原值。
- 内容规范化：`\r\n` 和 `\r` 转 `\n`，再 trim。
- 消息按 `Id` 和 `ClientMessageId` 去重。
- 会话按 `LastMessage.Id` 倒序。
- 未读数不能只依赖推送；收到消息或重连后重新拉取会话和 `GetMyInfo`。
- 每个会话一条发送链，保证同会话点击顺序和服务端提交顺序一致。

### 6.5 漫画在线阅读

接口：

- `GetComicList`
- `SearchComicSeries`
- `GetComicContent({Cid, Skip, Take})`

策略：

- 每批 6 页，与 Web 端一致。
- 只预加载当前页前后各 1 页，最多保留 3 张解码图。
- 网络下载和灰阶转换分开，失败页显示占位，可重试。
- 进度用章节 id 和页码表示；服务端小说 XPath 不适用于漫画。
- 不提供整章下载或 MOBI 导出。

## 7. UI 架构、输入、键盘与 e-ink 刷新

### 7.1 统一页面协议

每个页面实现同一协议：

```python
class Page:
    name: str
    params: dict
    state: dict

    def activate(self, app) -> None: ...
    def deactivate(self, app) -> None: ...
    def render(self, render_ctx) -> RenderResult: ...
    def handle_event(self, event) -> Command | None: ...
```

`RenderResult`：

```python
@dataclass
class RenderResult:
    image: Image.Image
    hit_regions: list[HitRegion]
    dirty_regions: list[tuple[int, int, int, int]] | None
    refresh: RefreshMode
```

`Command`：

- `NavigatePush`
- `NavigateReplace`
- `NavigatePop`
- `ModalPush`
- `ModalPop`
- `NetworkInvoke`
- `CacheTask`
- `SaveState`
- `Noop`

主线程事件循环：

```text
event = queue.get()
command = top_page.handle_event(event)
execute(command)
if state_changed:
    image = top_page.render(ctx)
    display.show(image, refresh, dirty)
```

### 7.2 触控与 hit-test

触控输入沿用 kComics 的 evdev 模型：

- 扫描 `/dev/input/event*`。
- 识别具备 `ABS_MT_POSITION_X/Y`、`ABS_MT_SLOT`、`ABS_MT_TRACKING_ID` 或 direct input 属性的设备。
- 读取 absinfo min/max，把硬件坐标映射为渲染像素。
- 解析 tap、long、left、right、up、down。

Hit-test 规则：

- 所有可点击区域必须显式注册为矩形。
- 弹窗和键盘最顶层优先命中。
- 未命中弹窗区域视为取消或忽略，不允许穿透到页面。
- 阅读页左 30% 上一页，右 30% 下一页，中间打开菜单。
- 滑动阈值 40px，tap 移动阈值 30px，long press 最短 500ms。

建议增加按键事件支持：Kindle 物理按键或外接键盘按 `ESC` 返回、左右方向键翻页；不同设备键码差异必须通过设备矩阵验证。

### 7.3 键盘与拼音

复用 kComics 的键盘思路，但实现为独立组件：

- 模式：中文、英文、数字符号、纯数字。
- 中文使用本地拼音词库，词库放在 `vendor/pinyin`。
- 候选区支持分页、空格首选、退格、取消、确认。
- 密码框使用掩码显示，但底层输入仍为普通字符串。
- 会话输入法状态只保存在页面 state，不写入全局状态。

UI 要求：

- 每次按键局部刷新，不整屏闪。
- 显示当前输入法、候选页和剩余长度。
- 提交前显示回车目标，例如“登录”“发送”“搜索”。

### 7.4 字体与文本绘制

系统 UI 字体：

- 默认 `/usr/java/lib/fonts/STHeitiMedium.ttf`。
- 缺失时使用 Pillow default，但必须在设置页显示字体降级。
- 常用字号：24、28、36、48；标题可按分辨率动态计算。

阅读字体：

- 服务端 `Font` 优先。
- 无 `Font` 时用系统字体。
- `font.getbbox` 用于垂直对齐，不用固定 baseline。
- 文本绘制统一使用前景灰阶值。

### 7.5 灰阶、夜间模式与局部刷新

渲染：

- 所有 UI 和阅读页使用 Pillow `L` 模式。
- 图片解码后 convert 到 `L`，可选轻微对比度增强。
- UI 不使用彩色，最多使用 0、64、128、192、255 五档灰阶。

夜间模式：

- 首选 EPDC `ENABLE_INVERSION` flag。
- 若目标型号 ioctl 不稳定，退路是在 Pillow 层 `ImageOps.invert()`。
- 夜间模式切换属于全屏变化，使用 GC16 全刷。

刷新策略：

| 场景 | 波形 | 模式 |
|---|---|---|
| 页面路由切换 | GC16 | full |
| 阅读翻页 | GC16 | partial 或 full，按脏区大小 |
| 列表选中高亮 | DU | partial |
| 键盘输入 | DU | partial |
| 灰阶图片变化 | GC16 | partial/full |
| 菜单弹出 | GL16/GC16 | partial |
| 夜间模式切换 | GC16 | full |

必须实现：

- 8px 或 8 高度对齐，每个型号可配置。
- 局部刷新后累计 N 次，默认 16 次，自动执行一次全刷清残影。
- 同一时间只有一个 framebuffer 提交线程，持全局 `_show_lock`。
- ioctl 必须有线程和超时包装，避免单次阻塞导致 UI 卡死。

### 7.6 加载、错误、确认弹窗

统一组件：

- Loading：显示操作名和可取消按钮，不允许连续重复提交。
- Error：显示短错误、重试和返回。
- Confirm：显示标题、正文、确认/取消；危险操作必须二次确认。
- Toast：只在底部局部刷新，2 秒后消失。
- Empty：显示空态和主操作。
- Offline banner：不弹窗，只在状态栏显示。

所有网络错误必须保留当前页面状态；不得在失败后直接回到首页。

## 8. 网络与缓存

### 8.1 SQLite 结构

核心表：

```sql
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE settings(key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE book_summary(id INTEGER PRIMARY KEY, data_json TEXT NOT NULL, fetched_at INTEGER NOT NULL);
CREATE TABLE book_info(id INTEGER PRIMARY KEY, data_json TEXT NOT NULL, fetched_at INTEGER NOT NULL);
CREATE TABLE chapter_content(cid INTEGER PRIMARY KEY, data_json TEXT NOT NULL, font_url TEXT, fetched_at INTEGER NOT NULL);
CREATE TABLE read_progress(
  user_id INTEGER,
  bid INTEGER,
  cid INTEGER,
  sort_num INTEGER,
  xpath TEXT,
  page_index INTEGER,
  char_offset INTEGER,
  chapter_hash TEXT,
  font_hash TEXT,
  updated_at INTEGER,
  dirty INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(user_id, bid)
);
CREATE TABLE outbox(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  state TEXT NOT NULL,
  retries INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

SQLite 配置：

- `journal_mode=WAL`
- `synchronous=NORMAL`
- `foreign_keys=ON`
- 剩余磁盘空间低于 50MB 时进入只读降级模式。
- 启动时至少执行 `PRAGMA quick_check`。

### 8.2 文件缓存

目录：

```text
data/cache/covers/
data/cache/images/
data/cache/fonts/
data/cache/chapters/
tmp/frames/
```

统一索引：

```sql
CREATE TABLE cache_files(
  url_hash TEXT PRIMARY KEY,
  url TEXT NOT NULL,
  path TEXT NOT NULL,
  mime TEXT,
  size INTEGER NOT NULL,
  etag TEXT,
  last_modified TEXT,
  last_used_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
```

缓存 key：

```text
sha256(normalized_url + transform_name)
```

示例 transform：

- `cover_256`
- `cover_512`
- `reader_1024`
- `reader_2048`
- `font_original`
- `page_gray`

同一图片的不同请求尺寸不能互相覆盖。

### 8.3 原子写与路径安全

写文件流程：

```text
target = cache_root / url_hash[:2] / url_hash
tmp = target + ".tmp." + random_suffix
write tmp -> fsync -> os.replace(tmp, target) -> fsync directory
```

要求：

- 禁止用远端文件名、书名、章节名构造磁盘路径。
- 所有路径由 hash 生成，扩展名按 MIME 白名单映射：`.jpg`、`.jpeg`、`.png`、`.webp`、`.gif`、`.ttf`、`.otf`、`.woff`。
- 每次写前用 `os.path.commonpath` 校验目标在 cache root 内。
- 拒绝 symlink；目标存在且是 symlink 时删除缓存索引并重建。
- 单文件限制：封面 8MB，阅读插图 20MB，字体 30MB。
- 下载中断只留下 tmp 文件，下次统一清理。

### 8.4 缓存配额与 LRU

默认配额：

| 类型 | 配额 |
|---|---|
| 封面 | 100MB |
| 阅读插图 | 150MB |
| 字体 | 30MB |
| 正文 JSON | 150MB |
| 页面临时文件 | 30MB |

清理策略：

1. 删除超过 24 小时的 tmp 文件。
2. 删除不在索引中的孤儿文件。
3. 按 `last_used_at` 删除 LRU。
4. 当前阅读书籍、当前字体和书架封面标记为 pinned，不删除。
5. 删除文件后同步删除 SQLite 索引。

### 8.5 下载、限速、断点与重试

REST/Hub：

- 共享 9/5.5s limiter。
- 不需要断点续传。
- 超时和错误统一走重试策略。

文件：

- 使用 `urllib.request`，避免引入 requests。
- 优先支持 `Range: bytes={size}-`，仅当目标文件已有部分 tmp 且服务器返回 `206`。
- 记录 `ETag` 和 `Last-Modified`，304 时只更新 `last_used_at`。
- 每次读取 64KB，不一次性读大文件。
- 下载速度可配置：512KB/s、1MB/s、2MB/s，默认不限制。
- 连接超时 8 秒，读超时 30 秒。
- 失败保留 tmp，最多重试 3 次。

### 8.6 低内存策略

限制：

- `Image.MAX_IMAGE_PIXELS` 设置为 40,000,000，防止解压炸弹。
- 解码图片单边最大 3072px，超过时用 `thumbnail` 降采样。
- 同一时刻最多 3 张解码图像：当前页、上一页、下一页。
- 大图先写磁盘再渲染，不在内存中累积 bytes。
- gzip 解压最大 8MB。
- 章节原始 HTML 最大 2MB；超过时禁用插图自动加载并使用保守排版。
- 请求 worker 最多 2 个，图片 worker 最多 2 个。
- 每次 `PIL.Image.open` 后显式 `load()` 或 `close()`。
- 每 30 秒执行一次轻量 `gc.collect()`，避免频繁 full GC。

## 9. 安装、打包与许可证

### 9.1 运行时与 vendor

运行环境：

- 越狱 Kindle 5.16.3+。
- Python 3.14 armhf，安装路径默认 `/mnt/us/python3/bin/python3.14`。
- 必须依赖：Pillow、lxml、SQLite、ssl、zlib/gzip、ctypes、mmap、struct、evdev。

依赖策略：

| 依赖 | 决策 |
|---|---|
| Pillow | 必须，使用 kComics 已验证的 armhf 二进制。 |
| lxml | 必须，HTML 解析和 XPath 需要。 |
| evdev | 可 vendor；也可自写 ioctl，但优先使用已验证包。 |
| msgpack-python | 优先 vendor；无包则自写子集。 |
| fontTools | P1/P2，用于 cmap 检查和风险诊断。 |
| websocket-client | 可选；P0 自写 RFC6455 子集。 |
| Calibre | 不打包，下载导出已排除。 |
| requests/websockets/asyncio 框架 | 不引入。 |

### 9.2 KUAL

安装路径：

```text
/mnt/us/extensions/lightnovelshelf/
  menu.json
  bin/
    launcher.sh
    config.json
    app.py
    src/
    vendor/
```

启动脚本要求：

- 使用 `setsid` 启动，避免 KUAL 进程阻塞。
- 单实例锁。
- 日志轮转。
- `SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt`。
- `LD_LIBRARY_PATH` 包含应用 lib 目录和 Python lib 目录。
- 退出时恢复 framebuffer 和被暂停进程。

### 9.3 KPM

`manifest.json` 使用 kComics 已验证的格式：

```json
{
  "manifest_version": 2,
  "id": "lightnovelshelf",
  "name": "LightNovelShelf",
  "author": "...",
  "description": "LightNovelShelf Kindle client",
  "version": [0, 1, 0],
  "dependencies": [],
  "supported_platforms": ["kindlehf"]
}
```

安装路径：

```text
/mnt/us/kmc/kpm/packages/lightnovelshelf/
```

KPM 入口脚本只需调用 `launcher.sh`。更新包必须保留用户数据目录，不把 `data/cache` 放进覆盖目录。

### 9.4 用户数据与升级

数据放在应用目录之外的稳定路径：

```text
/mnt/us/lightnovelshelf-data/
  app.db
  cache/
  logs/
```

升级流程：

1. 停止应用。
2. 备份 `app.db` 和 `config.json`。
3. 覆盖代码目录。
4. 执行 schema migration。
5. 迁移失败时回滚备份。

### 9.5 GPL 与版权

决策：

- 如果复制或修改 kComics 的 framebuffer、evdev、键盘、拼音词库或脚本代码，应用整体采用 GPLv3。
- 必须保留 kComics 原版权声明、修改声明和完整 GPL 文本。
- `THIRD-PARTY-NOTICES.md` 列出 Pillow、lxml、evdev、msgpack、fontTools、拼音词库的许可证和版本。
- 不复制 LightNovelShelf Web 的 UI 代码，只按公开 API 交互。若未来复制 Web 代码，需遵守 AGPL-3.0 并提供源码。
- 如果不复制任何 GPL 代码而是完全独立实现，也要在开发记录中保留来源说明和许可证审查记录。

## 10. 测试矩阵与分阶段实现计划

### 10.1 测试矩阵

协议测试：

- negotiate 成功、401、无 WebSockets transport、`negotiateVersion=0`。
- WebSocket handshake 成功、协议错误、服务器主动 close。
- MessagePack 编码：nil、bool、int、string、bin、map、array、timestamp。
- invocation 参数：`[params, {UseGzip: true}]`。
- completion 成功、completion error、invocationId 不匹配、超时。
- gzip 响应、gzip CRC 错误、超限响应。
- WebSocket 分片、ping/pong、masking、close frame。
- 重连延迟、token 刷新、无效 refresh token、请求限流。

阅读器测试：

- HTML 清洗：script/style/iframe、事件属性、javascript URL、未知标签。
- 标题、段落、引用、列表、代码、表格、图片、ruby。
- 脚注 marker 和目标解析。
- 字体 cmap 覆盖、缺失字符、字体 hash、TTF/OTF/WOFF。
- 分页：字号、行高、缩进、标点禁则、长英文、超长章节、空段落。
- XPath 生成/解析/上传兼容。
- 进度离线 outbox、断线重试、远端进度回退。

书架测试：

- index squeeze 稳定性。
- add/remove/move/rename/delete/create。
- 文件夹树环阻断。
- 后代路径前缀重写。
- 三方合并：远端新增、本地新增、双方删除、双方修改、双方移动。
- 离线保存和恢复网络后不覆盖远端。

UI/平台测试：

- hit-test 边界。
- 键盘模式切换、拼音候选、密码掩码。
- MTK 和 mxcfb ioctl。
- 8bpp 和 1bpp framebuffer。
- partial/full refresh、残影恢复、夜间模式。
- 触控 min/max 映射、多点触控 slot。
- launcher 保存/恢复 framebuffer、暂停/恢复进程、重复启动锁。

性能指标：

| 项目 | 目标 |
|---|---|
| 启动到首页可交互 | <3s |
| 缓存章节翻页 | <500ms，目标 250ms |
| 冷加载正文 | <1.5s |
| 文本阅读内存 RSS | <90MB |
| 图片阅读内存 RSS | <120MB |
| 空闲 CPU | <1% |
| WebSocket 稳定空闲 | 30 分钟无重连 |

设备矩阵：

| 组 | 设备/环境 | 验证点 |
|---|---|---|
| P0 A | Python 3.14 armhf 模拟/实机 | Pillow、lxml、msgpack、ssl |
| P0 B | Paperwhite 4/5 类 MTK | `WAVEFORM_MTK`、96 字节结构体 |
| P0 C | KOA/KT/KP 类 i.MX | `WAVEFORM`、标准 mxcfb 结构体 |
| P0 D | 8bpp grayscale 实机 | framebuffer row stride、夜反转 |
| P1 E | 1bpp 老机型 | 是否可支持，若失败则限制为 8bpp |
| P1 F | Cloudflare endpoint | TLS、WebSocket、重连 |
| P1 G | 翻转/横屏机型 | 坐标映射与页面布局 |

### 10.2 分阶段实现计划

**P0：可运行核心，最高优先级**

1. launcher、framebuffer、evdev、SQLite、配置、日志、崩溃恢复。
2. UI 框架、路由、状态条、按钮、列表、弹窗、中英文键盘。
3. REST 登录、RefreshToken、匿名模式。
4. SignalR negotiate、WebSocket、MessagePack、UseGzip、限流、重连。
5. 首页、小说列表、分类、排行、搜索、详情、章节目录。
6. 正文安全解析、系统字体排版、基础分页。
7. 进度 XPath 生成、本地保存、在线上传。
8. 封面缓存和列表缓存。

**P1：阅读体验闭环**

1. 服务端 `Font` 下载与 Pillow 加载。
2. 字体 cmap 诊断和样本回归。
3. 插图、脚注、复杂块级样式。
4. 阅读设置：字号、行高、缩进、边距、夜间模式、简繁转换。
5. 书架完整实现和三方合并。
6. 公告、通知、阅读历史。
7. 注册和密码重置。

**P2：社交与增值**

1. 评论读取、发布、回复、删除。
2. 积分、签到、签到日历、积分/金币流水。
3. 私信实时事件、会话、幂等发送、已读、拉黑。
4. 漫画列表、搜索、详情和在线阅读。
5. 字体 shaping 复杂样本评估；必要时引入 HarfBuzz/FreeType。

**P3：低频增强**

1. 商城、道具购买和使用。
2. 用户摘要、在线统计、处刑列表、贡献者列表。
3. 更精细的缓存管理和型号自动调优。
4. 安装器更新和日志导出，不做自动安装。

## 11. 关键风险与无法保证之处

1. **SignalR MessagePack 兼容性**：Python 端不是官方 SignalR 客户端。MessagePack timestamp、bin、map key 和 completion 格式必须用真实响应 fixture 验证。若服务器升级 Hub 协议，客户端需要同步更新。
2. **UseGzip 语义**：本方案依据 Web 端代码推断 `Response` 为 gzip 后的 JSON bytes。真实服务端若在特定方法返回不同结构，需要按方法白名单适配，不能写死全局假设。
3. **WebSocket/Cloudflare 差异**：Cloudflare 可能对 `access_token` query、Origin、空闲超时或消息大小有额外限制。必须先用真实域名压测，再放宽实现。
4. **字体混淆**：Pillow/FreeType 只保证直接 cmap 映射。若反爬字体依赖 GSUB、ligature、上下文替换或按 glyph id 渲染，Pillow 无法保证正确；需要样本验证后引入 shaping 依赖。
5. **HTML 与 XPath 兼容**：lxml 与浏览器对错误 HTML 的恢复结果可能不同。无 id XPath 可能跨端不可解析。P0 必须以真实章节建立 XPath 回归样本。
6. **图片格式**：Pillow 可支持 JPEG/PNG/WebP/GIF，但具体取决于 armhf 编译选项。AVIF/SVG/动态图不承诺支持；解码器缺失时显示错误占位。
7. **Kindle 型号兼容**：MTK 与 i.MX EPDC 结构体和 ioctl 编号不同；1bpp、非 8bpp、特殊 stride、旋转和触控轴差异都可能导致失败。P0 只承诺实测机型。
8. **局部刷新残影**：DU/A2 波形有残影风险。必须提供自动全刷和手动全刷，不承诺所有型号都适合频繁 partial。
9. **书架冲突**：服务端没有 revision，`SaveBookShelf` 是整棵覆盖。本方案能做三方合并，但无法证明并发保存的严格顺序；必须保留冲突 UI。
10. **通知实时性**：SignalR 断线期间服务端不会为普通客户端事件补推。应用只能重连后重新拉取列表，不承诺零漏读。
11. **Python 3.14 依赖**：Pillow/lxml/msgpack 的 armhf ABI 需要实际验证。缺包时必须走自写子集或降级方案。
12. **安全边界**：HTML 清洗可减少 XSS 风险，但 Kindle 无浏览器沙箱。必须持续维护白名单，避免把未知 URL 直接交给网络层或系统。
13. **电源与系统后台**：应用不能保证拦截休眠、锁屏或系统级切页。所有生命周期处理都必须容忍外部 `SIGSTOP/SIGCONT` 和 framebuffer 被系统恢复。
