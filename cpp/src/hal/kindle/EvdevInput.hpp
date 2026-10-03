#pragma once

#include "kinnovel/hal/IInput.hpp"
#include <linux/input.h>
#include <string>
#include <vector>
#include <map>
#include <atomic>
#include <chrono>

namespace kinnovel::hal {

struct SlotState {
    int trackingId = -1;
    int rawX = -1;
    int rawY = -1;
    int startX = -1;
    int startY = -1;
    std::chrono::steady_clock::time_point startTs;
    std::chrono::steady_clock::time_point lastTs;
    bool downReported = false;
};

class EvdevInput : public IInput {
public:
    EvdevInput();
    ~EvdevInput() override;

    bool initialize(int renderWidth, int renderHeight, const std::string& devicePath = "") override;
    void close() override;

    bool grab() override;
    bool ungrab() override;
    void resetGestureState() override;

    void listen(GestureCallback onGesture) override;
    void stop() override;

private:
    std::string findTouchDevice();
    void processEvent(const struct input_event& ev, GestureCallback& onGesture);
    void handleSlotUp(int slot, GestureCallback& onGesture);

    TouchPoint makePoint(int rawX, int rawY);

    int m_fd = -1;
    std::string m_devicePath;
    int m_renderW = 1072;
    int m_renderH = 1448;

    int m_minX = 0;
    int m_maxX = 1072;
    int m_minY = 0;
    int m_maxY = 1448;

    int m_currentSlot = 0;
    std::map<int, SlotState> m_slots;

    std::atomic<bool> m_running{false};
    bool m_grabbed = false;
};

} // namespace kinnovel::hal
