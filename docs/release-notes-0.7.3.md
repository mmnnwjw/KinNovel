阅读排版性能优化，打开章节、翻页和开关控件层都更快。

### 变更

- **打开章节更快**：优化字体缺字判断，长章节的排版时间大幅缩短。
- **翻页与菜单更快**：缓存已排版的页面内容，翻回上一页或开关控件层不再重新排版文字。
- **字体加载更快**：同一字体在多个章节间复用，减少重复加载。
- **显示效果不变**：优化前后渲染结果逐像素一致，正文与插图显示没有变化。

## 安装

1. 下载 `KinNovel-v0.7.3.zip`。
2. 解压后把 `KinNovel` 目录复制到 `/mnt/us/extensions/kinnovel`。
3. 在 `/mnt/us/extensions/kinnovel/bin/config.json` 中填写 `account_email` 和 `account_password`。
4. 从 KUAL 进入 `KinNovel` 并启动。

完整配置和排障说明见 README.md。