#pragma once

#include <cstdint>
#include <string>
#include <memory>
#include "kinnovel/core/Types.hpp"

namespace kinnovel::hal {

enum class Waveform {
    Auto,
    Du,
    Du4,
    Gc16,
    Gl16,
    Reagl,
    A2
};

enum class SwipeDirection {
    Left = 2,
    Right = 3,
    Down = 0,
    Up = 1
};

class IDisplay {
public:
    virtual ~IDisplay() = default;

    virtual bool initialize(const std::string& fbPath, const std::string& protocol) = 0;
    virtual bool probe() = 0;
    virtual void close() = 0;

    virtual int getWidth() const = 0;
    virtual int getHeight() const = 0;
    virtual int getBpp() const = 0;

    // Writes 8bpp grayscale image buffer to internal framebuffer/memory
    virtual void writeImage(const uint8_t* grayBuffer, int x, int y, int w, int h, int pitch) = 0;

    // Trigger EPDC or simulated screen refresh
    virtual bool refresh(const Rect& region, bool isFlashing, Waveform waveform, bool dither = false) = 0;

    // Check if hardware swipe animation is supported
    virtual bool supportsSwipeAnimation() const = 0;
    virtual void setSwipeAnimation(bool enabled, SwipeDirection direction, int steps = 12) = 0;

    // Wakeup controller
    virtual void powerOn() = 0;
};

} // namespace kinnovel::hal
