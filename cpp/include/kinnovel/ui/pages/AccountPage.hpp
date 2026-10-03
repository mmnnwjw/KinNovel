#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

struct UserProfile {
    int id = 0;
    std::string username;
    std::string email;
    int level = 0;
    int exp = 0;
    int coin = 0;
    int comicQuota = 0;
    int comicQuotaToday = 0;
    int signStreak = 0;
};

class AccountPage : public IPage {
public:
    AccountPage() = default;
    ~AccountPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void autoLogin(PageContext& context, bool force = false);

private:
    std::string m_mode = "login";
    bool m_loading = false;
    bool m_attempted = false;
    std::string m_error;
    UserProfile m_profile;
    std::unordered_map<std::string, Rect> m_rects;

    void renderProfile(PageContext& context, Canvas& canvas);
    void signIn(PageContext& context);
};

struct NotificationItem {
    int id = 0;
    std::string title;
    std::string content;
    std::string createdAt;
    bool isRead = false;
};

class NotificationsPage : public IPage {
public:
    NotificationsPage() = default;
    ~NotificationsPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context, int page = 1);

    static std::tuple<int, int, int> layout(int height);

private:
    std::vector<NotificationItem> m_items;
    int m_page = 1;
    int m_totalPages = 1;
    bool m_loading = false;
    bool m_loaded = false;
    std::unordered_map<std::string, Rect> m_rects;
};

class ShopPage : public IPage {
public:
    ShopPage() = default;
    ~ShopPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

private:
    std::unordered_map<std::string, Rect> m_rects;
};

} // namespace kinnovel::ui
