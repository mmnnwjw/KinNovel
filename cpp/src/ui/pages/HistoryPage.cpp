#include "kinnovel/ui/pages/HistoryPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "yyjson.h"

#include <algorithm>

namespace kinnovel::ui {

void HistoryPage::enter(PageContext& context) {
    load(context);
}

void HistoryPage::load(PageContext& context) {
    m_loading = true;

    context.runAsync("history", [this, &context]() {
        auto api = context.getApi();
        if (!api) return;

        std::string historyJson = api->getReadHistory();
        std::vector<int> ids;
        if (!historyJson.empty()) {
            yyjson_doc* doc = yyjson_read(historyJson.c_str(), historyJson.size(), 0);
            if (doc) {
                yyjson_val* root = yyjson_doc_get_root(doc);
                if (yyjson_is_obj(root)) {
                    yyjson_val* arr = yyjson_obj_get(root, "Novel");
                    if (yyjson_is_arr(arr)) {
                        size_t idx, max;
                        yyjson_val* val;
                        yyjson_arr_foreach(arr, idx, max, val) {
                            if (yyjson_is_num(val) && ids.size() < 24) {
                                ids.push_back(yyjson_get_int(val));
                            }
                        }
                    }
                }
                yyjson_doc_free(doc);
            }
        }

        std::vector<BookItem> items;
        if (!ids.empty()) {
            std::string booksJson = api->getBookListByIds(ids, "Novel");
            if (!booksJson.empty()) {
                yyjson_doc* bdoc = yyjson_read(booksJson.c_str(), booksJson.size(), 0);
                if (bdoc) {
                    yyjson_val* broot = yyjson_doc_get_root(bdoc);
                    if (yyjson_is_arr(broot)) {
                        size_t bidx, bmax;
                        yyjson_val* bval;
                        yyjson_arr_foreach(broot, bidx, bmax, bval) {
                            BookItem bi;
                            yyjson_val* bid = yyjson_obj_get(bval, "Id");
                            yyjson_val* bt = yyjson_obj_get(bval, "Title");
                            yyjson_val* bu = yyjson_obj_get(bval, "UserName");
                            if (bid && yyjson_is_num(bid)) bi.id = yyjson_get_int(bid);
                            if (bt && yyjson_is_str(bt)) bi.title = yyjson_get_str(bt);
                            if (bu && yyjson_is_str(bu)) bi.author = yyjson_get_str(bu);
                            items.push_back(std::move(bi));
                        }
                    }
                    yyjson_doc_free(bdoc);
                }
            }
        }

        m_items = std::move(items);
        m_page = 0;
        m_loaded = true;
        m_loading = false;
    });
}

void HistoryPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("阅读历史", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int rowH = std::max(76, static_cast<int>(canvas.getHeight() * 0.063));
    int startY = top + 12;
    int rows = std::max(1, (canvas.getHeight() - startY - 84) / rowH);

    int pages = std::max(1, (static_cast<int>(m_items.size()) + rows - 1) / rows);
    m_page = std::min(m_page, pages - 1);
    int start = m_page * rows;

    m_rects.clear();
    for (int row = 0; row < rows; ++row) {
        int idx = start + row;
        int y = startY + row * rowH;
        Rect r{ margin, y, canvas.getWidth() - 2 * margin, rowH - 6 };
        m_rects["item_" + std::to_string(row)] = r;
        m_rects["item_" + std::to_string(idx)] = r;

        if (idx >= static_cast<int>(m_items.size())) {
            continue;
        }

        const auto& item = m_items[idx];
        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid, 1);

        canvas.drawText(r.x + 12, y + 10,
                        canvas.fitText(item.title.empty() ? "未知" : item.title,
                                       context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 24),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        canvas.drawText(r.x + 12, y + 42,
                        item.author,
                        context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                        canvas.getTheme().muted);
    }

    int bottomY = canvas.getHeight() - 68;
    int gap = 8;
    int btnW = (canvas.getWidth() - 2 * margin - gap * 3) / 4;

    std::vector<std::tuple<std::string, std::string, bool>> buttons = {
        { "prev", "上一页", m_page > 0 },
        { "count", std::to_string(m_page + 1) + "/" + std::to_string(pages), true },
        { "next", "下一页", m_page < pages - 1 },
        { "clear", "清空", !m_items.empty() },
    };

    for (size_t i = 0; i < buttons.size(); ++i) {
        Rect r{ margin + static_cast<int>(i) * (btnW + gap), bottomY, btnW, 52 };
        canvas.button(r, std::get<1>(buttons[i]), std::get<2>(buttons[i]),
                      context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
        m_rects[std::get<0>(buttons[i])] = r;
        m_rects[std::get<0>(buttons[i]) + "_0"] = r;
    }

    if (m_loading) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    } else if (m_loaded && m_items.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "暂无阅读历史",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

void HistoryPage::clearHistory(PageContext& context) {
    context.runAsync("history", [this, &context]() {
        auto api = context.getApi();
        if (api) {
            api->clearReadHistory();
        }
        m_items.clear();
        m_page = 0;
        context.toast("已清空");
    });
}

bool HistoryPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["clear"].contains(px, py)) {
        if (!m_items.empty()) {
            context.confirm("确认清空阅读历史？", [this, &context]() {
                clearHistory(context);
            });
        }
        return true;
    }

    int rowH = std::max(76, static_cast<int>(context.getHeight() * 0.063));
    int top = std::max(72, static_cast<int>(context.getHeight() * 0.085));
    int rows = std::max(1, (context.getHeight() - top - 12 - 84) / rowH);
    int pages = std::max(1, (static_cast<int>(m_items.size()) + rows - 1) / rows);

    if (m_rects["prev"].contains(px, py)) {
        if (m_page > 0) {
            m_page--;
            context.show();
        }
        return true;
    }
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
                context.navigate("book", true, { { "book_id", std::to_string(m_items[idx].id) } });
            }
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
