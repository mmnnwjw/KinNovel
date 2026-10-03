#include "MockInput.hpp"
#include <cmath>

namespace kinnovel::hal {

bool MockInput::initialize(int renderWidth, int renderHeight, const std::string& /*devicePath*/) {
    m_renderW = renderWidth;
    m_renderH = renderHeight;
    return true;
}

void MockInput::close() {
    stop();
}

void MockInput::listen(GestureCallback onGesture) {
    m_callback = onGesture;
    m_running = true;

    while (m_running) {
        TouchGesture g;
        bool hasItem = false;
        {
            std::lock_guard<std::mutex> lock(m_mutex);
            if (!m_injected.empty()) {
                g = m_injected.front();
                m_injected.pop();
                hasItem = true;
            }
        }
        if (hasItem && m_callback) {
            m_callback(g);
        } else {
            break;
        }
    }
}

void MockInput::stop() {
    m_running = false;
}

void MockInput::injectTap(int x, int y, int durationMs) {
    TouchGesture g;
    g.kind = GestureKind::Tap;
    g.durationMs = durationMs;
    g.distancePx = 0;
    g.point.xPixel = x;
    g.point.yPixel = y;
    g.point.xRatio = m_renderW > 0 ? static_cast<float>(x) / m_renderW : 0.0f;
    g.point.yRatio = m_renderH > 0 ? static_cast<float>(y) / m_renderH : 0.0f;

    if (m_running && m_callback) {
        m_callback(g);
    } else {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_injected.push(g);
    }
}

void MockInput::injectLongPress(int x, int y, int durationMs) {
    TouchGesture g;
    g.kind = GestureKind::LongPress;
    g.durationMs = durationMs;
    g.distancePx = 0;
    g.point.xPixel = x;
    g.point.yPixel = y;
    g.point.xRatio = m_renderW > 0 ? static_cast<float>(x) / m_renderW : 0.0f;
    g.point.yRatio = m_renderH > 0 ? static_cast<float>(y) / m_renderH : 0.0f;

    if (m_running && m_callback) {
        m_callback(g);
    } else {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_injected.push(g);
    }
}

void MockInput::injectSwipe(GestureKind kind, int startX, int startY, int endX, int endY, int durationMs) {
    TouchGesture g;
    g.kind = kind;
    g.durationMs = durationMs;
    int dx = endX - startX;
    int dy = endY - startY;
    g.distancePx = static_cast<int>(std::sqrt(dx * dx + dy * dy));

    g.startPoint.xPixel = startX;
    g.startPoint.yPixel = startY;
    g.startPoint.xRatio = m_renderW > 0 ? static_cast<float>(startX) / m_renderW : 0.0f;
    g.startPoint.yRatio = m_renderH > 0 ? static_cast<float>(startY) / m_renderH : 0.0f;

    g.endPoint.xPixel = endX;
    g.endPoint.yPixel = endY;
    g.endPoint.xRatio = m_renderW > 0 ? static_cast<float>(endX) / m_renderW : 0.0f;
    g.endPoint.yRatio = m_renderH > 0 ? static_cast<float>(endY) / m_renderH : 0.0f;

    if (m_running && m_callback) {
        m_callback(g);
    } else {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_injected.push(g);
    }
}

} // namespace kinnovel::hal
