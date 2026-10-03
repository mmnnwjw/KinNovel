#pragma once

#include <mutex>
#include <condition_variable>
#include <deque>
#include <chrono>

namespace kinnovel::network {

class RateLimiter {
public:
    RateLimiter(int maximum = 9, double windowSeconds = 5.5);
    ~RateLimiter() = default;

    void wait();
    void reset();

    int getMaximum() const { return m_maximum; }
    double getWindowSeconds() const { return m_windowSeconds; }

private:
    int m_maximum;
    double m_windowSeconds;
    std::deque<std::chrono::steady_clock::time_point> m_history;
    std::mutex m_mutex;
    std::condition_variable m_cv;
};

} // namespace kinnovel::network
