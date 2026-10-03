# KinNovel C++ 重构功能对齐矩阵 (PARITY.md)

本文档详细列出 C++17 版本与原有 Python 3.14 版本之间的功能与协议对齐状态。

## 一、状态图例

- **已完成**：代码已实现，逻辑完整，且在原生环境 / 自动化测试中完全通过验证。
- **部分**：基础功能已具备，部分边缘分支或非核心能力待增强。
- **未实现**：当前版本未移植（通常为开发调试工具或已弃用特性）。
- **未实机验证**：由于无物理 Kindle 硬件，该项在 Mock / 模拟后端已通过测试，但在 Kindle 真机硬件上仍需实机测试确认。

---

## 二、功能对齐总览

| 模块 / 特性 | Python 版本实现 | C++17 版本实现 | 对齐状态 | 备注 |
| :--- | :--- | :--- | :---: | :--- |
| **配置兼容性** | `kinnovel.config.Config` 读取 `config.json` | `kinnovel::core::Config` 基于 `yyjson` | **已完成** | 100% 字段名、数据类型与默认值完全一致 |
| **字符与文本处理** | UTF-8 编码、空白压缩、零宽隐形字符清洗 | `kinnovel::core::Charset` | **已完成** | 单元测试验证完全一致 |
| **简繁中文转换** | 内置繁简字符映射表 / OpenCC | `kinnovel::core::Charset::convertChinese` | **已完成** | 支持 `t2s`、`s2t` |
| **SHA-256 哈希计算** | Python `hashlib.sha256` | `kinnovel::core::Sha256` (自研纯 C++) | **已完成** | 标准测试向量校验一致 |
| **文件与磁盘工具** | 原子写、LRU 缓存修剪、电量读取 | `kinnovel::core::Utils` | **已完成** | 单测验证通过 |
| **字体引擎** | FreeType + libbrotli (Pillow ABI) | `kinnovel::reader::FontEngine` (FreeType2) | **已完成** | 逐字符字形检测与动态回退 |
| **WOFF2 字体解压** | KOReader libfreetype.so.6 / libbrotli | `kinnovel::reader::WoffNormalizer` | **已完成** | 章节字体自动规范化加载 |
| **HTML 解析与块提取** | `lxml.html` XPath 语义 | `kinnovel::reader::HtmlParser` | **已完成** | 提取段落、标题、插图并生成相对路径 |
| **排版与分页折行** | Pillow 文本测量 + 避头避尾规则 | `kinnovel::reader::LayoutEngine` | **已完成** | **100% 匹配 golden_wrap.json 黄金测试** |
| **阅读位置恢复** | 基于相对 XPath 定位与字符偏移 | `kinnovel::reader::ReaderDocument::pageForPath` | **已完成** | 黄金断行测试与翻页恢复一致 |
| **HTTP 传输与限流** | `urllib` / `requests` + 9次/5.5s 滑动窗口 | `kinnovel::network::HttpTransport` (libcurl) | **已完成** | 带连接池、超时控制、令牌桶限流 |
| **REST API 客户端** | 登录、刷新令牌、目录、排行榜、详情等 | `kinnovel::network::ApiClient` | **已完成** | 包含 20+ 个业务接口与请求签名 |
| **SignalR 实时通信** | WebSocket 握手 + SignalR 协议帧解析 | `kinnovel::network::SignalRClient` | **已完成** | 支持 JSON 协议实时推送通知 |
| **磁盘与内存缓存** | 封面双重缓存、章节离线缓存 | `kinnovel::ui::ImageCache` + SessionStore | **已完成** | 内存 LRU + 磁盘哈希命名缓存 |
| **2D 灰度画布绘制** | Pillow Image / ImageDraw | `kinnovel::ui::Canvas` | **已完成** | 线段、矩形、圆角框、圆形、位图贴图 |
| **FreeType 文本栅格化**| Pillow FT_Render_Glyph | `Canvas::drawText` / `drawRun` | **已完成** | 抗锯齿灰度渲染、fitText 截断折叠 |
| **多分辨率缩放** | 按 1072x1448 比例计算 FontSet | `kinnovel::ui::FontSet` | **已完成** | 覆盖 8 档字号与自适应间距 |
| **白天 / 夜间模式** | 配色表反相切换 | `kinnovel::ui::Theme` | **已完成** | 支持即时切换、按钮与底色同步联动 |
| **首页 (HomePage)** | 按 `home_order` 自定义九宫格入口 | `kinnovel::ui::HomePage` | **已完成** | 支持配置排序、未登录状态自动重定向 |
| **书架 (ShelfPage)** | 目录分层、长按置顶、书籍跳转 | `kinnovel::ui::ShelfPage` | **已完成** | 支持多级目录与离线本地缓存同步 |
| **分类浏览 (BrowsePage)** | 分类筛选、标签选择、分页加载 | `kinnovel::ui::BrowsePage` | **已完成** | 支持多种排序与关键字筛选 |
| **排行榜 (RankPage)** | 日榜 / 周榜 / 月榜切换 | `kinnovel::ui::RankPage` | **已完成** | 响应迅速，分页流式展示 |
| **书籍详情 (BookDetail)**| 封面预览、简介折叠、章节目录 | `kinnovel::ui::BookDetailPage` | **已完成** | 关联章节列表、自动预取正文 |
| **系列页面 (SeriesPage)**| 书籍关联系列作品列表 | `kinnovel::ui::SeriesPage` | **已完成** | 点击直达书籍详情 |
| **阅读历史 (HistoryPage)**| 历史阅读列表与进度百分比 | `kinnovel::ui::HistoryPage` | **已完成** | 实时显示最后阅读位置与时间戳 |
| **正文阅读器 (ReaderPage)**| 翻页、双击唤出菜单、全屏插图 | `kinnovel::ui::ReaderPage` | **已完成** | 插图点按全屏浏览、字号行距实时热调 |
| **章节目录 (CatalogPage)**| 卷结构树、快速跳转目标章节 | `kinnovel::ui::CatalogPage` | **已完成** | 支持跨卷跳读 |
| **系统设置 (SettingsPage)**| 字号/行距/首行缩进/夜间模式/清缓存 | `kinnovel::ui::SettingsPage` | **已完成** | 步进调节数值、清空缓存前二次确认 |
| **关于页面 (AboutPage)** | 版本信息、致谢与版权声明 | `kinnovel::ui::AboutPage` | **已完成** | 完整呈现项目许可证与参考项目致谢 |
| **账号中心 (AccountPage)**| 用户资料展示、每日签到 | `kinnovel::ui::AccountPage` | **已完成** | 自动登录凭据重试、经验值与金币展示 |
| **通知中心 (Notifications)**| 系统公告与站内信通知分页列表 | `kinnovel::ui::NotificationsPage` | **已完成** | 翻页阅读、未读高亮 |
| **商城中心 (ShopPage)** | 额度购买、金币消耗二次确认 | `kinnovel::ui::ShopPage` | **已完成** | 弹窗二次确认防误触 |
| **公告系统 (Announcements)**| 全站公告列表与详情阅读、评论区 | `kinnovel::ui::AnnouncementsPage` | **已完成** | 支持分页与评论互动 |
| **弹窗与交互组件** | Modal 确认弹窗、错误提示、Toast | `PageContext::confirm` / `toast` | **已完成** | 居中弹出、点击确定/取消动作闭环 |
| **单实例进程锁** | `/tmp/kinnovel.lock` 目录排他锁 | C++ 内置 `flock` 文件排他锁 | **已完成** | 防止多次启动导致屏幕 ioctl 冲突 |
| **电子墨水屏控制器 (EPDC)**| FBInk / ioctl 屏幕驱动 | `kinnovel::hal::FbinkDisplay` | **未实机验证** | 在 Linux x86_64 上通过 `MockDisplay` 验证 |
| **多点触摸驱动 (evdev)** | 读取 `/dev/input/event*` | `kinnovel::hal::EvdevInput` | **未实机验证** | 在 Linux x86_64 上通过 `MockInput` 验证 |
| **LIPC 电源与休眠调度** | LIPC 信号监听与看门狗 | `kinnovel::hal::LipcPowerManager` | **未实机验证** | 在 Linux x86_64 上通过 `MockPowerManager` 验证 |
| **帧缓冲快照保存/恢复** | `bin/fb_snapshot.py` | `kinnovel snapshot save/restore` (C++) | **已完成** | 支持自动快照和命令行快照管理 |
| **KUAL 启动脚本** | `bin/start.sh` (Python 启动) | `bin/start.sh` (优先启动 C++ 二进制) | **已完成** | 兼容现有 KUAL 菜单，保留 Python 降级能力 |
| **KUAL 发布打包** | Python `tools/package_release.py` | `tools/package_kual.sh` | **已完成** | 输出单一 ELF 与配置，体积从 25MB 缩减至 4.5MB |

---

## 三、配置文件 (`config.json`) 兼容性

C++ 版本与 Python 版本在 `config.json` 字段上保持 **100% 兼容**，支持无缝原地升级：

| 字段名称 | 类型 | 默认值 | 说明 |
| :--- | :---: | :---: | :--- |
| `api_server` | string | `"https://api.lightnovel.life"` | API 服务器基址 |
| `account_email` | string | `""` | 登录邮箱 |
| `account_password` | string | `""` | 登录密码 |
| `screen_protocol` | string | `"auto"` | 屏幕协议 (mtk / rex / zelda / mxcfb / auto) |
| `framebuffer` | string | `"/dev/fb0"` | 帧缓冲设备节点 |
| `font_path` | string | `"/usr/java/lib/fonts/STHeitiMedium.ttf"` | 正文回退系统字体路径 |
| `font_size` | int | `48` | 正文字号大小 |
| `line_spacing` | double | `1.42` | 行距倍数 |
| `reader_margin` | int | `34` | 阅读器页面左右页边距 |
| `page_flash` | bool | `false` | 每页翻页是否强制全刷 (GC16) |
| `page_turn_animation` | bool | `true` | 是否启用滑动渐变动画 (MTK) |
| `reader_guide_dismissed` | bool | `false` | 是否已阅读新手阅读指南 |
| `night_mode` | bool | `false` | 夜间模式 (黑底白字) |
| `justify` | bool | `false` | 是否两端对齐排版 |
| `first_line_indent` | bool | `true` | 是否首行缩进两个字符 |
| `convert` | string / null | `null` | 繁简转换模式 (`"t2s"`, `"s2t"`, `null`) |
| `ignore_japanese` | bool | `false` | 检索或列表时是否忽略日文原生书目 |
| `ignore_ai` | bool | `false` | 是否忽略 AI 翻译或生成内容 |
| `request_limit` | int | `9` | 滑动窗口请求数上限 |
| `request_window_ms` | int | `5500` | 滑动窗口时间范围 (毫秒) |
| `cache_limit_mb` | int | `192` | 磁盘正文与图片缓存空间上限 (MB) |
| `strict_tls` | bool | `true` | 是否严格验证 TLS 证书 |
| `check_update` | bool | `true` | 启动时是否检查软件版本更新 |
| `home_order` | object | `{ "shelf": 0, "history": 1, ... }` | 首页图标排序与可见性 (-1 为隐藏) |

---

## 四、需待实机（Physical Kindle）验证清单

由于当前开发与 CI 环境为 Linux x86_64 及 Termux，以下硬件耦合特性**未在 Kindle 物理真机上验证**，在部署到真机时需重点关注：

1. **电子墨水屏各协议 ioctl 刷新**：
   - MTK (MediaTek, 如 Kindle 11 / PW5 新固件) 的 `FBIO_EINK_UPDATE_DISPLAY_FRAME`。
   - MXCFB (NXP/Freescale i.MX6, 如 PW3 / PW4 / Oasis 2) 的 `MXCFB_SEND_UPDATE`。
   - REX / Zelda 协议的特殊刷新标记。
2. **硬件翻页动画 (`supportsSwipeAnimation`)**：
   - MTK 硬件硬件级滑动翻页波形设置是否生效。
3. **电容触摸手势灵敏度**：
   - Kindle PW4/PW5 触摸驱动所发送的 `ABS_MT_POSITION_X` / `ABS_MT_POSITION_Y` 坐标轴是否发生旋转或缩放。
4. **LIPC 休眠/唤醒联动**：
   - Kindle 进入 Suspend 状态时，`LipcPowerManager` 的 `lipc-wait-event` 事件捕获与恢复唤醒重绘。
5. **系统进程挂起与恢复**：
   - `kill -STOP` 和 `kill -CONT` 对 Kindle 原生 `awesome`、`lipc-daemon` 和 `browser` 的影响。
