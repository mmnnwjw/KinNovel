<div align="center">

# KinNovel

在已越狱的 Kindle 上阅读轻书架的小说与漫画

[![最新版本](https://img.shields.io/github/v/release/mmnnwjw/KinNovel?label=%E6%9C%80%E6%96%B0%E7%89%88%E6%9C%AC)](https://github.com/mmnnwjw/KinNovel/releases/latest)
[![许可](https://img.shields.io/github/license/mmnnwjw/KinNovel?label=%E8%AE%B8%E5%8F%AF)](LICENSE)
![平台](https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Kindle%20%2B%20KUAL-informational)

<img src="docs/images/shelf.png" width="24%" alt="书架">
<img src="docs/images/reader.png" width="24%" alt="阅读">
<img src="docs/images/reader-image.png" width="24%" alt="插图">
<img src="docs/images/settings.png" width="24%" alt="设置">

</div>

## 这是什么

KinNovel 是 [轻书架](https://www.lightnovel.life) 的非官方 Kindle 客户端。它直接运行在 Kindle 的原生系统中，通过 KUAL 启动，不需要浏览器或 KOReader。登录账号后，书架、阅读进度和历史会与网站同步。

## 为什么用它

- **为墨水屏设计**：大号按钮，翻页尽量不闪屏，只在需要清除残影时整屏刷新。
- **小说与漫画都能读**：插图可全屏放大，漫画支持跨页和从右到左翻页。
- **离线也能读**：读过的章节、字体和图片都存在本地，没有网络或站点暂时不可用时照样能翻。
- **在 Kindle 上直接找书**：可以浏览排行和分类，也可以用屏幕键盘拼音输入来搜索。
- **装好就能用**：只有一个程序文件，不依赖 Python 等运行环境。

## 快速开始

需要一台已越狱、装有 [KUAL](https://www.mobileread.com/forums/showthread.php?t=203326) 的 Kindle（固件 5.x）。

1. 从 [Releases](https://github.com/mmnnwjw/KinNovel/releases/latest) 下载最新的 `KinNovel-vX.Y.Z.zip` 并解压。
2. 用 USB 连接 Kindle，把 `KinNovel` 文件夹里的全部内容复制到 `extensions/kinnovel/`。
3. 如需同步云端书架和进度，在 `extensions/kinnovel/bin/config.json` 中填写账号：

   ```json
   { "account_email": "you@example.com", "account_password": "••••••" }
   ```

4. 在 KUAL 中选择 **KinNovel**。

升级时直接覆盖安装，配置、进度和缓存都会保留。

### 基本操作

| 操作 | 效果 |
|:--|:--|
| 点屏幕右侧 / 左滑 | 下一页 |
| 点屏幕左侧 / 右滑 | 上一页 |
| 点屏幕中间 | 打开阅读菜单（目录、字号、亮度、设置） |
| 点插图 | 全屏查看并放大 |
| 「我的 → 退出」 | 退出并回到 Kindle 原来的界面 |

其他选项都在「设置」页中，也可以直接编辑 `config.json`。

## 获取帮助

- 程序日志在 `extensions/kinnovel/logs/kinnovel.log`，反馈问题时请附上。
- 问题与建议请提交到 [Issues](https://github.com/mmnnwjw/KinNovel/issues)。
- 目前只在 Kindle Paperwhite 5 上做过实机测试，欢迎反馈其他机型的使用情况。

## 参与开发

项目使用 Rust 编写，设计说明见 [`rust/DESIGN.md`](rust/DESIGN.md) 和 [`rust/UI-DESIGN.md`](rust/UI-DESIGN.md)。从源码构建：

```bash
git clone --recursive https://github.com/mmnnwjw/KinNovel.git
cd KinNovel/rust
./build.sh kindle       # 构建 Kindle 程序
./build.sh host-test    # 运行测试
python ../tools/package_release.py   # 打包发布用的 zip
```

早先的 Python 版本保存在 [`legacy`](https://github.com/mmnnwjw/KinNovel/tree/legacy) 分支。欢迎提交 Pull Request。

## 维护者与致谢

由 [@mmnnwjw](https://github.com/mmnnwjw) 维护，代码由 Claude Opus 5.5 与 Claude Sonnet 5.5 编写。

接口与阅读模型参考 [LightNovelShelf/Web](https://github.com/LightNovelShelf/Web)，屏幕驱动来自 [FBInk](https://github.com/NiLuJe/FBInk)，机型适配、刷新方式和屏幕键盘参考了 [KOReader](https://github.com/koreader/koreader) 与 [kComics](https://github.com/lxdklp/kComics)，拼音词库来自 [rime-pinyin-simp](https://github.com/rime/rime-pinyin-simp)。第三方组件及许可见 [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)。

## 许可

以 [GPLv3](LICENSE) 发布。轻书架上的内容、封面与字体归原站及权利人所有。
