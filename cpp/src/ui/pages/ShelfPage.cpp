#include "kinnovel/ui/pages/ShelfPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "yyjson.h"

#include <algorithm>
#include <unordered_set>

namespace kinnovel::ui {

std::string ShelfPage::lastParent(const ShelfItem& item) const {
    if (item.parents.empty()) return "";
    return item.parents.back();
}

void ShelfPage::enter(PageContext& context) {
    auto api = context.getApi();
    if (!api || api->getUserId() <= 0) {
        context.navigate("account");
        return;
    }
    load(context);
}

void ShelfPage::load(PageContext& context) {
    m_loading = true;

    context.runAsync("shelf", [this, &context]() {
        auto api = context.getApi();
        if (!api) return;

        std::string shelfJson = api->getBookShelf();
        if (shelfJson.empty()) return;

        yyjson_doc* doc = yyjson_read(shelfJson.c_str(), shelfJson.size(), 0);
        if (!doc) return;

        std::vector<ShelfItem> items;
        std::unordered_set<std::string> folderIds;

        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* data = yyjson_obj_get(root, "data");
            if (yyjson_is_arr(data)) {
                size_t idx, max;
                yyjson_val* item;
                yyjson_arr_foreach(data, idx, max, item) {
                    ShelfItem si;
                    yyjson_val* idVal = yyjson_obj_get(item, "id");
                    yyjson_val* typeVal = yyjson_obj_get(item, "type");
                    yyjson_val* titleVal = yyjson_obj_get(item, "title");
                    yyjson_val* idxVal = yyjson_obj_get(item, "index");

                    if (idVal && yyjson_is_num(idVal)) si.id = yyjson_get_int(idVal);
                    if (typeVal && yyjson_is_str(typeVal)) si.type = yyjson_get_str(typeVal);
                    if (titleVal && yyjson_is_str(titleVal)) si.title = yyjson_get_str(titleVal);
                    if (idxVal && yyjson_is_num(idxVal)) si.index = yyjson_get_int(idxVal);

                    yyjson_val* pArr = yyjson_obj_get(item, "parents");
                    if (yyjson_is_arr(pArr)) {
                        size_t pidx, pmax;
                        yyjson_val* pval;
                        yyjson_arr_foreach(pArr, pidx, pmax, pval) {
                            if (yyjson_is_str(pval)) si.parents.push_back(yyjson_get_str(pval));
                            else if (yyjson_is_num(pval)) si.parents.push_back(std::to_string(yyjson_get_int(pval)));
                        }
                    }

                    if (si.type == "FOLDER") {
                        folderIds.insert(std::to_string(si.id));
                    }
                    items.push_back(std::move(si));
                }
            }
        }
        yyjson_doc_free(doc);

        std::vector<std::string> validPath;
        for (const auto& p : m_path) {
            if (folderIds.count(p)) validPath.push_back(p);
        }
        m_path = std::move(validPath);

        std::string currentParent = m_path.empty() ? "" : m_path.back();
        std::vector<ShelfItem> visible;
        std::vector<int> bookIds;

        for (const auto& it : items) {
            if (lastParent(it) == currentParent) {
                visible.push_back(it);
                if (it.type != "FOLDER" && bookIds.size() < 24) {
                    bookIds.push_back(it.id);
                }
            }
        }

        std::sort(visible.begin(), visible.end(), [](const ShelfItem& a, const ShelfItem& b) {
            return a.index < b.index;
        });

        std::unordered_map<int, BookItem> books;
        if (!bookIds.empty()) {
            std::string booksJson = api->getBookListByIds(bookIds, "Novel");
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
                            yyjson_val* bc = yyjson_obj_get(bval, "Cover");
                            if (bid && yyjson_is_num(bid)) bi.id = yyjson_get_int(bid);
                            if (bt && yyjson_is_str(bt)) bi.title = yyjson_get_str(bt);
                            if (bu && yyjson_is_str(bu)) bi.author = yyjson_get_str(bu);
                            if (bc && yyjson_is_str(bc)) bi.cover = yyjson_get_str(bc);

                            books[bi.id] = bi;
                            if (context.getImages() && !bi.cover.empty() && books.size() <= 8) {
                                context.getImages()->prefetch(bi.cover);
                            }
                        }
                    }
                    yyjson_doc_free(bdoc);
                }
            }
        }

        m_items = std::move(items);
        m_visible = std::move(visible);
        m_books = std::move(books);
        m_page = 0;
        m_loaded = true;
        m_loading = false;
    });
}

void ShelfPage::render(PageContext& context, Canvas& canvas) {
    std::string folderName = "根目录";
    if (!m_path.empty()) {
        for (const auto& it : m_items) {
            if (std::to_string(it.id) == m_path.back()) {
                folderName = it.title.empty() ? "文件夹" : it.title;
                break;
            }
        }
    }

    int top = canvas.header("书架 · " + folderName, "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int rowH = std::max(74, static_cast<int>(canvas.getHeight() * 0.060));
    int perPage = std::max(1, (canvas.getHeight() - top - 110) / rowH);

    int pages = std::max(1, (static_cast<int>(m_visible.size()) + perPage - 1) / perPage);
    m_page = std::min(m_page, pages - 1);
    int start = m_page * perPage;

    m_rects.clear();
    for (int row = 0; row < perPage; ++row) {
        int idx = start + row;
        int y = top + 12 + row * rowH;
        Rect r{ margin, y, canvas.getWidth() - 2 * margin, rowH - 6 };
        m_rects["item_" + std::to_string(row)] = r;
        m_rects["item_" + std::to_string(idx)] = r;

        if (idx >= static_cast<int>(m_visible.size())) {
            continue;
        }

        const auto& item = m_visible[idx];
        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 8, canvas.getTheme().mid, 1);

        std::string title;
        std::string subtitle;
        if (item.type == "FOLDER") {
            title = "文件夹  " + (item.title.empty() ? "未命名" : item.title);
            subtitle = "点击进入";
        } else {
            auto bIt = m_books.find(item.id);
            if (bIt != m_books.end()) {
                title = bIt->second.title.empty() ? ("书籍 #" + std::to_string(item.id)) : bIt->second.title;
                subtitle = bIt->second.author;
            } else {
                title = "书籍 #" + std::to_string(item.id);
                subtitle = "";
            }
        }

        canvas.drawText(r.x + 14, y + 8,
                        canvas.fitText(title, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 28),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        canvas.drawText(r.x + 14, y + 40,
                        canvas.fitText(subtitle, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr, r.width - 28),
                        context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr,
                        canvas.getTheme().muted);
    }

    int bottomY = canvas.getHeight() - 72;
    int gap = 8;
    int btnW = (canvas.getWidth() - 2 * margin - gap * 3) / 4;

    std::vector<std::tuple<std::string, std::string, bool>> buttons = {
        { "up", "上一页", m_page > 0 },
        { "sync", "同步", true },
        { "prev_folder", "上一层", !m_path.empty() },
        { "down", "下一页 (" + std::to_string(m_page + 1) + "/" + std::to_string(pages) + ")", m_page < pages - 1 },
    };

    for (size_t i = 0; i < buttons.size(); ++i) {
        Rect r{ margin + static_cast<int>(i) * (btnW + gap), bottomY, btnW, 54 };
        canvas.button(r, std::get<1>(buttons[i]), std::get<2>(buttons[i]),
                      context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
        m_rects[std::get<0>(buttons[i])] = r;
        m_rects[std::get<0>(buttons[i]) + "_0"] = r;
    }

    if (m_loading) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "同步中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    } else if (m_loaded && m_visible.empty()) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "书架为空",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr,
                                canvas.getTheme().muted);
    }
}

bool ShelfPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["sync"].contains(px, py)) {
        load(context);
        context.show();
        return true;
    }
    if (m_rects["prev_folder"].contains(px, py)) {
        if (!m_path.empty()) {
            m_path.pop_back();
            load(context);
            context.show();
        }
        return true;
    }
    if (m_rects["up"].contains(px, py)) {
        if (m_page > 0) {
            m_page--;
            context.show();
        }
        return true;
    }

    int rowH = std::max(74, static_cast<int>(context.getHeight() * 0.060));
    int top = std::max(72, static_cast<int>(context.getHeight() * 0.085));
    int perPage = std::max(1, (context.getHeight() - top - 110) / rowH);
    int pages = std::max(1, (static_cast<int>(m_visible.size()) + perPage - 1) / perPage);

    if (m_rects["down"].contains(px, py)) {
        if (m_page < pages - 1) {
            m_page++;
            context.show();
        }
        return true;
    }

    int start = m_page * perPage;
    for (int r = 0; r < perPage; ++r) {
        int idx = start + r;
        std::string key = "item_" + std::to_string(idx);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(px, py)) {
            if (idx < static_cast<int>(m_visible.size())) {
                const auto& item = m_visible[idx];
                if (item.type == "FOLDER") {
                    m_path.push_back(std::to_string(item.id));
                    load(context);
                    context.show();
                } else {
                    context.navigate("book", true, { { "book_id", std::to_string(item.id) } });
                }
            }
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
