#include "EvdevInput.hpp"
#include "kinnovel/core/Logger.hpp"

#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <linux/input.h>
#include <glob.h>
#include <cmath>
#include <algorithm>

namespace kinnovel::hal {

EvdevInput::EvdevInput() = default;

EvdevInput::~EvdevInput() {
    close();
}

std::string EvdevInput::findTouchDevice() {
    glob_t globResult;
    if (glob("/dev/input/event*", GLOB_NOSORT, nullptr, &globResult) != 0) {
        return "";
    }

    std::string matchedPath;
    for (size_t i = 0; i < globResult.gl_pathc; ++i) {
        const char* path = globResult.gl_pathv[i];
        int fd = ::open(path, O_RDONLY | O_NONBLOCK);
        if (fd < 0) continue;

        unsigned long absBits[(ABS_MAX + 1) / (sizeof(unsigned long) * 8) + 1] = {0};
        if (::ioctl(fd, EVIOCGBIT(EV_ABS, sizeof(absBits)), absBits) >= 0) {
            bool hasX = (absBits[ABS_MT_POSITION_X / (sizeof(unsigned long) * 8)] & (1UL << (ABS_MT_POSITION_X % (sizeof(unsigned long) * 8)))) != 0;
            bool hasY = (absBits[ABS_MT_POSITION_Y / (sizeof(unsigned long) * 8)] & (1UL << (ABS_MT_POSITION_Y % (sizeof(unsigned long) * 8)))) != 0;

            if (hasX && hasY) {
                struct input_absinfo absX{}, absY{};
                if (::ioctl(fd, EVIOCGABS(ABS_MT_POSITION_X), &absX) >= 0 &&
                    ::ioctl(fd, EVIOCGABS(ABS_MT_POSITION_Y), &absY) >= 0) {
                    m_minX = absX.minimum;
                    m_maxX = absX.maximum;
                    m_minY = absY.minimum;
                    m_maxY = absY.maximum;
                    matchedPath = path;
                    ::close(fd);
                    break;
                }
            }
        }
        ::close(fd);
    }

    globfree(&globResult);
    return matchedPath;
}

bool EvdevInput::initialize(int renderWidth, int renderHeight, const std::string& devicePath) {
    close();
    m_renderW = renderWidth;
    m_renderH = renderHeight;

    if (!devicePath.empty()) {
        m_devicePath = devicePath;
    } else {
        m_devicePath = findTouchDevice();
    }

    if (m_devicePath.empty()) {
        Logger::warn("Input", "No evdev multi-touch device found");
        return false;
    }

    m_fd = ::open(m_devicePath.c_str(), O_RDONLY);
    if (m_fd < 0) {
        Logger::error("Input", "Failed to open input device: " + m_devicePath);
        return false;
    }

    Logger::info("Input", "Opened touch device: " + m_devicePath + " (Range X: " +
                 std::to_string(m_minX) + ".." + std::to_string(m_maxX) + ", Y: " +
                 std::to_string(m_minY) + ".." + std::to_string(m_maxY) + ")");
    return true;
}

void EvdevInput::close() {
    stop();
    if (m_fd >= 0) {
        ungrab();
        ::close(m_fd);
        m_fd = -1;
    }
}

bool EvdevInput::grab() {
    if (m_fd >= 0 && !m_grabbed) {
        if (::ioctl(m_fd, EVIOCGRAB, 1) == 0) {
            m_grabbed = true;
            Logger::info("Input", "Exclusively grabbed touch device");
            return true;
        }
    }
    return m_grabbed;
}

bool EvdevInput::ungrab() {
    if (m_fd >= 0 && m_grabbed) {
        ::ioctl(m_fd, EVIOCGRAB, 0);
        m_grabbed = false;
        Logger::info("Input", "Released touch device grab");
        return true;
    }
    return false;
}

void EvdevInput::resetGestureState() {
    m_slots.clear();
    m_currentSlot = 0;
}

TouchPoint EvdevInput::makePoint(int rawX, int rawY) {
    TouchPoint p;
    int rangeX = std::max(1, m_maxX - m_minX);
    int rangeY = std::max(1, m_maxY - m_minY);

    float rx = static_cast<float>(rawX - m_minX) / rangeX;
    float ry = static_cast<float>(rawY - m_minY) / rangeY;

    rx = std::clamp(rx, 0.0f, 1.0f);
    ry = std::clamp(ry, 0.0f, 1.0f);

    p.xRatio = rx;
    p.yRatio = ry;
    p.xPixel = std::clamp(static_cast<int>(rx * m_renderW), 0, m_renderW - 1);
    p.yPixel = std::clamp(static_cast<int>(ry * m_renderH), 0, m_renderH - 1);

    return p;
}

void EvdevInput::handleSlotUp(int slot, GestureCallback& onGesture) {
    auto it = m_slots.find(slot);
    if (it == m_slots.end()) return;

    SlotState& s = it->second;
    if (s.rawX < 0 || s.rawY < 0) {
        m_slots.erase(it);
        return;
    }

    auto now = std::chrono::steady_clock::now();
    double durationS = std::chrono::duration<double>(now - s.startTs).count();
    int durationMs = static_cast<int>(durationS * 1000.0);

    TouchPoint startPt = makePoint(s.startX >= 0 ? s.startX : s.rawX,
                                   s.startY >= 0 ? s.startY : s.rawY);
    TouchPoint endPt = makePoint(s.rawX, s.rawY);

    int dx = endPt.xPixel - startPt.xPixel;
    int dy = endPt.yPixel - startPt.yPixel;
    int dist = static_cast<int>(std::sqrt(dx * dx + dy * dy));

    GestureKind kind = GestureKind::Unknown;

    // Gesture threshold aligned with Python MultiTouchParser
    // tap_max_move_px = 30, tap_max_duration_s = 0.30s
    // swipe_min_distance_px = 40
    if (dist <= 30) {
        if (durationS <= 0.30) {
            kind = GestureKind::Tap;
        } else {
            kind = GestureKind::LongPress;
        }
    } else if (dist >= 40) {
        if (std::abs(dx) > std::abs(dy)) {
            kind = (dx < 0) ? GestureKind::Left : GestureKind::Right;
        } else {
            kind = (dy < 0) ? GestureKind::Up : GestureKind::Down;
        }
    }

    if (kind != GestureKind::Unknown && onGesture) {
        TouchGesture g;
        g.kind = kind;
        g.durationMs = durationMs;
        g.distancePx = dist;
        g.point = startPt;
        g.startPoint = startPt;
        g.endPoint = endPt;
        onGesture(g);
    }

    m_slots.erase(it);
}

void EvdevInput::processEvent(const struct input_event& ev, GestureCallback& onGesture) {
    if (ev.type == EV_ABS) {
        if (ev.code == ABS_MT_SLOT) {
            m_currentSlot = ev.value;
        } else if (ev.code == ABS_MT_TRACKING_ID) {
            if (ev.value == -1) {
                handleSlotUp(m_currentSlot, onGesture);
            } else {
                SlotState& s = m_slots[m_currentSlot];
                s.trackingId = ev.value;
                s.rawX = -1;
                s.rawY = -1;
                s.startX = -1;
                s.startY = -1;
                s.startTs = std::chrono::steady_clock::now();
                s.lastTs = s.startTs;
                s.downReported = false;
            }
        } else if (ev.code == ABS_MT_POSITION_X || ev.code == ABS_X) {
            SlotState& s = m_slots[m_currentSlot];
            s.rawX = ev.value;
            if (s.startX < 0) s.startX = ev.value;
            s.lastTs = std::chrono::steady_clock::now();
        } else if (ev.code == ABS_MT_POSITION_Y || ev.code == ABS_Y) {
            SlotState& s = m_slots[m_currentSlot];
            s.rawY = ev.value;
            if (s.startY < 0) s.startY = ev.value;
            s.lastTs = std::chrono::steady_clock::now();
        }
    }
}

void EvdevInput::listen(GestureCallback onGesture) {
    if (m_fd < 0) return;
    m_running = true;

    struct input_event events[32];
    while (m_running) {
        ssize_t n = ::read(m_fd, events, sizeof(events));
        if (n <= 0) {
            if (!m_running) break;
            usleep(10000);
            continue;
        }

        size_t count = n / sizeof(struct input_event);
        for (size_t i = 0; i < count; ++i) {
            processEvent(events[i], onGesture);
        }
    }
}

void EvdevInput::stop() {
    m_running = false;
}

} // namespace kinnovel::hal
