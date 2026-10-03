#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/reader/ReaderDocument.hpp"

#include <vector>
#include <string>
#include <unordered_map>
#include <memory>

namespace kinnovel::ui {

struct ChapterPayload {
    int id = 0;
    int bookId = 0;
    int sortNum = 0;
    std::string title;
    std::string bookName;
    std::string font;
    std::vector<std::string> chapters;
    std::string content;
};

class ReaderPage : public IPage {
public:
    ReaderPage() = default;
    ~ReaderPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    bool headerBlocked() const override;
    void uploadProgress(PageContext& context) override;

    void loadChapter(PageContext& context, int bookId, int sortNum, bool fresh, bool atLast, int swipeDelta);
    void turnPage(PageContext& context, int delta);
    void changeChapter(PageContext& context, int delta, bool atLast = false);

    int getBookId() const { return m_bookId; }
    void setBookId(int id) { m_bookId = id; }
    int getSortNum() const { return m_sortNum; }
    void setSortNum(int sn) { m_sortNum = sn; }
    int getPage() const { return m_page; }
    void setPage(int p) { m_page = p; }
    bool isChromeVisible() const { return m_chromeVisible; }
    void setChromeVisible(bool v) { m_chromeVisible = v; }
    const std::string& getFullscreenImage() const { return m_fullscreenImage; }
    void setFullscreenImage(const std::string& url) { m_fullscreenImage = url; }

    const std::shared_ptr<reader::ReaderDocument>& getDoc() const { return m_doc; }
    void setDoc(std::shared_ptr<reader::ReaderDocument> doc) { m_doc = std::move(doc); }

    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }
    void setRect(const std::string& key, const Rect& r) { m_rects[key] = r; }

    const std::unordered_map<std::string, std::tuple<int, int, int, int, std::string>>& getImageRects() const { return m_imageRects; }
    void setImageRect(const std::string& key, int x, int y, int w, int h, const std::string& url) {
        m_imageRects[key] = { x, y, w, h, url };
    }

    void setChapterPayload(ChapterPayload p) { m_chapter = std::move(p); m_hasData = true; }
    void setSignature(const std::string& sig) { m_signature = sig; }
    void setLastTurnAt(double t) { m_lastTurnAt = t; }

private:
    int m_bookId = 0;
    int m_sortNum = 1;
    int m_page = 0;
    bool m_chromeVisible = false;
    bool m_loading = false;
    bool m_hasData = false;
    int m_lastSaved = -1;

    ChapterPayload m_chapter;
    std::shared_ptr<reader::ReaderDocument> m_doc;
    std::string m_signature;
    std::string m_fullscreenImage;

    std::unordered_map<std::string, Rect> m_rects;
    std::unordered_map<std::string, std::tuple<int, int, int, int, std::string>> m_imageRects;
    double m_lastTurnAt = 0.0;
    uint64_t m_generation = 0;

    std::string computeSignature(PageContext& context, int bookId, int sortNum) const;
    void saveLocalProgress(PageContext& context);
    void maybeShowGuide(PageContext& context);
    void renderChrome(PageContext& context, Canvas& canvas, const std::string& title);
};

class CatalogPage : public IPage {
public:
    CatalogPage() = default;
    ~CatalogPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    int getBookId() const { return m_bookId; }
    int getSortNum() const { return m_sortNum; }
    int getCatalogPage() const { return m_catalogPage; }
    void setCatalogPage(int cp) { m_catalogPage = cp; }

private:
    int m_bookId = 0;
    int m_sortNum = 1;
    int m_catalogPage = 0;
    std::vector<std::string> m_chapters;
    std::unordered_map<std::string, Rect> m_rects;

    std::pair<int, int> catalogLayout(int height) const;
};

} // namespace kinnovel::ui
