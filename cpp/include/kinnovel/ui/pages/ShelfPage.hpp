#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/ui/pages/BrowsePage.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

struct ShelfItem {
    int id = 0;
    std::string type; // FOLDER, NOVEL, COMIC
    std::string title;
    std::vector<std::string> parents;
    int index = 0;
};

class ShelfPage : public IPage {
public:
    ShelfPage() = default;
    ~ShelfPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    void load(PageContext& context);

    int getPage() const { return m_page; }
    void setPage(int p) { m_page = p; }
    const std::vector<ShelfItem>& getItems() const { return m_items; }
    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }

private:
    std::vector<ShelfItem> m_items;
    std::vector<ShelfItem> m_visible;
    std::unordered_map<int, BookItem> m_books;
    std::vector<std::string> m_path;
    int m_page = 0;

    bool m_loading = false;
    bool m_loaded = false;
    std::unordered_map<std::string, Rect> m_rects;

    std::string lastParent(const ShelfItem& item) const;
};

} // namespace kinnovel::ui
