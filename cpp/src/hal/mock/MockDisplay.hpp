#pragma once

#include "kinnovel/hal/IDisplay.hpp"
#include <vector>
#include <string>
#include <mutex>

namespace kinnovel::hal {

class MockDisplay : public IDisplay {
public:
    MockDisplay(int width = 1072, int height = 1448);
    ~MockDisplay() override = default;

    bool initialize(const std::string& fbPath, const std::string& protocol) override;
    bool probe() override;
    void close() override;

    int getWidth() const override { return m_width; }
    int getHeight() const override { return m_height; }
    int getBpp() const override { return 8; }

    void writeImage(const uint8_t* grayBuffer, int x, int y, int w, int h, int pitch) override;
    bool refresh(const Rect& region, bool isFlashing, Waveform waveform, bool dither = false) override;

    bool supportsSwipeAnimation() const override { return m_supportsSwipe; }
    void setSwipeAnimation(bool enabled, SwipeDirection direction, int steps = 12) override;
    void powerOn() override {}

    // Mock-specific inspection methods
    bool saveToPng(const std::string& filePath) const;
    const std::vector<uint8_t>& getBuffer() const { return m_buffer; }
    int getRefreshCount() const { return m_refreshCount; }
    int getFlashingRefreshCount() const { return m_flashingCount; }

    void setDimensions(int w, int h);
    void setSupportsSwipe(bool s) { m_supportsSwipe = s; }

private:
    int m_width;
    int m_height;
    bool m_supportsSwipe = false;
    std::vector<uint8_t> m_buffer;
    mutable std::mutex m_mutex;
    int m_refreshCount = 0;
    int m_flashingCount = 0;
};

} // namespace kinnovel::hal
