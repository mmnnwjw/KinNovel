#include "kinnovel/ui/pages/RankPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "yyjson.h"

#include <algorithm>

namespace kinnovel::ui {

void RankPage::calculateLayout(int height, int& top, int& listY, int& rowH, int& perPage, int& navY) const {
    top = std::max(72, static_cast<int>(height * 0.085));
    listY = top + 84;
    rowH = std::max(64, static_cast<int>(height * 0.058));
    navY = height - 72;
    perPage = std::max(1, (navY - listY - 12) / rowH);
}

int RankPage::calculateTotalPages(int height) {
    int top, listY, rowH, perPage, navY;
    calculateLayout(height, top, listY, rowH, perPage, navY);
    m_totalPages = std::max(1, (static_cast<int>(m_items.size()) + perPage - 1) / perPage);
    m_page = std::max(1, std::min(m_page, m_totalPages));
    return m_totalPages;
}

void RankPage::enter(PageContext& context) {
    m_page = 1;
    load(context);
}

void RankPage::load(PageContext& context) {
    m_loading = true;
    m_page = 1;
    uint64_t gen = ++m_generation;

    int days = 1;
    if (m_kind == "weekly") days = 7;
    else if (m_kind == "monthly") days = 31;

    context.runAsync("rank", [this, &context, gen, days]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getRank(days);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        if (gen != m_generation) {
            yyjson_doc_free(doc);
            return;
        }

        yyjson_val* root = yyjson_doc_get_root(doc);
        std::vector<BookItem> items;
        if (yyjson_is_arr(root)) {
            size_t idx, max;
            yyjson_val* val;
            yyjson_arr_foreach(root, idx, max, val) {
                BookItem bi;
                yyjson_val* idVal = yyjson_obj_get(val, "Id");
                yyjson_val* titleVal = yyjson_obj_get(val, "Title");
                yyjson_val* userVal = yyjson_obj_get(val, "UserName");
                if (idVal && yyjson_is_num(idVal)) bi.id = yyjson_get_int(idVal);
                if (titleVal && yyjson_is_str(titleVal)) bi.title = yyjson_get_str(titleVal);
                if (userVal && yyjson_is_str(userVal)) bi.author = yyjson_get_str(userVal);
                items.push_back(std::move(bi));
            }
        }
        yyjson_doc_free(doc);

        m_items = std::move(items);
        calculateTotalPages(context.getHeight());
        m_loaded = true;
        m_loading = false;
    });
}

void RankPage::render(PageContext& context, Canvas& canvas) {
    int top, listY, rowH, perPage, navY;
    calculateLayout(canvas.getHeight(), top, listY, rowH, perPage, navY);

    canvas.header("排行榜", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int gap = 10;
    int tabW = (canvas.getWidth() - 2 * margin - 2 * gap) / 3;

    std::vector<std::pair<std::string, std::string>> tabs = {
        { "daily", "日榜" }, { "weekly", "周榜" }, { "monthly", "月榜" }
    };

    m_rects.clear();
    for (size_t i = 0; i < tabs.size(); ++i) {
        Rect r{ margin + static_cast<int>(i) * (tabW + gap), top + 12, tabW, 58 };
        canvas.button(r, tabs[i].second, m_kind == tabs[i].first,
                      context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        m_rects["kind_" + tabs[i].first] = r;
    }

    int totalPages = calculateTotalPages(canvas.getHeight());
    int start = (m_page - 1) * perPage;

    for (int row = 0; row < perPage; ++row) {
        int idx = start + row;
        int y = listY + row * rowH;
        Rect r{ margin, y, canvas.getWidth() - 2 * margin, rowH - 6 };
        m_rects["item_" + std::to_string(row)] = r;

        if (idx >= static_cast<int>(m_items.size())) {
            continue;
        }

        const auto& item = m_items[idx];
        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid, 1);
        canvas.drawCenteredText(r.x + 38, y + r.height / 2, std::to_string(idx + 1),
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);

        std::string title = item.title.empty() ? "未知" : item.title;
        std::string author = item.author.empty() ? "" : item.author;

        canvas.drawText(r.x + 78, y + 8,
                        canvas.fitText(title, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 106),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        canvas.drawText(r.x + 78, y + 40,
                        canvas.fitText(author, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr, r.width - 106),
                        context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                        canvas.getTheme().muted);
    }

    int navW = static_cast<int>(canvas.getWidth() * 0.25);
    Rect prevRect{ margin, navY, navW, 54 };
    Rect countRect{ (canvas.getWidth() - navW) / 2, navY, navW, 54 };
    Rect nextRect{ canvas.getWidth() - margin - navW, navY, navW, 54 };

    canvas.button(prevRect, "上一页", m_page > 1);
    canvas.button(countRect, std::to_string(m_page) + "/" + std::to_string(totalPages), true);
    canvas.button(nextRect, "下一页", m_page < totalPages);

    m_rects["prev"] = prevRect;
    m_rects["count"] = countRect;
    m_rects["next"] = nextRect;

    if (m_loading) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    }
}

bool RankPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["prev"].contains(px, py)) {
        if (m_page > 1) {
            m_page--;
            context.show();
        }
        return true;
    }
    if (m_rects["next"].contains(px, py)) {
        if (m_page < m_totalPages) {
            m_page++;
            context.show();
        }
        return true;
    }
    if (m_rects["count"].contains(px, py)) {
        return true;
    }

    if (m_rects["kind_daily"].contains(px, py)) {
        m_kind = "daily";
        load(context);
        context.show();
        return true;
    }
    if (m_rects["kind_weekly"].contains(px, py)) {
        m_kind = "weekly";
        load(context);
        context.show();
        return true;
    }
    if (m_rects["kind_monthly"].contains(px, py)) {
        m_kind = "monthly";
        load(context);
        context.show();
        return true;
    }

    int top, listY, rowH, perPage, navY;
    calculateLayout(context.getHeight(), top, listY, rowH, perPage, navY);
    int start = (m_page - 1) * perPage;

    for (int row = 0; row < perPage; ++row) {
        std::string key = "item_" + std::to_string(row);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(px, py)) {
            int idx = start + row;
            if (idx < static_cast<int>(m_items.size())) {
                context.navigate("book", true, { { "book_id", std::to_string(m_items[idx].id) } });
            }
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
