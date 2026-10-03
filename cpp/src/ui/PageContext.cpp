#include "kinnovel/ui/PageContext.hpp"
#include "kinnovel/core/Logger.hpp"
#include "kinnovel/core/Utils.hpp"

#include <chrono>

namespace kinnovel::ui {

namespace {

double getMonotonicSeconds() {
    using namespace std::chrono;
    return duration<double>(steady_clock::now().time_since_epoch()).count();
}

} // namespace

PageContext::PageContext(std::shared_ptr<hal::IDisplay> display,
                         std::shared_ptr<core::Config> config,
                         std::shared_ptr<network::ApiClient> api,
                         std::shared_ptr<FontSet> fonts,
                         std::shared_ptr<ImageCache> images,
                         std::shared_ptr<hal::IPower> power)
    : m_display(std::move(display))
    , m_config(std::move(config))
    , m_api(std::move(api))
    , m_fonts(std::move(fonts))
    , m_images(std::move(images))
    , m_power(std::move(power)) {
}

PageContext::~PageContext() {
    m_closed = true;
}

void PageContext::registerPage(const std::string& name, std::shared_ptr<IPage> page) {
    if (!name.empty() && page) {
        m_pages[name] = std::move(page);
    }
}

bool PageContext::hasPage(const std::string& name) const {
    return m_pages.find(name) != m_pages.end();
}

void PageContext::navigate(const std::string& name, bool push,
                           const std::unordered_map<std::string, std::string>& params) {
    auto it = m_pages.find(name);
    if (it == m_pages.end()) {
        Logger::error("PageContext", "navigate unknown page: " + name);
        return;
    }

    if (push && name != m_pageName) {
        m_stack.push_back({ m_pageName, m_params });
        if (m_stack.size() > 20) {
            m_stack.erase(m_stack.begin());
        }
    }

    m_previousPage = m_pageName;
    m_pageName = name;
    m_params = params;
    m_modal.reset();

    it->second->enter(*this);
    show();
}

void PageContext::replace(const std::string& name,
                          const std::unordered_map<std::string, std::string>& params) {
    auto it = m_pages.find(name);
    if (it == m_pages.end()) {
        Logger::error("PageContext", "replace unknown page: " + name);
        return;
    }

    m_previousPage = m_pageName;
    m_pageName = name;
    m_params = params;
    m_modal.reset();

    it->second->enter(*this);
    show();
}

void PageContext::back() {
    if (m_modal) {
        m_modal.reset();
        show();
        return;
    }

    double now = getMonotonicSeconds();
    if (now - m_lastBackAt < 0.35) {
        return;
    }
    m_lastBackAt = now;

    if (!m_stack.empty()) {
        m_previousPage = m_pageName;
        auto top = m_stack.back();
        m_stack.pop_back();
        m_pageName = top.first;
        m_params = top.second;
    } else {
        m_previousPage = m_pageName;
        m_pageName = "home";
        m_params.clear();
    }

    auto it = m_pages.find(m_pageName);
    if (it != m_pages.end()) {
        it->second->enter(*this);
    }
    show();
}

void PageContext::home() {
    m_stack.clear();
    m_previousPage = m_pageName;
    m_pageName = "home";
    m_params.clear();
    m_modal.reset();

    auto it = m_pages.find("home");
    if (it != m_pages.end()) {
        it->second->enter(*this);
    }
    show();
}

std::shared_ptr<Canvas> PageContext::render() {
    Theme theme(m_config ? m_config->getBool("night_mode", false) : false);
    auto canvas = std::make_shared<Canvas>(getWidth(), getHeight(), theme, m_fonts.get());

    auto it = m_pages.find(m_pageName);
    if (it != m_pages.end()) {
        it->second->render(*this, *canvas);
    }
    m_headerState = canvas->getHeaderState();

    if (!m_status.empty()) {
        int width = std::min(canvas->getWidth() - 80, std::max(300, static_cast<int>(m_status.size()) * 24));
        int x = (canvas->getWidth() - width) / 2;
        int y = canvas->getHeight() - 90;
        canvas->drawRoundedRect(x, y, width, 64, 14, theme.foreground, theme.background, 2);
        canvas->drawCenteredText(canvas->getWidth() / 2, y + 32, m_status,
                                 m_fonts && m_fonts->small ? m_fonts->small.get() : nullptr);
    }

    if (m_modal) {
        m_modal->rects = canvas->popup(
            m_modal->lines,
            m_modal->buttons,
            m_fonts && m_fonts->small ? m_fonts->small.get() : nullptr,
            m_fonts && m_fonts->body ? m_fonts->body.get() : nullptr
        );
    }

    return canvas;
}

void PageContext::show(bool isFlashing, bool force) {
    if (!force && m_power && m_power->isSleeping()) {
        return;
    }
    if (m_closed || !m_display) {
        return;
    }

    std::lock_guard<std::recursive_mutex> lock(m_showLock);
    auto canvas = render();
    bool flash = isFlashing || (m_config && m_config->getBool("page_flash", false));

    m_display->writeImage(canvas->getBuffer().data(), 0, 0, canvas->getWidth(), canvas->getHeight(), canvas->getWidth());
    m_display->refresh(Rect{ 0, 0, canvas->getWidth(), canvas->getHeight() },
                       flash, flash ? hal::Waveform::Gc16 : hal::Waveform::Auto, false);
}

bool PageContext::handle(const hal::TouchGesture& event) {
    if (event.kind != hal::GestureKind::Tap &&
        event.kind != hal::GestureKind::LongPress &&
        event.kind != hal::GestureKind::Down) {
        return false;
    }

    int px = event.point.xPixel;
    int py = event.point.yPixel;

    if (event.kind == hal::GestureKind::Tap || event.kind == hal::GestureKind::LongPress) {
        if (px == 0 && py == 0) return false;
        if (px < 0 || py < 0 || px >= getWidth() || py >= getHeight()) {
            return false;
        }
    }

    if (m_modal) {
        if (event.kind != hal::GestureKind::Tap) {
            return true;
        }
        for (const auto& btn : m_modal->rects) {
            if (btn.second.contains(px, py)) {
                auto actIt = m_modal->actions.find(btn.first);
                std::function<void()> action = (actIt != m_modal->actions.end()) ? actIt->second : nullptr;
                m_modal.reset();
                if (action) {
                    action();
                }
                show();
                return true;
            }
        }
        return true;
    }

    if (event.kind == hal::GestureKind::Tap && m_headerState.height > 0) {
        if (py < m_headerState.height) {
            auto it = m_pages.find(m_pageName);
            if (it != m_pages.end() && it->second->headerBlocked()) {
                return true;
            }

            if (px < static_cast<int>(getWidth() * 0.16) && !m_headerState.left.empty()) {
                if (it != m_pages.end()) {
                    it->second->uploadProgress(*this);
                }
                back();
                return true;
            }

            if (px > static_cast<int>(getWidth() * 0.84) && m_headerState.right == "主页") {
                if (it != m_pages.end()) {
                    it->second->uploadProgress(*this);
                }
                home();
                return true;
            }
        }
    }

    auto it = m_pages.find(m_pageName);
    if (it != m_pages.end()) {
        return it->second->handle(event, *this);
    }
    return false;
}

void PageContext::toast(const std::string& message, double seconds) {
    m_status = message;
    uint64_t gen = ++m_toastGeneration;
    show();

    std::thread([this, gen, seconds]() {
        std::this_thread::sleep_for(std::chrono::milliseconds(static_cast<int>(seconds * 1000)));
        if (m_toastGeneration == gen) {
            m_status.clear();
            show();
        }
    }).detach();
}

void PageContext::confirm(const std::vector<std::string>& lines, std::function<void()> onYes,
                          std::function<void()> onNo,
                          const std::string& yes, const std::string& no) {
    auto modal = std::make_unique<ModalState>();
    modal->lines = lines;
    modal->buttons = { yes, no };
    modal->actions[yes] = std::move(onYes);
    if (onNo) modal->actions[no] = std::move(onNo);
    m_modal = std::move(modal);
    show();
}

void PageContext::confirm(const std::string& line, std::function<void()> onYes,
                          std::function<void()> onNo,
                          const std::string& yes, const std::string& no) {
    confirm(std::vector<std::string>{ line }, std::move(onYes), std::move(onNo), yes, no);
}

void PageContext::message(const std::vector<std::string>& lines, const std::string& closeLabel,
                          std::function<void()> onClose) {
    auto modal = std::make_unique<ModalState>();
    modal->lines = lines;
    modal->buttons = { closeLabel };
    if (onClose) modal->actions[closeLabel] = std::move(onClose);
    m_modal = std::move(modal);
    show();
}

void PageContext::message(const std::string& line, const std::string& closeLabel,
                          std::function<void()> onClose) {
    message(std::vector<std::string>{ line }, closeLabel, std::move(onClose));
}

void PageContext::runAsync(const std::string& ownerPage, std::function<void()> operation) {
    runAsync(ownerPage, std::move(operation), nullptr, nullptr);
}

void PageContext::runAsync(const std::string& ownerPage,
                          std::function<void()> operation,
                          std::function<void()> onSuccess,
                          std::function<void(const std::exception&)> onError) {
    std::thread([this, ownerPage, op = std::move(operation),
                 sc = std::move(onSuccess), er = std::move(onError)]() {
        try {
            if (op) op();
            if (m_pageName == ownerPage) {
                if (sc) sc();
                show();
            }
        } catch (const std::exception& exc) {
            Logger::error("PageContext", "Async error in page " + ownerPage + ": " + exc.what());
            if (m_pageName == ownerPage) {
                if (er) {
                    er(exc);
                } else {
                    message(std::vector<std::string>{ "操作失败", exc.what() });
                }
                show();
            }
        } catch (...) {
            Logger::error("PageContext", "Unknown async error in page " + ownerPage);
            if (m_pageName == ownerPage) {
                message("操作失败: 未知错误");
                show();
            }
        }
    }).detach();
}

int PageContext::pruneCache() {
    int limitMb = m_config ? m_config->getInt("cache_limit_mb", 192) : 192;
    uint64_t limitBytes = static_cast<uint64_t>(limitMb) * 1024 * 1024;
    uint64_t perDir = std::max<uint64_t>(1, limitBytes / 4);

    std::string cacheDir = core::Config::getCacheDir();
    int removed = 0;
    removed += core::Utils::pruneCache(cacheDir + "/covers", perDir);
    removed += core::Utils::pruneCache(cacheDir + "/fonts", perDir);
    removed += core::Utils::pruneCache(cacheDir + "/images", perDir);
    removed += core::Utils::pruneCache(cacheDir + "/content", perDir);
    return removed;
}

void PageContext::stop() {
    if (m_onStop) {
        m_onStop();
    }
}

} // namespace kinnovel::ui
