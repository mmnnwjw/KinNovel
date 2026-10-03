#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/ui/pages/BrowsePage.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

class HistoryPage : public IPage {
public:
    HistoryPage() = default;
    ~HistoryPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context);

    int getPage() const { return m_page; }
    void setPage(int p) { m_page = p; }
    const std::vector<BookItem>& getItems() const { return m_items; }
    void setItems(std::vector<BookItem> items) { m_items = std::move(items); m_loaded = true; }
    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }

private:
    std::vector<BookItem> m_items;
    int m_page = 0;

    bool m_loading = false;
    bool m_loaded = false;
    std::unordered_map<std::string, Rect> m_rects;

    void clearHistory(PageContext& context);
};

} // namespace kinnovel::ui
