#pragma once

#include <vector>
#include <string>
#include <memory>
#include <mutex>
#include <list>
#include <unordered_map>
#include <cstdint>

namespace kinnovel::ui {

struct ImageBuffer {
    int width = 0;
    int height = 0;
    std::vector<uint8_t> data; // 8-bit grayscale
};

class ImageCache {
public:
    explicit ImageCache(size_t maxMemoryItems = 24);
    ~ImageCache() = default;

    std::shared_ptr<ImageBuffer> get(const std::string& url);
    void put(const std::string& url, std::shared_ptr<ImageBuffer> buffer);

    void prefetch(const std::string& url, bool strictTls = false);
    std::shared_ptr<ImageBuffer> cover(const std::string& url, int width, int height,
                                       bool strictTls = false, bool fetch = false);

    void clearMemory();

    static std::shared_ptr<ImageBuffer> fit(const ImageBuffer& src, int targetW, int targetH);
    static std::shared_ptr<ImageBuffer> contain(const ImageBuffer& src, int targetW, int targetH);

private:
    size_t m_maxMemoryItems;
    std::mutex m_mutex;
    std::list<std::pair<std::string, std::shared_ptr<ImageBuffer>>> m_lruList;
    std::unordered_map<std::string, std::list<std::pair<std::string, std::shared_ptr<ImageBuffer>>>::iterator> m_lruMap;

    std::string getCachePath(const std::string& url);
    static std::string scaleUrl(const std::string& url);
};

} // namespace kinnovel::ui
