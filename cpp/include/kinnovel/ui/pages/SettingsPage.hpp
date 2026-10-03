#pragma once

#include "kinnovel/ui/IPage.hpp"
#include "kinnovel/ui/Canvas.hpp"

#include <vector>
#include <string>
#include <unordered_map>

namespace kinnovel::ui {

class SettingsPage : public IPage {
public:
    SettingsPage() = default;
    ~SettingsPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    const std::unordered_map<std::string, Rect>& getRects() const { return m_rects; }

private:
    std::unordered_map<std::string, Rect> m_rects;
};

class AboutPage : public IPage {
public:
    AboutPage() = default;
    ~AboutPage() override = default;

    void enter(PageContext& context) override;
    void render(PageContext& context, Canvas& canvas) override;
    bool handle(const hal::TouchGesture& event, PageContext& context) override;

    static const std::vector<std::string>& getAboutLines();
};

} // namespace kinnovel::ui
