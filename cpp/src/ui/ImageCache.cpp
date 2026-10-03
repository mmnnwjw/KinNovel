#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"

#include "kinnovel/ui/ImageCache.hpp"
#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/network/HttpTransport.hpp"

#include <filesystem>
#include <cmath>
#include <algorithm>

namespace kinnovel::ui {

ImageCache::ImageCache(size_t maxMemoryItems)
    : m_maxMemoryItems(maxMemoryItems) {
}

std::string ImageCache::scaleUrl(const std::string& url) {
    if (url.find("size=") == std::string::npos || url.find("height=") != std::string::npos) {
        return url;
    }
    std::string sep = (url.find('?') == std::string::npos) ? "?" : "&";
    return url + sep + "height=1024";
}

std::string ImageCache::getCachePath(const std::string& url) {
    std::string cacheDir = core::Config::getCacheDir();
    return cacheDir + "/covers/" + core::Utils::stableCacheName(url) + ".png";
}

void ImageCache::put(const std::string& url, std::shared_ptr<ImageBuffer> buffer) {
    if (url.empty() || !buffer) return;
    std::lock_guard<std::mutex> lock(m_mutex);

    auto it = m_lruMap.find(url);
    if (it != m_lruMap.end()) {
        m_lruList.erase(it->second);
        m_lruMap.erase(it);
    }

    m_lruList.push_front({ url, buffer });
    m_lruMap[url] = m_lruList.begin();

    while (m_lruList.size() > m_maxMemoryItems) {
        auto last = m_lruList.end();
        --last;
        m_lruMap.erase(last->first);
        m_lruList.pop_back();
    }
}

std::shared_ptr<ImageBuffer> ImageCache::get(const std::string& url) {
    if (url.empty()) return nullptr;

    {
        std::lock_guard<std::mutex> lock(m_mutex);
        auto it = m_lruMap.find(url);
        if (it != m_lruMap.end()) {
            m_lruList.splice(m_lruList.begin(), m_lruList, it->second);
            return it->second->second;
        }
    }

    std::string path = getCachePath(url);
    if (!std::filesystem::exists(path)) {
        // Also check images/ directory
        std::string alt = core::Config::getCacheDir() + "/images/" + core::Utils::stableCacheName(url, ".img");
        if (std::filesystem::exists(alt)) {
            path = alt;
        } else {
            return nullptr;
        }
    }

    int w = 0;
    int h = 0;
    int channels = 0;
    uint8_t* raw = stbi_load(path.c_str(), &w, &h, &channels, 1);
    if (!raw || w <= 0 || h <= 0) {
        if (raw) stbi_image_free(raw);
        return nullptr;
    }

    auto img = std::make_shared<ImageBuffer>();
    img->width = w;
    img->height = h;
    img->data.assign(raw, raw + (w * h));
    stbi_image_free(raw);

    core::Utils::touch(path);
    put(url, img);
    return img;
}

void ImageCache::prefetch(const std::string& url, bool strictTls) {
    if (url.empty() || get(url) != nullptr) return;

    std::string path = getCachePath(url);
    if (std::filesystem::exists(path)) {
        std::error_code ec;
        auto sz = std::filesystem::file_size(path, ec);
        if (!ec && sz > 0) return;
    }

    std::string scaled = scaleUrl(url);
    network::HttpTransport::downloadFile(scaled, path, 25, strictTls);
}

std::shared_ptr<ImageBuffer> ImageCache::cover(const std::string& url, int width, int height,
                                               bool strictTls, bool fetch) {
    auto img = get(url);
    if (!img && fetch) {
        prefetch(url, strictTls);
        img = get(url);
    }
    if (!img) return nullptr;

    return fit(*img, width, height);
}

void ImageCache::clearMemory() {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_lruList.clear();
    m_lruMap.clear();
}

std::shared_ptr<ImageBuffer> ImageCache::fit(const ImageBuffer& src, int targetW, int targetH) {
    if (src.width <= 0 || src.height <= 0 || targetW <= 0 || targetH <= 0) {
        return nullptr;
    }

    double targetAspect = static_cast<double>(targetW) / static_cast<double>(targetH);
    double srcAspect = static_cast<double>(src.width) / static_cast<double>(src.height);

    double cropX = 0.0;
    double cropY = 0.0;
    double cropW = src.width;
    double cropH = src.height;

    if (srcAspect > targetAspect) {
        cropW = src.height * targetAspect;
        cropX = (src.width - cropW) / 2.0;
    } else {
        cropH = src.width / targetAspect;
        cropY = (src.height - cropH) / 2.0;
    }

    auto out = std::make_shared<ImageBuffer>();
    out->width = targetW;
    out->height = targetH;
    out->data.resize(static_cast<size_t>(targetW * targetH));

    for (int ty = 0; ty < targetH; ++ty) {
        double sy = cropY + (static_cast<double>(ty) + 0.5) * (cropH / targetH) - 0.5;
        int y0 = std::max(0, std::min(src.height - 1, static_cast<int>(std::floor(sy))));
        int y1 = std::max(0, std::min(src.height - 1, y0 + 1));
        double fy = std::max(0.0, std::min(1.0, sy - y0));

        for (int tx = 0; tx < targetW; ++tx) {
            double sx = cropX + (static_cast<double>(tx) + 0.5) * (cropW / targetW) - 0.5;
            int x0 = std::max(0, std::min(src.width - 1, static_cast<int>(std::floor(sx))));
            int x1 = std::max(0, std::min(src.width - 1, x0 + 1));
            double fx = std::max(0.0, std::min(1.0, sx - x0));

            double p00 = src.data[static_cast<size_t>(y0 * src.width + x0)];
            double p10 = src.data[static_cast<size_t>(y0 * src.width + x1)];
            double p01 = src.data[static_cast<size_t>(y1 * src.width + x0)];
            double p11 = src.data[static_cast<size_t>(y1 * src.width + x1)];

            double top = p00 * (1.0 - fx) + p10 * fx;
            double bottom = p01 * (1.0 - fx) + p11 * fx;
            double val = top * (1.0 - fy) + bottom * fy;

            out->data[static_cast<size_t>(ty * targetW + tx)] =
                static_cast<uint8_t>(std::max(0.0, std::min(255.0, std::round(val))));
        }
    }

    return out;
}

std::shared_ptr<ImageBuffer> ImageCache::contain(const ImageBuffer& src, int targetW, int targetH) {
    if (src.width <= 0 || src.height <= 0 || targetW <= 0 || targetH <= 0) {
        return nullptr;
    }

    double scale = std::min(static_cast<double>(targetW) / src.width,
                            static_cast<double>(targetH) / src.height);
    int outW = std::max(1, static_cast<int>(std::round(src.width * scale)));
    int outH = std::max(1, static_cast<int>(std::round(src.height * scale)));

    auto out = std::make_shared<ImageBuffer>();
    out->width = outW;
    out->height = outH;
    out->data.resize(static_cast<size_t>(outW * outH));

    for (int ty = 0; ty < outH; ++ty) {
        double sy = (static_cast<double>(ty) + 0.5) / scale - 0.5;
        int y0 = std::max(0, std::min(src.height - 1, static_cast<int>(std::floor(sy))));
        int y1 = std::max(0, std::min(src.height - 1, y0 + 1));
        double fy = std::max(0.0, std::min(1.0, sy - y0));

        for (int tx = 0; tx < outW; ++tx) {
            double sx = (static_cast<double>(tx) + 0.5) / scale - 0.5;
            int x0 = std::max(0, std::min(src.width - 1, static_cast<int>(std::floor(sx))));
            int x1 = std::max(0, std::min(src.width - 1, x0 + 1));
            double fx = std::max(0.0, std::min(1.0, sx - x0));

            double p00 = src.data[static_cast<size_t>(y0 * src.width + x0)];
            double p10 = src.data[static_cast<size_t>(y0 * src.width + x1)];
            double p01 = src.data[static_cast<size_t>(y1 * src.width + x0)];
            double p11 = src.data[static_cast<size_t>(y1 * src.width + x1)];

            double top = p00 * (1.0 - fx) + p10 * fx;
            double bottom = p01 * (1.0 - fx) + p11 * fx;
            double val = top * (1.0 - fy) + bottom * fy;

            out->data[static_cast<size_t>(ty * outW + tx)] =
                static_cast<uint8_t>(std::max(0.0, std::min(255.0, std::round(val))));
        }
    }

    return out;
}

} // namespace kinnovel::ui
