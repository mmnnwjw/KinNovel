#pragma once

#include "kinnovel/hal/IPower.hpp"
#include <atomic>

namespace kinnovel::hal {

class MockPowerManager : public IPower {
public:
    MockPowerManager() = default;
    ~MockPowerManager() override = default;

    void start(IPowerObserver* observer) override {
        m_observer = observer;
    }

    void stop() override {
        m_observer = nullptr;
    }

    bool isSleeping() const override {
        return m_isSleeping;
    }

    void handlePowerKey() override {
        if (m_isSleeping) {
            triggerResume();
        } else {
            triggerSuspend();
        }
    }

    void triggerSuspend() {
        m_isSleeping = true;
        if (m_observer) {
            m_observer->onSuspend();
        }
    }

    void triggerResume() {
        m_isSleeping = false;
        if (m_observer) {
            m_observer->onResume();
        }
    }

private:
    IPowerObserver* m_observer = nullptr;
    std::atomic<bool> m_isSleeping{false};
};

} // namespace kinnovel::hal
