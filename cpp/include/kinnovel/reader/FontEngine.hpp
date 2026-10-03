#pragma once

#include <ft2build.h>
#include FT_FREETYPE_H

#include <string>
#include <vector>
#include <memory>
#include <unordered_map>
#include <cstdint>

namespace kinnovel::reader {

class FontFace {
public:
    FontFace(FT_Face face, int size, std::string path, std::vector<uint8_t> memoryBuffer = {});
    ~FontFace();

    FT_Face getFtFace() const { return m_face; }
    int getSize() const { return m_size; }
    const std::string& getPath() const { return m_path; }

    bool isGlyphAvailable(uint32_t cp);
    double getCharAdvance(uint32_t cp);

private:
    FT_Face m_face = nullptr;
    int m_size = 0;
    std::string m_path;
    std::vector<uint8_t> m_memoryBuffer;

    std::unordered_map<uint32_t, bool> m_glyphCache;
    std::unordered_map<uint32_t, double> m_advanceCache;

    bool m_hasNotdef = false;
    int m_notdefWidth = 0;
    int m_notdefRows = 0;
    int m_notdefPitch = 0;
    std::vector<uint8_t> m_notdefBitmap;

    void initNotdef();
};

struct FontRun {
    std::string text;
    std::shared_ptr<FontFace> font;
};

class FontEngine {
public:
    static FontEngine& instance();

    FontEngine();
    ~FontEngine();

    FontEngine(const FontEngine&) = delete;
    FontEngine& operator=(const FontEngine&) = delete;

    std::shared_ptr<FontFace> loadFont(const std::string& path, int pixelSize);
    std::shared_ptr<FontFace> loadFontFromMemory(std::vector<uint8_t> data, int pixelSize, const std::string& key);

    std::shared_ptr<FontFace> getCharFont(const std::shared_ptr<FontFace>& primary,
                                          const std::shared_ptr<FontFace>& fallback,
                                          uint32_t cp);

    std::vector<FontRun> splitFontRuns(const std::string& utf8Text,
                                       const std::shared_ptr<FontFace>& primary,
                                       const std::shared_ptr<FontFace>& fallback);

    double measureText(const std::string& utf8Text,
                       const std::shared_ptr<FontFace>& primary,
                       const std::shared_ptr<FontFace>& fallback);

private:
    FT_Library m_library = nullptr;
    std::unordered_map<std::string, std::shared_ptr<FontFace>> m_cache;
};

class FontResolver {
public:
    explicit FontResolver(std::string systemFontPath);

    std::shared_ptr<FontFace> getSystemFont(int size);
    std::shared_ptr<FontFace> resolve(const std::string& chapterFontUrl,
                                      const std::string& baseUrl,
                                      int size,
                                      bool strictTls = false);

    bool isCustomFontLoaded() const { return m_customFontLoaded; }
    const std::string& getLastError() const { return m_lastError; }
    void setSystemFontPath(const std::string& path) { m_systemFontPath = path; }

private:
    std::string m_systemFontPath;
    bool m_customFontLoaded = false;
    std::string m_lastError;
    std::unordered_map<int, std::shared_ptr<FontFace>> m_systemCache;
    std::unordered_map<std::string, std::shared_ptr<FontFace>> m_chapterCache;
};

} // namespace kinnovel::reader
