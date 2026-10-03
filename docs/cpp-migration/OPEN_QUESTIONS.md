# KinNovel C++ 重构待确认问题集 (OPEN_QUESTIONS.md)

本文档记录在代码分析与技术选型过程中发现的潜在歧义、外部依赖不确定性及待实机确认项。遇到此类问题时不随意猜测，记录于此并按最稳妥规范推进。

---

### Q1: CA 根证书打包策略
- **背景**：Kindle 原生系统不同固件版本中 `/etc/ssl/certs/ca-certificates.crt` 的完整性不一致。在 Python 脚本中依赖此系统路径，部分读者曾反馈证书缺失导致 TLS 握手失败。
- **重构方案**：C++ 静态构建的 libcurl + mbedTLS 将随包自带精简的 Mozilla Root CA 证书文件（如 `res/cacert.pem`），同时保留 `strict_tls` 开关。如果自带证书存在则优先使用自带证书，避免依赖系统缺失的证书链。
- **状态**：方案已明确，待阶段 3 集成验证。

### Q2: 单元测试与黄金测试字体基准
- **背景**：Python 版 `tests/test_reader.py` 与 `tests/test_pages.py` 硬编码了 Windows 本地路径 `"C:/Windows/Fonts/simhei.ttf"`，在 Linux 与 Termux 环境下均无法直接运行排版断言。
- **重构方案**：在 `cpp/tests/fixtures/` 中内嵌一份开源轻量开源中文字体（如思源黑体/方正开源子集/Unifont TTF），并使用该字体生成黄金排版断言，彻底解耦宿主机系统字体环境。
- **状态**：方案已明确，阶段 2 落地。

### Q3: 章节字体 WOFF2 容器规范
- **背景**：LightNovelShelf 站点动态下发的混淆字体主要为 WOFF2 格式。FreeType 配合 Brotli 可以直接读取大多数 WOFF2 文件。若存在部分旧格式或非标准 SFNT 表头（如部分 WOFF1 容器），Python 端实现了手动的 SFNT 表重构（`normalize_font`）。
- **重构方案**：C++ 端优先使用带 Brotli 的 FreeType 原生加载 WOFF2；同时保留 `WoffNormalizer` 作为备选逻辑，对 WOFF1 的 zlib 压缩表进行 SFNT 重建。
- **状态**：双轨保障，已列入阶段 2 实现计划。

### Q4: 物理设备 EPDC 与 LIPC 行为（未实机验证）
- **背景**：根据开发环境约束，当前无 Kindle 真机进行实时调试。MTK、Rex、Zelda、MXCFB 等平台的 ioctl 调用以及 LIPC 电源事件只能基于 FBInk 源码与 kComics 现有实现进行静态对齐。
- **重构方案**：
  1. 通过 `IDisplay` 抽象出 FBInk 后端与 PC 模拟后端，所有 UI/排版逻辑在 PC 端可完整截图验证。
  2. 凡是涉及 Kindle EPDC ioctl 与 LIPC 进程挂起的特性，在文档与提交记录中明确标注“未实机验证”，等待实机读者与维护者反馈。
- **状态**：持续跟踪。
