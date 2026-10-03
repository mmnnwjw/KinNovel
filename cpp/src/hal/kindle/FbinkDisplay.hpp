#pragma once

#include "kinnovel/hal/IDisplay.hpp"
#include <string>
#include <vector>
#include <mutex>

namespace kinnovel::hal {

class FbinkDisplay : public IDisplay {
public:
    FbinkDisplay();
    ~FbinkDisplay() override;

    bool initialize(const std::string& fbPath, const std::string& protocol) override;
    bool probe() override;
    void close() override;

    int getWidth() const override { return m_width; }
    int getHeight() const override { return m_height; }
    int getBpp() const override { return m_bpp; }

    void writeImage(const uint8_t* grayBuffer, int x, int y, int w, int h, int pitch) override;
    bool refresh(const Rect& region, bool isFlashing, Waveform waveform, bool dither = false) override;

    bool supportsSwipeAnimation() const override { return m_protocol == "mtk"; }
    void setSwipeAnimation(bool enabled, SwipeDirection direction, int steps = 12) override;
    void powerOn() override;

private:
    bool openFramebuffer(const std::string& fbPath);
    bool initEpdc();
    uint32_t getNextMarker();

    std::string m_fbPath;
    std::string m_protocol;
    int m_fd = -1;
    int m_width = 1072;
    int m_height = 1448;
    int m_bpp = 8;
    int m_lineLength = 1072;
    size_t m_smemLen = 0;
    uint8_t* m_mappedMem = nullptr;

    bool m_swipeEnabled = false;
    SwipeDirection m_swipeDirection = SwipeDirection::Left;
    int m_swipeSteps = 12;

    uint32_t m_marker = 0;
    int m_temp = 25;
    std::mutex m_mutex;
};

} // namespace kinnovel::hal
