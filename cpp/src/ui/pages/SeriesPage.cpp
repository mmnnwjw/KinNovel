#include "kinnovel/ui/pages/SeriesPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "yyjson.h"

#include <algorithm>

namespace kinnovel::ui {

void SeriesPage::enter(PageContext& context) {
    const auto& params = context.getParams();
    auto tIt = params.find("title");
    m_title = (tIt != params.end() && !tIt->second.empty()) ? tIt->second : "系列";

    auto snIt = params.find("series_name");
    m_seriesName = (snIt != params.end()) ? snIt->second : "";

    auto idIt = params.find("current_id");
    if (idIt != params.end()) {
        try {
            m_currentId = std::stoi(idIt->second);
        } catch (...) {
            m_currentId = 0;
        }
    }

    m_page = 0;
    m_totalPages = 1;
    m_rects.clear();
    m_loading = false;

    // Notice: series page does NOT prefetch covers!
    bool hasOther = false;
    for (const auto& item : m_items) {
        if (item.id != m_currentId) {
            hasOther = true;
            break;
        }
    }

    m_loaded = hasOther || m_seriesName.empty();
    if (!hasOther && !m_seriesName.empty()) {
        load(context);
    }
}

void SeriesPage::load(PageContext& context) {
    m_loading = true;
    uint64_t gen = ++m_generation;

    bool ignoreJap = context.getConfig() ? context.getConfig()->getBool("ignore_japanese", false) : false;
    bool ignoreAi = context.getConfig() ? context.getConfig()->getBool("ignore_ai", false) : false;

    context.runAsync("series", [this, &context, gen, ignoreJap, ignoreAi]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getBooksBySeries(m_seriesName, 1, 24, "latest", ignoreJap, ignoreAi);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        if (gen != m_generation) {
            yyjson_doc_free(doc);
            return;
        }

        yyjson_val* root = yyjson_doc_get_root(doc);
        std::vector<SeriesBook> items;
        int total = 1;
        if (yyjson_is_obj(root)) {
            yyjson_val* tpVal = yyjson_obj_get(root, "TotalPages");
            if (tpVal && yyjson_is_num(tpVal)) total = yyjson_get_int(tpVal);

            yyjson_val* arr = yyjson_obj_get(root, "Data");
            if (yyjson_is_arr(arr)) {
                size_t idx, max;
                yyjson_val* val;
                yyjson_arr_foreach(arr, idx, max, val) {
                    SeriesBook sb;
                    yyjson_val* idVal = yyjson_obj_get(val, "Id");
                    yyjson_val* titleVal = yyjson_obj_get(val, "Title");
                    yyjson_val* coverVal = yyjson_obj_get(val, "Cover");
                    if (idVal && yyjson_is_num(idVal)) sb.id = yyjson_get_int(idVal);
                    if (titleVal && yyjson_is_str(titleVal)) sb.title = yyjson_get_str(titleVal);
                    if (coverVal && yyjson_is_str(coverVal)) sb.cover = yyjson_get_str(coverVal);
                    items.push_back(std::move(sb));
                }
            }
        }
        yyjson_doc_free(doc);

        m_items = std::move(items);
        m_page = 0;
        m_totalPages = std::max(1, total);
        m_loaded = true;
        m_loading = false;
    });
}

void SeriesPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header(m_title.empty() ? "系列" : m_title, "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int rowH = std::max(76, static_cast<int>(canvas.getHeight() * 0.063));
    int startY = top + 12;
    int rows = std::max(1, (canvas.getHeight() - startY - 92) / rowH);

    int pages = std::max(1, (static_cast<int>(m_items.size()) + rows - 1) / rows);
    m_page = std::min(m_page, pages - 1);
    int start = m_page * rows;

    m_rects.clear();
    for (int row = 0; row < rows; ++row) {
        int idx = start + row;
        int y = startY + row * rowH;
        Rect r{ margin, y, canvas.getWidth() - 2 * margin, rowH - 7 };
        m_rects["item_" + std::to_string(row)] = r;
        m_rects["item_" + std::to_string(idx)] = r;

        if (idx >= static_cast<int>(m_items.size())) {
            continue;
        }

        const auto& item = m_items[idx];
        bool current = (item.id == m_currentId);

        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid,
                              current ? canvas.getTheme().inverseBg : canvas.getTheme().background, 1);
        int textX = r.x + 14;
        uint8_t fill = current ? canvas.getTheme().inverseFg : canvas.getTheme().foreground;

        canvas.drawText(textX, y + 9,
                        canvas.fitText(item.title.empty() ? "未知" : item.title,
                                       context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 28),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr,
                        fill);

        if (current) {
            canvas.drawText(textX, y + 42, "当前书籍",
                            context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                            canvas.getTheme().muted);
        }
    }

    int navY = canvas.getHeight() - 68;
    int navW = static_cast<int>(canvas.getWidth() * 0.25);
    Rect prevRect{ margin, navY, navW, 52 };
    Rect countRect{ (canvas.getWidth() - navW) / 2, navY, navW, 52 };
    Rect nextRect{ canvas.getWidth() - margin - navW, navY, navW, 52 };

    canvas.button(prevRect, "上一页", m_page > 0,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
    canvas.button(countRect, std::to_string(m_page + 1) + "/" + std::to_string(pages), true,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
    canvas.button(nextRect, "下一页", m_page < pages - 1,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);

    m_rects["prev"] = prevRect;
    m_rects["count"] = countRect;
    m_rects["next"] = nextRect;

    if (m_loading) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    } else if (m_loaded && m_items.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "本系列暂无其他书籍",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

bool SeriesPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["prev"].contains(px, py)) {
        if (m_page > 0) {
            m_page--;
            context.show();
        }
        return true;
    }

    int rowH = std::max(76, static_cast<int>(context.getHeight() * 0.063));
    int top = std::max(72, static_cast<int>(context.getHeight() * 0.085));
    int rows = std::max(1, (context.getHeight() - top - 12 - 92) / rowH);
    int pages = std::max(1, (static_cast<int>(m_items.size()) + rows - 1) / rows);

    if (m_rects["next"].contains(px, py)) {
        if (m_page < pages - 1) {
            m_page++;
            context.show();
        }
        return true;
    }

    int start = m_page * rows;
    for (int r = 0; r < rows; ++r) {
        int idx = start + r;
        std::string key = "item_" + std::to_string(idx);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(px, py)) {
            if (idx < static_cast<int>(m_items.size())) {
                if (m_items[idx].id != m_currentId) {
                    context.navigate("book", true, { { "book_id", std::to_string(m_items[idx].id) } });
                }
            }
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
