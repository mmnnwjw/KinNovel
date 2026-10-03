#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

struct AnnouncementItem {
    int id = 0;
    std::string title;
    std::string createdAt;
};

class AnnouncementsPage : public IPage {
public:
    AnnouncementsPage() = default;
    ~AnnouncementsPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context, int page = 1);

    static std::tuple<int, int, int> layout(int height);

private:
    std::vector<AnnouncementItem> m_items;
    int m_page = 1;
    int m_totalPages = 1;
    bool m_loading = false;
    bool m_loaded = false;
    uint64_t m_generation = 0;
    std::unordered_map<std::string, Rect> m_rects;
};

class AnnouncementDetailPage : public IPage {
public:
    AnnouncementDetailPage() = default;
    ~AnnouncementDetailPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

private:
    int m_id = 0;
    std::string m_title;
    std::string m_content;
    bool m_loading = false;
};

struct CommentItem {
    int id = 0;
    std::string username;
    std::string content;
    std::string createdAt;
};

class CommentsPage : public IPage {
public:
    CommentsPage() = default;
    ~CommentsPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context, int page = 1);

private:
    std::string m_commentType = "Book";
    int m_targetId = 0;
    std::vector<CommentItem> m_items;
    int m_page = 1;
    int m_totalPages = 1;
    bool m_loading = false;
    std::unordered_map<std::string, Rect> m_rects;
};

} // namespace kinnovel::ui
