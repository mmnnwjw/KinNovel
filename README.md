# KinNovel

KinNovel 是运行在已越狱 Kindle 上的轻书架（LightNovelShelf）阅读客户端。
项目使用 Python 3.14、Pillow、evdev 和 Kindle EPDC framebuffer，界面运行在
Kindle 原生系统上，不依赖浏览器、Qt 或桌面环境。

当前版本：`0.2.0`

## 功能

- 启动时读取配置中的账号密码并自动登录。
- 小说排行榜、最近更新、分类、阅读历史和书籍详情。
- 章节列表分页、正文自动分页、上一章/下一章和章节目录。
- 使用服务端章节字体渲染正文，支持 Kindle 上的 WOFF2 章节字体。
- 夜间模式、字号、行距、首行缩进和简繁转换设置。
- 正文图片显示；点击图片进入全屏预览，再次点击退出。
- 书架浏览、打开文件夹、翻页、加入书架、移出书架和删除文件夹。
- 公告、评论浏览、通知、签到和商城。
- 可配置主页模块顺序和显示状态。

## 当前不包含的功能

- 文字输入和触屏输入法。
- 搜索、登录表单、注册、找回密码、评论发表、私信和文件夹命名。
- 漫画图片阅读器。漫画接口和页面未包含在当前版本中。
- 上传、发布、编辑、删除、下载和导出。
- 论坛和社区。

登录凭据固定从配置文件读取。请勿把填写了账号密码的配置文件提交到公开仓库。

## 环境要求

1. 已越狱的 Kindle。
2. Kindle 系统版本 `5.16.3` 或更高。
3. 已安装 KUAL。
4. 已安装 `πthon (HF) for Kindle`，默认路径为：

```text
/mnt/us/python3/bin/python3.14
```

如果还没有 Python，可参考 kComics 的安装方式：

- MRPI：把 `Update_install_python3.bin` 放到 `/mnt/us/mrpackages/`，在 Kindle 搜索框输入 `;log mrpi`。
- KPM：执行：

```text
;kpm install file:///mnt/us/python3_3.14.3_kindlehf.kpkg
```

## 安装

### 方式一：使用 Release 压缩包

1. 下载 Release 中的 `KinNovel-v0.2.0.zip`。
2. 解压得到 `KinNovel` 文件夹。
3. 使用 USB 或 SFTP 复制到 Kindle：

```text
/mnt/us/extensions/kinnovel/
```

4. 确认入口文件存在：

```text
/mnt/us/extensions/kinnovel/bin/start.sh
/mnt/us/extensions/kinnovel/config.xml
/mnt/us/extensions/kinnovel/manifest.json
/mnt/us/extensions/kinnovel/menu.json
```

5. 如果权限被复制工具修改，通过 Kindle SSH 执行：

```sh
chmod +x /mnt/us/extensions/kinnovel/bin/start.sh
```

6. 打开 KUAL，进入 `KinNovel`，点击 `KinNovel` 启动。

### 方式二：SSH 手动安装

把整个项目复制到 Kindle：

```sh
mkdir -p /mnt/us/extensions/kinnovel
cp -R KinNovel/bin /mnt/us/extensions/kinnovel/
cp KinNovel/config.xml KinNovel/manifest.json KinNovel/menu.json \
   /mnt/us/extensions/kinnovel/
chmod +x /mnt/us/extensions/kinnovel/bin/start.sh
```

启动：

```sh
/bin/sh /mnt/us/extensions/kinnovel/bin/start.sh
```

### 方式三：项目内安装脚本

如果项目已经完整复制到 Kindle，可执行：

```sh
/bin/sh /mnt/us/extensions/kinnovel/install.sh
```

## 配置

配置文件位置：

```text
/mnt/us/extensions/kinnovel/bin/config.json
```

默认配置：

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

### 账号配置

```json
{
  "account_email": "user@example.com",
  "account_password": "your-password"
}
```

应用启动后自动使用这两个字段登录，不显示登录界面。`account_password` 是明文
存储，只应保存在个人 Kindle 上，不能提交到 Git 或公开分享。

### 显示配置

| 配置项 | 说明 | 默认值 |
|---|---|---|
| `screen_protocol` | EPDC 协议。`auto` 会依次尝试 `mtk`、`mxcfb` | `auto` |
| `framebuffer` | Kindle framebuffer 路径 | `/dev/fb0` |
| `page_flash` | 翻页时是否强制全屏刷新 | `false` |
| `night_mode` | 是否反转黑白显示 | `false` |

如果启动后屏幕无显示或刷新异常，可依次尝试：

```json
{"screen_protocol": "mtk"}
```

```json
{"screen_protocol": "mxcfb"}
```

### 阅读配置

| 配置项 | 说明 | 默认值 |
|---|---|---|
| `font_size` | 正文字号，可在设置页用 `-` / `+` 调整 | `48` |
| `line_spacing` | 正文行距，可在设置页调整 | `1.42` |
| `reader_margin` | 正文左右边距（像素） | `34` |
| `font_path` | Kindle 系统中文字体 | `/usr/java/lib/fonts/STHeitiMedium.ttf` |
| `first_line_indent` | 段落首行缩进 | `true` |
| `justify` | 是否两端对齐 | `false` |
| `convert` | 简繁转换：`null`、`t2s`、`s2t` | `null` |
| `ignore_japanese` | 列表过滤日文作品 | `false` |
| `ignore_ai` | 列表过滤 AI 作品 | `false` |

章节正文会优先使用服务端返回的章节字体。Kindle 上需要 KOReader 自带的
FreeType 支持 WOFF2；启动脚本默认检测：

```text
/mnt/us/koreader/libs/libfreetype.so.6
```

如果没有 KOReader，正文会回退到 `font_path` 指定的系统字体。

### 主页模块顺序

`home_order` 控制主页模块：

- `-1`：隐藏该模块。
- `0`、`1`、`2` 等：按数值从小到大排列。
- 排列方向为从左到右、从上到下。
- 数值相同时按内置顺序排列。

可配置模块：

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

例如，只保留书架和最近/分类：

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

### 网络和缓存

| 配置项 | 说明 | 默认值 |
|---|---|---|
| `api_server` | LightNovelShelf API 地址 | `https://api.lightnovel.life` |
| `request_limit` | 时间窗口内允许的请求数量 | `9` |
| `request_window_ms` | 请求限流窗口，单位毫秒 | `5500` |
| `cache_limit_mb` | 封面、字体、图片和正文缓存总上限 | `192` |
| `strict_tls` | 是否严格验证 HTTPS 证书 | `true` |

只有设备缺少 CA 证书且日志明确报告 TLS 证书错误时，才建议临时设置：

```json
{"strict_tls": false}
```

### 更新检查

```json
{"check_update": true}
```

当前版本保留配置字段，但更新检查模块未包含在发布包内。

## 使用说明

### 启动

1. 打开 KUAL。
2. 进入 `KinNovel`。
3. 点击 `KinNovel`。
4. 应用会暂停占用 framebuffer 的系统进程、保存原屏幕并启动。
5. 退出应用后，原屏幕和系统进程会恢复。

### 书库和排行榜

- 最近/分类支持上一页、下一页和页码显示。
- 排行榜支持日榜、周榜、月榜以及上一页、下一页。
- 列表页每页显示数量会根据屏幕高度计算。

### 章节跳转

- 在书籍详情中选择第 N 章：从第 N 章第一页开始阅读。
- 在第 N 章第一页继续向左：进入第 N-1 章最后一页。
- 点击阅读器中的章节目录：点选章节后从该章第一页开始。

### 阅读器

- 点击屏幕左右边缘：上一页或下一页。
- 点击顶部主页图标：返回主页。
- 点击顶部返回图标：返回上一页。
- 点击正文插图：进入全屏预览。
- 在全屏预览中点击任意位置：退出预览。
- 底部按钮：上一章、章节目录、设置、下一章。

### 书架

- 点击文件夹进入下一层。
- 点击书籍进入详情。
- 长按项目可移出书籍或删除文件夹。
- 底部分页可浏览超过一屏的书架内容。

## 日志和排障

日志位置：

```text
/mnt/us/extensions/kinnovel/logs/kinnovel.log
```

### 应用无法启动

检查 Python：

```sh
/mnt/us/python3/bin/python3.14 --version
```

检查启动脚本：

```sh
chmod +x /mnt/us/extensions/kinnovel/bin/start.sh
```

### 应用残留锁

异常退出后如果无法再次启动，删除：

```sh
rm -rf /tmp/kinnovel.lock
```

### 屏幕刷新异常

修改 `screen_protocol`：

- Paperwhite 4 等 MTK 机型：`"mtk"`
- 常见 i.MX 机型：`"mxcfb"`

### 章节字体显示异常

确认日志中是否加载了 KOReader FreeType：

```text
/mnt/us/koreader/libs/libfreetype.so.6
```

如果章节字体无法加载，应用会提示“章节字体加载失败，正文可能显示异常”。

### 网络错误

检查 `api_server` 是否可访问，并确认 Kindle 网络正常。可将 `strict_tls`
临时设为 `false` 做兼容测试，测试后应恢复为 `true`。

## 卸载

删除扩展目录：

```sh
rm -rf /mnt/us/extensions/kinnovel
```

或执行：

```sh
/bin/sh /mnt/us/extensions/kinnovel/uninstall.sh
```

卸载会同时删除缓存、日志和保存在设备上的账号配置。

## 开发

运行测试：

```sh
PYTHONPATH=bin/src python -m unittest discover -s tests -v
```

生成桌面预览：

```sh
python tools/render_preview.py
```

预览输出到：

```text
build/previews/
```

## 许可证

GPLv3。第三方组件和代码来源见 `LICENSE` 与 `THIRD-PARTY-NOTICES.md`。
