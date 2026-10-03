#pragma once

#include <string>
#include <functional>
#include "kinnovel/core/Types.hpp"

namespace kinnovel::hal {

enum class GestureKind {
    Unknown,
    Tap,
    LongPress,
    Left,
    Right,
    Up,
    Down
};

struct TouchPoint {
    int xPixel = 0;
    int yPixel = 0;
    float xRatio = 0.0f;
    float yRatio = 0.0f;
};

struct TouchGesture {
    GestureKind kind = GestureKind::Unknown;
    int durationMs = 0;
    int distancePx = 0;
    TouchPoint point;      // Valid for Tap and LongPress
    TouchPoint startPoint; // Valid for swipe gestures
    TouchPoint endPoint;   // Valid for swipe gestures
};

using GestureCallback = std::function<void(const TouchGesture&)>;

class IInput {
public:
    virtual ~IInput() = default;

    virtual bool initialize(int renderWidth, int renderHeight, const std::string& devicePath = "") = 0;
    virtual void close() = 0;

    virtual bool grab() = 0;
    virtual bool ungrab() = 0;
    virtual void resetGestureState() = 0;

    virtual void listen(GestureCallback onGesture) = 0;
    virtual void stop() = 0;
};

} // namespace kinnovel::hal
