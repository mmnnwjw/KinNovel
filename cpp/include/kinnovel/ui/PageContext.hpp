#pragma once

#include "kinnovel/core/Config.hpp"
#include "kinnovel/hal/IDisplay.hpp"
#include "kinnovel/hal/IInput.hpp"
#include "kinnovel/hal/IPower.hpp"
#include "kinnovel/network/ApiClient.hpp"
#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/ui/FontSet.hpp"
#include "kinnovel/ui/ImageCache.hpp"
#include "kinnovel/ui/IPage.hpp"

#include <string>
#include <vector>
#include <unordered_map>
#include <memory>
#include <mutex>
#include <functional>
#include <atomic>
#include <thread>

namespace kinnovel::ui {

struct ModalState {
    std::vector<std::string> lines;
    std::vector<std::string> buttons;
    std::unordered_map<std::string, std::function<void()>> actions;
    std::vector<std::pair<std::string, Rect>> rects;
};

class PageContext {
public:
    PageContext(std::shared_ptr<hal::IDisplay> display,
                std::shared_ptr<core::Config> config,
                std::shared_ptr<network::ApiClient> api,
                std::shared_ptr<FontSet> fonts,
                std::shared_ptr<ImageCache> images,
                std::shared_ptr<hal::IPower> power = nullptr);
    ~PageContext();

    int getWidth() const { return m_display ? m_display->getWidth() : 1072; }
    int getHeight() const { return m_display ? m_display->getHeight() : 1448; }

    std::shared_ptr<hal::IDisplay> getDisplay() const { return m_display; }
    std::shared_ptr<core::Config> getConfig() const { return m_config; }
    std::shared_ptr<network::ApiClient> getApi() const { return m_api; }
    std::shared_ptr<FontSet> getFonts() const { return m_fonts; }
    std::shared_ptr<ImageCache> getImages() const { return m_images; }
    std::shared_ptr<hal::IPower> getPower() const { return m_power; }

    void registerPage(const std::string& name, std::shared_ptr<IPage> page);
    bool hasPage(const std::string& name) const;

    const std::string& getPageName() const { return m_pageName; }
    const std::string& getPreviousPage() const { return m_previousPage; }
    std::unordered_map<std::string, std::string>& getParams() { return m_params; }
    const std::unordered_map<std::string, std::string>& getParams() const { return m_params; }

    std::vector<std::pair<std::string, std::unordered_map<std::string, std::string>>>& getStack() { return m_stack; }
    const std::vector<std::pair<std::string, std::unordered_map<std::string, std::string>>>& getStack() const { return m_stack; }

    void navigate(const std::string& name, bool push = true,
                  const std::unordered_map<std::string, std::string>& params = {});
    void replace(const std::string& name,
                 const std::unordered_map<std::string, std::string>& params = {});
    void back();
    void home();

    std::shared_ptr<Canvas> render();
    void show(bool isFlashing = false, bool force = false);
    bool handle(const hal::TouchGesture& event);

    void toast(const std::string& message, double seconds = 2.0);
    void confirm(const std::vector<std::string>& lines, std::function<void()> onYes,
                 std::function<void()> onNo = nullptr,
                 const std::string& yes = "确定", const std::string& no = "取消");
    void confirm(const std::string& line, std::function<void()> onYes,
                 std::function<void()> onNo = nullptr,
                 const std::string& yes = "确定", const std::string& no = "取消");
    void message(const std::vector<std::string>& lines, const std::string& closeLabel = "确定",
                 std::function<void()> onClose = nullptr);
    void message(const std::string& line, const std::string& closeLabel = "确定",
                 std::function<void()> onClose = nullptr);

    void runAsync(const std::string& ownerPage, std::function<void()> operation);
    void runAsync(const std::string& ownerPage,
                  std::function<void()> operation,
                  std::function<void()> onSuccess,
                  std::function<void(const std::exception&)> onError = nullptr);

    int pruneCache();

    void setOnStopCallback(std::function<void()> cb) { m_onStop = std::move(cb); }
    void stop();

    const HeaderState& getHeaderState() const { return m_headerState; }
    const std::unique_ptr<ModalState>& getModal() const { return m_modal; }
    void setModal(std::unique_ptr<ModalState> modal) { m_modal = std::move(modal); }

private:
    std::shared_ptr<hal::IDisplay> m_display;
    std::shared_ptr<core::Config> m_config;
    std::shared_ptr<network::ApiClient> m_api;
    std::shared_ptr<FontSet> m_fonts;
    std::shared_ptr<ImageCache> m_images;
    std::shared_ptr<hal::IPower> m_power;

    std::unordered_map<std::string, std::shared_ptr<IPage>> m_pages;
    std::vector<std::pair<std::string, std::unordered_map<std::string, std::string>>> m_stack;

    std::string m_pageName = "home";
    std::string m_previousPage;
    std::unordered_map<std::string, std::string> m_params;

    std::unique_ptr<ModalState> m_modal;
    std::string m_status;
    std::atomic<uint64_t> m_toastGeneration{0};

    HeaderState m_headerState;
    double m_lastBackAt = 0.0;
    std::recursive_mutex m_showLock;
    std::atomic<bool> m_closed{false};

    std::function<void()> m_onStop;
};

} // namespace kinnovel::ui
