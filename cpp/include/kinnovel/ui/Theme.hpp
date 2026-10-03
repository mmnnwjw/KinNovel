#pragma once

#include <cstdint>

namespace kinnovel::ui {

class Theme {
public:
    explicit Theme(bool night = false)
        : m_night(night) {
        update();
    }

    static Theme day() { return Theme(false); }
    static Theme night() { return Theme(true); }

    void setNight(bool night) {
        m_night = night;
        update();
    }

    bool isNight() const { return m_night; }

    uint8_t background = 255;
    uint8_t foreground = 0;
    uint8_t muted = 105;
    uint8_t light = 225;
    uint8_t mid = 170;
    uint8_t inverseFg = 255;
    uint8_t inverseBg = 0;

private:
    bool m_night = false;

    void update() {
        if (m_night) {
            background = 0;
            foreground = 255;
            muted = 165;
            light = 40;
            mid = 85;
            inverseFg = 0;
            inverseBg = 255;
        } else {
            background = 255;
            foreground = 0;
            muted = 105;
            light = 225;
            mid = 170;
            inverseFg = 255;
            inverseBg = 0;
        }
    }
};

} // namespace kinnovel::ui
