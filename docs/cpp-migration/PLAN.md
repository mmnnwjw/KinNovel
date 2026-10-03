# KinNovel C++ 重构实施方案与架构设计 (PLAN.md)

本文档制定 KinNovel 从 Python 重构至 C++17 的整体工程方案、模块划分、接口草图与实现路线。

---

## 1. 架构目标与技术选型

### 1.1 核心目标
1. **单一静态二进制**：生成零动态依赖的 ARM Linux ELF 二进制（`armhf`，适用于 Kindle 固件 ≥ 5.16.3），彻底剥离设备对 πthon/Python 3.14 及其动态库的依赖。
2. **原生跨平台构建**：代码库在 x86_64 Linux 开发机上可直接编译并运行完整的无 I/O 核心单元测试、排版黄金测试以及 PC 渲染模拟截图，无需连接真机即可完成绝大部分开发与验证。
3. **100% 协议与行为兼容**：保证 `config.json` 字段与缺省值兼容；保证 SignalR Hub 调用格式与 Gzip Base64 解码对齐；保证阅读进度 XPath 与断词断行算法与 Web 端及 Python 版逐页一致。
4. **512MB 内存预算控制**：严格控制常驻与峰值内存，图片解码在后台异步处理，大文件避免整段加载，严防内存泄漏与 OOM。

### 1.2 依赖技术选型与权衡决策

| 领域 | 选型 | 备选 | 选型理由与决策记录 |
|---|---|---|---|
| **E-Ink/EPDC** | **FBInk (静态库)** | 原生手写 ioctl | FBInk 是 Kindle 平台最成熟的开源 E-Ink 驱动库，已完善处理 MTK/Rex/Zelda/MXCFB 4类硬件差异、环境温度探针与边界对齐，极大降低手写私有 ioctl 导致的碎屏/死锁风险。通过 `IDisplay` 接口封装。 |
| **JSON** | **yyjson** | nlohmann/json | `yyjson` 采用纯 C 开发，性能比常规 C++ JSON 库快数倍，内存占用极低，极适合频繁反序列化 Hub 返回的大型 JSON/ApiEnvelope 报文。 |
| **字体渲染** | **FreeType 2 + Google Brotli (静态)** | 系统已有动态库 | 必须在构建时静态集成 Brotli 支持以直接解压并渲染 WOFF2 混淆字体，彻底消除对 KOReader 外部路径的依赖。 |
| **HTTP & TLS** | **libcurl + mbedTLS (静态)** | OpenSSL, Poco | mbedTLS 内存开销小、体积精简，且易于完全静态编译并自带 CA 证书包。支持灵活关闭证书校验（`strict_tls`）。 |
| **WebSocket** | **libcurl WebSocket 或轻量 RFC 6455 实现** | websocketpp, boost | 避免引入庞大的 Boost 依赖，使用轻量 C++17 实现或 libcurl 现代 WebSocket API，严格满足 32MB 帧限额与 `\x1e` 粘包切分。 |
| **HTML 解析** | **gumbo-parser** | lexbor, libxml2 | gumbo-parser 符合 HTML5 规范，容错能力强，纯 C 实现且静态编译体积极小，便于遍历 DOM 生成相对 XPath。 |
| **简繁转换** | **内嵌映射表 (Embedded Table)** | OpenCC | OpenCC 字典文件过大且依赖较多；针对正文阅读场景，采用内嵌紧凑 Trie/哈希查找表实现 `t2s` / `s2t`，无额外磁盘 I/O 依赖。 |
| **图像编解码** | **stb_image + stb_image_write** | libpng, libjpeg-turbo | Header-only，无任何第三方动态链接依赖，支持内存/文件直接解码为灰度 8bpp 缓冲区，满足插图与封面需求。 |
| **构建系统** | **CMake 3.22+** | Meson, Makefile | 行业标准，GitHub Actions 支持极佳，便于原生开发环境与 `koxtoolchain` Docker 交叉编译环境共用。 |

---

## 2. C++ 目录结构规划

所有 C++ 源代码置于 `cpp/` 目录下，保持与主仓库既有结构隔离：

```text
cpp/
├── CMakeLists.txt                 # 主 CMake 构建脚本
├── cmake/
│   ├── Toolchain-kindlehf.cmake   # kindlehf 交叉编译工具链定义
│   └── Dependencies.cmake         # 第三方依赖管理 (FetchContent/Vendored)
├── include/
│   └── kinnovel/
│       ├── app/                   # 应用外壳与生命周期
│       │   ├── Application.hpp
│       │   └── Logger.hpp
│       ├── core/                  # 核心基础库 (无 I/O，全平台通用)
│       │   ├── Config.hpp
│       │   ├── Utils.hpp
│       │   ├── Sha256.hpp
│       │   ├── Charset.hpp
│       │   └── Types.hpp
│       ├── hal/                   # 硬件抽象接口 (HAL)
│       │   ├── IDisplay.hpp
│       │   ├── IInput.hpp
│       │   └── IPower.hpp
│       ├── net/                   # 网络传输与协议层
│       │   ├── RateLimiter.hpp
│       │   ├── IHttpClient.hpp
│       │   ├── WebSocketClient.hpp
│       │   ├── SignalRClient.hpp
│       │   └── ApiClient.hpp
│       ├── reader/                # 排版阅读与文本引擎
│       │   ├── DomModel.hpp
│       │   ├── HtmlSanitizer.hpp
│       │   ├── WoffNormalizer.hpp
│       │   ├── FontEngine.hpp
│       │   ├── LayoutEngine.hpp
│       │   └── ReaderDocument.hpp
│       ├── ui/                    # UI 控件层与渲染引擎
│       │   ├── Theme.hpp
│       │   ├── Canvas.hpp
│       │   ├── ImageCache.hpp
│       │   ├── PageContext.hpp
│       │   └── IPage.hpp
│       └── pages/                 # 各业务页面接口定义
│           ├── HomePage.hpp
│           ├── ShelfPage.hpp
│           ├── BrowsePage.hpp
│           ├── RankPage.hpp
│           ├── BookPage.hpp
│           ├── SeriesPage.hpp
│           ├── HistoryPage.hpp
│           ├── ReaderPage.hpp
│           ├── SettingsPage.hpp
│           ├── AccountPage.hpp
│           └── AnnouncementsPage.hpp
├── src/
│   ├── app/                       # Application, main.cpp, Logger 实现
│   ├── core/                      # Config, Utils, Sha256, Charset 实现
│   ├── hal/
│   │   ├── kindle/                # Kindle 真机后端 (FBInk, evdev, lipc)
│   │   │   ├── FbinkDisplay.cpp
│   │   │   ├── EvdevInput.cpp
│   │   │   └── LipcPowerManager.cpp
│   │   └── mock/                  # PC 模拟后端 (无头内存/PNG/SDL)
│   │       ├── MockDisplay.cpp
│   │       ├── MockInput.cpp
│   │       └── MockPowerManager.cpp
│   ├── net/                       # 网络层实现
│   ├── reader/                    # 排版与字体引擎实现
│   ├── ui/                        # Canvas, ImageCache, PageContext 实现
│   └── pages/                     # 业务页面渲染与交互逻辑实现
├── tests/
│   ├── CMakeLists.txt
│   ├── unit/                      # 基础单元测试 (Config, Utils, Net, Parser)
│   ├── golden/                    # 排版黄金测试基准数据与验证测试
│   ├── mock/                      # 本地 Mock HTTP/SignalR 服务端
│   └── screenshots/               # PC 渲染基准截图
└── third_party/                   # vendored 静态头文件或小依赖 (yyjson, stb, etc.)
```

---

## 3. 核心接口草图设计

### 3.1 硬件抽象接口 (HAL)

#### 1. 显示抽象 `IDisplay`
```cpp
namespace kinnovel::hal {

enum class Waveform {
    Auto, Du, Du4, Gc16, Gl16, Reagl, A2
};

enum class SwipeDirection {
    Left, Right, Up, Down
};

struct DisplayRect {
    int x = 0;
    int y = 0;
    int width = 0;
    int height = 0;
};

class IDisplay {
public:
    virtual ~IDisplay() = default;

    virtual bool initialize(const std::string& fbPath, const std::string& protocol) = 0;
    virtual bool probe() = 0;
    virtual void close() = 0;

    virtual int getWidth() const = 0;
    virtual int getHeight() const = 0;
    virtual int getBpp() const = 0;

    // 写入 8bpp 灰阶位图 (0=黑, 255=白) 到对应位置
    virtual void writeImage(const uint8_t* grayBuffer, int x, int y, int w, int h, int pitch) = 0;

    // 提交 e-ink 屏幕刷新
    virtual bool refresh(const DisplayRect& region, bool isFlashing, Waveform waveform, bool dither = false) = 0;

    // 硬件平移翻页动画 (仅部分硬件如 MTK 支持)
    virtual bool supportsSwipeAnimation() const = 0;
    virtual void setSwipeAnimation(bool enabled, SwipeDirection direction, int steps = 12) = 0;

    // 唤醒 / 睡眠控制器
    virtual void powerOn() = 0;
};

} // namespace kinnovel::hal
```

#### 2. 触控输入抽象 `IInput`
```cpp
namespace kinnovel::hal {

enum class GestureKind {
    Unknown, Tap, LongPress, Left, Right, Up, Down
};

struct Point {
    int xPixel = 0;
    int yPixel = 0;
    float xRatio = 0.0f;
    float yRatio = 0.0f;
};

struct TouchGesture {
    GestureKind kind = GestureKind::Unknown;
    int durationMs = 0;
    int distancePx = 0;
    Point point;        // 用于 Tap / LongPress
    Point startPoint;   // 用于滑动
    Point endPoint;     // 用于滑动
};

using GestureCallback = std::function<void(const TouchGesture&)>;

class IInput {
public:
    virtual ~IInput() = default;

    virtual bool initialize(int renderWidth, int renderHeight) = 0;
    virtual void close() = 0;

    virtual bool grab() = 0;
    virtual bool ungrab() = 0;
    virtual void resetGestureState() = 0;

    // 启动监听循环 (阻塞或线程内运行)
    virtual void listen(GestureCallback onGesture) = 0;
    virtual void stop() = 0;
};

} // namespace kinnovel::hal
```

#### 3. 电源与休眠调度 `IPower`
```cpp
namespace kinnovel::hal {

class IPowerObserver {
public:
    virtual ~IPowerObserver() = default;
    virtual void onSuspend() = 0;
    virtual void onResume() = 0;
};

class IPower {
public:
    virtual ~IPower() = default;

    virtual void start(IPowerObserver* observer) = 0;
    virtual void stop() = 0;
    virtual bool isSleeping() const = 0;

    // 手动触发电源键挂起/恢复
    virtual void handlePowerKey() = 0;
};

} // namespace kinnovel::hal
```

---

### 3.2 网络传输与领域契约

#### 1. 请求限流器 `RateLimiter`
```cpp
namespace kinnovel::net {

class RateLimiter {
public:
    explicit RateLimiter(int maxRequests = 9, double windowSeconds = 5.5);
    void wait();

private:
    int m_maxRequests;
    double m_windowSeconds;
    std::deque<std::chrono::steady_clock::time_point> m_history;
    std::mutex m_mutex;
    std::condition_variable m_cv;
};

} // namespace kinnovel::net
```

#### 2. SignalR Hub 客户端 `ISignalRClient`
```cpp
namespace kinnovel::net {

struct ApiEnvelope {
    bool success = false;
    int status = 200;
    std::string msg;
    std::string rawResponse; // 解压后的 JSON 文本或原始值
};

class ISignalRClient {
public:
    virtual ~ISignalRClient() = default;

    virtual void setServer(const std::string& server) = 0;
    virtual void setTokenProvider(std::function<std::string()> provider) = 0;

    // 执行 Hub 调用并等待返回解压后的 JSON 字符串
    virtual ApiEnvelope invoke(const std::string& method,
                               const std::string& jsonParams = "{}",
                               bool useGzip = true,
                               int timeoutSeconds = 30) = 0;

    virtual void close() = 0;
};

} // namespace kinnovel::net
```

---

### 3.3 排版与阅读引擎

#### 1. 字体与字形解析 `FontEngine`
```cpp
namespace kinnovel::reader {

class FontEngine {
public:
    explicit FontEngine(const std::string& systemFontPath);
    ~FontEngine();

    // 探测字符字形在主字体中是否存在 (且不为 .notdef 方框)
    bool isGlyphAvailable(void* ftFace, uint32_t codepoint);

    // 度量一段字符的实际像素宽度 (含逐字符回退)
    float measureText(const std::string& utf8Text, void* primaryFace, void* fallbackFace);

    // 从磁盘文件加载字体 (自动识别 TTF/OTF/WOFF/WOFF2)
    void* loadFont(const std::string& fontPath, int pixelSize);
};

} // namespace kinnovel::reader
```

#### 2. 块结构与排版模型 `ReaderDocument`
```cpp
namespace kinnovel::reader {

enum class BlockKind { Text, Heading, Image, Footnote };

struct LayoutItem {
    std::string type; // "text" | "image"
    std::string text;
    std::string url;
    int x = 0;
    int y = 0;
    int width = 0;
    int height = 0;
    int size = 0;
    std::string path; // 相对 XPath
    int offset = 0;   // 字符位移
};

using PageLayout = std::vector<LayoutItem>;

class ReaderDocument {
public:
    ReaderDocument(const std::string& htmlContent,
                   const std::string& chapterFontPath,
                   const std::string& systemFontPath,
                   const Config& config);

    // 计算分页
    const std::vector<PageLayout>& paginate(int usableWidth, int usableHeight);

    // 阅读进度映射
    int getPageForPath(const std::string& xpath, int offset = -1) const;
    std::pair<std::string, int> getFirstAnchorOnPage(int pageIndex) const;
    std::string getFirstPathOnPage(int pageIndex) const;

    int getPageCount() const;
};

} // namespace kinnovel::reader
```

---

### 3.4 UI 抽象与页面路由

#### 1. 页面接口 `IPage`
```cpp
namespace kinnovel::ui {

class PageContext;
class Canvas;

class IPage {
public:
    virtual ~IPage() = default;

    virtual void onEnter(PageContext& ctx) {}
    virtual void render(PageContext& ctx, Canvas& canvas) = 0;
    virtual void handle(const hal::TouchGesture& gesture, PageContext& ctx) = 0;

    // 可选：阅读器防误触拦截
    virtual bool isHeaderBlocked() const { return false; }
    virtual void uploadProgress(PageContext& ctx) {}
};

} // namespace kinnovel::ui
```

---

## 4. 实施阶段与推进路线图

按 `kinnovel_cpp.md` 要求，严格依照 0 到 6 阶段推进：

- [x] **阶段 0：深度分析与重构规划 (当前)**
  - 通读 Python 源码及测试代码。
  - 产出 `docs/cpp-migration/ANALYSIS.md`。
  - 产出 `docs/cpp-migration/PLAN.md`。
  - 明确最高风险 Top 3 与未在 README 记录的行为。
- [ ] **阶段 1：工程骨架与硬件抽象**
  - 创建 `cpp/` CMake 构建系统，配置原生构建与 `kindlehf` 交叉编译。
  - 实现 HAL 抽象：`IDisplay`、`IInput`、`IPower`。
  - 实现 PC 模拟后端（PNG 输出）与 Kindle FBInk 后端。
  - 静态编译验证（`readelf -d` 零意外动态依赖）。
- [ ] **阶段 2：核心排版与字体引擎 (可离线单测)**
  - 实现配置解析、简繁转换、HTML 清洗与块模型提取。
  - 实现 FreeType+Brotli 静态加载、逐字回退检测。
  - 导出 Python 版脱敏章节的“黄金基准”（字符流与 XPath 断点）至 `cpp/tests/golden/`。
  - C++ 排版算法与之逐页比对，通过全部黄金测试。
- [ ] **阶段 3：网络与通信协议层**
  - 实现 Sliding Window 限流器。
  - 实现 RFC 6455 WebSocket 与 SignalR JSON 协议客户端，支持 ApiEnvelope Gzip 解包。
  - 实现 Session 凭据管理、自动登录与 Token 刷新。
  - 本地 Mock 服务端测试端到端断网、超时与 401 场景。
- [ ] **阶段 4：UI 控件层与业务页面**
  - 实现 Canvas 灰阶渲染管线、Theme 黑白/夜间切换。
  - 实现页面路由器、模态弹窗与异步任务池。
  - 迁移实现全部页面（首页、书架、浏览、排行、书籍详情、阅读器、设置、账号等）。
  - PC 端截图对比测试。
- [ ] **阶段 5：系统集成与 KUAL 打包**
  - 启动进程挂起与原屏保存；LIPC 休眠/唤醒联动；单实例锁。
  - 生成 `bin/start.sh` 与 KUAL 扩展包结构。
- [ ] **阶段 6：文档收尾与 Draft PR**
  - 更新 README、`PARITY.md`、`DECISIONS.md`。
  - 在 GitHub 开设 Draft PR，列明待实机验证项。
