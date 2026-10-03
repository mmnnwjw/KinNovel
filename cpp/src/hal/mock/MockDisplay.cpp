#include "MockDisplay.hpp"
#include <algorithm>
#include <cstring>
#include <filesystem>

#define STB_IMAGE_WRITE_IMPLEMENTATION
#include "third_party/stb_image_write.h"

namespace kinnovel::hal {

MockDisplay::MockDisplay(int width, int height)
    : m_width(width), m_height(height), m_buffer(width * height, 255) {
}

bool MockDisplay::initialize(const std::string& /*fbPath*/, const std::string& /*protocol*/) {
    std::lock_guard<std::mutex> lock(m_mutex);
    std::fill(m_buffer.begin(), m_buffer.end(), 255);
    m_refreshCount = 0;
    m_flashingCount = 0;
    return true;
}

bool MockDisplay::probe() {
    return true;
}

void MockDisplay::close() {
}

void MockDisplay::setDimensions(int w, int h) {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_width = w;
    m_height = h;
    m_buffer.assign(w * h, 255);
}

void MockDisplay::writeImage(const uint8_t* grayBuffer, int x, int y, int w, int h, int pitch) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!grayBuffer || w <= 0 || h <= 0) return;

    int dstX = std::max(0, x);
    int dstY = std::max(0, y);
    int maxW = std::min(w, m_width - dstX);
    int maxH = std::min(h, m_height - dstY);

    if (maxW <= 0 || maxH <= 0) return;

    for (int row = 0; row < maxH; ++row) {
        const uint8_t* srcRow = grayBuffer + row * pitch;
        uint8_t* dstRow = m_buffer.data() + (dstY + row) * m_width + dstX;
        std::memcpy(dstRow, srcRow, maxW);
    }
}

bool MockDisplay::refresh(const Rect& /*region*/, bool isFlashing, Waveform /*waveform*/, bool /*dither*/) {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_refreshCount++;
    if (isFlashing) {
        m_flashingCount++;
    }
    return true;
}

void MockDisplay::setSwipeAnimation(bool enabled, SwipeDirection /*direction*/, int /*steps*/) {
    m_supportsSwipe = enabled;
}

bool MockDisplay::saveToPng(const std::string& filePath) const {
    std::lock_guard<std::mutex> lock(m_mutex);
    std::filesystem::path p(filePath);
    if (p.has_parent_path()) {
        std::filesystem::create_directories(p.parent_path());
    }
    int res = stbi_write_png(filePath.c_str(), m_width, m_height, 1, m_buffer.data(), m_width);
    return res != 0;
}

} // namespace kinnovel::hal
