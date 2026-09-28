<p align="center">
  <img src="https://img.shields.io/badge/Platform-Kindle-111111?style=for-the-badge" alt="Kindle">
  <img src="https://img.shields.io/badge/Python-3.14-3776AB?style=for-the-badge&logo=python&logoColor=white" alt="Python 3.14">
  <img src="https://img.shields.io/badge/License-GPLv3-2C7A7B?style=for-the-badge" alt="GPLv3">
  <img src="https://img.shields.io/badge/Vibe-Coded-8A2BE2?style=for-the-badge" alt="Vibe Coded">
</p>

<p align="center">
  <strong>KinNovel</strong><br>
  在已越狱 Kindle 的原生系统上阅读轻书架<br>
  所有代码都是他们写的：DeepSeek V4.1 Flash、Kimi K3、GLM 5.3
</p>

---

## 项目简介

KinNovel 是面向已越狱 Kindle 的轻书架（LightNovelShelf）小说阅读客户端。
应用直接使用 Kindle framebuffer、EPDC 刷新和 evdev 触摸输入，不依赖浏览器、
Qt 或桌面环境。

当前版本：[`0.6.0`](https://github.com/mmnnwjw/KinNovel/releases/tag/v0.6.0)

## 界面预览

<table>
  <tr>
    <td width="50%">
      <a href="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/home.png">
        <img src="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/home.png" alt="KinNovel 主界面" width="100%">
      </a>
      <p align="center"><strong>主界面</strong></p>
    </td>
    <td width="50%">
      <a href="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/rank.png">
        <img src="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/rank.png" alt="KinNovel 排行榜" width="100%">
      </a>
      <p align="center"><strong>排行榜</strong></p>
    </td>
  </tr>
  <tr>
    <td width="50%">
      <a href="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/book-1854.png">
        <img src="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/book-1854.png" alt="KinNovel 书籍详情" width="100%">
      </a>
      <p align="center"><strong>书籍详情</strong></p>
    </td>
    <td width="50%">
      <a href="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/reader-1854-chapter-5.png">
        <img src="https://raw.githubusercontent.com/mmnnwjw/KinNovel/main/docs/images/readme/reader-1854-chapter-5.png" alt="KinNovel 阅读器" width="100%">
      </a>
      <p align="center"><strong>阅读器</strong></p>
    </td>
  </tr>
</table>

## 支持内容

| 模块 | 支持内容 |
|---|---|
| 账号 | 从 `config.json` 读取账号并自动登录；自动刷新访问令牌 |
| 首页 | 书架、阅读历史、排行榜、最近/分类、账号、设置、关于和退出 |
| 首页配置 | 使用 `home_order` 隐藏模块并调整左右、上下顺序 |
| 状态栏 | 顶栏显示当前时间与设备电量 |
| 排行榜 | 日榜、周榜、月榜和底部分页 |
| 最近/分类 | 最近更新、上架时间、总点击数、类型筛选和底部分页 |
| 书籍 | 书籍详情、简介、标签、系列列表、章节列表和章节分页 |
| 阅读 | 章节正文、compact 视图与覆盖式控件层、上一章/下一章、章节目录、阅读进度和阅读历史 |
| 缓存 | 打开书籍详情时后台预热目标章节；章节正文磁盘缓存与离线回退；可选预加载前后各一章 |
| 排版 | 自动分页、字号、行距、首行缩进、标点禁则、简繁转换和夜间模式 |
| 字体 | 内置 WOFF2 FreeType 运行时；加载章节字体；缺失字形自动使用系统字体补足 |
| 图片 | 正文插图、服务端缩放图、全章插图预取、点击全屏预览与再次点击退出 |
| 书架 | 多层文件夹、翻页、加入书架、移出书架和删除文件夹 |
| 账号功能 | 个人资料、签到、通知、公告、评论浏览和商城 |
| 显示 | MTK、Rex、Zelda 与 MXCFB 多机型 EPDC 自动探测输出、多分辨率界面适配、原屏快照恢复和系统进程暂停/恢复 |
| 电源与休眠 | 监听物理电源键与原生 LIPC 休眠/唤醒事件（goingToScreenSaver / outOfScreenSaver）；休眠自动让渡屏幕与触控，唤醒全刷无损恢复上次阅读界面；电源状态看门狗在事件丢失时自动恢复，避免卡死在休眠态 |

## 环境要求

1. 已越狱的 Kindle。
2. Kindle 系统版本 `5.16.3` 或更高。
3. 已安装 KUAL。
4. 已安装 [πthon (HF) for Kindle](https://github.com/lxdklp/python-for-kindle)。
5. Python 默认路径：

```text
/mnt/us/python3/bin/python3.14
```

## 安装

### Release 安装包

发布包只包含运行和安装所需文件，不包含测试、研究脚本和开发文档。

1. 下载 Release 中的 [`KinNovel-v0.6.0.zip`](https://github.com/mmnnwjw/KinNovel/releases/download/v0.6.0/KinNovel-v0.6.0.zip)。
2. 解压得到 `KinNovel` 文件夹。
3. 复制到 Kindle 的 `extensions` 目录：

```text
/mnt/us/extensions/kinnovel/
```

4. 检查目录：

```text
/mnt/us/extensions/kinnovel/bin/start.sh
/mnt/us/extensions/kinnovel/bin/config.json
/mnt/us/extensions/kinnovel/config.xml
/mnt/us/extensions/kinnovel/manifest.json
/mnt/us/extensions/kinnovel/menu.json
```

5. 在 `/mnt/us/extensions/kinnovel/bin/config.json` 中填写 `account_email`
   和 `account_password`（详见下方「配置」）。
6. 打开 KUAL，点击 `KinNovel` 启动。

### SSH 安装

把安装包中的内容复制到 Kindle：

```sh
mkdir -p /mnt/us/extensions/kinnovel
cp -R KinNovel/bin /mnt/us/extensions/kinnovel/
cp KinNovel/config.xml KinNovel/manifest.json KinNovel/menu.json \
   /mnt/us/extensions/kinnovel/
```

在 `/mnt/us/extensions/kinnovel/bin/config.json` 中填写 `account_email`
和 `account_password`（详见下方「配置」）。

直接启动：

```sh
/bin/sh /mnt/us/extensions/kinnovel/bin/start.sh
```

## 配置

配置文件：

```text
/mnt/us/extensions/kinnovel/bin/config.json
```

### 账号

```json
{
  "account_email": "user@example.com",
  "account_password": "your-password"
}
```

应用启动后自动登录，不显示登录表单。密码以明文保存在设备本地，请勿提交到
Git，也不要分享带有真实凭据的 `config.json`。

### 完整默认配置

```json
{
  "api_server": "https://api.lightnovel.life",
  "account_email": "",
  "account_password": "",
  "screen_protocol": "auto",
  "framebuffer": "/dev/fb0",
  "font_path": "/usr/java/lib/fonts/STHeitiMedium.ttf",
  "font_size": 48,
  "line_spacing": 1.42,
  "reader_margin": 34,
  "page_flash": false,
  "page_turn_animation": true,
  "reader_guide_dismissed": false,
  "night_mode": false,
  "justify": false,
  "first_line_indent": true,
  "convert": null,
  "ignore_japanese": false,
  "ignore_ai": false,
  "request_limit": 9,
  "request_window_ms": 5500,
  "cache_limit_mb": 192,
  "strict_tls": true,
  "check_update": true,
  "home_order": {
    "shelf": 0,
    "history": 1,
    "rank": 2,
    "browse": 3,
    "account": 4,
    "settings": 5,
    "about": 6,
    "exit": 7,
    "announcements": -1,
    "notifications": -1,
    "shop": -1
  }
}
```

### 显示

| 字段 | 说明 | 默认值 |
|---|---|---|
| `screen_protocol` | EPDC 刷新协议；`auto` 自动探测（按 `mtk`、`rex`、`zelda`、`mxcfb` 顺序）；支持手动指定 | `auto` |
| `framebuffer` | Kindle framebuffer 路径 | `/dev/fb0` |
| `page_flash` | 翻页时是否强制全屏刷新 | `false` |
| `page_turn_animation` | KPW5 及更新 MTK 平台使用原生 EPDC 翻页动画 | `true` |
| `reader_guide_dismissed` | 是否不再显示阅读页首次操作指引 | `false` |
| `night_mode` | 是否使用黑白反转的夜间模式 | `false` |

各协议支持机型：

- `mtk`：Paperwhite 5 (PW5 / Bellatrix)、Kindle 11 等联发科芯片机型
- `rex`：Paperwhite 4 (KPW4 / Rex)、Kindle Touch 4 (KT4) 等 i.MX6SLL 芯片机型
- `zelda`：Kindle Oasis 2 (KOA2)、Kindle Oasis 3 (KOA3) 等 i.MX7D 芯片机型
- `mxcfb`：Paperwhite 2/3、Kindle Voyage、Kindle Touch 2/3 等经典 i.MX6SL 芯片机型

> [!NOTE]
> 目前除 **KPW5** 经开发机完整实机测试外，其余机型（KPW4/KT4、KOA2/3、老款 PW2/3 等）刷新协议已参照 KOReader / FBInk 严格对齐结构体尺寸与 ioctl 实现，处于**待实机测试反馈状态**，欢迎使用对应设备的读者测试并提交反馈！

屏幕无法显示或刷新异常时，可手动指定协议（写入 `bin/config.json`）：

```json
{"screen_protocol": "rex"}
```

### 阅读

| 字段 | 说明 | 默认值 |
|---|---|---|
| `font_size` | 正文字号；设置页可用 `-` / `+` 调整 | `48` |
| `line_spacing` | 行距；设置页可用 `-` / `+` 调整 | `1.42` |
| `reader_margin` | 正文左右边距，单位像素 | `34` |
| `font_path` | Kindle 系统中文字体 | `/usr/java/lib/fonts/STHeitiMedium.ttf` |
| `first_line_indent` | 首行缩进 | `true` |
| `justify` | 两端对齐 | `false` |
| `convert` | 简繁转换：`null`、`t2s`、`s2t` | `null` |
| `prefetch_chapters` | 预加载前后各一章的正文与字体；章节正文始终写入磁盘缓存，离线时自动回退 | `false` |
| `ignore_japanese` | 列表过滤日文作品 | `false` |
| `ignore_ai` | 列表过滤 AI 作品 | `false` |
| `reader_guide_dismissed` | 是否不再显示阅读页操作指引 | `false` |

### 首页模块

`home_order` 的规则：

- `-1`：隐藏模块。
- 非负整数：数值越小越靠前。
- 排列方向：从左到右，从上到下。
- 数值相同：按程序内置顺序排列。

| 值 | 模块 |
|---|---|
| `shelf` | 书架 |
| `history` | 阅读历史 |
| `rank` | 排行榜 |
| `browse` | 最近/分类 |
| `account` | 我的账号 |
| `settings` | 设置 |
| `about` | 关于 |
| `exit` | 退出 |
| `announcements` | 公告 |
| `notifications` | 通知 |
| `shop` | 商城 |

例如只显示书架和最近/分类：

```json
{
  "home_order": {
    "shelf": 0,
    "browse": 1,
    "history": -1,
    "rank": -1,
    "account": -1,
    "settings": -1,
    "about": -1,
    "exit": -1,
    "announcements": -1,
    "notifications": -1,
    "shop": -1
  }
}
```

### 网络与缓存

| 字段 | 说明 | 默认值 |
|---|---|---|
| `api_server` | LightNovelShelf API 地址 | `https://api.lightnovel.life` |
| `request_limit` | 请求窗口内允许的最大请求数 | `9` |
| `request_window_ms` | 请求限流窗口，单位毫秒 | `5500` |
| `cache_limit_mb` | 封面、字体、图片和正文缓存上限 | `192` |
| `strict_tls` | 是否严格校验证书 | `true` |

只有日志明确报告设备缺少 CA 证书时，才临时使用：

```json
{"strict_tls": false}
```

## 使用

### 启动与退出

1. 打开 KUAL。
2. 进入 `KinNovel`。
3. 点击 `KinNovel`。
4. 应用会暂停占用 framebuffer 的系统进程并保存原屏幕。
5. 退出应用后，原屏幕和系统进程自动恢复。

### 阅读器

进入阅读页默认是 compact 视图：顶部一条细状态栏显示书名、页码、时间和电量，正文占满屏幕。

- 点击左右边缘：上一页或下一页。
- 从屏幕顶端向下滑：唤出完整控件层。
- 控件层顶栏显示书名，左侧返回图标回到上一级，右侧主页图标回到首页。
- 控件层底栏：上一章、目录、设置、下一章，最下方是章节进度条。
- 收起控件层：点击顶栏中部或正文任意位置回到 compact 视图。
- 点击正文插图：进入全屏预览；预览中点击任意位置退出。
- 首次进入阅读页显示操作指引弹窗；选择"不再提示"后不再显示，选择"感觉会忘记"则下次进入继续显示。

### 章节跳转

- 选择第 N 章：从第 N 章第一页开始。
- 在第 N 章首屏继续向左：进入第 N-1 章最后一页。
- 从章节目录点选章节：从该章第一页开始。

### 书架

- 点击文件夹：进入下一层。
- 点击书籍：打开详情。
- 长按项目：移出书籍或删除文件夹。
- 底部分页：浏览超过一屏的书架内容。

## 字体说明

LightNovelShelf 会为章节返回专用字体。阅读器必须同时使用该字体进行文本测量
和绘制，否则正文可能显示为错误汉字。

官方 Web 阅读器使用：

```css
font-family: read, sans-serif !important;
```

Pillow 不会自动进行 CSS 字体回退。KinNovel 会逐字检测字形，章节字体缺少或
轮廓为空时，仅对该字符使用 `font_path` 指定的系统字体。

WOFF2 章节字体需要支持 Brotli 的 FreeType。Release 包内置了：

```text
/mnt/us/extensions/kinnovel/bin/lib/freetype-woff2/libfreetype.so.6
```

因此无需预先安装 KOReader。若内置库不可用，启动脚本才会尝试设备上的
KOReader 路径。

## 排障

日志：

```text
/mnt/us/extensions/kinnovel/logs/kinnovel.log
```

### 无法启动

确认 Python 3.14 正常工作：

```sh
/mnt/us/python3/bin/python3.14 --version
```

若因跨平台传输丢失了执行权限导致点击无反应，可尝试补全权限：

```sh
chmod +x /mnt/us/extensions/kinnovel/bin/start.sh
```

### 出现残留锁

```sh
rm -rf /tmp/kinnovel.lock
```

### 屏幕刷新异常

根据你的设备机型，在 `bin/config.json` 中明确指定：

- **PW5 / Kindle 11**：`{"screen_protocol": "mtk"}`
- **KPW4 / KT4**：`{"screen_protocol": "rex"}`
- **KOA2 / KOA3**：`{"screen_protocol": "zelda"}`
- **PW2 / PW3 / Voyage / KT2/3**：`{"screen_protocol": "mxcfb"}`

### 字体异常

确认内置 FreeType 存在：

```sh
ls -l /mnt/us/extensions/kinnovel/bin/lib/freetype-woff2/libfreetype.so.6
```

### 网络异常

检查 `api_server`、Wi-Fi 和 Kindle 时间。测试完成后应把 `strict_tls`
恢复为 `true`。

## 卸载

```sh
/bin/sh /mnt/us/extensions/kinnovel/uninstall.sh
```

或：

```sh
rm -rf /mnt/us/extensions/kinnovel
```

卸载会删除缓存、日志和设备上的账号配置。

## 参考项目

本项目参考并复用了以下公开项目。各项目借鉴内容如下：

### LightNovelShelf/Web

- 项目：<https://github.com/LightNovelShelf/Web>
- 用途：LightNovelShelf 客户端行为与接口契约参考。
- 借鉴内容：
  - REST 登录、刷新令牌和统一响应结构。
  - SignalR Hub 方法、参数和响应字段。
  - 小说列表、详情、章节和阅读进度逻辑。
  - 章节字体加载模型。
  - 阅读位置的相对 XPath 语义。
- 说明：KinNovel 使用 Python 重新实现 Kindle 端，不包含原 Quasar/Vue
  前端源码。

### kComics

- 项目：<https://github.com/lxdklp/kComics>
- 用途：Kindle 原生运行层参考与 GPLv3 代码复用。
- 借鉴内容：
  - `/dev/fb0` 与 EPDC framebuffer 输出。
  - MTK 和 MXCFB 波形、刷新区域与刷新标志。
  - evdev 触摸设备识别和手势解析。
  - KUAL 启动、系统进程暂停、屏幕快照和恢复流程。
  - Kindle ARM 运行库的组织和启动方式。

### FBInk

- 项目：<https://github.com/NiLuJe/FBInk>
- 用途：Kindle 各代硬件电子墨水屏控制器（EPDC）底层驱动接口与平台特性（Quirks）参考。
- 借鉴内容：
  - 各硬件平台（MTK hwtcon、Rex i.MX6SLL、Zelda i.MX7D、经典 MXCFB i.MX6SL）ioctl 命令字与波形参数规范。
  - 板载环境温度传感器常量（`TEMP_USE_AMBIENT = 0x1000`）及波形温度补偿机制。
  - 刷新区域边界对齐算法（8 像素步长对齐）与防残影策略。

### KOReader

- 项目：<https://github.com/koreader/koreader>
- 用途：休眠与电源事件调度机制参考；多机型（Rex / Zelda / MTK / MXCFB）EPDC 驱动结构体与 ioctl 定义参考；提供章节 WOFF2 字体所需的 FreeType/Brotli 运行时。
- 使用方式：
  - 借鉴 KOReader 对多平台 `mxcfb-kindle.h` 驱动结构体（如 `mxcfb_update_data_rex` 与 `mxcfb_update_data_zelda`）的逆向与封装。
  - 休眠与唤醒流程参考了 KOReader 对 Kindle LIPC 电源事件与进程状态的协调管理。
  - Release 包内置 KOReader `v2026.03` 的 FreeType 与匹配 zlib：

```text
/mnt/us/extensions/kinnovel/bin/lib/freetype-woff2/libfreetype.so.6
```

  - 启动时优先加载内置库，因此用户不需要额外安装 KOReader。
  - 如果内置库缺失，会回退到设备已有的
    `/mnt/us/koreader/libs/libfreetype.so.6`。
- 说明：内置二进制按 GPLv3 分发，来源和版本记录在
  `bin/lib/freetype-woff2/README.txt`。

### 运行时组件

| 项目 | 用途 | 许可 |
|---|---|---|
| [Pillow](https://github.com/python-pillow/Pillow) | 图像、字体和 framebuffer 渲染 | MIT-CMU |
| [lxml](https://github.com/lxml/lxml) | 章节 HTML 解析 | BSD |
| [python-evdev](https://github.com/gvalkov/python-evdev) | Kindle 触摸与按键事件 | BSD |

完整第三方说明见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

## 许可证

KinNovel 使用 GPLv3 发布。LightNovelShelf 内容、封面、正文和字体归原站及
对应权利人所有。使用前请阅读并遵守站点规则和内容许可。
