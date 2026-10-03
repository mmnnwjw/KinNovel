#include "kinnovel/ui/pages/AccountPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/core/Utils.hpp"
#include "yyjson.h"

#include <algorithm>

namespace kinnovel::ui {

void AccountPage::enter(PageContext& context) {
    auto api = context.getApi();
    if (api && api->getUserId() > 0) {
        m_mode = "profile";
        m_error.clear();
        context.show();
        return;
    }

    m_mode = "login";
    context.show();
    if (!m_loading && !m_attempted) {
        autoLogin(context, false);
    }
}

void AccountPage::autoLogin(PageContext& context, bool force) {
    if (m_loading) return;
    if (m_attempted && !force) return;

    auto config = context.getConfig();
    std::string email = config ? config->getString("account_email", "") : "";
    std::string password = config ? config->getString("account_password", "") : "";

    m_attempted = true;
    m_error.clear();

    if (email.empty() || password.empty()) {
        m_error = "配置文件中未设置账号或密码";
        context.show();
        return;
    }

    m_loading = true;
    context.show();

    context.runAsync("account", [this, &context, email, password]() {
        auto api = context.getApi();
        if (!api) return;
        try {
            api->login(email, password);
            m_loading = false;
            m_mode = "profile";
            m_error.clear();
        } catch (const std::exception& exc) {
            m_loading = false;
            m_error = exc.what();
        }
    });
}

void AccountPage::renderProfile(PageContext& context, Canvas& canvas) {
    int top = canvas.header("我的账号", "返回", "主页");
    auto api = context.getApi();

    UserProfile prof;
    if (api) {
        std::string userJson = api->getUser();
        if (!userJson.empty()) {
            yyjson_doc* doc = yyjson_read(userJson.c_str(), userJson.size(), 0);
            if (doc) {
                yyjson_val* root = yyjson_doc_get_root(doc);
                if (yyjson_is_obj(root)) {
                    yyjson_val* idVal = yyjson_obj_get(root, "Id");
                    yyjson_val* unVal = yyjson_obj_get(root, "UserName");
                    yyjson_val* emVal = yyjson_obj_get(root, "Email");
                    yyjson_val* lvVal = yyjson_obj_get(root, "Level");

                    if (idVal && yyjson_is_num(idVal)) prof.id = yyjson_get_int(idVal);
                    if (unVal && yyjson_is_str(unVal)) prof.username = yyjson_get_str(unVal);
                    if (emVal && yyjson_is_str(emVal)) prof.email = yyjson_get_str(emVal);
                    if (lvVal && yyjson_is_num(lvVal)) prof.level = yyjson_get_int(lvVal);

                    yyjson_val* gwVal = yyjson_obj_get(root, "Growth");
                    if (yyjson_is_obj(gwVal)) {
                        yyjson_val* expVal = yyjson_obj_get(gwVal, "Exp");
                        yyjson_val* coinVal = yyjson_obj_get(gwVal, "Coin");
                        yyjson_val* cqVal = yyjson_obj_get(gwVal, "ComicQuota");
                        yyjson_val* cqtVal = yyjson_obj_get(gwVal, "ComicQuotaToday");
                        yyjson_val* ssVal = yyjson_obj_get(gwVal, "SignStreak");

                        if (expVal && yyjson_is_num(expVal)) prof.exp = yyjson_get_int(expVal);
                        if (coinVal && yyjson_is_num(coinVal)) prof.coin = yyjson_get_int(coinVal);
                        if (cqVal && yyjson_is_num(cqVal)) prof.comicQuota = yyjson_get_int(cqVal);
                        if (cqtVal && yyjson_is_num(cqtVal)) prof.comicQuotaToday = yyjson_get_int(cqtVal);
                        if (ssVal && yyjson_is_num(ssVal)) prof.signStreak = yyjson_get_int(ssVal);
                    }
                }
                yyjson_doc_free(doc);
            }
        }
    }

    int margin = static_cast<int>(canvas.getWidth() * 0.07);
    int width = canvas.getWidth() - 2 * margin;
    int y = top + 24;

    std::vector<std::pair<std::string, std::string>> fields = {
        { "用户名", prof.username.empty() ? "未知" : prof.username },
        { "邮箱", prof.email },
        { "等级", std::to_string(prof.level) },
        { "经验", std::to_string(prof.exp) },
        { "金币", std::to_string(prof.coin) },
        { "漫画额度", std::to_string(prof.comicQuota) + " 永久 / " + std::to_string(prof.comicQuotaToday) + " 今日" },
        { "连续签到", std::to_string(prof.signStreak) + " 天" },
    };

    m_rects.clear();
    for (const auto& f : fields) {
        canvas.drawText(margin, y, f.first,
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr,
                        canvas.getTheme().muted);
        canvas.drawText(margin + 180, y, f.second,
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        y += 52;
    }

    std::vector<std::pair<std::string, std::string>> buttons = {
        { "sign", "每日签到" },
        { "notifications", "通知" },
        { "shop", "商城" }
    };

    int gap = 12;
    int btnWidth = (width - gap) / 2;
    for (size_t i = 0; i < buttons.size(); ++i) {
        int r = static_cast<int>(i) / 2;
        int c = static_cast<int>(i) % 2;
        Rect rect{ margin + c * (btnWidth + gap), y + r * 76, btnWidth, 62 };
        canvas.button(rect, buttons[i].second, true,
                      context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        m_rects[buttons[i].first] = rect;
        m_rects[buttons[i].first + "_0"] = rect;
    }
}

void AccountPage::render(PageContext& context, Canvas& canvas) {
    if (m_mode == "profile" && context.getApi() && context.getApi()->getUserId() > 0) {
        renderProfile(context, canvas);
        return;
    }

    int top = canvas.header("账号", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.10);
    int width = canvas.getWidth() - 2 * margin;
    int y = top + 60;

    canvas.drawCenteredText(canvas.getWidth() / 2, y, "自动登录",
                            context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    y += 80;

    auto config = context.getConfig();
    std::string email = config ? config->getString("account_email", "") : "";
    std::string password = config ? config->getString("account_password", "") : "";
    bool configured = !email.empty() && !password.empty();

    std::vector<std::string> lines = {
        "账号来源: bin/config.json",
        configured ? "账号状态: 已配置" : "账号状态: 未配置",
        "",
        "无需输入账号密码。",
        "启动后会使用配置文件凭据自动登录。",
    };

    if (!configured) {
        lines.push_back("");
        lines.push_back("请在设备上编辑:");
        lines.push_back("/mnt/us/extensions/kinnovel/bin/config.json");
        lines.push_back("设置 account_email 和 account_password。");
    }

    if (m_loading) {
        lines.push_back("");
        lines.push_back("正在登录…");
    }

    if (!m_error.empty()) {
        lines.push_back("");
        lines.push_back("登录失败: " + m_error.substr(0, 80));
    }

    for (const auto& line : lines) {
        uint8_t fill = line.empty() ? canvas.getTheme().foreground : canvas.getTheme().muted;
        canvas.drawText(margin, y, line,
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, fill);
        y += (context.getFonts() && context.getFonts()->small ? context.getFonts()->small->getSize() : 28) + 9;
    }

    m_rects.clear();
    if (configured && !m_loading) {
        Rect rect{ margin, y + 24, width, 62 };
        canvas.button(rect, "重试自动登录", true,
                      context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        m_rects["retry"] = rect;
        m_rects["retry_0"] = rect;
    }
}

void AccountPage::signIn(PageContext& context) {
    context.runAsync("account", [&context]() {
        auto api = context.getApi();
        if (!api) return;
        std::string res = api->signIn();
        int reward = 0;
        if (!res.empty()) {
            yyjson_doc* doc = yyjson_read(res.c_str(), res.size(), 0);
            if (doc) {
                yyjson_val* root = yyjson_doc_get_root(doc);
                yyjson_val* rVal = yyjson_obj_get(root, "Reward");
                if (rVal && yyjson_is_num(rVal)) reward = yyjson_get_int(rVal);
                yyjson_doc_free(doc);
            }
        }
        api->refreshUser();
        context.toast("签到成功 +" + std::to_string(reward) + " 经验");
    });
}

bool AccountPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int x = event.point.xPixel;
    int y = event.point.yPixel;

    if (m_rects["retry"].contains(x, y)) {
        autoLogin(context, true);
        return true;
    }
    if (m_rects["sign"].contains(x, y)) {
        signIn(context);
        return true;
    }
    if (m_rects["notifications"].contains(x, y)) {
        context.navigate("notifications");
        return true;
    }
    if (m_rects["shop"].contains(x, y)) {
        context.navigate("shop");
        return true;
    }

    return false;
}

// ==================== NotificationsPage ====================

std::tuple<int, int, int> NotificationsPage::layout(int height) {
    int top = std::max(72, static_cast<int>(height * 0.085));
    int rowH = std::max(74, static_cast<int>(height * 0.061));
    int perPage = std::max(1, (height - top - 150) / rowH);
    return { top, rowH, perPage };
}

void NotificationsPage::enter(PageContext& context) {
    m_page = 1;
    load(context, 1);
}

void NotificationsPage::load(PageContext& context, int page) {
    m_loading = true;
    m_page = std::max(1, page);

    auto [top, rowH, perPage] = layout(context.getHeight());
    int pPage = perPage;

    context.runAsync("notifications", [this, &context, pPage]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getNotifications(m_page, pPage);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        std::vector<NotificationItem> items;
        int total = 1;
        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* tpVal = yyjson_obj_get(root, "TotalPages");
            if (tpVal && yyjson_is_num(tpVal)) total = yyjson_get_int(tpVal);

            yyjson_val* arr = yyjson_obj_get(root, "Data");
            if (yyjson_is_arr(arr)) {
                size_t idx, max;
                yyjson_val* val;
                yyjson_arr_foreach(arr, idx, max, val) {
                    NotificationItem ni;
                    yyjson_val* idVal = yyjson_obj_get(val, "Id");
                    yyjson_val* tVal = yyjson_obj_get(val, "Title");
                    yyjson_val* cVal = yyjson_obj_get(val, "Content");
                    yyjson_val* caVal = yyjson_obj_get(val, "CreatedAt");
                    yyjson_val* irVal = yyjson_obj_get(val, "IsRead");

                    if (idVal && yyjson_is_num(idVal)) ni.id = yyjson_get_int(idVal);
                    if (tVal && yyjson_is_str(tVal)) ni.title = yyjson_get_str(tVal);
                    if (cVal && yyjson_is_str(cVal)) ni.content = yyjson_get_str(cVal);
                    if (caVal && yyjson_is_str(caVal)) ni.createdAt = yyjson_get_str(caVal);
                    if (irVal && yyjson_is_bool(irVal)) ni.isRead = yyjson_get_bool(irVal);

                    items.push_back(std::move(ni));
                }
            }
        }
        yyjson_doc_free(doc);

        m_items = std::move(items);
        m_totalPages = std::max(1, total);
        m_loaded = true;
        m_loading = false;
    });
}

void NotificationsPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("通知", "返回", "主页");
    auto [_, rowH, perPage] = layout(canvas.getHeight());
    int margin = static_cast<int>(canvas.getWidth() * 0.035);

    m_rects.clear();
    for (int r = 0; r < perPage; ++r) {
        int y = top + 12 + r * rowH;
        Rect rect{ margin, y, canvas.getWidth() - 2 * margin, rowH - 6 };
        m_rects["item_" + std::to_string(r)] = rect;

        if (r >= static_cast<int>(m_items.size())) {
            continue;
        }

        const auto& item = m_items[r];
        canvas.drawRoundedRect(rect.x, rect.y, rect.width, rect.height, 8, canvas.getTheme().mid, 1);
        std::string date = item.createdAt.substr(0, 10);
        std::string label = "[" + date + "] " + item.title;

        canvas.drawText(rect.x + 12, y + 8,
                        canvas.fitText(label, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, rect.width - 24),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        canvas.drawText(rect.x + 12, y + 40,
                        canvas.fitText(item.content, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr, rect.width - 24),
                        context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                        canvas.getTheme().muted);
    }

    int navY = canvas.getHeight() - 72;
    int navW = static_cast<int>(canvas.getWidth() * 0.25);
    Rect prevRect{ margin, navY, navW, 54 };
    Rect countRect{ (canvas.getWidth() - navW) / 2, navY, navW, 54 };
    Rect nextRect{ canvas.getWidth() - margin - navW, navY, navW, 54 };

    canvas.button(prevRect, "上一页", m_page > 1,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
    canvas.button(countRect, std::to_string(m_page) + "/" + std::to_string(m_totalPages), true,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
    canvas.button(nextRect, "下一页", m_page < m_totalPages,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);

    m_rects["prev"] = prevRect;
    m_rects["count"] = countRect;
    m_rects["next"] = nextRect;

    if (m_loading) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    } else if (m_loaded && m_items.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "暂无通知",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

bool NotificationsPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int x = event.point.xPixel;
    int y = event.point.yPixel;

    if (m_rects["prev"].contains(x, y)) {
        if (m_page > 1) {
            load(context, m_page - 1);
            context.show();
        }
        return true;
    }
    if (m_rects["next"].contains(x, y)) {
        if (m_page < m_totalPages) {
            load(context, m_page + 1);
            context.show();
        }
        return true;
    }

    return false;
}

// ==================== ShopPage ====================

void ShopPage::enter(PageContext& context) {
    (void)context;
}

void ShopPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("商城", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.05);
    int y = top + 24;

    canvas.drawCenteredText(canvas.getWidth() / 2, y + 40, "轻小说商城",
                            context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);

    m_rects.clear();
    Rect r{ margin, y + 100, canvas.getWidth() - 2 * margin, 64 };
    canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid, 1);
    canvas.drawText(r.x + 14, r.y + 20, "永久漫画额度 +1 (100金币)",
                    context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
    m_rects["buy_quota"] = r;
}

bool ShopPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int x = event.point.xPixel;
    int y = event.point.yPixel;

    if (m_rects["buy_quota"].contains(x, y)) {
        context.confirm("购买永久漫画额度？", [&context]() {
            context.runAsync("shop", [&context]() {
                auto api = context.getApi();
                if (api) {
                    api->buyShopItem("ComicQuotaPermanent", 1);
                    api->refreshUser();
                }
                context.toast("购买成功");
            });
        });
        return true;
    }

    return false;
}

} // namespace kinnovel::ui
