#include "kinnovel/ui/pages/BrowsePage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/core/Utils.hpp"
#include "yyjson.h"

#include <algorithm>

namespace kinnovel::ui {

void BrowsePage::calculateLayout(int height, int& top, int& filterY, int& filterH,
                                int& listY, int& rowH, int& perPage, int& navY) const {
    top = std::max(72, static_cast<int>(height * 0.085));
    filterY = top + 16;
    filterH = 60;
    listY = filterY + filterH + 18;
    rowH = std::max(64, static_cast<int>(height * 0.058));
    navY = height - 72;
    perPage = std::max(1, (navY - listY - 12) / rowH);
}

void BrowsePage::enter(PageContext& context) {
    m_page = 1;
    load(context, 1);
    if (m_categories.empty()) {
        loadCategories(context);
    }
}

void BrowsePage::loadCategories(PageContext& context) {
    context.runAsync("browse", [this, &context]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getBookCategories("Novel");
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_arr(root)) {
            size_t idx, max;
            yyjson_val* val;
            std::vector<CategoryItem> cats;
            yyjson_arr_foreach(root, idx, max, val) {
                yyjson_val* idVal = yyjson_obj_get(val, "Id");
                yyjson_val* nameVal = yyjson_obj_get(val, "Name");
                CategoryItem cat;
                if (idVal && yyjson_is_num(idVal)) cat.id = yyjson_get_int(idVal);
                if (nameVal && yyjson_is_str(nameVal)) cat.name = yyjson_get_str(nameVal);
                cats.push_back(std::move(cat));
            }
            m_categories = std::move(cats);
        }
        yyjson_doc_free(doc);
    });
}

void BrowsePage::load(PageContext& context, int page) {
    m_loading = true;
    m_page = std::max(1, page);
    uint64_t gen = ++m_generation;

    int top, fy, fh, ly, rh, perPage, navY;
    calculateLayout(context.getHeight(), top, fy, fh, ly, rh, perPage, navY);

    int categoryId = -1;
    if (m_categoryIndex > 0 && m_categoryIndex <= static_cast<int>(m_categories.size())) {
        categoryId = m_categories[m_categoryIndex - 1].id;
    }

    bool ignoreJap = context.getConfig() ? context.getConfig()->getBool("ignore_japanese", false) : false;
    bool ignoreAi = context.getConfig() ? context.getConfig()->getBool("ignore_ai", false) : false;

    context.runAsync("browse", [this, &context, gen, perPage, categoryId, ignoreJap, ignoreAi]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getBookList(m_page, perPage, "", m_order, ignoreJap, ignoreAi, categoryId);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        if (gen != m_generation) {
            yyjson_doc_free(doc);
            return;
        }

        yyjson_val* root = yyjson_doc_get_root(doc);
        std::vector<BookItem> items;
        int retPage = m_page;
        int total = 1;

        if (yyjson_is_obj(root)) {
            yyjson_val* pVal = yyjson_obj_get(root, "Page");
            yyjson_val* tVal = yyjson_obj_get(root, "TotalPages");
            if (pVal && yyjson_is_num(pVal)) retPage = yyjson_get_int(pVal);
            if (tVal && yyjson_is_num(tVal)) total = yyjson_get_int(tVal);

            yyjson_val* arr = yyjson_obj_get(root, "Data");
            if (yyjson_is_arr(arr)) {
                size_t idx, max;
                yyjson_val* val;
                yyjson_arr_foreach(arr, idx, max, val) {
                    BookItem bi;
                    yyjson_val* idVal = yyjson_obj_get(val, "Id");
                    yyjson_val* titleVal = yyjson_obj_get(val, "Title");
                    yyjson_val* userVal = yyjson_obj_get(val, "UserName");
                    yyjson_val* timeVal = yyjson_obj_get(val, "LastUpdatedAt");
                    if (idVal && yyjson_is_num(idVal)) bi.id = yyjson_get_int(idVal);
                    if (titleVal && yyjson_is_str(titleVal)) bi.title = yyjson_get_str(titleVal);
                    if (userVal && yyjson_is_str(userVal)) bi.author = yyjson_get_str(userVal);
                    if (timeVal && yyjson_is_str(timeVal)) bi.updatedAt = yyjson_get_str(timeVal);
                    items.push_back(std::move(bi));
                }
            }
        }

        yyjson_doc_free(doc);

        m_items = std::move(items);
        m_page = std::max(1, retPage);
        m_totalPages = std::max(1, total);
        m_loaded = true;
        m_loading = false;
    });
}

void BrowsePage::render(PageContext& context, Canvas& canvas) {
    int top, filterY, filterH, listY, rowH, perPage, navY;
    calculateLayout(canvas.getHeight(), top, filterY, filterH, listY, rowH, perPage, navY);

    canvas.header("最近/分类", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int gap = 10;

    std::string orderLabel = (m_order == "latest") ? "最近更新" : (m_order == "new" ? "上架时间" : "总点击");
    std::string catLabel = "全部类型";
    if (m_categoryIndex > 0 && m_categoryIndex <= static_cast<int>(m_categories.size())) {
        catLabel = m_categories[m_categoryIndex - 1].name;
    }

    std::vector<std::string> filters = { orderLabel, catLabel, "刷新" };
    int btnWidth = (canvas.getWidth() - 2 * margin - gap * 2) / 3;

    m_rects.clear();
    for (size_t i = 0; i < filters.size(); ++i) {
        Rect r{ margin + static_cast<int>(i) * (btnWidth + gap), filterY, btnWidth, filterH };
        canvas.button(r, filters[i], i != 2);
        m_rects["filter_" + std::to_string(i)] = r;
    }

    for (int row = 0; row < perPage; ++row) {
        int y = listY + row * rowH;
        Rect r{ margin, y, canvas.getWidth() - 2 * margin, rowH - 6 };
        m_rects["item_" + std::to_string(row)] = r;

        if (row >= static_cast<int>(m_items.size())) {
            continue;
        }

        const auto& item = m_items[row];
        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid, 1);
        canvas.drawCenteredText(r.x + 38, y + r.height / 2,
                                std::to_string((m_page - 1) * perPage + row + 1),
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);

        std::string title = item.title.empty() ? "未知" : item.title;
        std::string author = item.author.empty() ? "未知作者" : item.author;
        std::string updated = core::Utils::formatTime(item.updatedAt);

        canvas.drawText(r.x + 78, y + 8,
                        canvas.fitText(title, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 106),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        canvas.drawText(r.x + 78, y + 40,
                        canvas.fitText(author + " · " + updated, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr, r.width - 106),
                        context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                        canvas.getTheme().muted);
    }

    int navW = static_cast<int>(canvas.getWidth() * 0.25);
    Rect prevRect{ margin, navY, navW, 54 };
    Rect countRect{ (canvas.getWidth() - navW) / 2, navY, navW, 54 };
    Rect nextRect{ canvas.getWidth() - margin - navW, navY, navW, 54 };

    canvas.button(prevRect, "上一页", m_page > 1);
    canvas.button(countRect, std::to_string(m_page) + "/" + std::to_string(m_totalPages), true);
    canvas.button(nextRect, "下一页", m_page < m_totalPages);

    m_rects["prev"] = prevRect;
    m_rects["count"] = countRect;
    m_rects["next"] = nextRect;

    if (m_loading) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    } else if (m_loaded && m_items.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "暂无内容",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

bool BrowsePage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["prev"].contains(px, py)) {
        if (m_page > 1) {
            load(context, m_page - 1);
            context.show();
        }
        return true;
    }
    if (m_rects["next"].contains(px, py)) {
        if (m_page < m_totalPages) {
            load(context, m_page + 1);
            context.show();
        }
        return true;
    }
    if (m_rects["count"].contains(px, py)) {
        load(context, m_page);
        context.show();
        return true;
    }

    if (m_rects["filter_0"].contains(px, py)) {
        if (m_order == "latest") m_order = "new";
        else if (m_order == "new") m_order = "view";
        else m_order = "latest";
        load(context, 1);
        context.show();
        return true;
    }
    if (m_rects["filter_1"].contains(px, py)) {
        m_categoryIndex = (m_categoryIndex + 1) % (static_cast<int>(m_categories.size()) + 1);
        load(context, 1);
        context.show();
        return true;
    }
    if (m_rects["filter_2"].contains(px, py)) {
        load(context, m_page);
        context.show();
        return true;
    }

    for (size_t i = 0; i < m_items.size(); ++i) {
        std::string key = "item_" + std::to_string(i);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(px, py)) {
            context.navigate("book", true, { { "book_id", std::to_string(m_items[i].id) } });
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
