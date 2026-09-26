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

当前版本：`0.4.0`

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
| 书籍 | 书籍详情、简介、标签、章节列表和章节分页 |
| 阅读 | 章节正文、上一章/下一章、章节目录、阅读进度和阅读历史 |
| 缓存 | 章节正文磁盘缓存与离线回退；可选预加载前后各一章 |
| 排版 | 自动分页、字号、行距、首行缩进、标点禁则、简繁转换和夜间模式 |
| 字体 | 内置 WOFF2 FreeType 运行时；加载章节字体；缺失字形自动使用系统字体补足 |
| 图片 | 正文插图、服务端缩放图、全章插图预取、点击全屏预览和再次点击退出 |
| 书架 | 多层文件夹、翻页、加入书架、移出书架和删除文件夹 |
| 账号功能 | 个人资料、签到、通知、公告、评论浏览和商城 |
| 显示 | MTK 与 MXCFB EPDC 自动探测输出、多分辨率界面适配、原屏快照恢复和系统进程暂停/恢复 |
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

1. 下载 Release 中的 `KinNovel-v0.4.0.zip`。
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
6. 打开 KUAL，进入 `KinNovel`，点击 `KinNovel` 启动。

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

### 安装脚本

如果安装包已完整复制到 `/mnt/us/extensions/kinnovel`：

```sh
/bin/sh /mnt/us/extensions/kinnovel/install.sh
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
  "night_mode": false,
  "justify": false,
  "first_line_indent": true,
  "convert": null,
  "ignore_japanese": false,
  "ignore_ai": false,
  "prefetch_chapters": false,
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
| `screen_protocol` | EPDC 协议；`auto` 自动探测 `mtk`、`mxcfb` | `auto` |
| `framebuffer` | Kindle framebuffer 路径 | `/dev/fb0` |
| `page_flash` | 翻页时是否强制全屏刷新 | `false` |
| `night_mode` | 是否使用黑白反转的夜间模式 | `false` |

屏幕无法显示或刷新异常时，可手动指定：

```json
{"screen_protocol": "mtk"}
```

```json
{"screen_protocol": "mxcfb"}
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

- 点击左右边缘：上一页或下一页。
- 点击顶部返回图标：返回上一页。
- 点击顶部主页图标：返回主页。
- 点击正文插图：进入全屏预览。
- 全屏预览中再次点击：退出预览。
- 底栏：上一章、章节目录、设置、下一章。

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

依次尝试：

```json
{"screen_protocol": "mtk"}
```

```json
{"screen_protocol": "mxcfb"}
```

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

### KOReader

- 项目：<https://github.com/koreader/koreader>
- 用途：休眠与电源事件调度机制参考；提供章节 WOFF2 字体所需的 FreeType/Brotli 运行时。
- 使用方式：
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
