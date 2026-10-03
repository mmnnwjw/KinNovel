#include "kinnovel/ui/pages/SettingsPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/core/Utils.hpp"
#include "yyjson.h"

#include <cmath>
#include <iomanip>
#include <sstream>

namespace kinnovel::ui {

void SettingsPage::enter(PageContext& context) {
    (void)context;
}

void SettingsPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("设置", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.05);
    int y = top + 18;
    int rowCount = 13;
    int available = canvas.getHeight() - top - 30;
    int gap = (available >= 12 * 64 + 11 * 14) ? 14 : 8;
    int height = std::max(44, std::min(64, (available - gap * (rowCount - 1)) / rowCount));

    m_rects.clear();
    auto config = context.getConfig();

    auto row = [&](const std::string& label, const std::string& value, const std::string& action) {
        Rect rect{ margin, y, canvas.getWidth() - 2 * margin, height };
        std::string act = action.empty() ? label : action;
        m_rects[act] = rect;
        m_rects[act + "_0"] = rect;

        canvas.drawRoundedRect(rect.x, rect.y, rect.width, rect.height, 9, canvas.getTheme().mid, 1);
        canvas.drawText(rect.x + 14, y + std::max(4, (height - (context.getFonts() && context.getFonts()->small ? context.getFonts()->small->getSize() : 28)) / 2),
                        label, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);

        if (!value.empty()) {
            std::string fitted = canvas.fitText(value, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr, rect.width / 2 - 20);
            Rect bbox = canvas.textBBox(fitted, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
            canvas.drawText(rect.x + rect.width - bbox.width - 14,
                            y + std::max(4, (height - (context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny->getSize() : 20)) / 2),
                            fitted, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                            canvas.getTheme().muted);
        }
        y += height + gap;
    };

    auto stepperRow = [&](const std::string& label, const std::string& value, const std::string& action) {
        Rect rect{ margin, y, canvas.getWidth() - 2 * margin, height };
        canvas.drawRoundedRect(rect.x, rect.y, rect.width, rect.height, 9, canvas.getTheme().mid, 1);
        canvas.drawText(rect.x + 14, y + std::max(4, (height - (context.getFonts() && context.getFonts()->small ? context.getFonts()->small->getSize() : 28)) / 2),
                        label, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);

        int btnSize = std::min(50, height - 8);
        Rect plusRect{ rect.x + rect.width - btnSize - 8, y + (height - btnSize) / 2, btnSize, btnSize };
        int valWidth = 78;
        Rect valRect{ plusRect.x - valWidth - 8, y + (height - btnSize) / 2, valWidth, btnSize };
        Rect minusRect{ valRect.x - btnSize - 8, y + (height - btnSize) / 2, btnSize, btnSize };

        canvas.button(minusRect, "-", true, context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        canvas.button(plusRect, "+", true, context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        canvas.drawCenteredText(valRect.x + valRect.width / 2, valRect.y + valRect.height / 2, value,
                                context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);

        m_rects[action + "_down"] = minusRect;
        m_rects[action + "_down_0"] = minusRect;
        m_rects[action + "_up"] = plusRect;
        m_rects[action + "_up_0"] = plusRect;

        y += height + gap;
    };

    std::string server = context.getApi() ? context.getApi()->getServer() : "https://api.lightnovel.life";
    row("服务器", server, "");

    int fontSize = config ? config->getInt("font_size", 48) : 48;
    stepperRow("正文字号", std::to_string(fontSize), "font");

    double spacing = config ? config->getDouble("line_spacing", 1.42) : 1.42;
    std::ostringstream spOss;
    spOss << std::fixed << std::setprecision(2) << spacing;
    stepperRow("行距", spOss.str(), "spacing");

    bool night = config ? config->getBool("night_mode", false) : false;
    row("夜间模式", night ? "开" : "关", "night");

    bool indent = config ? config->getBool("first_line_indent", true) : true;
    row("首行缩进", indent ? "开" : "关", "indent");

    std::string convert = config ? config->getString("convert", "") : "";
    std::string convStr = "关闭";
    if (convert == "t2s") convStr = "繁转简";
    else if (convert == "s2t") convStr = "简转繁";
    row("简繁转换", convStr, "convert");

    bool ignoreJap = config ? config->getBool("ignore_japanese", false) : false;
    row("忽略日文", ignoreJap ? "开" : "关", "ignore_japanese");

    bool ignoreAi = config ? config->getBool("ignore_ai", false) : false;
    row("忽略 AI", ignoreAi ? "开" : "关", "ignore_ai");

    bool prefetch = config ? config->getBool("prefetch_chapters", true) : true;
    row("预加载章节", prefetch ? "开" : "关", "prefetch");

    bool anim = config ? config->getBool("page_turn_animation", true) : true;
    row("翻页动画（仅较新型号支持）", anim ? "开" : "关", "animation");

    bool flash = config ? config->getBool("page_flash", false) : false;
    row("翻页闪屏", flash ? "开" : "关", "flash");

    uint64_t cacheSz = 0;
    std::string cacheDir = core::Config::getCacheDir();
    cacheSz += core::Utils::directorySize(cacheDir + "/covers");
    cacheSz += core::Utils::directorySize(cacheDir + "/images");
    cacheSz += core::Utils::directorySize(cacheDir + "/fonts");
    cacheSz += core::Utils::directorySize(cacheDir + "/content");
    double cacheMb = static_cast<double>(cacheSz) / (1024.0 * 1024.0);
    std::ostringstream cszOss;
    cszOss << std::fixed << std::setprecision(1) << cacheMb << " MB";
    row("缓存", cszOss.str(), "clear_cache");

    bool loggedIn = context.getApi() && context.getApi()->getUserId() > 0;
    row(loggedIn ? "退出登录" : "登录账号", loggedIn ? "已登录" : "未登录", "account");
}

bool SettingsPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    auto config = context.getConfig();
    if (!config) return false;

    int x = event.point.xPixel;
    int y = event.point.yPixel;

    if (m_rects["font_down"].contains(x, y)) {
        int cur = config->getInt("font_size", 48);
        config->setInt("font_size", std::max(20, std::min(64, cur - 2)));
        context.show();
        return true;
    }
    if (m_rects["font_up"].contains(x, y)) {
        int cur = config->getInt("font_size", 48);
        config->setInt("font_size", std::max(20, std::min(64, cur + 2)));
        context.show();
        return true;
    }

    if (m_rects["spacing_down"].contains(x, y)) {
        double cur = config->getDouble("line_spacing", 1.42);
        config->setDouble("line_spacing", std::round(std::max(1.0, std::min(2.0, cur - 0.05)) * 100.0) / 100.0);
        context.show();
        return true;
    }
    if (m_rects["spacing_up"].contains(x, y)) {
        double cur = config->getDouble("line_spacing", 1.42);
        config->setDouble("line_spacing", std::round(std::max(1.0, std::min(2.0, cur + 0.05)) * 100.0) / 100.0);
        context.show();
        return true;
    }

    if (m_rects["night"].contains(x, y)) {
        config->setBool("night_mode", !config->getBool("night_mode", false));
        context.show();
        return true;
    }
    if (m_rects["indent"].contains(x, y)) {
        config->setBool("first_line_indent", !config->getBool("first_line_indent", true));
        context.show();
        return true;
    }
    if (m_rects["convert"].contains(x, y)) {
        std::string cur = config->getString("convert", "");
        if (cur.empty()) config->setString("convert", "t2s");
        else if (cur == "t2s") config->setString("convert", "s2t");
        else config->setString("convert", "");
        context.show();
        return true;
    }
    if (m_rects["ignore_japanese"].contains(x, y)) {
        config->setBool("ignore_japanese", !config->getBool("ignore_japanese", false));
        context.show();
        return true;
    }
    if (m_rects["ignore_ai"].contains(x, y)) {
        config->setBool("ignore_ai", !config->getBool("ignore_ai", false));
        context.show();
        return true;
    }
    if (m_rects["prefetch"].contains(x, y)) {
        config->setBool("prefetch_chapters", !config->getBool("prefetch_chapters", true));
        context.show();
        return true;
    }
    if (m_rects["animation"].contains(x, y)) {
        config->setBool("page_turn_animation", !config->getBool("page_turn_animation", true));
        context.show();
        return true;
    }
    if (m_rects["flash"].contains(x, y)) {
        config->setBool("page_flash", !config->getBool("page_flash", false));
        context.show();
        return true;
    }
    if (m_rects["clear_cache"].contains(x, y)) {
        context.confirm("确认清空封面、正文和字体的磁盘缓存？", [&context]() {
            std::string cacheDir = core::Config::getCacheDir();
            core::Utils::clearCache(cacheDir + "/covers");
            core::Utils::clearCache(cacheDir + "/images");
            core::Utils::clearCache(cacheDir + "/fonts");
            core::Utils::clearCache(cacheDir + "/content");
            if (context.getImages()) {
                context.getImages()->clearMemory();
            }
            context.toast("缓存已清空");
        });
        return true;
    }
    if (m_rects["account"].contains(x, y)) {
        context.navigate("account");
        return true;
    }

    return false;
}

// ==================== AboutPage ====================

const std::vector<std::string>& AboutPage::getAboutLines() {
    static const std::vector<std::string> lines = {
        "KinNovel 0.6.0",
        "运行于 Kindle 原生系统的轻书架客户端",
        "",
        "开发参考",
        "LightNovelShelf/Web",
        "接口、业务逻辑、阅读器行为、章节字体机制",
        "kComics",
        "framebuffer、EPDC、evdev、启动和恢复流程",
        "KOReader",
        "休眠与电源事件调度、内置 FreeType 与 WOFF2 字体支持",
        "",
        "主要依赖",
        "C++17、FreeType2、libcurl、OpenSSL、yyjson、fbink",
        "",
        "本项目按 GPLv3 发布。",
        "LightNovelShelf 内容与接口归原站及其权利人所有。",
        "请遵守站点规则和内容版权。",
    };
    return lines;
}

void AboutPage::enter(PageContext& context) {
    (void)context;
}

void AboutPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("关于", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.07);
    int y = top + 24;

    const auto& lines = getAboutLines();
    for (const auto& line : lines) {
        if (line.empty()) {
            y += 18;
            continue;
        }

        bool isHeading = (line == "KinNovel 0.6.0" || line == "开发参考" || line == "主要依赖");
        auto font = isHeading ? (context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr)
                              : (context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        uint8_t fill = isHeading ? canvas.getTheme().foreground : canvas.getTheme().muted;

        canvas.drawText(margin, y, line, font, fill);
        y += (font ? font->getSize() : 28) + 8;
    }
}

bool AboutPage::handle(const hal::TouchGesture& event, PageContext& context) {
    (void)event;
    (void)context;
    return false;
}

} // namespace kinnovel::ui
