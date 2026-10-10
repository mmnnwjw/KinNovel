<div align="center">

# KinNovel

**轻书架 Kindle 客户端**

面向电子墨水屏的原生阅读器 · 单一静态程序（3.7 MB）· 仅依赖 KUAL

[下载最新版](https://github.com/mmnnwjw/KinNovel/releases/latest) &nbsp;·&nbsp; [安装](#安装) &nbsp;·&nbsp; [手势](#手势) &nbsp;·&nbsp; [离线](#离线与网络) &nbsp;·&nbsp; [配置](#配置)

<br>

<img src="docs/images/shelf.png" width="30%" alt="书架">&nbsp;
<img src="docs/images/reader.png" width="30%" alt="阅读">&nbsp;
<img src="docs/images/preview.png" width="30%" alt="插图预览">

<sub>Kindle Paperwhite 5 实机截屏，内容来自设备本地阅读缓存</sub>

</div>

<br>

KinNovel 是 [轻书架](https://www.lightnovel.life) 的 Kindle 客户端，运行于已越狱的 Kindle，通过 KUAL 启动。支持书架、阅读历史、排行、分类、书籍详情、评论、公告、通知与签到；章节、字体与插图按需下载并缓存在本地，离线状态下可继续阅读已缓存内容。

程序不依赖浏览器或 KOReader，直接驱动屏幕：排版、刷新区域与波形均由程序自行决定。在 Kindle Paperwhite 5 上，翻页耗时约 11 ms；页面切换时执行一次全屏刷新以清除残影。

<br>

## 界面

<table>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/images/reader.png" alt="阅读页"><br>
      <b>正文</b><br>
      <sub>使用站点下发的章节字体，遵循中文标点禁则。顶栏显示章节名、时间与电量、页码，底部为章内进度线。首页优先显示，其余页面在空闲时分页。</sub>
    </td>
    <td width="50%" valign="top">
      <img src="docs/images/reader-menu.png" alt="阅读菜单"><br>
      <b>菜单</b><br>
      <sub>点击屏幕中部或下滑呼出，提供换章、目录、设置、字号与日夜模式切换。从设置返回后按新参数重新排版，并保持当前阅读位置。</sub>
    </td>
  </tr>
  <tr>
    <td valign="top">
      <img src="docs/images/reader-image.png" alt="整页插图"><br>
      <b>插图</b><br>
      <sub>封面、彩页等插图按整页排版。点击图片区域进入全屏预览；图片以外的区域仍用于翻页。</sub>
    </td>
    <td valign="top">
      <img src="docs/images/preview-zoom.png" alt="插图放大"><br>
      <b>预览</b><br>
      <sub>默认适配全屏，可放大 1.5–4 倍并滑动平移。「原图」加载未经 CDN 缩放的原始资源，替换后保持当前缩放与视野。</sub>
    </td>
  </tr>
  <tr>
    <td valign="top">
      <img src="docs/images/chapters.png" alt="目录"><br>
      <b>目录</b><br>
      <sub>可在阅读中打开，标记当前章节，点击条目跳转。</sub>
    </td>
    <td valign="top">
      <img src="docs/images/settings.png" alt="设置"><br>
      <b>设置</b><br>
      <sub>分为阅读、显示、内容三组；修改即时生效并写入配置文件。</sub>
    </td>
  </tr>
</table>

此外还包括书籍详情、系列、评论、公告、消息通知、积分商城与每日签到等页面。

<br>

## 刷新策略

电子墨水屏需要在残影与闪烁之间取舍。KinNovel 按场景选择刷新方式：

| 场景 | 刷新方式 |
|:--|:--|
| 翻页 | 全屏 REAGL，无闪烁；PW5 及更新机型启用原生翻页动画 |
| 进入 / 返回页面、换章、日夜切换 | 全屏闪刷一次 |
| 关闭弹窗 | 仅对弹窗区域闪刷 |
| 列表翻页、按钮状态 | 仅刷新变化区域；累计刷新面积达到阈值后执行一次全屏清理 |
| 按压反馈 | 反相 + A2 波形 |

屏幕驱动基于 [FBInk](https://github.com/NiLuJe/FBInk)，各机型的波形与刷新接口由其适配。设备休眠时程序释放屏幕，唤醒后完整重绘。

<br>

## 安装

> 前提：已越狱的 Kindle（固件 5.x）与 [KUAL](https://www.mobileread.com/forums/showthread.php?t=203326)。

1. 从 [Releases](https://github.com/mmnnwjw/KinNovel/releases/latest) 下载 `KinNovel-vX.Y.Z.zip` 并解压。
2. 通过 USB 连接 Kindle，将 `KinNovel` 文件夹**内的全部内容**复制到 `extensions/kinnovel/`。
3. （可选）如需使用云端书架、阅读历史与通知，在 `extensions/kinnovel/bin/config.json` 中填写账号：

   ```json
   { "account_email": "you@example.com", "account_password": "••••••" }
   ```

4. 在 KUAL 中选择 **KinNovel**。

升级时直接覆盖安装，配置、阅读进度与缓存均会保留。从 0.8（Python 版）升级时，原有的 `bin/src`、`bin/lib`、`bin/vendor` 等运行时文件可手动删除，或通过 SSH 运行包内的 `install.sh` 自动清理。

<br>

## 手势

| 阅读时 | |
|:--|:--|
| 点右侧 · 左滑 · 翻页键「前」 | 下一页，章末进入下一章 |
| 点左侧 · 右滑 · 翻页键「后」 | 上一页，章首回到上一章末页 |
| 点中间 · 下滑 | 呼出菜单 |
| 上滑 · 点菜单外 | 收起菜单 |
| 点击插图 | 全屏预览 |

| 插图预览 | |
|:--|:--|
| 放大 / 缩小 · 翻页键 | 切换缩放 |
| 放大后滑动 | 平移半屏 |
| 点击图片 | 显示 / 隐藏工具栏 |

列表页支持上下、左右滑动翻页。通过「我的 → 退出」退出程序，系统界面将自动恢复。

<br>

## 离线与网络

- **本地缓存**：章节、字体、封面与插图写入磁盘缓存，离线时可继续阅读；缓存超出上限时按最近使用时间清理。
- **服务器故障降级**：请求出现网络错误、超时或 502–504 时，程序将服务器标记为暂不可用，此后直接使用本地缓存，书架显示本地列表，并以递增间隔（30 秒起，最长 5 分钟）重新探测；任一请求成功即恢复。
- **过期缓存**：缓存超过 12 小时的章节先行显示，同时在后台下载新版本，下次打开时生效。
- **阅读进度上传**：在退出阅读、设备休眠时上传；换章时以低优先级上传；打开目录或插图预览时不上传；位置未变化时不重复上传。所有请求遵守站点频率限制（5.5 秒内最多 9 次）。

<br>

## 配置

配置文件位于 `extensions/kinnovel/bin/config.json`，多数选项可在「设置」页面中修改。

| 键 | 作用 | 默认 |
|:--|:--|:--|
| `account_email` · `account_password` | 账号，启动时自动登录（明文保存在设备上） | 空 |
| `font_size` · `line_spacing` · `reader_margin` | 字号 · 行距 · 左右边距 | `48` · `1.42` · `34` |
| `first_line_indent` | 首行缩进 | `true` |
| `convert` | 简繁转换：`null` · `"t2s"` · `"s2t"` | `null` |
| `prefetch_chapters` | 后台预加载前后章节 | `false` |
| `night_mode` | 夜间模式 | `false` |
| `page_turn_animation` | 翻页动画（PW5 及更新机型） | `true` |
| `page_flash` · `full_refresh_every` | 每次翻页闪刷 · 阅读时残影清理间隔（屏） | `false` · `6` |
| `ignore_japanese` · `ignore_ai` | 列表中隐藏日文 / AI 翻译作品 | `false` |
| `cache_limit_mb` | 缓存上限 | `192` |
| `font_path` | 界面字体与正文回退字体 | `STHeitiMedium.ttf` |
| `api_server` · `strict_tls` | 接口地址 · 严格校验证书 | 官方地址 · `true` |

<details>
<summary><b>故障排查</b></summary>

<br>

- 日志文件：`extensions/kinnovel/logs/kinnovel.log`。
- 提示程序已在运行但界面未出现：删除 `/tmp/kinnovel.lock` 后重新启动。
- 正文显示为乱码：该章节字体尚未下载，联网打开一次即可。
- 残影较多：在「设置 → 显示」中减小清理间隔，或开启「翻页全屏刷新」。

</details>

<br>

## 现状

- 已在 **Kindle Paperwhite 5（固件 5.17.1）** 上完成实机验证：阅读、翻页与动画、插图与预览、菜单、设置、日夜切换、休眠与唤醒、退出恢复，以及服务器故障时的降级。其他机型由 FBInk 适配，尚未实机验证，欢迎反馈。
- 轻书架接口服务器目前处于故障状态。联网功能依据接口约定实现并以合成数据测试，尚未与线上服务联调；如遇联网问题，请提交 Issue。
- 书架编辑（加入 / 移出书架、文件夹）尚未实现。

<br>

## 从源码构建

```bash
git clone --recursive https://github.com/mmnnwjw/KinNovel.git
cd KinNovel/rust
./build.sh kindle       # armv7 静态二进制（cargo-zigbuild）
./build.sh host-test    # 单元测试
./build.sh host         # 主机预览：kinnovel --preview out.pgm [--tap X,Y]…
python ../tools/package_release.py
```

项目由六个 crate 组成：`kn-render`（位图、字体、图片解码）、`kn-platform`（FBInk 屏幕、触摸、电源）、`kn-ui`（页面、刷新调度、后台任务）、`kn-text`（HTML 解析、禁则断行、分页、阅读位置）、`kn-net`（SignalR 客户端、限流、服务器健康状态）、`kn-app`（各个页面与本地存储）。设计文档见 [`rust/DESIGN.md`](rust/DESIGN.md) 与 [`rust/UI-DESIGN.md`](rust/UI-DESIGN.md)；0.8 版 Python 实现保留在 [`legacy`](https://github.com/mmnnwjw/KinNovel/tree/legacy) 分支。

<br>

## 致谢

[LightNovelShelf/Web](https://github.com/LightNovelShelf/Web) —— 接口约定、阅读位置与章节字体模型 &nbsp;·&nbsp;
[FBInk](https://github.com/NiLuJe/FBInk) —— 电子墨水屏驱动（GPL-3.0，静态链接） &nbsp;·&nbsp;
[KOReader](https://github.com/koreader/koreader) 与 [kComics](https://github.com/lxdklp/kComics) —— 电源事件、刷新策略与 KUAL 启动流程的参考

第三方组件及许可见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

<br>

<div align="center">
<sub>KinNovel 由 Claude Opus 5.5 与 Claude Sonnet 5.5 编写 &nbsp;·&nbsp; 以 <a href="LICENSE">GPLv3</a> 发布</sub><br>
<sub>轻书架的内容、封面、正文与字体归原站及权利人所有</sub>
</div>
