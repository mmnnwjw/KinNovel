排除全部漫画：书架 / 阅读历史 / 排行榜 / 最近 只显示小说。

### 变更

- **排行榜**：`GetRank` 返回的混合列表中过滤 `Type=Comic`。实测日榜 48 条中 44 本小说、4 本漫画，现在只显示 44 本小说。
- **最近/分类**：`GetBookList` 的每条结果按 `Type` 过滤漫画（服务端该接口不支持类型参数，只能客户端过滤）。
- **书架**：`GetBookShelf` 的 `type=COMIC` 条目不再显示，小说与文件夹保留。
- **阅读历史**：继续只读取 `GetReadHistory` 的 `Novel` 列表，漫画本就独立在 `Comic` 字段，不会出现。
- **系列 / 按 ID 批量取书**：同样统一过滤漫画，避免从系列页或历史进入漫画。
- 过滤集中在 `bin/src/kinnovel/api.py`（`is_comic` / `novel_items` / `_novel_data`），各页面无需重复判断，缺失 `Type` 字段时按小说保留。

### 测试

- Kindle 实机（Python 3.14）运行 **119 项单元测试全部通过**（1 项跳过），新增漫画过滤用例覆盖 `Type`/`type` 两种字段写法与裸数组返回。
- 在线探测：排行榜原始 48 条（44 小说 + 4 漫画）→ 过滤后 44 条、漫画 0 条；最近 10 条、书架 5 条均无漫画。
- 实测应用启动至「正在监听触摸输入」无异常，`SIGTERM` 后干净退出并恢复原屏。

## 安装

1. 下载 `KinNovel-v0.7.2.zip`。
2. 解压后把 `KinNovel` 目录复制到 `/mnt/us/extensions/kinnovel`。
3. 在 `/mnt/us/extensions/kinnovel/bin/config.json` 中填写 `account_email` 和 `account_password`。
4. 从 KUAL 进入 `KinNovel` 并启动。

完整配置和排障说明见 README.md。
