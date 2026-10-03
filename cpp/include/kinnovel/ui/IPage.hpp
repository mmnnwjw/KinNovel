#pragma once

#include "kinnovel/hal/IInput.hpp"

#include <string>

namespace kinnovel::ui {

class PageContext;
class Canvas;

class IPage {
public:
    virtual ~IPage() = default;

    virtual void enter(PageContext& context) { (void)context; }
    virtual void render(PageContext& context, Canvas& canvas) = 0;
    virtual bool handle(const hal::TouchGesture& gesture, PageContext& context) = 0;

    virtual bool headerBlocked() const { return false; }
    virtual void uploadProgress(PageContext& context) { (void)context; }
};

} // namespace kinnovel::ui
