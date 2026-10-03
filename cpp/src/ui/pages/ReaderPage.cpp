#include "kinnovel/ui/pages/ReaderPage.hpp"
#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/core/Utils.hpp"
#include "yyjson.h"

#include <cmath>
#include <algorithm>
#include <sstream>

namespace kinnovel::ui {

namespace {

static const int CHROME_FOOTER = 92;

static const std::vector<std::string> GUIDE_LINES = {
    "点击左侧：上一页",
    "点击右侧：下一页",
    "顶端下滑：呼出控件",
    "点击中间：回到阅读",
    "点击图片：全屏预览",
    "再次点击：退出预览",
};

} // namespace

std::string ReaderPage::computeSignature(PageContext& context, int bookId, int sortNum) const {
    auto config = context.getConfig();
    int fontSize = config ? config->getInt("font_size", 48) : 48;
    double lineSpacing = config ? config->getDouble("line_spacing", 1.42) : 1.42;
    int margin = config ? config->getInt("reader_margin", 34) : 34;
    std::string fontPath = config ? config->getString("font_path", "") : "";
    std::string convert = config ? config->getString("convert", "") : "";
    bool indent = config ? config->getBool("first_line_indent", true) : true;

    int top = std::max(40, static_cast<int>(context.getHeight() * 0.035));

    std::ostringstream oss;
    oss << bookId << ":" << sortNum << ":" << fontSize << ":" << lineSpacing << ":"
        << margin << ":" << context.getWidth() << ":" << context.getHeight() << ":"
        << top << ":" << fontPath << ":" << convert << ":" << (indent ? "1" : "0");
    return oss.str();
}

void ReaderPage::maybeShowGuide(PageContext& context) {
    if (context.getConfig() && context.getConfig()->getBool("reader_guide_dismissed", false)) {
        return;
    }
    if (context.getPreviousPage() == "reader") {
        return;
    }

    context.confirm(
        GUIDE_LINES,
        []() {},
        [&context]() {
            if (context.getConfig()) {
                context.getConfig()->setBool("reader_guide_dismissed", true);
            }
        },
        "感觉会忘记",
        "不再提示"
    );
}

bool ReaderPage::headerBlocked() const {
    return (core::Utils::monotonicSeconds() - m_lastTurnAt) < 0.35;
}

void ReaderPage::uploadProgress(PageContext& context) {
    auto api = context.getApi();
    if (!api || api->getUserId() <= 0 || !m_doc || !m_hasData) return;

    int bookId = m_chapter.bookId > 0 ? m_chapter.bookId : m_bookId;
    int chapterId = m_chapter.id;
    std::string xpath = m_doc->firstPathOnPage(m_page);

    context.runAsync("reader", [api, bookId, chapterId, xpath]() {
        api->saveReadPosition(bookId, chapterId, xpath);
    });
}

void ReaderPage::saveLocalProgress(PageContext& context) {
    if (!context.getApi() || context.getApi()->getUserId() <= 0 || !m_doc || !m_hasData) {
        return;
    }
    if (m_page == m_lastSaved) {
        return;
    }
    m_lastSaved = m_page;

    int bookId = m_chapter.bookId > 0 ? m_chapter.bookId : m_bookId;
    auto anchor = m_doc->firstAnchorOnPage(m_page);

    std::string progressDir = core::Config::getCacheDir() + "/progress";
    std::string progressPath = progressDir + "/" + std::to_string(bookId) + "-" + std::to_string(m_sortNum) + ".json";

    std::string jsonStr = "{\"path\":\"" + anchor.first + "\",\"offset\":" + std::to_string(anchor.second) +
                          ",\"page\":" + std::to_string(m_page) + "}";
    core::Utils::atomicWrite(progressPath, jsonStr);
}

void ReaderPage::enter(PageContext& context) {
    m_fullscreenImage.clear();
    m_chromeVisible = false;

    auto& params = context.getParams();
    int bookId = m_bookId;
    int sortNum = m_sortNum;

    auto bIt = params.find("book_id");
    if (bIt != params.end()) {
        try { bookId = std::stoi(bIt->second); } catch (...) {}
    }
    auto sIt = params.find("sort_num");
    if (sIt != params.end()) {
        try { sortNum = std::stoi(sIt->second); } catch (...) {}
    }

    bool fresh = (params.find("fresh") != params.end());
    bool atLast = (params.find("at_last") != params.end());
    int swipeDelta = 0;
    auto swIt = params.find("swipe_delta");
    if (swIt != params.end()) {
        try { swipeDelta = std::stoi(swIt->second); } catch (...) {}
    }

    maybeShowGuide(context);

    // Consume one-time intents
    params.erase("fresh");
    params.erase("at_last");
    params.erase("swipe_delta");

    loadChapter(context, bookId, sortNum, fresh, atLast, swipeDelta);
}

void ReaderPage::loadChapter(PageContext& context, int bookId, int sortNum, bool fresh, bool atLast, int swipeDelta) {
    (void)swipeDelta;
    m_bookId = bookId;
    m_sortNum = sortNum;
    std::string sig = computeSignature(context, bookId, sortNum);

    if (m_hasData && m_bookId == bookId && m_sortNum == sortNum && m_signature == sig && m_doc) {
        if (fresh) {
            m_page = 0;
        } else if (atLast) {
            m_page = std::max(0, static_cast<int>(m_doc->getPageCount()) - 1);
        }
        context.show();
        return;
    }

    uint64_t gen = ++m_generation;
    m_loading = true;

    context.runAsync("reader", [this, &context, gen, bookId, sortNum, sig, fresh, atLast]() {
        auto api = context.getApi();
        if (!api) return;

        std::string convert = context.getConfig() ? context.getConfig()->getString("convert", "") : "";
        std::string jsonStr = api->getNovelContent(bookId, sortNum, convert);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        if (gen != m_generation || bookId != m_bookId || sortNum != m_sortNum) {
            yyjson_doc_free(doc);
            return;
        }

        ChapterPayload payload;
        payload.bookId = bookId;
        payload.sortNum = sortNum;

        std::string readPosPath;
        int readChapterId = 0;

        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* chVal = yyjson_obj_get(root, "Chapter");
            if (yyjson_is_obj(chVal)) {
                yyjson_val* idVal = yyjson_obj_get(chVal, "Id");
                yyjson_val* bidVal = yyjson_obj_get(chVal, "BookId");
                yyjson_val* tVal = yyjson_obj_get(chVal, "Title");
                yyjson_val* bnVal = yyjson_obj_get(chVal, "BookName");
                yyjson_val* fVal = yyjson_obj_get(chVal, "Font");
                yyjson_val* cVal = yyjson_obj_get(chVal, "Content");

                if (idVal && yyjson_is_num(idVal)) payload.id = yyjson_get_int(idVal);
                if (bidVal && yyjson_is_num(bidVal)) payload.bookId = yyjson_get_int(bidVal);
                if (tVal && yyjson_is_str(tVal)) payload.title = yyjson_get_str(tVal);
                if (bnVal && yyjson_is_str(bnVal)) payload.bookName = yyjson_get_str(bnVal);
                if (fVal && yyjson_is_str(fVal)) payload.font = yyjson_get_str(fVal);
                if (cVal && yyjson_is_str(cVal)) payload.content = yyjson_get_str(cVal);

                yyjson_val* chs = yyjson_obj_get(chVal, "Chapters");
                if (yyjson_is_arr(chs)) {
                    size_t cidx, cmax;
                    yyjson_val* citem;
                    yyjson_arr_foreach(chs, cidx, cmax, citem) {
                        if (yyjson_is_str(citem)) payload.chapters.push_back(yyjson_get_str(citem));
                    }
                }
            }

            yyjson_val* rp = yyjson_obj_get(root, "ReadPosition");
            if (yyjson_is_obj(rp)) {
                yyjson_val* rcid = yyjson_obj_get(rp, "ChapterId");
                yyjson_val* rpath = yyjson_obj_get(rp, "Position");
                if (rcid && yyjson_is_num(rcid)) readChapterId = yyjson_get_int(rcid);
                if (rpath && yyjson_is_str(rpath)) readPosPath = yyjson_get_str(rpath);
            }
        }
        yyjson_doc_free(doc);

        auto config = context.getConfig();
        std::string fontPath = config ? config->getString("font_path", "") : "";
        auto readerDoc = std::make_shared<reader::ReaderDocument>(
            payload.content, payload.font, api->getServer(), fontPath, config, payload.title, payload.chapters);

        int top = std::max(40, static_cast<int>(context.getHeight() * 0.035));
        int contentH = std::max(160, context.getHeight() - top);
        readerDoc->prepare(context.getWidth(), contentH);

        m_chapter = std::move(payload);
        m_doc = std::move(readerDoc);
        m_signature = sig;
        m_hasData = true;
        m_loading = false;

        if (atLast) {
            m_page = std::max(0, static_cast<int>(m_doc->getPageCount()) - 1);
        } else if (fresh) {
            m_page = 0;
        } else {
            int targetPage = 0;
            if (readChapterId == m_chapter.id && !readPosPath.empty()) {
                targetPage = m_doc->pageForPath(readPosPath, -1);
            }
            m_page = targetPage;
        }

        saveLocalProgress(context);
    });
}

void ReaderPage::turnPage(PageContext& context, int delta) {
    if (!m_doc) return;

    int target = m_page + delta;
    if (target >= 0 && target < static_cast<int>(m_doc->getPageCount())) {
        m_page = target;
        m_lastTurnAt = core::Utils::monotonicSeconds();
        saveLocalProgress(context);
        context.show();
        return;
    }

    changeChapter(context, delta, delta < 0);
}

void ReaderPage::changeChapter(PageContext& context, int delta, bool atLast) {
    int nextSort = m_sortNum + delta;
    if (nextSort < 1 || (!m_chapter.chapters.empty() && nextSort > static_cast<int>(m_chapter.chapters.size()))) {
        context.toast(delta < 0 ? "已经是第一页" : "已经是最后一页");
        return;
    }

    m_lastTurnAt = core::Utils::monotonicSeconds();
    std::unordered_map<std::string, std::string> p;
    p["book_id"] = std::to_string(m_bookId);
    p["sort_num"] = std::to_string(nextSort);
    if (atLast) p["at_last"] = "1";
    p["swipe_delta"] = std::to_string(delta);
    context.replace("reader", p);
}

void ReaderPage::renderChrome(PageContext& context, Canvas& canvas, const std::string& title) {
    std::string bookName = !m_chapter.bookName.empty() ? m_chapter.bookName : title;
    canvas.header(bookName, "返回", "主页");

    int footerTop = canvas.getHeight() - CHROME_FOOTER;
    canvas.drawRect(0, footerTop, canvas.getWidth(), CHROME_FOOTER, canvas.getTheme().background, true);

    int barY = canvas.getHeight() - 76;
    int margin = static_cast<int>(canvas.getWidth() * 0.025);
    int gap = 6;
    int btnW = (canvas.getWidth() - 2 * margin - 3 * gap) / 4;

    std::vector<std::pair<std::string, std::string>> btns = {
        { "prev", "上一章" },
        { "catalog", "目录" },
        { "settings", "设置" },
        { "next", "下一章" }
    };

    for (size_t i = 0; i < btns.size(); ++i) {
        Rect r{ margin + static_cast<int>(i) * (btnW + gap), barY, btnW, 56 };
        canvas.button(r, btns[i].second, true,
                      context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
        m_rects[btns[i].first + "_0"] = r;
        m_rects[btns[i].first] = r;
    }

    if (!m_doc) return;

    int totalPages = std::max(1, static_cast<int>(m_doc->getPageCount()));
    int progressW = static_cast<int>((static_cast<double>(m_page + 1) / totalPages) * (canvas.getWidth() - 2 * margin));
    canvas.drawRect(margin, canvas.getHeight() - 10, progressW, 5, canvas.getTheme().foreground, true);
}

void ReaderPage::render(PageContext& context, Canvas& canvas) {
    if (!m_fullscreenImage.empty()) {
        if (context.getImages()) {
            auto img = context.getImages()->get(m_fullscreenImage);
            if (img) {
                auto fitted = ImageCache::contain(*img, canvas.getWidth(), canvas.getHeight());
                if (fitted) {
                    int x = (canvas.getWidth() - fitted->width) / 2;
                    int y = (canvas.getHeight() - fitted->height) / 2;
                    canvas.paste(fitted->data.data(), x, y, fitted->width, fitted->height);
                    return;
                }
            }
        }
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2, "图片加载中…",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        return;
    }

    std::string title = !m_chapter.title.empty() ? m_chapter.title : "阅读";
    int top = std::max(40, static_cast<int>(canvas.getHeight() * 0.035));
    int contentH = std::max(160, canvas.getHeight() - top);

    if (!m_chromeVisible) {
        std::string pageLabel = m_doc ? (std::to_string(m_page + 1) + "/" + std::to_string(m_doc->getPageCount())) : "1/1";
        top = canvas.compactHeader(title, pageLabel);
    }

    m_rects.clear();
    m_imageRects.clear();

    if (!m_doc) {
        canvas.drawCenteredText(canvas.getWidth() / 2, canvas.getHeight() / 2,
                                m_loading ? "正在下载并排版…" : "暂无正文",
                                context.getFonts() && context.getFonts()->body ? context.getFonts()->body.get() : nullptr);
        if (m_chromeVisible) {
            renderChrome(context, canvas, title);
        }
        return;
    }

    const auto& page = m_doc->getPage(m_page);
    for (const auto& item : page) {
        if (item.type == "text") {
            int y = top + item.y;
            if (y + item.size > top + contentH) continue;
            canvas.drawTextFallback(item.x, y, item.text, item.font.get(), item.fallbackFont.get());
        } else if (item.type == "image") {
            int imageY = top + item.y;
            bool drawn = false;
            if (context.getImages()) {
                auto img = context.getImages()->get(item.url);
                if (img) {
                    auto fitted = ImageCache::contain(*img, item.width, item.height);
                    if (fitted) {
                        int x = item.x + (item.width - fitted->width) / 2;
                        canvas.paste(fitted->data.data(), x, imageY, fitted->width, fitted->height);
                        m_imageRects[item.path + "_" + std::to_string(item.y)] = { x, imageY, fitted->width, fitted->height, item.url };
                        drawn = true;
                    }
                }
            }
            if (!drawn) {
                canvas.drawCenteredText(canvas.getWidth() / 2, imageY + item.height / 2, "[图片]",
                                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr,
                                        canvas.getTheme().muted);
                m_imageRects[item.path + "_" + std::to_string(item.y)] = { item.x, imageY, item.width, item.height, item.url };
            }
        }
    }

    if (m_chromeVisible) {
        renderChrome(context, canvas, title);
    }
}

bool ReaderPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind == hal::GestureKind::Down) {
        if (!m_chromeVisible && event.startPoint.yRatio < 0.16f) {
            m_chromeVisible = true;
            context.show();
        }
        return true;
    }

    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (!m_fullscreenImage.empty()) {
        m_fullscreenImage.clear();
        context.show();
        return true;
    }

    if (m_chromeVisible) {
        if (py >= context.getHeight() - CHROME_FOOTER) {
            for (const auto& kv : m_rects) {
                if (kv.second.contains(px, py)) {
                    if (kv.first == "prev" || kv.first == "prev_0") {
                        changeChapter(context, -1, true);
                    } else if (kv.first == "next" || kv.first == "next_0") {
                        changeChapter(context, 1, false);
                    } else if (kv.first == "catalog" || kv.first == "catalog_0") {
                        context.navigate("catalog", true, {
                            { "book_id", std::to_string(m_bookId) },
                            { "sort_num", std::to_string(m_sortNum) }
                        });
                    } else if (kv.first == "settings" || kv.first == "settings_0") {
                        context.navigate("settings");
                    }
                    return true;
                }
            }
            return true;
        }

        m_chromeVisible = false;
        context.show();
        return true;
    }

    int top = std::max(40, static_cast<int>(context.getHeight() * 0.035));
    if (py < top) {
        return false;
    }

    for (const auto& kv : m_imageRects) {
        int ix = std::get<0>(kv.second);
        int iy = std::get<1>(kv.second);
        int iw = std::get<2>(kv.second);
        int ih = std::get<3>(kv.second);
        if (px >= ix && px < ix + iw && py >= iy && py < iy + ih) {
            m_fullscreenImage = std::get<4>(kv.second);
            context.show();
            return true;
        }
    }

    if (px < static_cast<int>(context.getWidth() * 0.25)) {
        turnPage(context, -1);
        return true;
    }
    if (px > static_cast<int>(context.getWidth() * 0.75)) {
        turnPage(context, 1);
        return true;
    }

    // Middle area tap toggles chrome
    m_chromeVisible = true;
    context.show();
    return true;
}

// ==================== CatalogPage ====================

std::pair<int, int> CatalogPage::catalogLayout(int height) const {
    int rowH = std::max(62, static_cast<int>(height * 0.052));
    int headerH = std::max(72, static_cast<int>(height * 0.085));
    int rows = std::max(1, (height - headerH - 100) / rowH);
    return { rowH, rows };
}

void CatalogPage::enter(PageContext& context) {
    const auto& params = context.getParams();
    auto bIt = params.find("book_id");
    if (bIt != params.end()) {
        try { m_bookId = std::stoi(bIt->second); } catch (...) {}
    }
    auto sIt = params.find("sort_num");
    if (sIt != params.end()) {
        try { m_sortNum = std::stoi(sIt->second); } catch (...) {}
    }

    m_catalogPage = 0;
    m_rects.clear();

    // Fetch book chapters if needed
    context.runAsync("catalog", [this, &context]() {
        auto api = context.getApi();
        if (!api || m_bookId <= 0) return;
        std::string jsonStr = api->getBookInfo(m_bookId);
        if (jsonStr.empty()) return;

        yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
        if (!doc) return;

        std::vector<std::string> chs;
        yyjson_val* root = yyjson_doc_get_root(doc);
        if (yyjson_is_obj(root)) {
            yyjson_val* bVal = yyjson_obj_get(root, "Book");
            if (yyjson_is_obj(bVal)) {
                yyjson_val* chArr = yyjson_obj_get(bVal, "Chapters");
                if (yyjson_is_arr(chArr)) {
                    size_t idx, max;
                    yyjson_val* chVal;
                    yyjson_arr_foreach(chArr, idx, max, chVal) {
                        yyjson_val* ct = yyjson_obj_get(chVal, "Title");
                        if (ct && yyjson_is_str(ct)) {
                            chs.push_back(yyjson_get_str(ct));
                        } else {
                            chs.push_back("第 " + std::to_string(idx + 1) + " 章");
                        }
                    }
                }
            }
        }
        yyjson_doc_free(doc);
        m_chapters = std::move(chs);
    });
}

void CatalogPage::render(PageContext& context, Canvas& canvas) {
    int top = canvas.header("章节目录", "返回", "主页");
    int margin = static_cast<int>(canvas.getWidth() * 0.035);
    auto [rowH, rows] = catalogLayout(canvas.getHeight());

    int pages = std::max(1, (static_cast<int>(m_chapters.size()) + rows - 1) / rows);
    m_catalogPage = std::min(m_catalogPage, pages - 1);
    int start = m_catalogPage * rows;

    m_rects.clear();
    for (int r = 0; r < rows; ++r) {
        int idx = start + r;
        int y = top + 12 + r * rowH;
        Rect rect{ margin, y, canvas.getWidth() - 2 * margin, rowH - 6 };
        m_rects["catalog_" + std::to_string(idx)] = rect;
        m_rects["catalog_" + std::to_string(r)] = rect;

        if (idx >= static_cast<int>(m_chapters.size())) {
            continue;
        }

        bool current = (m_sortNum == idx + 1);
        canvas.drawRoundedRect(rect.x, rect.y, rect.width, rect.height, 8, canvas.getTheme().mid,
                              current ? canvas.getTheme().inverseBg : canvas.getTheme().background, 1);
        uint8_t fill = current ? canvas.getTheme().inverseFg : canvas.getTheme().foreground;

        std::string label = std::to_string(idx + 1) + ". " + m_chapters[idx];
        canvas.drawText(rect.x + 12, y + 10,
                        canvas.fitText(label, context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr, rect.width - 24),
                        context.getFonts() && context.getFonts()->small ? context.getFonts()->small.get() : nullptr,
                        fill);
    }

    int navY = canvas.getHeight() - 72;
    int navW = static_cast<int>(canvas.getWidth() * 0.25);
    Rect prevRect{ margin, navY, navW, 52 };
    Rect countRect{ (canvas.getWidth() - navW) / 2, navY, navW, 52 };
    Rect nextRect{ canvas.getWidth() - margin - navW, navY, navW, 52 };

    canvas.button(prevRect, "上页", m_catalogPage > 0,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
    canvas.button(countRect, std::to_string(m_catalogPage + 1) + "/" + std::to_string(pages), true,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);
    canvas.button(nextRect, "下页", m_catalogPage < pages - 1,
                  context.getFonts() && context.getFonts()->tiny ? context.getFonts()->tiny.get() : nullptr);

    m_rects["prev"] = prevRect;
    m_rects["count"] = countRect;
    m_rects["next"] = nextRect;
}

bool CatalogPage::handle(const hal::TouchGesture& event, PageContext& context) {
    if (event.kind != hal::GestureKind::Tap) return false;

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (m_rects["prev"].contains(px, py)) {
        if (m_catalogPage > 0) {
            m_catalogPage--;
            context.show();
        }
        return true;
    }

    auto [rowH, rows] = catalogLayout(context.getHeight());
    int pages = std::max(1, (static_cast<int>(m_chapters.size()) + rows - 1) / rows);

    if (m_rects["next"].contains(px, py)) {
        if (m_catalogPage < pages - 1) {
            m_catalogPage++;
            context.show();
        }
        return true;
    }

    int start = m_catalogPage * rows;
    for (int r = 0; r < rows; ++r) {
        int idx = start + r;
        std::string key = "catalog_" + std::to_string(r);
        auto it = m_rects.find(key);
        if (it != m_rects.end() && it->second.contains(px, py)) {
            // Drop stale reader from stack if present (matches Python test_catalog_jump_drops_stale_reader_from_stack)
            if (!context.getStack().empty() && context.getStack().back().first == "reader") {
                context.getStack().pop_back();
            }
            context.replace("reader", {
                { "book_id", std::to_string(m_bookId) },
                { "sort_num", std::to_string(idx + 1) },
                { "fresh", "1" }
            });
            return true;
        }
    }

    return false;
}

} // namespace kinnovel::ui
