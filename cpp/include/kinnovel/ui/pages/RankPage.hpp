#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/ui/pages/BrowsePage.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

class RankPage : public IPage {
public:
    RankPage() = default;
    ~RankPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context);

    int getPage() const { return m_page; }
    void setPage(int p) { m_page = p; }
    int getTotalPages() const { return m_totalPages; }
    void setTotalPages(int tp) { m_totalPages = tp; }
    const std::vector<BookItem>& getItems() const { return m_items; }
    void setItems(std::vector<BookItem> items) { m_items = std::move(items); m_loaded = true; }
    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }

private:
    std::string m_kind = "daily";
    std::vector<BookItem> m_items;
    int m_page = 1;
    int m_totalPages = 1;

    bool m_loading = false;
    bool m_loaded = false;
    uint64_t m_generation = 0;
    std::unordered_map<std::string, Rect> m_rects;

    void calculateLayout(int height, int& top, int& listY, int& rowH, int& perPage, int& navY) const;
    int calculateTotalPages(int height);
};

} // namespace kinnovel::ui
