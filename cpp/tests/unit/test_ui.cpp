#include <cassert>
#include <iostream>
#include <memory>
#include <filesystem>
#include <chrono>
#include <thread>

#include "kinnovel/ui/Theme.hpp"
#include "kinnovel/ui/Canvas.hpp"
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
#include "kinnovel/core/Config.hpp"
#include "src/hal/mock/MockDisplay.hpp"
#include "src/hal/mock/MockPowerManager.hpp"

using namespace kinnovel;
using namespace kinnovel::ui;
using namespace kinnovel::hal;

static std::string findFontPath() {
    std::vector<std::string> candidates = {
        "cpp/tests/fixtures/testfont.ttf",
        "../cpp/tests/fixtures/testfont.ttf",
        "../../cpp/tests/fixtures/testfont.ttf",
        "../../../cpp/tests/fixtures/testfont.ttf",
        "/system/fonts/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
    };
    for (const auto& path : candidates) {
        if (std::filesystem::exists(path)) return path;
    }
    return "";
}

static std::string findScreenshotDir() {
    std::vector<std::string> candidates = {
        "../tests/screenshots",
        "cpp/tests/screenshots",
        "../../tests/screenshots",
        "../cpp/tests/screenshots"
    };
    for (const auto& path : candidates) {
        if (std::filesystem::exists(path)) return path;
    }
    std::filesystem::create_directories("cpp/tests/screenshots");
    return "cpp/tests/screenshots";
}

void testCanvasPrimitives() {
    std::cout << "[TEST] Running testCanvasPrimitives..." << std::endl;
    int width = 800;
    int height = 1200;
    Canvas canvas(width, height, Theme::day());

    // Primitives
    canvas.clear(255);
    canvas.drawLine(10, 10, 790, 10, 0, 2);
    canvas.drawRect(20, 30, 200, 100, 128, false);
    canvas.drawRect(240, 30, 200, 100, 200, true);
    canvas.drawRoundedRect(460, 30, 200, 100, 12, 50, 2);
    canvas.drawCircle(100, 220, 60, 100, true);
    canvas.drawCircle(260, 220, 60, 0, false);

    // Text & Font
    std::string fontPath = findFontPath();
    std::shared_ptr<FontSet> fonts;
    if (!fontPath.empty()) {
        fonts = std::make_shared<FontSet>();
        fonts->init(fontPath, width, height);
    }

    if (fonts && fonts->title) {
        canvas.drawText(20, 320, "KinNovel C++17 UI Test", fonts->title.get());
    }

    // Widgets
    Rect btnRect{ 20, 400, 220, 60 };
    canvas.button(btnRect, "点击按钮", true, fonts && fonts->body ? fonts->body.get() : nullptr);

    int headerH = canvas.header("首页标题", "返回", "主页");
    assert(headerH > 0);
    (void)headerH;

    // Popup
    canvas.popup({ "提示信息", "确定要执行此操作吗？" }, { "确定", "取消" });

    // Save screenshot
    std::string ssDir = findScreenshotDir();
    MockDisplay display(width, height);
    display.writeImage(canvas.getBuffer().data(), 0, 0, width, height, width);
    display.saveToPng(ssDir + "/canvas_primitives.png");
    assert(std::filesystem::exists(ssDir + "/canvas_primitives.png"));

    std::cout << "  -> testCanvasPrimitives PASSED" << std::endl;
}

void testImageCache() {
    std::cout << "[TEST] Running testImageCache..." << std::endl;
    ImageCache cache(10);

    // Create a 100x100 dummy checkerboard image
    auto img = std::make_shared<ImageBuffer>();
    img->width = 100;
    img->height = 100;
    img->data.resize(100 * 100);
    for (int y = 0; y < 100; ++y) {
        for (int x = 0; x < 100; ++x) {
            img->data[y * 100 + x] = ((x / 10 + y / 10) % 2 == 0) ? 0 : 255;
        }
    }

    cache.put("checker", img);
    auto retrieved = cache.get("checker");
    assert(retrieved != nullptr);
    assert(retrieved->width == 100 && retrieved->height == 100);

    // Test bilinear scaling (contain)
    auto contained = ImageCache::contain(*img, 50, 50);
    assert(contained != nullptr);
    assert(contained->width == 50 && contained->height == 50);

    // Test fit
    auto fitted = ImageCache::fit(*img, 60, 80);
    assert(fitted != nullptr);
    assert(fitted->width == 60 && fitted->height == 60);

    std::cout << "  -> testImageCache PASSED" << std::endl;
}

void testPageNavigationAndModals() {
    std::cout << "[TEST] Running testPageNavigationAndModals..." << std::endl;

    int width = 800;
    int height = 1200;
    auto display = std::make_shared<MockDisplay>(width, height);
    auto power = std::make_shared<MockPowerManager>();
    auto config = std::make_shared<core::Config>("/tmp/test_kinnovel_config.json");
    config->load();

    auto images = std::make_shared<ImageCache>(20);

    std::string fontPath = findFontPath();
    std::shared_ptr<FontSet> fonts;
    if (!fontPath.empty()) {
        fonts = std::make_shared<FontSet>();
        fonts->init(fontPath, width, height);
    }

    PageContext context(display, config, nullptr, fonts, images, power);

    // Register all pages
    context.registerPage("home", std::make_shared<HomePage>());
    context.registerPage("browse", std::make_shared<BrowsePage>());
    context.registerPage("rank", std::make_shared<RankPage>());
    context.registerPage("book", std::make_shared<BookDetailPage>());
    context.registerPage("series", std::make_shared<SeriesPage>());
    context.registerPage("shelf", std::make_shared<ShelfPage>());
    context.registerPage("history", std::make_shared<HistoryPage>());
    context.registerPage("reader", std::make_shared<ReaderPage>());
    context.registerPage("catalog", std::make_shared<CatalogPage>());
    context.registerPage("settings", std::make_shared<SettingsPage>());
    context.registerPage("about", std::make_shared<AboutPage>());
    context.registerPage("account", std::make_shared<AccountPage>());
    context.registerPage("notifications", std::make_shared<NotificationsPage>());
    context.registerPage("shop", std::make_shared<ShopPage>());
    context.registerPage("announcements", std::make_shared<AnnouncementsPage>());
    context.registerPage("announcement", std::make_shared<AnnouncementDetailPage>());
    context.registerPage("comments", std::make_shared<CommentsPage>());

    std::string ssDir = findScreenshotDir();

    // Start at home
    context.navigate("home");
    assert(context.getPageName() == "home");
    context.show();
    display->saveToPng(ssDir + "/page_home.png");
    assert(std::filesystem::exists(ssDir + "/page_home.png"));

    // Navigate to settings
    context.navigate("settings");
    assert(context.getPageName() == "settings");
    context.show();
    display->saveToPng(ssDir + "/page_settings.png");
    assert(std::filesystem::exists(ssDir + "/page_settings.png"));

    // Tap "返回" header button on left (e.g. x=30, y=30)
    TouchGesture tapBack;
    tapBack.kind = GestureKind::Tap;
    tapBack.point.xPixel = 30;
    tapBack.point.yPixel = 30;
    bool handledBack = context.handle(tapBack);
    assert(handledBack);
    (void)handledBack;
    assert(context.getPageName() == "home");

    // Navigate to about
    context.navigate("about");
    assert(context.getPageName() == "about");
    context.show();
    display->saveToPng(ssDir + "/page_about.png");
    assert(std::filesystem::exists(ssDir + "/page_about.png"));

    // Tap "主页" header button on right (e.g. x=770, y=30)
    TouchGesture tapHome;
    tapHome.kind = GestureKind::Tap;
    tapHome.point.xPixel = 770;
    tapHome.point.yPixel = 30;
    bool handledHome = context.handle(tapHome);
    assert(handledHome);
    (void)handledHome;
    assert(context.getPageName() == "home");

    // Test modal confirm
    bool confirmed = false;
    context.confirm("确定执行操作？", [&confirmed]() {
        confirmed = true;
    });
    assert(context.hasModal());
    context.show();
    display->saveToPng(ssDir + "/modal_confirm.png");
    assert(std::filesystem::exists(ssDir + "/modal_confirm.png"));

    // Tap "确定" inside modal popup (in the bottom part of popup)
    TouchGesture tapConfirm;
    tapConfirm.kind = GestureKind::Tap;
    tapConfirm.point.xPixel = 260; // Left button ("确定")
    tapConfirm.point.yPixel = 670;
    context.handle(tapConfirm);
    assert(!context.hasModal());
    assert(confirmed);

    // Test toast
    context.toast("已保存设置");
    assert(!context.getToast().empty());
    context.show();
    display->saveToPng(ssDir + "/toast.png");
    assert(std::filesystem::exists(ssDir + "/toast.png"));

    // Test Night Mode
    config->setBool("night_mode", true);
    context.navigate("settings");
    context.show();
    display->saveToPng(ssDir + "/page_settings_night.png");
    assert(std::filesystem::exists(ssDir + "/page_settings_night.png"));

    std::cout << "  -> testPageNavigationAndModals PASSED" << std::endl;
}

int main() {
    std::cout << "========================================" << std::endl;
    std::cout << "  Running KinNovel UI Tests" << std::endl;
    std::cout << "========================================" << std::endl;

    testCanvasPrimitives();
    testImageCache();
    testPageNavigationAndModals();

    std::cout << "========================================" << std::endl;
    std::cout << "  ALL UI TESTS PASSED!" << std::endl;
    std::cout << "========================================" << std::endl;
    return 0;
}
