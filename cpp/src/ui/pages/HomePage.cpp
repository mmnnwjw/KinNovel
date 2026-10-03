#include "kinnovel/ui/pages/HomePage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "yyjson.h"

#include <algorithm>

namespace kinnovel::ui {

namespace {

static const std::vector<std::pair<std::string, std::string>> MODULES = {
    { "shelf", "书架" },
    { "history", "阅读历史" },
    { "rank", "排行榜" },
    { "browse", "最近/分类" },
    { "account", "我的账号" },
    { "settings", "设置" },
    { "about", "关于" },
    { "exit", "退出" },
    { "announcements", "公告" },
    { "notifications", "通知" },
    { "shop", "商城" },
};

static const std::unordered_map<std::string, int> DEFAULT_ORDER = {
    { "shelf", 0 },
    { "history", 1 },
    { "rank", 2 },
    { "browse", 3 },
    { "account", 4 },
    { "settings", 5 },
    { "about", 6 },
    { "exit", 7 },
    { "announcements", -1 },
    { "notifications", -1 },
    { "shop", -1 },
};

} // namespace

void HomePage::enter(PageContext& context) {
    (void)context;
}

std::vector<std::pair<std::string, std::string>> HomePage::getItems(PageContext& context) const {
    auto config = context.getConfig();
    struct ItemSort {
        int order;
        int fallbackIndex;
        std::string target;
        std::string label;
    };

    std::vector<ItemSort> sorted;
    for (size_t i = 0; i < MODULES.size(); ++i) {
        const auto& mod = MODULES[i];
        int fallbackOrder = -1;
        auto defIt = DEFAULT_ORDER.find(mod.first);
        if (defIt != DEFAULT_ORDER.end()) {
            fallbackOrder = defIt->second;
        } else {
            fallbackOrder = static_cast<int>(i);
        }

        int order = fallbackOrder;
        if (config) {
            std::string orderJson = config->getString("home_order", "");
            if (!orderJson.empty()) {
                yyjson_doc* doc = yyjson_read(orderJson.c_str(), orderJson.size(), 0);
                if (doc) {
                    yyjson_val* root = yyjson_doc_get_root(doc);
                    if (yyjson_is_obj(root)) {
                        yyjson_val* v = yyjson_obj_get(root, mod.first.c_str());
                        if (v && yyjson_is_num(v)) {
                            order = yyjson_get_int(v);
                        }
                    }
                    yyjson_doc_free(doc);
                }
            }
        }

        if (order < 0) {
            continue;
        }

        sorted.push_back({ order, static_cast<int>(i), mod.first, mod.second });
    }

    std::sort(sorted.begin(), sorted.end(), [](const ItemSort& a, const ItemSort& b) {
        if (a.order != b.order) return a.order < b.order;
        return a.fallbackIndex < b.fallbackIndex;
    });

    std::vector<std::pair<std::string, std::string>> result;
    for (const auto& item : sorted) {
        result.push_back({ item.target, item.label });
    }
    return result;
}

void HomePage::render(PageContext& context, Canvas& canvas) {
    int width = canvas.getWidth();
    int height = canvas.getHeight();
    auto fonts = context.getFonts();

    reader::FontFace* heroFont = fonts && fonts->hero ? fonts->hero.get() : nullptr;
    reader::FontFace* bodyFont = fonts && fonts->body ? fonts->body.get() : nullptr;
    reader::FontFace* smallFont = fonts && fonts->small ? fonts->small.get() : nullptr;

    canvas.drawCenteredText(width / 2, static_cast<int>(height * 0.10), "KinNovel", heroFont, canvas.getTheme().foreground);
    canvas.drawCenteredText(width / 2, static_cast<int>(height * 0.16), "Kindle 轻书架", bodyFont, canvas.getTheme().muted);

    auto api = context.getApi();
    std::string username;
    bool loggedIn = false;
    if (api && api->getUserId() > 0) {
        loggedIn = true;
        std::string userJson = api->getUser();
        if (!userJson.empty()) {
            yyjson_doc* doc = yyjson_read(userJson.c_str(), userJson.size(), 0);
            if (doc) {
                yyjson_val* root = yyjson_doc_get_root(doc);
                yyjson_val* u = yyjson_obj_get(root, "UserName");
                if (u && yyjson_is_str(u)) {
                    username = yyjson_get_str(u);
                }
                yyjson_doc_free(doc);
            }
        }
        if (username.empty()) username = "已登录";
    } else {
        username = "未登录";
    }

    std::string status = username;
    if (!m_onlineCount.empty()) {
        status += "  ·  在线 " + m_onlineCount;
    }
    canvas.drawCenteredText(width / 2, static_cast<int>(height * 0.20), status, smallFont, canvas.getTheme().muted);

    int margin = static_cast<int>(width * 0.055);
    int gap = std::max(12, static_cast<int>(width * 0.025));
    int columns = 2;
    int buttonWidth = (width - 2 * margin - gap * (columns - 1)) / columns;
    int buttonHeight = std::max(64, static_cast<int>(height * 0.074));
    int startY = static_cast<int>(height * 0.245);
    int rowGap = std::max(14, static_cast<int>(height * 0.018));

    m_rects.clear();
    auto items = getItems(context);
    for (size_t i = 0; i < items.size(); ++i) {
        int row = static_cast<int>(i) / columns;
        int col = static_cast<int>(i) % columns;
        int x = margin + col * (buttonWidth + gap);
        int y = startY + row * (buttonHeight + rowGap);
        Rect rect{ x, y, buttonWidth, buttonHeight };

        bool active = (items[i].first == "shelf") || loggedIn || (items[i].first != "account");
        canvas.button(rect, items[i].second, active);
        m_rects[items[i].first] = rect;
    }
}

bool HomePage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) {
        return false;
    }

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    for (const auto& kv : m_rects) {
        if (kv.second.contains(px, py)) {
            const std::string& target = kv.first;
            if (target == "exit") {
                context.stop();
            } else if ((target == "shelf" || target == "history" || target == "notifications" || target == "shop") &&
                       (!context.getApi() || context.getApi()->getUserId() <= 0)) {
                context.toast("请先登录");
                context.navigate("account");
            } else {
                context.navigate(target);
            }
            return true;
        }
    }
    return false;
}

} // namespace kinnovel::ui
