#include "kinnovel/network/RateLimiter.hpp"
#include <algorithm>

namespace kinnovel::network {

RateLimiter::RateLimiter(int maximum, double windowSeconds)
    : m_maximum(std::max(1, maximum)),
      m_windowSeconds(std::max(0.1, windowSeconds)) {
}

void RateLimiter::wait() {
    std::unique_lock<std::mutex> lock(m_mutex);
    auto windowDuration = std::chrono::duration<double>(m_windowSeconds);

    while (true) {
        auto now = std::chrono::steady_clock::now();
        while (!m_history.empty() && (now - m_history.front()) >= windowDuration) {
            m_history.pop_front();
        }

        if (static_cast<int>(m_history.size()) < m_maximum) {
            m_history.push_back(now);
            return;
        }

        auto elapsed = now - m_history.front();
        auto waitTime = windowDuration - elapsed + std::chrono::milliseconds(20);
        m_cv.wait_for(lock, std::max(std::chrono::milliseconds(20),
                                    std::chrono::duration_cast<std::chrono::milliseconds>(waitTime)));
    }
}

void RateLimiter::reset() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_history.clear();
    m_cv.notify_all();
}

} // namespace kinnovel::network
