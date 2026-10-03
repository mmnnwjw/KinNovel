#include "kinnovel/ui/pages/BookDetailPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Charset.hpp"
#include "yyjson.h"

#include <regex>
#include <algorithm>

namespace kinnovel::ui {

namespace {

std::string cleanIntro(const std::string& html) {
    std::regex tagRe("<[^>]+>");
    std::string s = std::regex_replace(html, tagRe, " ");
    size_t pos = 0;
    while ((pos = s.find("&nbsp;", pos)) != std::string::npos) {
        s.replace(pos, 6, " ");
        pos += 1;
    }
    return s;
}

} // namespace

void BookDetailPage::enter(PageContext& context) {
    load(context);
}

void BookDetailPage::prefetchReadingTarget(PageContext& context, const BookDetailData& data) {
    if (data.chapters.empty()) return;

    int targetSort = 1;
    if (data.readChapterId > 0) {
        for (const auto& ch : data.chapters) {
            if (ch.id == data.readChapterId) {
                targetSort = ch.sortNum;
                break;
            }
        }
    } else {
        targetSort = data.chapters[0].sortNum;
    }

    int bookId = data.id;
    context.runAsync("book", [&context, bookId, targetSort]() {
        auto api = context.getApi();
        if (api) {
            api->getNovelContent(bookId, targetSort);
        }
    });
}

void BookDetailPage::load(PageContext& context, bool force) {
    auto api = context.getApi();
    if (!api || api->getUserId() <= 0) {
        context.toast("查看详情需要登录");
        context.navigate("account");
        return;
    }

    int bookId = 0;
    auto it = context.getParams().find("book_id");
    if (it != context.getParams().end()) {
        try {
            bookId = std::stoi(it->second);
        } catch (...) {
            bookId = 0;
        }
    }

    if (bookId <= 0) {
        context.message("书籍 ID 无效");
        return;
    }

    if (!force && m_data.id == bookId && !m_data.title.empty()) {
        return;
    }

    m_bookId = bookId;
    m_loading = true;
    uint64_t gen = ++m_generation;

    context.runAsync("book", [this, &context, gen, bookId]() {
        auto api = context.getApi();
        if (!api) return;
        std::string jsonStr = api->getBookInfo(bookId);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        if (gen != m_generation || bookId != m_bookId) {
            yyjson_doc_free(doc);
            return;
        }

        BookDetailData d;
        d.id = bookId;

        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* bVal = yyjson_obj_get(root, "Book");
            if (yyjson_is_obj(bVal)) {
                yyjson_val* tVal = yyjson_obj_get(bVal, "Title");
                yyjson_val* aVal = yyjson_obj_get(bVal, "Author");
                yyjson_val* cVal = yyjson_obj_get(bVal, "Cover");
                yyjson_val* iVal = yyjson_obj_get(bVal, "Introduction");
                yyjson_val* luVal = yyjson_obj_get(bVal, "LastUpdatedChapter");
                yyjson_val* lutVal = yyjson_obj_get(bVal, "LastUpdatedAt");
                yyjson_val* vVal = yyjson_obj_get(bVal, "Views");
                yyjson_val* fVal = yyjson_obj_get(bVal, "Favorite");

                if (tVal && yyjson_is_str(tVal)) d.title = yyjson_get_str(tVal);
                if (aVal && yyjson_is_str(aVal)) d.author = yyjson_get_str(aVal);
                if (cVal && yyjson_is_str(cVal)) d.cover = yyjson_get_str(cVal);
                if (iVal && yyjson_is_str(iVal)) d.introduction = yyjson_get_str(iVal);
                if (luVal && yyjson_is_str(luVal)) d.lastUpdatedChapter = yyjson_get_str(luVal);
                if (lutVal && yyjson_is_str(lutVal)) d.lastUpdatedAt = yyjson_get_str(lutVal);
                if (vVal && yyjson_is_num(vVal)) d.views = yyjson_get_int(vVal);
                if (fVal && yyjson_is_num(fVal)) d.favorite = yyjson_get_int(fVal);

                yyjson_val* chArr = yyjson_obj_get(bVal, "Chapters");
                if (yyjson_is_arr(chArr)) {
                    size_t idx, max;
                    yyjson_val* chVal;
                    yyjson_arr_foreach(chArr, idx, max, chVal) {
                        ChapterInfo ci;
                        yyjson_val* cid = yyjson_obj_get(chVal, "Id");
                        yyjson_val* cs = yyjson_obj_get(chVal, "SortNum");
                        yyjson_val* ct = yyjson_obj_get(chVal, "Title");
                        if (cid && yyjson_is_num(cid)) ci.id = yyjson_get_int(cid);
                        if (cs && yyjson_is_num(cs)) ci.sortNum = yyjson_get_int(cs);
                        if (ct && yyjson_is_str(ct)) ci.title = yyjson_get_str(ct);
                        d.chapters.push_back(std::move(ci));
                    }
                }
            }

            yyjson_val* stVal = yyjson_obj_get(root, "SeriesTitle");
            if (stVal && yyjson_is_str(stVal)) d.seriesTitle = yyjson_get_str(stVal);

            yyjson_val* sArr = yyjson_obj_get(root, "Series");
            if (yyjson_is_arr(sArr)) {
                size_t idx, max;
                yyjson_val* sbVal;
                yyjson_arr_foreach(sArr, idx, max, sbVal) {
                    SeriesBook sb;
                    yyjson_val* sbid = yyjson_obj_get(sbVal, "Id");
                    yyjson_val* sbt = yyjson_obj_get(sbVal, "Title");
                    yyjson_val* sbc = yyjson_obj_get(sbVal, "Cover");
                    if (sbid && yyjson_is_num(sbid)) sb.id = yyjson_get_int(sbid);
                    if (sbt && yyjson_is_str(sbt)) sb.title = yyjson_get_str(sbt);
                    if (sbc && yyjson_is_str(sbc)) sb.cover = yyjson_get_str(sbc);
                    d.series.push_back(std::move(sb));
                }
            }

            yyjson_val* rpVal = yyjson_obj_get(root, "ReadPosition");
            if (yyjson_is_obj(rpVal)) {
                yyjson_val* rcid = yyjson_obj_get(rpVal, "ChapterId");
                yyjson_val* rpos = yyjson_obj_get(rpVal, "Position");
                if (rcid && yyjson_is_num(rcid)) d.readChapterId = yyjson_get_int(rcid);
                if (rpos && yyjson_is_str(rpos)) d.readPosition = yyjson_get_str(rpos);
            }
        }
        yyjson_doc_free(doc);

        m_data = std::move(d);
        m_loading = false;
        m_chapterPage = 0;

        if (!m_data.cover.empty() && context.getImages()) {
            context.getImages()->prefetch(m_data.cover);
        }

        // Check bookshelf
        std::string shelfJson = api->getBookShelf();
        if (!shelfJson.empty()) {
            yyjson_doc* sdoc = yyjson_read(shelfJson.c_str(), shelfJson.size(), 0);
            if (sdoc) {
                yyjson_val* sroot = yyjson_doc_get_root(sdoc);
                yyjson_val* sdata = yyjson_obj_get(sroot, "data");
                if (yyjson_is_arr(sdata)) {
                    size_t sidx, smax;
                    yyjson_val* item;
                    bool found = false;
                    yyjson_arr_foreach(sdata, sidx, smax, item) {
                        yyjson_val* idVal = yyjson_obj_get(item, "id");
                        if (idVal && yyjson_is_num(idVal) && yyjson_get_int(idVal) == bookId) {
                            found = true;
                            break;
                        }
                    }
                    m_bound = found;
                }
                yyjson_doc_free(sdoc);
            }
        }

        prefetchReadingTarget(context, m_data);
    });
}

int BookDetailPage::renderChapterRows(PageContext& context, Canvas& canvas, int startY) {
    int perPage = 6;
    int totalPages = std::max(1, (static_cast<int>(m_data.chapters.size()) + perPage - 1) / perPage);
    m_chapterPage = std::min(m_chapterPage, totalPages - 1);
    int start = m_chapterPage * perPage;

    int rowH = std::max(58, static_cast<int>(canvas.getHeight() * 0.047));
    int available = canvas.getHeight() - startY - 150;
    rowH = std::min(rowH, std::max(46, available / perPage));

    for (int row = 0; row < perPage; ++row) {
        int idx = start + row;
        int y = startY + row * rowH;
        Rect r{ static_cast<int>(canvas.getWidth() * 0.035), y, static_cast<int>(canvas.getWidth() * 0.93), rowH - 5 };
        m_rects["chapter_" + std::to_string(row)] = r;
        m_rects["chapter_" + std::to_string(idx)] = r;

        if (idx >= static_cast<int>(m_data.chapters.size())) {
            continue;
        }

        const auto& ch = m_data.chapters[idx];
        canvas.drawRoundedRect(r.x, r.y, r.width, r.height, 9, canvas.getTheme().mid, 1);
        std::string label = ch.title.empty() ? ("第 " + std::to_string(idx + 1) + " 章") : ch.title;
        canvas.drawText(r.x + 12, y + (rowH - 30) / 2,
                        canvas.fitText(label, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, r.width - 24),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
    }
    return totalPages;
}

void BookDetailPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("书籍详情", "返回", "主页");
    if (m_data.id <= 0) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2,
                                m_loading ? "加载中…" : "暂无数据",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        return;
    }

    m_rects.clear();
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    int coverW = static_cast<int>(canvas.getWidth() * 0.23);
    int coverH = static_cast<int>(coverW * 1.45);
    int coverY = top + 20;

    if (context.getImages()) {
        auto coverImg = context.getImages()->cover(m_data.cover, coverW, coverH);
        if (coverImg) {
            canvas.paste(coverImg->data.data(), margin, coverY, coverW, coverH);
        }
    }
    canvas.drawRect(margin, coverY, coverW, coverH, canvas.getTheme().mid, false);

    int infoX = margin + coverW + 24;
    int infoW = canvas.getWidth() - infoX - margin;
    int y = coverY;

    auto titleLines = canvas.wrap(m_data.title.empty() ? "未知" : m_data.title,
                                  context.getFonts() && context.getFonts()->title ? context.getFonts()->title.get() : nullptr, infoW);
    for (size_t i = 0; i < std::min<size_t>(2, titleLines.size()); ++i) {
        canvas.drawText(infoX, y, titleLines[i],
                        context.getFonts() && context.getFonts()->title ? context.getFonts()->title.get() : nullptr);
        y += (context.getFonts() && context.getFonts()->title ? context.getFonts()->title->getSize() : 44) + 8;
    }

    bool hasSeries = !m_data.seriesTitle.empty() || m_data.series.size() > 1;
    if (hasSeries) {
        Rect sRect{ infoX, y + 2, std::min(150, infoW), 44 };
        canvas.button(sRect, "系列", true, context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
        m_rects["series_0"] = sRect;
        m_rects["series"] = sRect;
        y += 50;
    }

    std::vector<std::string> details = {
        "作者: " + (m_data.author.empty() ? "未知" : m_data.author),
        "最后更新: " + (m_data.lastUpdatedChapter.empty() ? "未知" : m_data.lastUpdatedChapter),
        "更新时间: " + core::Utils::formatTime(m_data.lastUpdatedAt),
        "浏览: " + std::to_string(m_data.views) + "  收藏: " + std::to_string(m_data.favorite),
    };

    for (const auto& line : details) {
        canvas.drawText(infoX, y, canvas.fitText(line, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, infoW),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr,
                        canvas.getTheme().muted);
        y += (context.getFonts() && context.getFonts()->small ? context.getFonts()->small->getSize() : 28) + 8;
    }

    int summaryY = std::max(coverY + coverH + 12, y + 8);
    canvas.drawText(margin, summaryY, "简介", context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    summaryY += (context.getFonts() && context.getFonts()->body ? context.getFonts()->body->getSize() : 34) + 4;

    std::string intro = cleanIntro(m_data.introduction);
    auto introLines = canvas.wrap(intro, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, canvas.getWidth() - 2 * margin);
    for (size_t i = 0; i < std::min<size_t>(3, introLines.size()); ++i) {
        canvas.drawText(margin, summaryY, introLines[i],
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr,
                        canvas.getTheme().muted);
        summaryY += (context.getFonts() && context.getFonts()->small ? context.getFonts()->small->getSize() : 28) + 5;
    }

    int chapterY = std::max(summaryY + 12, static_cast<int>(canvas.getHeight() * 0.46));
    canvas.drawText(margin, chapterY, "章节", context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
    int totalPages = renderChapterRows(context, canvas, chapterY + (context.getFonts() && context.getFonts()->body ? context.getFonts()->body->getSize() : 34) + 8);

    int bottomY = canvas.getHeight() - 82;
    int gap = 8;
    int btnW = (canvas.getWidth() - 2 * margin - gap * 4) / 5;

    std::vector<std::pair<std::string, std::string>> buttons = {
        { "read", (m_data.readChapterId > 0) ? "继续阅读" : "开始阅读" },
        { "shelf", m_bound ? "移出书架" : "加入书架" },
        { "comments", "评论" },
        { "prev", "上页" },
        { "next", "下页 (" + std::to_string(m_chapterPage + 1) + "/" + std::to_string(totalPages) + ")" },
    };

    for (size_t i = 0; i < buttons.size(); ++i) {
        Rect r{ margin + static_cast<int>(i) * (btnW + gap), bottomY, btnW, 58 };
        bool active = (buttons[i].first != "prev" && buttons[i].first != "next") ||
                      (buttons[i].first == "prev" && m_chapterPage > 0) ||
                      (buttons[i].first == "next" && m_chapterPage < totalPages - 1);
        canvas.button(r, buttons[i].second, active,
                      context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr);
        m_rects[buttons[i].first + "_0"] = r;
        m_rects[buttons[i].first] = r;
    }
}

void BookDetailPage::toggleShelf(PageContext& context) {
    auto api = context.getApi();
    if (!api || api->getUserId() <= 0) {
        context.toast("请先登录");
        context.navigate("account");
        return;
    }

    bool wasBound = m_bound;
    int bookId = m_bookId;
    std::string bType = (m_data.type == "Comic") ? "COMIC" : "NOVEL";

    context.runAsync("book", [this, &context, wasBound, bookId, bType]() {
        auto api = context.getApi();
        if (!api) return;
        std::string shelfJson = api->getBookShelf();
        yyjson_doc* doc = yyjson_read(shelfJson.c_str(), shelfJson.size(), 0);

        yyjson_mut_doc* mut = yyjson_mut_doc_new(nullptr);
        yyjson_mut_val* arr = yyjson_mut_arr(mut);

        if (doc) {
            yyjson_val* root = yyjson_doc_get_root(doc);
            yyjson_val* data = yyjson_obj_get(root, "data");
            if (yyjson_is_arr(data)) {
                size_t idx, max;
                yyjson_val* val;
                yyjson_arr_foreach(data, idx, max, val) {
                    yyjson_val* idVal = yyjson_obj_get(val, "id");
                    if (idVal && yyjson_is_num(idVal) && yyjson_get_int(idVal) == bookId) {
                        if (wasBound) continue; // remove
                    }
                    yyjson_mut_arr_add_val(arr, yyjson_val_mut_copy(mut, val));
                }
            }
            yyjson_doc_free(doc);
        }

        if (!wasBound) {
            yyjson_mut_val* newObj = yyjson_mut_obj(mut);
            yyjson_mut_obj_add_int(mut, newObj, "id", bookId);
            yyjson_mut_obj_add_str(mut, newObj, "type", bType.c_str());
            yyjson_mut_arr_insert(arr, newObj, 0);
        }

        size_t len = 0;
        char* jsonOut = yyjson_mut_write(mut, 0, &len);
        if (jsonOut) {
            api->saveBookShelf(std::string(jsonOut, len));
            free(jsonOut);
        }
        yyjson_mut_doc_free(mut);

        m_bound = !wasBound;
        context.toast(m_bound ? "已加入书架" : "已移出书架");
    });
}

bool BookDetailPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["series"].contains(px, py)) {
        context.navigate("series", true, {
            { "title", m_data.seriesTitle.empty() ? "系列" : m_data.seriesTitle },
            { "series_name", m_data.seriesTitle },
            { "current_id", std::to_string(m_bookId) }
        });
        return true;
    }

    if (m_rects["read"].contains(px, py)) {
        int sortNum = 1;
        if (m_data.readChapterId > 0) {
            for (const auto& ch : m_data.chapters) {
                if (ch.id == m_data.readChapterId) {
                    sortNum = ch.sortNum;
                    break;
                }
            }
        } else if (!m_data.chapters.empty()) {
            sortNum = m_data.chapters[0].sortNum;
        }
        context.navigate("reader", true, {
            { "book_id", std::to_string(m_bookId) },
            { "sort_num", std::to_string(sortNum) }
        });
        return true;
    }

    if (m_rects["shelf"].contains(px, py)) {
        toggleShelf(context);
        return true;
    }

    if (m_rects["comments"].contains(px, py)) {
        context.navigate("comments", true, {
            { "comment_type", "Book" },
            { "target_id", std::to_string(m_bookId) }
        });
        return true;
    }

    int totalPages = std::max(1, (static_cast<int>(m_data.chapters.size()) + 5) / 6);
    if (m_rects["prev"].contains(px, py)) {
        if (m_chapterPage > 0) {
            m_chapterPage--;
            context.show();
        }
        return true;
    }
    if (m_rects["next"].contains(px, py)) {
        if (m_chapterPage < totalPages - 1) {
            m_chapterPage++;
            context.show();
        }
        return true;
    }

    int start = m_chapterPage * 6;
    for (int i = 0; i < 6; ++i) {
        std::string key = "chapter_" + std::to_string(i);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(px, py)) {
            int idx = start + i;
            if (idx < static_cast<int>(m_data.chapters.size())) {
                context.navigate("reader", true, {
                    { "book_id", std::to_string(m_bookId) },
                    { "sort_num", std::to_string(m_data.chapters[idx].sortNum) },
                    { "fresh", "1" }
                });
            }
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
