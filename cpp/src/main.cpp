#include <iostream>
#include <fstream>
#include <string>
#include <vector>
#include <memory>
#include <chrono>
#include <thread>
#include <atomic>
#include <csignal>
#include <filesystem>
#include <fcntl.h>
#include <unistd.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <dirent.h>

#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Logger.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/hal/IDisplay.hpp"
#include "kinnovel/hal/IInput.hpp"
#include "kinnovel/hal/IPower.hpp"
#include "src/hal/kindle/FbinkDisplay.hpp"
#include "src/hal/kindle/EvdevInput.hpp"
#include "src/hal/kindle/LipcPowerManager.hpp"
#include "src/hal/mock/MockDisplay.hpp"
#include "src/hal/mock/MockInput.hpp"
#include "src/hal/mock/MockPowerManager.hpp"
#include "kinnovel/network/ApiClient.hpp"
#include "kinnovel/ui/FontSet.hpp"
#include "kinnovel/ui/ImageCache.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/ui/pages/HomePage.hpp"
#include "kinnovel/ui/pages/BrowsePage.hpp"
#include "kinnovel/ui/pages/RankPage.hpp"
#include "kinnovel/ui/pages/BookDetailPage.hpp"
#include "kinnovel/ui/pages/SeriesPage.hpp"
#include "kinnovel/ui/pages/ShelfPage.hpp"
#include "kinnovel/ui/pages/HistoryPage.hpp"
#include "kinnovel/ui/pages/ReaderPage.hpp"
#include "kinnovel/ui/pages/SettingsPage.hpp"
#include "kinnovel/ui/pages/AccountPage.hpp"
#include "kinnovel/ui/pages/AnnouncementsPage.hpp"

using namespace kinnovel;
using namespace kinnovel::ui;
using namespace kinnovel::hal;

namespace {

static const char* VERSION_STRING = "KinNovel 0.6.0 (C++17 build)";
static const char* DEFAULT_FB_PATH = "/dev/fb0";

std::string getTempDir() {
    if (std::filesystem::exists("/tmp")) {
        std::error_code ec;
        auto perms = std::filesystem::status("/tmp", ec).permissions();
        if ((perms & std::filesystem::perms::owner_write) != std::filesystem::perms::none) {
            return "/tmp";
        }
    }
    const char* envTmp = std::getenv("TMPDIR");
    if (envTmp && *envTmp) return envTmp;
    return "/tmp";
}

std::string getLockFilePath() {
    return getTempDir() + "/kinnovel.lock";
}

std::string getPauseListPath() {
    return getTempDir() + "/kinnovel_paused_pids";
}

std::string getSnapshotPath() {
    return getTempDir() + "/kinnovel_fb.bin";
}

std::atomic<bool> g_running{true};
std::shared_ptr<IInput> g_input;
std::shared_ptr<IDisplay> g_display;
std::shared_ptr<IPower> g_power;
int g_lockFd = -1;
bool g_isKindle = false;

void signalHandler(int sig) {
    (void)sig;
    g_running = false;
    if (g_input) {
        g_input->stop();
    }
}

bool acquireSingleInstanceLock() {
    g_lockFd = ::open(getLockFilePath().c_str(), O_CREAT | O_RDWR, 0666);
    if (g_lockFd < 0) {
        return false;
    }
    if (::flock(g_lockFd, LOCK_EX | LOCK_NB) != 0) {
        ::close(g_lockFd);
        g_lockFd = -1;
        return false;
    }
    std::string pidStr = std::to_string(::getpid()) + "\n";
    (void)::ftruncate(g_lockFd, 0);
    (void)::write(g_lockFd, pidStr.data(), pidStr.size());
    return true;
}

void releaseSingleInstanceLock() {
    if (g_lockFd >= 0) {
        ::flock(g_lockFd, LOCK_UN);
        ::close(g_lockFd);
        g_lockFd = -1;
        ::unlink(getLockFilePath().c_str());
    }
}

std::vector<pid_t> findFbUsers() {
    std::vector<pid_t> pids;
    DIR* procDir = ::opendir("/proc");
    if (!procDir) return pids;

    pid_t myPid = ::getpid();
    struct dirent* ent;
    while ((ent = ::readdir(procDir)) != nullptr) {
        if (ent->d_type != DT_DIR) continue;
        char* endp = nullptr;
        long pid = std::strtol(ent->d_name, &endp, 10);
        if (*endp != '\0' || pid <= 0 || pid == myPid) continue;

        std::string fdDirPath = std::string("/proc/") + ent->d_name + "/fd";
        DIR* fdDir = ::opendir(fdDirPath.c_str());
        if (!fdDir) continue;

        struct dirent* fdEnt;
        char linkTarget[256];
        bool usesFb = false;
        while ((fdEnt = ::readdir(fdDir)) != nullptr) {
            std::string linkPath = fdDirPath + "/" + fdEnt->d_name;
            ssize_t len = ::readlink(linkPath.c_str(), linkTarget, sizeof(linkTarget) - 1);
            if (len > 0) {
                linkTarget[len] = '\0';
                if (std::string(linkTarget) == DEFAULT_FB_PATH) {
                    usesFb = true;
                    break;
                }
            }
        }
        ::closedir(fdDir);

        if (usesFb) {
            pids.push_back(static_cast<pid_t>(pid));
        }
    }
    ::closedir(procDir);
    return pids;
}

void pauseFbUsers() {
    auto pids = findFbUsers();
    std::ofstream out(getPauseListPath());
    for (pid_t pid : pids) {
        if (::kill(pid, 0) == 0) {
            ::kill(pid, SIGSTOP);
            out << pid << "\n";
            Logger::info("Launcher", "Paused pid=" + std::to_string(pid));
        }
    }
}

void resumeFbUsers() {
    std::ifstream in(getPauseListPath());
    if (!in) return;

    pid_t pid;
    while (in >> pid) {
        if (::kill(pid, 0) == 0) {
            ::kill(pid, SIGCONT);
            Logger::info("Launcher", "Resumed pid=" + std::to_string(pid));
        }
    }
    in.close();
    ::unlink(getPauseListPath().c_str());
}

void reloadKindleUi() {
    if (g_isKindle) {
        (void)::system("lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home >/dev/null 2>&1");
    }
}

bool saveFbSnapshot(const std::string& outPath) {
    int fd = ::open(DEFAULT_FB_PATH, O_RDONLY);
    if (fd < 0) return false;

    struct stat st;
    if (::fstat(fd, &st) < 0) {
        ::close(fd);
        return false;
    }

    size_t size = st.st_size > 0 ? static_cast<size_t>(st.st_size) : (1072 * 1448 * 2);
    std::vector<uint8_t> buffer(size);
    ssize_t n = ::read(fd, buffer.data(), size);
    ::close(fd);

    if (n <= 0) return false;
    buffer.resize(static_cast<size_t>(n));

    std::ofstream out(outPath, std::ios::binary);
    if (!out) return false;
    out.write(reinterpret_cast<const char*>(buffer.data()), buffer.size());
    return true;
}

bool restoreFbSnapshot(const std::string& inPath, IDisplay* display) {
    std::ifstream in(inPath, std::ios::binary | std::ios::ate);
    if (!in) return false;

    std::streamsize size = in.tellg();
    in.seekg(0, std::ios::beg);
    std::vector<uint8_t> buffer(size);
    if (!in.read(reinterpret_cast<char*>(buffer.data()), size)) return false;

    int fd = ::open(DEFAULT_FB_PATH, O_WRONLY);
    if (fd >= 0) {
        (void)::write(fd, buffer.data(), buffer.size());
        ::close(fd);
    }

    if (display) {
        display->refresh(Rect{ 0, 0, display->getWidth(), display->getHeight() }, true, Waveform::Gc16);
    }
    return true;
}

class AppPowerObserver : public IPowerObserver {
public:
    AppPowerObserver(PageContext* context, IDisplay* display, IInput* input)
        : m_context(context), m_display(display), m_input(input) {}

    void onSuspend() override {
        Logger::info("App", "Device suspended");
        if (m_input) m_input->ungrab();
    }

    void onResume() override {
        Logger::info("App", "Device resumed");
        if (m_display) {
            m_display->powerOn();
        }
        if (m_input) {
            m_input->grab();
        }
        if (m_context) {
            m_context->show(true);
        }
    }

private:
    PageContext* m_context;
    IDisplay* m_display;
    IInput* m_input;
};

std::string findFontPath(const std::shared_ptr<core::Config>& config) {
    if (config) {
        std::string cfgFont = config->getString("font_path", "");
        if (!cfgFont.empty() && std::filesystem::exists(cfgFont)) {
            return cfgFont;
        }
    }

    std::vector<std::string> candidates = {
        "/usr/java/lib/fonts/STHeitiMedium.ttf",
        "/usr/java/lib/fonts/KindleBlackBox.ttf",
        "/system/fonts/NotoSansCJK-Regular.ttc",
        "/system/fonts/DroidSansFallback.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "cpp/tests/fixtures/testfont.ttf",
        "../cpp/tests/fixtures/testfont.ttf"
    };

    for (const auto& path : candidates) {
        if (std::filesystem::exists(path)) {
            return path;
        }
    }
    return "";
}

std::string findConfigPath(const std::string& custom) {
    if (!custom.empty()) return custom;

    std::vector<std::string> candidates = {
        "/mnt/us/extensions/kinnovel/bin/config.json",
        "./bin/config.json",
        "config.json",
        "../bin/config.json"
    };

    for (const auto& path : candidates) {
        if (std::filesystem::exists(path)) {
            return path;
        }
    }
    return "/mnt/us/extensions/kinnovel/bin/config.json";
}

void printHelp() {
    std::cout << VERSION_STRING << "\n\n"
              << "Usage: kinnovel [options] [command]\n\n"
              << "Options:\n"
              << "  -h, --help               Show this help message and exit\n"
              << "  -v, --version            Show version information and exit\n"
              << "  -c, --config <path>      Specify configuration file path\n"
              << "  -d, --device <type>      Hardware device backend (kindle|mock|auto)\n"
              << "  -p, --page <name>        Starting page name (default: home)\n"
              << "  --dry-run, --test        Initialize and render one frame, then exit cleanly\n"
              << "  --snapshot-save <path>   Save current framebuffer to file and exit\n"
              << "  --snapshot-restore <path> Restore framebuffer from file and exit\n\n"
              << "Commands:\n"
              << "  snapshot save <path>     Save current framebuffer to file and exit\n"
              << "  snapshot restore <path>  Restore framebuffer from file and exit\n";
}

} // namespace

int main(int argc, char* argv[]) {
    std::string configPath;
    std::string deviceType = "auto";
    std::string startPage = "home";
    bool dryRun = false;

    // Parse command line arguments
    for (int i = 1; i < argc; ++i) {
        std::string arg = argv[i];
        if (arg == "-h" || arg == "--help") {
            printHelp();
            return 0;
        } else if (arg == "-v" || arg == "--version") {
            std::cout << VERSION_STRING << "\n";
            return 0;
        } else if ((arg == "-c" || arg == "--config") && i + 1 < argc) {
            configPath = argv[++i];
        } else if ((arg == "-d" || arg == "--device") && i + 1 < argc) {
            deviceType = argv[++i];
        } else if ((arg == "-p" || arg == "--page") && i + 1 < argc) {
            startPage = argv[++i];
        } else if (arg == "--dry-run" || arg == "--test") {
            dryRun = true;
        } else if (arg == "--snapshot-save" && i + 1 < argc) {
            return saveFbSnapshot(argv[++i]) ? 0 : 1;
        } else if (arg == "--snapshot-restore" && i + 1 < argc) {
            return restoreFbSnapshot(argv[++i], nullptr) ? 0 : 1;
        } else if (arg == "snapshot" && i + 2 < argc) {
            std::string sub = argv[++i];
            std::string path = argv[++i];
            if (sub == "save") return saveFbSnapshot(path) ? 0 : 1;
            if (sub == "restore") return restoreFbSnapshot(path, nullptr) ? 0 : 1;
        }
    }

    // Single-instance lock
    if (!dryRun) {
        if (!acquireSingleInstanceLock()) {
            std::cerr << "KinNovel is already running.\n";
            return 1;
        }
    }

    // Signals
    struct sigaction sa;
    std::memset(&sa, 0, sizeof(sa));
    sa.sa_handler = signalHandler;
    sigaction(SIGINT, &sa, nullptr);
    sigaction(SIGTERM, &sa, nullptr);
    sigaction(SIGHUP, &sa, nullptr);

    // Load config
    std::string actualConfigPath = findConfigPath(configPath);
    auto config = std::make_shared<core::Config>(actualConfigPath);
    config->load();

    // Check device type
    if (deviceType == "auto") {
        g_isKindle = std::filesystem::exists(DEFAULT_FB_PATH);
    } else {
        g_isKindle = (deviceType == "kindle");
    }

    if (g_isKindle && !dryRun) {
        pauseFbUsers();
        saveFbSnapshot(getSnapshotPath());
        std::this_thread::sleep_for(std::chrono::milliseconds(300));
    }

    // Initialize HAL
    std::string fbPath = config->getString("framebuffer", DEFAULT_FB_PATH);
    std::string protocol = config->getString("screen_protocol", "auto");

    if (g_isKindle) {
        auto fbink = std::make_shared<FbinkDisplay>();
        if (!fbink->initialize(fbPath, protocol)) {
            Logger::warn("Main", "FbinkDisplay initialization failed, falling back to MockDisplay");
            g_display = std::make_shared<MockDisplay>(1072, 1448);
        } else {
            g_display = fbink;
        }

        auto evdev = std::make_shared<EvdevInput>();
        if (!evdev->initialize(g_display->getWidth(), g_display->getHeight())) {
            Logger::warn("Main", "EvdevInput initialization failed, falling back to MockInput");
            g_input = std::make_shared<MockInput>();
        } else {
            g_input = evdev;
        }

        g_power = std::make_shared<LipcPowerManager>();
    } else {
        g_display = std::make_shared<MockDisplay>(1072, 1448);
        g_input = std::make_shared<MockInput>();
        g_power = std::make_shared<MockPowerManager>();
    }

    g_input->grab();

    // API & Network
    auto api = std::make_shared<network::ApiClient>(config);
    std::string email = config->getString("account_email", "");
    std::string pass = config->getString("account_password", "");
    if (!email.empty() && !pass.empty()) {
        api->login(email, pass);
    }

    // Image Cache
    auto images = std::make_shared<ImageCache>(32);

    // Fonts
    std::string fontPath = findFontPath(config);
    std::shared_ptr<FontSet> fonts;
    if (!fontPath.empty()) {
        fonts = std::make_shared<FontSet>();
        fonts->init(fontPath, g_display->getWidth(), g_display->getHeight());
    }

    // PageContext
    auto context = std::make_shared<PageContext>(g_display, config, api, fonts, images, g_power);

    // Register pages
    context->registerPage("home", std::make_shared<HomePage>());
    context->registerPage("browse", std::make_shared<BrowsePage>());
    context->registerPage("rank", std::make_shared<RankPage>());
    context->registerPage("book", std::make_shared<BookDetailPage>());
    context->registerPage("series", std::make_shared<SeriesPage>());
    context->registerPage("shelf", std::make_shared<ShelfPage>());
    context->registerPage("history", std::make_shared<HistoryPage>());
    context->registerPage("reader", std::make_shared<ReaderPage>());
    context->registerPage("catalog", std::make_shared<CatalogPage>());
    context->registerPage("settings", std::make_shared<SettingsPage>());
    context->registerPage("about", std::make_shared<AboutPage>());
    context->registerPage("account", std::make_shared<AccountPage>());
    context->registerPage("notifications", std::make_shared<NotificationsPage>());
    context->registerPage("shop", std::make_shared<ShopPage>());
    context->registerPage("announcements", std::make_shared<AnnouncementsPage>());
    context->registerPage("announcement", std::make_shared<AnnouncementDetailPage>());
    context->registerPage("comments", std::make_shared<CommentsPage>());

    context->setOnStopCallback([&]() {
        g_running = false;
        if (g_input) g_input->stop();
    });

    // Start Power manager
    AppPowerObserver observer(context.get(), g_display.get(), g_input.get());
    if (g_power) {
        g_power->start(&observer);
    }

    // Navigate to start page
    context->navigate(startPage);
    context->show(true);

    if (dryRun) {
        Logger::info("Main", "Dry run completed successfully.");
    } else {
        // Event loop
        std::thread inputThread([&]() {
            g_input->listen([&](const TouchGesture& g) {
                if (g_running) {
                    context->handle(g);
                }
            });
        });

        while (g_running) {
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }

        if (inputThread.joinable()) {
            inputThread.join();
        }
    }

    // Cleanup
    if (g_power) {
        g_power->stop();
    }
    if (g_input) {
        g_input->ungrab();
        g_input->close();
    }
    if (g_display) {
        g_display->close();
    }

    if (g_isKindle && !dryRun) {
        restoreFbSnapshot(getSnapshotPath(), nullptr);
        resumeFbUsers();
        reloadKindleUi();
    }

    releaseSingleInstanceLock();
    return 0;
}
