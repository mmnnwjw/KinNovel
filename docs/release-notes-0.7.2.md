排除全部漫画：书架 / 阅读历史 / 排行榜 / 最近只显示小说。

### 变更

- **排行榜**：`GetRank` 返回的混合列表中过滤 `Type=Comic`；实测日榜 48 条（44 本小说 + 4 本漫画）现在只显示 44 本小说。
- **最近/分类**：`GetBookList` 结果按 `Type` 过滤漫画（该接口不支持服务端类型参数，只能客户端过滤）。
- **书架**：`GetBookShelf` 的 `type=COMIC` 条目不再显示，小说与文件夹保留。
- **阅读历史**：继续只读取 `GetReadHistory` 的 `Novel` 列表，漫画本就独立在 `Comic` 字段。
- **系列与按 ID 取书**：同样统一过滤漫画，避免从系列页或历史进入漫画。
- **过滤位置**：集中在 `bin/src/kinnovel/api.py`（`is_comic` / `novel_items` / `_novel_data`），缺失 `Type` 字段时按小说保留，避免误杀。

## 安装

1. 下载 `KinNovel-v0.7.2.zip`。
2. 解压后把 `KinNovel` 目录复制到 `/mnt/us/extensions/kinnovel`。
3. 在 `/mnt/us/extensions/kinnovel/bin/config.json` 中填写 `account_email` 和 `account_password`。
4. 从 KUAL 进入 `KinNovel` 并启动。

完整配置和排障说明见 README.md。
