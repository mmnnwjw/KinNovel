#include "kinnovel/ui/pages/AnnouncementsPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "yyjson.h"

#include <regex>
#include <algorithm>
#include <sstream>

namespace kinnovel::ui {

std::tuple<int, int, int> AnnouncementsPage::layout(int height) {
    int top = std::max(72, static_cast<int>(height * 0.085));
    int rowH = std::max(74, static_cast<int>(height * 0.061));
    int perPage = std::max(1, (height - top - 150) / rowH);
    return { top, rowH, perPage };
}

void AnnouncementsPage::enter(PageContext& context) {
    m_page = 1;
    load(context, 1);
}

void AnnouncementsPage::load(PageContext& context, int page) {
    m_loading = true;
    m_page = std::max(1, page);
    uint64_t gen = ++m_generation;

    auto [top, rowH, perPage] = layout(context.getHeight());
    int pPage = perPage;

    context.runAsync("announcements", [this, &context, gen, pPage]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getAnnouncementList(m_page, pPage);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        if (gen != m_generation) {
            yyjson_doc_free(doc);
            return;
        }

        std::vector<AnnouncementItem> items;
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
                    AnnouncementItem ai;
                    yyjson_val* idVal = yyjson_obj_get(val, "Id");
                    yyjson_val* tVal = yyjson_obj_get(val, "Title");
                    yyjson_val* caVal = yyjson_obj_get(val, "CreatedAt");

                    if (idVal && yyjson_is_num(idVal)) ai.id = yyjson_get_int(idVal);
                    if (tVal && yyjson_is_str(tVal)) ai.title = yyjson_get_str(tVal);
                    if (caVal && yyjson_is_str(caVal)) ai.createdAt = yyjson_get_str(caVal);
                    items.push_back(std::move(ai));
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

void AnnouncementsPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("公告", "返回", "主页");
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
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "暂无公告",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

bool AnnouncementsPage::handle(const hal::TouchGesture& event, PageContext& context) {
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

    auto [_, rowH, perPage] = layout(context.getHeight());
    for (int r = 0; r < perPage; ++r) {
        std::string key = "item_" + std::to_string(r);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(x, y)) {
            if (r < static_cast<int>(m_items.size())) {
                context.navigate("announcement", true, {
                    { "announcement_id", std::to_string(m_items[r].id) }
                });
            }
            return true;
        }
    }

    return false;
}

// ==================== AnnouncementDetailPage ====================

void AnnouncementDetailPage::enter(PageContext& context) {
    const auto& params = context.getParams();
    auto aIt = params.find("announcement_id");
    if (aIt != params.end()) {
        try { m_id = std::stoi(aIt->second); } catch (...) {}
    }

    m_title.clear();
    m_content.clear();
    m_loading = true;

    int aid = m_id;
    context.runAsync("announcement", [this, &context, aid]() {
        auto api = context.getApi();
        if (!api || aid <= 0) return;
        std::string jsonStr = api->getAnnouncementDetail(aid);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* tVal = yyjson_obj_get(root, "Title");
            yyjson_val* cVal = yyjson_obj_get(root, "Content");
            if (tVal && yyjson_is_str(tVal)) m_title = yyjson_get_str(tVal);
            if (cVal && yyjson_is_str(cVal)) m_content = yyjson_get_str(cVal);
        }
        yyjson_doc_free(doc);
        m_loading = false;
    });
}

void AnnouncementDetailPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("公告详情", "返回", "主页");
    if (m_loading || m_title.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        return;
    }

    int margin = static_cast<int>(canvas.getWidth() * 0.05);
    int width = canvas.getWidth() - 2 * margin;
    int y = top + 14;

    auto titleLines = canvas.wrap(m_title, context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr, width);
    for (const auto& line : titleLines) {
        canvas.drawText(margin, y, line, context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        y += (context.getFonts() && context.getFonts()->body ? context.getFonts()->body->getSize() : 34) + 8;
    }
    y += 10;

    std::string text = m_content;
    text = std::regex_replace(text, std::regex("<br\\s*/?>", std::regex_constants::icase), "\n");
    text = std::regex_replace(text, std::regex("</p\\s*>", std::regex_constants::icase), "\n\n");
    text = std::regex_replace(text, std::regex("<[^>]+>"), "");
    size_t pos = 0;
    while ((pos = text.find("&nbsp;", pos)) != std::string::npos) {
        text.replace(pos, 6, " ");
        pos += 1;
    }
    pos = 0;
    while ((pos = text.find("&amp;", pos)) != std::string::npos) {
        text.replace(pos, 5, "&");
        pos += 1;
    }

    int lineHeight = std::max((context.getFonts() && context.getFonts()->small ? context.getFonts()->small->getSize() : 28) + 8, 42);
    std::istringstream stream(text);
    std::string paragraph;

    while (std::getline(stream, paragraph)) {
        auto lines = canvas.wrap(paragraph, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, width);
        for (const auto& line : lines) {
            if (y + lineHeight > canvas.getHeight() - 20) {
                canvas.drawText(margin, canvas.getHeight() - 28, "…",
                                context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
                return;
            }
            canvas.drawText(margin, y, line, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
            y += lineHeight;
        }
    }
}

bool AnnouncementDetailPage::handle(const hal::TouchGesture& event, PageContext& context) {
    (void)event;
    (void)context;
    return false;
}

// ==================== CommentsPage ====================

void CommentsPage::enter(PageContext& context) {
    const auto& params = context.getParams();
    auto ctIt = params.find("comment_type");
    m_commentType = (ctIt != params.end()) ? ctIt->second : "Book";

    auto tIt = params.find("target_id");
    if (tIt != params.end()) {
        try { m_targetId = std::stoi(tIt->second); } catch (...) {}
    }

    m_page = 1;
    load(context, 1);
}

void CommentsPage::load(PageContext& context, int page) {
    m_loading = true;
    m_page = std::max(1, page);

    int targetId = m_targetId;
    std::string type = m_commentType;

    context.runAsync("comments", [this, &context, targetId, type]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getComments(type, targetId, m_page);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        std::vector<CommentItem> items;
        int total = 1;
        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* tp = yyjson_obj_get(root, "TotalPages");
            if (tp && yyjson_is_num(tp)) total = yyjson_get_int(tp);

            yyjson_val* arr = yyjson_obj_get(root, "Data");
            if (yyjson_is_arr(arr)) {
                size_t idx, max;
                yyjson_val* val;
                yyjson_arr_foreach(arr, idx, max, val) {
                    CommentItem ci;
                    yyjson_val* idVal = yyjson_obj_get(val, "Id");
                    yyjson_val* unVal = yyjson_obj_get(val, "UserName");
                    yyjson_val* cVal = yyjson_obj_get(val, "Content");
                    yyjson_val* caVal = yyjson_obj_get(val, "CreatedAt");

                    if (idVal && yyjson_is_num(idVal)) ci.id = yyjson_get_int(idVal);
                    if (unVal && yyjson_is_str(unVal)) ci.username = yyjson_get_str(unVal);
                    if (cVal && yyjson_is_str(cVal)) ci.content = yyjson_get_str(cVal);
                    if (caVal && yyjson_is_str(caVal)) ci.createdAt = yyjson_get_str(caVal);
                    items.push_back(std::move(ci));
                }
            }
        }
        yyjson_doc_free(doc);

        m_items = std::move(items);
        m_totalPages = std::max(1, total);
        m_loading = false;
    });
}

void CommentsPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("评论", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int y = top + 14;

    m_rects.clear();
    for (size_t i = 0; i < std::min<size_t>(6, m_items.size()); ++i) {
        const auto& c = m_items[i];
        Rect r{ margin, y, canvas.getWidth() - 2 * margin, 84 };
        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid, 1);
        std::string date = c.createdAt.substr(0, 10);
        canvas.drawText(r.x + 12, y + 8, c.username + " · " + date,
                        context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                        canvas.getTheme().muted);
        canvas.drawText(r.x + 12, y + 36,
                        canvas.fitText(c.content, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 24),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        y += 92;
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
    } else if (m_items.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "暂无评论",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

bool CommentsPage::handle(const hal::TouchGesture& event, PageContext& context) {
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

} // namespace kinnovel::ui
