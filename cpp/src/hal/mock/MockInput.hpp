#pragma once

#include "kinnovel/hal/IInput.hpp"
#include <mutex>
#include <atomic>
#include <queue>

namespace kinnovel::hal {

class MockInput : public IInput {
public:
    MockInput() = default;
    ~MockInput() override = default;

    bool initialize(int renderWidth, int renderHeight, const std::string& devicePath = "") override;
    void close() override;

    bool grab() override { m_grabbed = true; return true; }
    bool ungrab() override { m_grabbed = false; return true; }
    void resetGestureState() override {}

    void listen(GestureCallback onGesture) override;
    void stop() override;

    // Test injection helpers
    void injectTap(int x, int y, int durationMs = 50);
    void injectLongPress(int x, int y, int durationMs = 600);
    void injectSwipe(GestureKind kind, int startX, int startY, int endX, int endY, int durationMs = 150);

    bool isGrabbed() const { return m_grabbed; }

private:
    int m_renderW = 0;
    int m_renderH = 0;
    bool m_grabbed = false;
    std::atomic<bool> m_running{false};
    GestureCallback m_callback;
    std::mutex m_mutex;
    std::queue<TouchGesture> m_injected;
};

} // namespace kinnovel::hal
