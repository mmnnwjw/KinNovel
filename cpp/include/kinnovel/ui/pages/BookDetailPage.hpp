#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

struct ChapterInfo {
    int id = 0;
    int sortNum = 0;
    std::string title;
};

struct SeriesBook {
    int id = 0;
    std::string title;
    std::string cover;
};

struct BookDetailData {
    int id = 0;
    std::string type;
    std::string title;
    std::string author;
    std::string cover;
    std::string introduction;
    std::string lastUpdatedChapter;
    std::string lastUpdatedAt;
    int views = 0;
    int favorite = 0;

    std::string seriesTitle;
    std::vector<SeriesBook> series;
    std::vector<ChapterInfo> chapters;

    int readChapterId = 0;
    std::string readPosition;
    std::vector<std::string> tags;
};

class BookDetailPage : public IPage {
public:
    BookDetailPage() = default;
    ~BookDetailPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context, bool force = false);
    void prefetchReadingTarget(PageContext& context, const BookDetailData& data);

    int getBookId() const { return m_bookId; }
    void setBookId(int id) { m_bookId = id; }
    const BookDetailData& getData() const { return m_data; }
    void setData(BookDetailData data) { m_data = std::move(data); }
    bool isBound() const { return m_bound; }
    void setBound(bool b) { m_bound = b; }
    int getChapterPage() const { return m_chapterPage; }
    void setChapterPage(int cp) { m_chapterPage = cp; }
    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }

private:
    int m_bookId = 0;
    BookDetailData m_data;
    int m_chapterPage = 0;
    bool m_bound = false;
    bool m_loading = false;
    uint64_t m_generation = 0;
    std::unordered_map<std::string, Rect> m_rects;

    int renderChapterRows(PageContext& context, Canvas& canvas, int startY);
    void toggleShelf(PageContext& context);
};

} // namespace kinnovel::ui
