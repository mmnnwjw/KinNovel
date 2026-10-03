#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/ui/pages/BookDetailPage.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

class SeriesPage : public IPage {
public:
    SeriesPage() = default;
    ~SeriesPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context);

    int getPage() const { return m_page; }
    void setPage(int p) { m_page = p; }
    int getTotalPages() const { return m_totalPages; }
    void setTotalPages(int tp) { m_totalPages = tp; }
    const std::vector<SeriesBook>& getItems() const { return m_items; }
    void setItems(std::vector<SeriesBook> items) { m_items = std::move(items); m_loaded = true; }
    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }

private:
    std::string m_title = "系列";
    std::string m_seriesName;
    std::vector<SeriesBook> m_items;
    int m_currentId = 0;
    int m_page = 0;
    int m_totalPages = 1;

    bool m_loading = false;
    bool m_loaded = false;
    uint64_t m_generation = 0;
    std::unordered_map<std::string, Rect> m_rects;
};

} // namespace kinnovel::ui
