阅读排版性能大幅优化：分页最高提速约 20 倍，渲染结果逐像素一致。

### 变更

- **缺字判定改用 cmap 查询**：旧实现每个新字符都要 `getmask()` 栅格化一次，再单独画一张位图和字体 `.notdef` 逐字节比对（实测 1.4–2.4ms/字，占分页耗时约 91%）。现在通过 FreeType `FT_Get_Char_Index` 直接查 cmap，实测 0.011ms/字；原栅格化逻辑保留为库不可用时的兜底，v0.3.1 修掉的 STHeiti notdef 方框问题不会回归。
- **字体对象全局缓存**：`ImageFont.truetype` 结果按 (路径, 字号) 复用，不再每章重复解析 1MB 级 WOFF2，单章省约 450ms。
- **页面内容位图缓存**：正文画进独立位图并做 3 页 LRU，控件层显隐、提示浮层、翻页回看、插图到达都直接复用，不再每次重新栅格化整页文字；缓存键包含文档版本、页码、插图代际、分辨率与夜间模式，插图到达后自动失效重绘。
- **实机效果**：分页 ch1 1620→799ms（热 80ms）、ch2 276→44ms、ch3 7259→264ms；文本页渲染 128→13ms，含插图页 104→10ms，控件层切换 21/13ms。
- **渲染一致性**：旧/新分页 20 页 4100 万像素差异 0，缓存/未缓存差异 0，cmap 与栅格化判定 605 字无一处不一致。

## 安装

1. 下载 `KinNovel-v0.7.3.zip`。
2. 解压后把 `KinNovel` 目录复制到 `/mnt/us/extensions/kinnovel`。
3. 在 `/mnt/us/extensions/kinnovel/bin/config.json` 中填写 `account_email` 和 `account_password`。
4. 从 KUAL 进入 `KinNovel` 并启动。

完整配置和排障说明见 README.md。
