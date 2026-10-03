#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

class HomePage : public IPage {
public:
    HomePage() = default;
    ~HomePage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    std::vector<std::pair<std::string, std::string>> getItems(PageContext& context) const;

private:
    std::unordered_map<std::string, Rect> m_rects;
    std::string m_onlineCount;
};

} // namespace kinnovel::ui
