#include "kinnovel/reader/FontEngine.hpp"
#include "kinnovel/reader/WoffNormalizer.hpp"
#include "kinnovel/core/Charset.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Logger.hpp"

#include <cmath>
#include <cstring>
#include <fstream>
#include <filesystem>

namespace kinnovel::reader {

FontFace::FontFace(FT_Face face, int size, std::string path, std::vector<uint8_t> memoryBuffer)
    : m_face(face), m_size(size), m_path(std::move(path)), m_memoryBuffer(std::move(memoryBuffer)) {
    if (m_face) {
        FT_Set_Pixel_Sizes(m_face, 0, m_size);
        initNotdef();
    }
}

FontFace::~FontFace() {
    if (m_face) {
        FT_Done_Face(m_face);
        m_face = nullptr;
    }
}

void FontFace::initNotdef() {
    if (!m_face) return;
    // Glyph 0 is always .notdef in SFNT / FreeType
    FT_Error err = FT_Load_Glyph(m_face, 0, FT_LOAD_NO_HINTING | FT_LOAD_RENDER);
    if (err == 0 && m_face->glyph && m_face->glyph->bitmap.width > 0 && m_face->glyph->bitmap.rows > 0) {
        m_hasNotdef = true;
        m_notdefWidth = m_face->glyph->bitmap.width;
        m_notdefRows = m_face->glyph->bitmap.rows;
        m_notdefPitch = m_face->glyph->bitmap.pitch;
        size_t bsize = std::abs(m_notdefPitch) * m_notdefRows;
        m_notdefBitmap.resize(bsize);
        std::memcpy(m_notdefBitmap.data(), m_face->glyph->bitmap.buffer, bsize);
    }
}

bool FontFace::isGlyphAvailable(uint32_t cp) {
    if (!m_face) return false;
    if (cp == 0) return true;
    // Spaces and formatting
    if (cp == ' ' || cp == '\t' || cp == '\n' || cp == '\r' || cp == 0x3000 || cp == 0x00A0) {
        return true;
    }

    auto it = m_glyphCache.find(cp);
    if (it != m_glyphCache.end()) {
        return it->second;
    }

    FT_UInt gidx = FT_Get_Char_Index(m_face, cp);
    if (gidx == 0) {
        m_glyphCache[cp] = false;
        return false;
    }

    FT_Error err = FT_Load_Glyph(m_face, gidx, FT_LOAD_NO_HINTING | FT_LOAD_RENDER);
    if (err != 0 || !m_face->glyph) {
        m_glyphCache[cp] = false;
        return false;
    }

    // Non-space character producing no pixels is considered unavailable
    if (m_face->glyph->bitmap.width == 0 && m_face->glyph->bitmap.rows == 0) {
        m_glyphCache[cp] = false;
        return false;
    }

    // Check if identical to .notdef bitmap
    if (m_hasNotdef &&
        static_cast<int>(m_face->glyph->bitmap.width) == m_notdefWidth &&
        static_cast<int>(m_face->glyph->bitmap.rows) == m_notdefRows &&
        m_face->glyph->bitmap.pitch == m_notdefPitch) {
        size_t bsize = std::abs(m_notdefPitch) * m_notdefRows;
        if (bsize > 0 && m_notdefBitmap.size() == bsize &&
            std::memcmp(m_face->glyph->bitmap.buffer, m_notdefBitmap.data(), bsize) == 0) {
            m_glyphCache[cp] = false;
            return false;
        }
    }

    m_glyphCache[cp] = true;
    return true;
}

double FontFace::getCharAdvance(uint32_t cp) {
    if (!m_face) return 0.0;
    auto it = m_advanceCache.find(cp);
    if (it != m_advanceCache.end()) {
        return it->second;
    }

    FT_UInt gidx = FT_Get_Char_Index(m_face, cp);
    FT_Error err = FT_Load_Glyph(m_face, gidx, FT_LOAD_NO_HINTING);
    double adv = 0.0;
    if (err == 0 && m_face->glyph) {
        adv = static_cast<double>(m_face->glyph->advance.x) / 64.0;
    }
    m_advanceCache[cp] = adv;
    return adv;
}

FontEngine& FontEngine::instance() {
    static FontEngine s_instance;
    return s_instance;
}

FontEngine::FontEngine() {
    FT_Error err = FT_Init_FreeType(&m_library);
    if (err != 0) {
        Logger::error("FontEngine", "Failed to initialize FreeType library: " + std::to_string(err));
    }
}

FontEngine::~FontEngine() {
    m_cache.clear();
    if (m_library) {
        FT_Done_FreeType(m_library);
        m_library = nullptr;
    }
}

std::shared_ptr<FontFace> FontEngine::loadFont(const std::string& path, int pixelSize) {
    if (!m_library) return nullptr;
    std::string key = path + "@" + std::to_string(pixelSize);
    auto it = m_cache.find(key);
    if (it != m_cache.end()) {
        return it->second;
    }

    // If file is WOFF1, normalize it if necessary
    std::string actualPath = path;
    if (std::filesystem::exists(path) && WoffNormalizer::normalizeFile(path) != path) {
        actualPath = WoffNormalizer::normalizeFile(path);
    }

    FT_Face face = nullptr;
    FT_Error err = FT_New_Face(m_library, actualPath.c_str(), 0, &face);
    if (err != 0 || !face) {
        Logger::warn("FontEngine", "FreeType failed to open font: " + actualPath + ", error: " + std::to_string(err));
        return nullptr;
    }

    auto fontFace = std::make_shared<FontFace>(face, pixelSize, actualPath);
    m_cache[key] = fontFace;
    return fontFace;
}

std::shared_ptr<FontFace> FontEngine::loadFontFromMemory(std::vector<uint8_t> data, int pixelSize, const std::string& key) {
    if (!m_library || data.empty()) return nullptr;
    std::string cacheKey = "mem:" + key + "@" + std::to_string(pixelSize);
    auto it = m_cache.find(cacheKey);
    if (it != m_cache.end()) {
        return it->second;
    }

    std::vector<uint8_t> actualData;
    if (WoffNormalizer::isWoff(data)) {
        if (!WoffNormalizer::normalize(data, actualData)) {
            actualData = std::move(data);
        }
    } else {
        actualData = std::move(data);
    }

    FT_Face face = nullptr;
    FT_Error err = FT_New_Memory_Face(m_library, actualData.data(), static_cast<FT_Long>(actualData.size()), 0, &face);
    if (err != 0 || !face) {
        Logger::warn("FontEngine", "FreeType failed to load font from memory key: " + key + ", error: " + std::to_string(err));
        return nullptr;
    }

    auto fontFace = std::make_shared<FontFace>(face, pixelSize, key, std::move(actualData));
    m_cache[cacheKey] = fontFace;
    return fontFace;
}

std::shared_ptr<FontFace> FontEngine::getCharFont(const std::shared_ptr<FontFace>& primary,
                                                  const std::shared_ptr<FontFace>& fallback,
                                                  uint32_t cp) {
    if (!fallback || (primary && primary->isGlyphAvailable(cp))) {
        return primary;
    }
    if (fallback->isGlyphAvailable(cp)) {
        return fallback;
    }
    return primary;
}

std::vector<FontRun> FontEngine::splitFontRuns(const std::string& utf8Text,
                                               const std::shared_ptr<FontFace>& primary,
                                               const std::shared_ptr<FontFace>& fallback) {
    std::vector<FontRun> runs;
    auto cps = core::Charset::utf8ToCodepoints(utf8Text);
    for (uint32_t cp : cps) {
        auto selected = getCharFont(primary, fallback, cp);
        std::string chUtf8 = core::Charset::codepointToUtf8(cp);
        if (!runs.empty() && runs.back().font == selected) {
            runs.back().text += chUtf8;
        } else {
            runs.push_back({chUtf8, selected});
        }
    }
    return runs;
}

double FontEngine::measureText(const std::string& utf8Text,
                               const std::shared_ptr<FontFace>& primary,
                               const std::shared_ptr<FontFace>& fallback) {
    double totalWidth = 0.0;
    auto cps = core::Charset::utf8ToCodepoints(utf8Text);
    for (uint32_t cp : cps) {
        auto selected = getCharFont(primary, fallback, cp);
        if (selected) {
            totalWidth += selected->getCharAdvance(cp);
        }
    }
    return totalWidth;
}

FontResolver::FontResolver(std::string systemFontPath)
    : m_systemFontPath(std::move(systemFontPath)) {
}

std::shared_ptr<FontFace> FontResolver::getSystemFont(int size) {
    int key = size;
    auto it = m_systemCache.find(key);
    if (it != m_systemCache.end()) {
        return it->second;
    }

    auto face = FontEngine::instance().loadFont(m_systemFontPath, size);
    if (!face) {
        // Fallback paths if system font path does not exist
        const std::vector<std::string> fallbacks = {
            "/system/fonts/NotoSansCJK-Regular.ttc",
            "/system/fonts/NotoSerifCJK-Regular.ttc",
            "/system/fonts/DroidSansFallback.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "C:/Windows/Fonts/simhei.ttf",
            "C:/Windows/Fonts/msyh.ttc"
        };
        for (const auto& fb : fallbacks) {
            if (std::filesystem::exists(fb)) {
                face = FontEngine::instance().loadFont(fb, size);
                if (face) break;
            }
        }
    }

    m_systemCache[key] = face;
    return face;
}

std::shared_ptr<FontFace> FontResolver::resolve(const std::string& chapterFontUrl,
                                                const std::string& baseUrl,
                                                int size,
                                                bool strictTls) {
    (void)strictTls;
    if (!chapterFontUrl.empty()) {
        std::string fullUrl = core::Utils::absoluteUrl(baseUrl, chapterFontUrl);
        std::string key = fullUrl + "@" + std::to_string(size);
        auto it = m_chapterCache.find(key);
        if (it != m_chapterCache.end()) {
            return it->second;
        }

        // Determine cache path
        std::string suffix = ".font";
        size_t queryPos = fullUrl.find('?');
        std::string cleanUrl = (queryPos != std::string::npos) ? fullUrl.substr(0, queryPos) : fullUrl;
        size_t slashPos = cleanUrl.rfind('/');
        std::string filename = (slashPos != std::string::npos) ? cleanUrl.substr(slashPos + 1) : cleanUrl;
        size_t dotPos = filename.rfind('.');
        if (dotPos != std::string::npos) {
            std::string ext = filename.substr(dotPos);
            for (char& c : ext) c = std::tolower(static_cast<unsigned char>(c));
            if (ext == ".ttf" || ext == ".otf" || ext == ".woff" || ext == ".woff2") {
                suffix = ext;
            }
        }

        std::string fontPath = core::Config::getCacheDir() + "/fonts/" + core::Utils::stableCacheName(fullUrl) + suffix;
        if (std::filesystem::exists(fontPath)) {
            auto loaded = FontEngine::instance().loadFont(fontPath, size);
            if (loaded) {
                m_customFontLoaded = true;
                m_chapterCache[key] = loaded;
                return loaded;
            }
        }
    }
    return getSystemFont(size);
}

} // namespace kinnovel::reader
