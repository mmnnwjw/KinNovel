#include "FbinkDisplay.hpp"
#include "kinnovel/core/Logger.hpp"

#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <linux/fb.h>
#include <cstring>
#include <algorithm>

namespace kinnovel::hal {

// IOCTL macro helper
#ifndef _IOW_CUSTOM
#define _IOW_CUSTOM(type, nr, size) ((1 << 30) | ((type) << 8) | (nr) | ((size) << 16))
#define _IOWR_CUSTOM(type, nr, size) ((3 << 30) | ((type) << 8) | (nr) | ((size) << 16))
#endif

// Kindle EPDC driver structures matching kernel headers & Python reference

#pragma pack(push, 4)

struct MxcfbRect {
    uint32_t left;
    uint32_t top;
    uint32_t width;
    uint32_t height;
};

struct MxcfbRectMtk {
    uint32_t top;
    uint32_t left;
    uint32_t width;
    uint32_t height;
};

struct MxcfbAltBufferData {
    uint32_t phys_addr;
    uint32_t width;
    uint32_t height;
    MxcfbRect alt_update_region;
};

struct MxcfbAltBufferDataMtk {
    uint32_t phys_addr;
    uint32_t width;
    uint32_t height;
    MxcfbRectMtk alt_update_region;
};

struct MxcfbSwipeData {
    uint32_t direction;
    uint32_t steps;
};

// 68 bytes
struct MxcfbUpdateDataMxcfb {
    MxcfbRect update_region;
    uint32_t waveform_mode;
    uint32_t update_mode;
    uint32_t update_marker;
    uint32_t hist_bw_waveform_mode;
    uint32_t hist_gray_waveform_mode;
    int32_t temp;
    uint32_t flags;
    MxcfbAltBufferData alt_buffer_data;
};

// 80 bytes
struct MxcfbUpdateDataRex {
    MxcfbRectMtk update_region;
    uint32_t waveform_mode;
    uint32_t update_mode;
    uint32_t update_marker;
    int32_t temp;
    uint32_t flags;
    int32_t dither_mode;
    int32_t quant_bit;
    MxcfbAltBufferDataMtk alt_buffer_data;
    uint32_t hist_bw_waveform_mode;
    uint32_t hist_gray_waveform_mode;
};

// 88 bytes
struct MxcfbUpdateDataZelda {
    MxcfbRectMtk update_region;
    uint32_t waveform_mode;
    uint32_t update_mode;
    uint32_t update_marker;
    int32_t temp;
    uint32_t flags;
    int32_t dither_mode;
    int32_t quant_bit;
    MxcfbAltBufferDataMtk alt_buffer_data;
    uint32_t hist_bw_waveform_mode;
    uint32_t hist_gray_waveform_mode;
    uint32_t ts_pxp;
    uint32_t ts_epdc;
};

// 96 bytes
struct MxcfbUpdateDataMtk {
    MxcfbRectMtk update_region;
    uint32_t waveform_mode;
    uint32_t update_mode;
    uint32_t update_marker;
    int32_t temp;
    uint32_t flags;
    int32_t dither_mode;
    int32_t quant_bit;
    MxcfbAltBufferDataMtk alt_buffer_data;
    MxcfbSwipeData swipe_data;
    uint32_t hist_bw_waveform_mode;
    uint32_t hist_gray_waveform_mode;
    uint32_t ts_pxp;
    uint32_t ts_epdc;
};

struct MxcfbUpdateMarkerData {
    uint32_t update_marker;
    uint32_t collision_test;
};

#pragma pack(pop)

FbinkDisplay::FbinkDisplay() = default;

FbinkDisplay::~FbinkDisplay() {
    close();
}

bool FbinkDisplay::initialize(const std::string& fbPath, const std::string& protocol) {
    std::lock_guard<std::mutex> lock(m_mutex);
    close();

    m_fbPath = fbPath.empty() ? "/dev/fb0" : fbPath;

    std::vector<std::string> protos;
    if (protocol.empty() || protocol == "auto") {
        protos = {"mtk", "rex", "zelda", "mxcfb"};
    } else {
        protos = {protocol};
    }

    if (!openFramebuffer(m_fbPath)) {
        return false;
    }

    for (const auto& p : protos) {
        m_protocol = p;
        m_temp = (p == "rex" || p == "zelda") ? 0x1000 : 25;
        if (initEpdc() && probe()) {
            Logger::info("Display", "Initialized Kindle EPDC protocol: " + m_protocol);
            return true;
        }
    }

    Logger::warn("Display", "Failed to probe Kindle EPDC ioctl. Falling back to mmap-only mode.");
    return true; // Still allow memory drawing even if ioctl is not supported (e.g. standard fb)
}

bool FbinkDisplay::openFramebuffer(const std::string& fbPath) {
    m_fd = ::open(fbPath.c_str(), O_RDWR);
    if (m_fd < 0) {
        Logger::error("Display", "Failed to open framebuffer: " + fbPath);
        return false;
    }

    struct fb_var_screeninfo vinfo{};
    struct fb_fix_screeninfo finfo{};

    if (::ioctl(m_fd, FBIOGET_VSCREENINFO, &vinfo) < 0) {
        Logger::error("Display", "ioctl FBIOGET_VSCREENINFO failed");
        ::close(m_fd);
        m_fd = -1;
        return false;
    }

    if (::ioctl(m_fd, FBIOGET_FSCREENINFO, &finfo) < 0) {
        Logger::error("Display", "ioctl FBIOGET_FSCREENINFO failed");
        ::close(m_fd);
        m_fd = -1;
        return false;
    }

    m_width = vinfo.xres;
    m_height = vinfo.yres;
    m_bpp = vinfo.bits_per_pixel;
    m_smemLen = finfo.smem_len;
    m_lineLength = finfo.line_length ? finfo.line_length : (m_width * m_bpp / 8);

    m_mappedMem = static_cast<uint8_t*>(::mmap(nullptr, m_smemLen, PROT_READ | PROT_WRITE, MAP_SHARED, m_fd, 0));
    if (m_mappedMem == MAP_FAILED) {
        Logger::error("Display", "mmap framebuffer memory failed");
        m_mappedMem = nullptr;
        ::close(m_fd);
        m_fd = -1;
        return false;
    }

    Logger::info("Display", "Opened " + fbPath + " (" + std::to_string(m_width) + "x" +
                            std::to_string(m_height) + ", bpp=" + std::to_string(m_bpp) +
                            ", line_len=" + std::to_string(m_lineLength) + ")");
    return true;
}

bool FbinkDisplay::initEpdc() {
    if (m_fd < 0) return false;
    uint32_t delay = 0;
    uint32_t cmd = _IOW_CUSTOM('F', 0x30, 4);
    ::ioctl(m_fd, cmd, &delay);
    return true;
}

bool FbinkDisplay::probe() {
    Rect r{0, 0, 16, 16};
    return refresh(r, false, Waveform::Gc16);
}

void FbinkDisplay::close() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_mappedMem && m_smemLen > 0) {
        ::munmap(m_mappedMem, m_smemLen);
        m_mappedMem = nullptr;
    }
    if (m_fd >= 0) {
        ::close(m_fd);
        m_fd = -1;
    }
}

uint32_t FbinkDisplay::getNextMarker() {
    m_marker = (m_marker + 1) & 0xFFFFFFFF;
    if (m_marker == 0) m_marker = 1;
    return m_marker;
}

void FbinkDisplay::writeImage(const uint8_t* grayBuffer, int x, int y, int w, int h, int pitch) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (!m_mappedMem || !grayBuffer || w <= 0 || h <= 0) return;

    int dstX = std::max(0, x);
    int dstY = std::max(0, y);
    int maxW = std::min(w, m_width - dstX);
    int maxH = std::min(h, m_height - dstY);

    if (maxW <= 0 || maxH <= 0) return;

    if (m_bpp == 8) {
        for (int row = 0; row < maxH; ++row) {
            const uint8_t* srcRow = grayBuffer + row * pitch;
            uint8_t* dstRow = m_mappedMem + (dstY + row) * m_lineLength + dstX;
            std::memcpy(dstRow, srcRow, maxW);
        }
    } else if (m_bpp == 16) {
        // RGB565 conversion if screen is in 16bpp mode
        for (int row = 0; row < maxH; ++row) {
            const uint8_t* srcRow = grayBuffer + row * pitch;
            uint16_t* dstRow = reinterpret_cast<uint16_t*>(m_mappedMem + (dstY + row) * m_lineLength) + dstX;
            for (int col = 0; col < maxW; ++col) {
                uint8_t g = srcRow[col];
                uint16_t rgb565 = ((g >> 3) << 11) | ((g >> 2) << 5) | (g >> 3);
                dstRow[col] = rgb565;
            }
        }
    }
}

bool FbinkDisplay::refresh(const Rect& region, bool isFlashing, Waveform waveform, bool dither) {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_fd < 0) return false;

    // 8-pixel alignment
    int alignment = 8;
    int rx = (region.x / alignment) * alignment;
    int ry = (region.y / alignment) * alignment;
    int right = std::min(m_width, region.x + region.width);
    int bottom = std::min(m_height, region.y + region.height);
    int rw = std::min(m_width - rx, ((right - rx + alignment - 1) / alignment) * alignment);
    int rh = std::min(m_height - ry, ((bottom - ry + alignment - 1) / alignment) * alignment);

    if (rw <= 0 || rh <= 0) return false;

    uint32_t updateMode = isFlashing ? 1 : 0; // FULL = 1, PARTIAL = 0
    uint32_t flags = 0;
    uint32_t marker = getNextMarker();

    uint32_t waveVal = 2; // Default GC16
    if (m_protocol == "mtk" || m_protocol == "rex" || m_protocol == "zelda") {
        switch (waveform) {
            case Waveform::Du:    waveVal = 1; break;
            case Waveform::Gc16:  waveVal = 2; break;
            case Waveform::Gl16:  waveVal = 3; break;
            case Waveform::Reagl: waveVal = 4; break;
            case Waveform::A2:    waveVal = 6; break;
            case Waveform::Du4:   waveVal = 7; break;
            case Waveform::Auto:  waveVal = 257; break;
        }
        if (isFlashing && (waveVal == 257 || waveVal == 1 || waveVal == 6 || waveVal == 7)) {
            waveVal = 2; // Force GC16 on flash
        }
    } else { // mxcfb
        switch (waveform) {
            case Waveform::Du:    waveVal = 1; break;
            case Waveform::Gc16:  waveVal = 2; break;
            case Waveform::A2:    waveVal = 4; break;
            case Waveform::Gl16:  waveVal = 5; break;
            case Waveform::Du4:   waveVal = 7; break;
            case Waveform::Reagl: waveVal = 8; break;
            case Waveform::Auto:  waveVal = 0x101; break;
        }
        if (isFlashing && (waveVal == 0x101 || waveVal == 1 || waveVal == 4 || waveVal == 7)) {
            waveVal = 2;
        }
    }

    if (dither) {
        flags |= (m_protocol == "mtk" ? 0x6000 : 0xA000);
        updateMode = 1; // Full update for dithering
    }

    if (m_protocol == "mtk" && m_swipeEnabled) {
        flags |= 0x10000; // ENABLE_SWIPE
        m_swipeEnabled = false;
    }

    int ret = -1;
    if (m_protocol == "mtk") {
        MxcfbUpdateDataMtk data{};
        data.update_region = {static_cast<uint32_t>(ry), static_cast<uint32_t>(rx),
                              static_cast<uint32_t>(rw), static_cast<uint32_t>(rh)};
        data.waveform_mode = waveVal;
        data.update_mode = updateMode;
        data.update_marker = marker;
        data.temp = m_temp;
        data.flags = flags;
        data.dither_mode = 1;
        data.swipe_data.direction = static_cast<uint32_t>(m_swipeDirection);
        data.swipe_data.steps = m_swipeSteps;
        data.hist_bw_waveform_mode = (waveVal == 4) ? 4 : 1;
        data.hist_gray_waveform_mode = (waveVal == 4) ? 4 : 2;

        uint32_t cmd = _IOW_CUSTOM('F', 0x2E, sizeof(data));
        ret = ::ioctl(m_fd, cmd, &data);
    } else if (m_protocol == "rex") {
        MxcfbUpdateDataRex data{};
        data.update_region = {static_cast<uint32_t>(ry), static_cast<uint32_t>(rx),
                              static_cast<uint32_t>(rw), static_cast<uint32_t>(rh)};
        data.waveform_mode = waveVal;
        data.update_mode = updateMode;
        data.update_marker = marker;
        data.temp = m_temp;
        data.flags = flags;
        data.hist_bw_waveform_mode = 1;
        data.hist_gray_waveform_mode = 2;

        uint32_t cmd = _IOW_CUSTOM('F', 0x2E, sizeof(data));
        ret = ::ioctl(m_fd, cmd, &data);
    } else if (m_protocol == "zelda") {
        MxcfbUpdateDataZelda data{};
        data.update_region = {static_cast<uint32_t>(ry), static_cast<uint32_t>(rx),
                              static_cast<uint32_t>(rw), static_cast<uint32_t>(rh)};
        data.waveform_mode = waveVal;
        data.update_mode = updateMode;
        data.update_marker = marker;
        data.temp = m_temp;
        data.flags = flags;
        data.hist_bw_waveform_mode = 1;
        data.hist_gray_waveform_mode = 2;

        uint32_t cmd = _IOW_CUSTOM('F', 0x2E, sizeof(data));
        ret = ::ioctl(m_fd, cmd, &data);
    } else { // mxcfb
        MxcfbUpdateDataMxcfb data{};
        data.update_region = {static_cast<uint32_t>(rx), static_cast<uint32_t>(ry),
                              static_cast<uint32_t>(rw), static_cast<uint32_t>(rh)};
        data.waveform_mode = waveVal;
        data.update_mode = updateMode;
        data.update_marker = marker;
        data.temp = m_temp;
        data.flags = flags;
        data.hist_bw_waveform_mode = (waveVal == 8) ? 8 : 1;
        data.hist_gray_waveform_mode = (waveVal == 8) ? 8 : 2;

        uint32_t cmd = _IOW_CUSTOM('F', 0x2E, sizeof(data));
        ret = ::ioctl(m_fd, cmd, &data);
    }

    return ret >= 0;
}

void FbinkDisplay::setSwipeAnimation(bool enabled, SwipeDirection direction, int steps) {
    if (m_protocol == "mtk") {
        m_swipeEnabled = enabled;
        m_swipeDirection = direction;
        m_swipeSteps = steps;
    }
}

void FbinkDisplay::powerOn() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_fd >= 0) {
        uint32_t delay = 0;
        uint32_t cmd = _IOW_CUSTOM('F', 0x30, 4);
        ::ioctl(m_fd, cmd, &delay);
    }
}

} // namespace kinnovel::hal
