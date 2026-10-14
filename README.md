<div align="center">

# KinNovel

在已越狱的 Kindle 上阅读轻书架的小说与漫画

[![最新版本](https://img.shields.io/github/v/release/mmnnwjw/KinNovel?label=%E6%9C%80%E6%96%B0%E7%89%88%E6%9C%AC)](https://github.com/mmnnwjw/KinNovel/releases/latest)
[![构建](https://img.shields.io/github/actions/workflow/status/mmnnwjw/KinNovel/ci.yml?branch=main&label=%E6%9E%84%E5%BB%BA)](https://github.com/mmnnwjw/KinNovel/actions/workflows/ci.yml)
[![许可](https://img.shields.io/github/license/mmnnwjw/KinNovel?label=%E8%AE%B8%E5%8F%AF)](LICENSE)
![平台](https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Kindle%20%2B%20KUAL-informational)

<img src="docs/images/shelf.png" width="31%" alt="书架：继续阅读卡片与云端书架">
<img src="docs/images/reader-bookmark.png" width="31%" alt="阅读菜单：刚添加了书签">
<img src="docs/images/reader-image.png" width="31%" alt="阅读器里插图与正文混排">

</div>

## 关于

KinNovel 是 [轻书架](https://www.lightnovel.life) 的非官方 Kindle 客户端。它直接运行在 Kindle 的原生系统里，通过 KUAL 启动。

## 功能

- **小说**：字号、行距、段间距、页边距可调，简繁转换，插图点开可全屏放大。
- **书签与跳页**：菜单里一键加书签，在「目录与书签」里查看；拖动进度条可在章内快速跳页。
- **漫画**：支持跨页和从右到左翻页，后台预取后面几页。
- **书架**：显示云端书架和文件夹；长按可以移出书架，在详情页可以加入书架。
- **找书**：浏览最新、排行和分类，也可以用屏幕键盘拼音输入来搜索。
- **离线阅读**：章节、字体和图片都会缓存，网络断开或站点暂时不可用时照样能读。
- **阅读统计**：记录每天的阅读时长和页数、连续阅读天数，以及读得最多的书。统计只保存在本机。
- **更多**：阅读时可调前光和色温，可选休眠时显示当前书的封面，「存储空间」页可清理缓存，有新版本时会提示。

## 安装

### 需要

- 已越狱、装有 [KUAL](https://www.mobileread.com/forums/showthread.php?t=203326) 的 Kindle，固件 5.x。
- 目前只在 Kindle Paperwhite 5 上实机测试过。其他机型按 KOReader 的机型表做了适配，欢迎反馈使用情况。

### 步骤

1. 从 [Releases](https://github.com/mmnnwjw/KinNovel/releases/latest) 下载 `KinNovel-vX.Y.Z.zip` 并解压。
2. 用 USB 连接 Kindle，把 `KinNovel` 文件夹里的全部内容复制到 `extensions/kinnovel/`。
3. 如需同步云端书架和进度，在 `extensions/kinnovel/bin/config.json` 中填写账号：

   ```json
   { "account_email": "you@example.com", "account_password": "••••••" }
   ```

4. 在 KUAL 中选择 **KinNovel**。

升级时直接覆盖安装即可，配置、进度、书签和缓存都会保留。

## 使用

| 操作 | 效果 |
|:--|:--|
| 点屏幕右侧 / 左滑 | 下一页 |
| 点屏幕左侧 / 右滑 | 上一页 |
| 点屏幕中间 / 下滑 | 打开阅读菜单（目录与书签、字号、亮度、设置） |
| 菜单顶栏的书签图标 | 给当前页加书签，或删除这一页的书签 |
| 拖动菜单里的进度条 | 跳到本章任意一页 |
| 点插图 | 全屏查看并放大 |
| 在「目录与书签」里长按书签 | 删除书签 |
| 长按书架上的书 / 文件夹 | 移出书架 / 删除文件夹 |
| 列表页上滑 / 下滑 | 下一页 / 上一页 |
| 「我的 → 退出」 | 退出并回到 Kindle 原来的界面 |

搜索时用屏幕键盘输入：

| 操作 | 效果 |
|:--|:--|
| 「中 / 英」键 | 切换中文（拼音）和英文 |
| 打拼音后点候选词 | 输入这个词；按空格、回车或标点输入第一个候选 |
| 候选词较多时按 ← → | 翻看更多候选 |
| 「符号」键 | 切换到标点符号，Shift 换一页 |
| 在 ， 。 键上向上滑 | 输入 ； ： |
| 长按退格 / 长按 Shift | 清空输入 / 锁定大写 |

其他选项都在「我的 → 设置」里，也可以直接编辑 `config.json`。「休眠显示封面」重启 KinNovel 后生效。

### 遇到问题

- 程序日志在 `extensions/kinnovel/logs/kinnovel.log`。异常退出时，下次启动会提示一次，详细信息也写在这个日志里。
- 问题和建议请提交到 [Issues](https://github.com/mmnnwjw/KinNovel/issues)，并附上日志。

## 参与开发

项目用 Rust 编写。设计说明见 [`rust/DESIGN.md`](rust/DESIGN.md)，各页面的界面规范见 [`rust/UI-DESIGN.md`](rust/UI-DESIGN.md)，各机型的差异见 [`rust/research-kindle-devices.md`](rust/research-kindle-devices.md)。

```bash
git clone --recursive https://github.com/mmnnwjw/KinNovel.git
cd KinNovel/rust
./build.sh host-test    # 运行测试
./build.sh kindle       # 构建 Kindle 程序 (armv7, 静态链接)
./build.sh host         # 主机预览程序: kinnovel --preview out.pgm
```

构建需要 Rust、[cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild)，以及 pip 安装的 `ziglang`。推送 `vX.Y.Z` 标签后，GitHub Actions 会自动构建并发布 Release。

早先的 Python 版本保存在 [`legacy`](https://github.com/mmnnwjw/KinNovel/tree/legacy) 分支。

欢迎提交 Pull Request。

## 计划

- [ ] 在书架里新建文件夹，以及在文件夹之间移动书。

## 维护者与致谢

由 [@mmnnwjw](https://github.com/mmnnwjw) 维护，代码由 Claude Opus 5.5 与 Claude Sonnet 5.5 编写。

- 接口与阅读模型参考 [LightNovelShelf/Web](https://github.com/LightNovelShelf/Web)。
- 屏幕驱动来自 [FBInk](https://github.com/NiLuJe/FBInk)。
- 机型适配、刷新策略、屏幕键盘和休眠封面参考 [KOReader](https://github.com/koreader/koreader) 与 [kComics](https://github.com/lxdklp/kComics)。
- 拼音词库来自 [rime-pinyin-simp](https://github.com/rime/rime-pinyin-simp)。

第三方组件及其许可见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

## 许可

以 [GPLv3](LICENSE) 发布。轻书架上的内容、封面与字体归原站及权利人所有。
