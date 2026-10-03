#pragma once

#include "kinnovel/reader/DomModel.hpp"
#include "kinnovel/reader/FontEngine.hpp"
#include "kinnovel/core/Config.hpp"
#include "yyjson.h"

#include <string>
#include <vector>
#include <memory>

namespace kinnovel::reader {

struct WrappedLine {
    std::string text;
    int lineStartOffset = 0; // Unicode character offset in input string
};

struct LayoutItem {
    std::string type; // "text" or "image"
    std::string text;
    int x = 0;
    int y = 0;
    std::shared_ptr<FontFace> font;
    std::shared_ptr<FontFace> fallbackFont;
    int size = 0;
    std::string path;
    int offset = 0;

    // For image
    std::string url;
    int width = 0;
    int height = 0;
};

using Page = std::vector<LayoutItem>;

class LayoutEngine {
public:
    static std::vector<WrappedLine> wrapLineParts(
        const std::string& text,
        const std::shared_ptr<FontFace>& font,
        double maxWidth,
        const std::shared_ptr<FontFace>& fallbackFont = nullptr,
        bool removeTrailingSpaces = true);

    static std::vector<std::string> wrapLine(
        const std::string& text,
        const std::shared_ptr<FontFace>& font,
        double maxWidth,
        const std::shared_ptr<FontFace>& fallbackFont = nullptr,
        bool removeTrailingSpaces = true);
};

class ReaderDocument {
public:
    ReaderDocument(const yyjson_val* chapterVal,
                   std::string baseUrl,
                   std::string systemFont,
                   std::shared_ptr<core::Config> config);

    ReaderDocument(std::string rawHtml,
                   std::string fontUrl,
                   std::string baseUrl,
                   std::string systemFont,
                   std::shared_ptr<core::Config> config,
                   std::string title = "",
                   std::vector<std::string> chapters = {});

    ~ReaderDocument();

    const std::vector<Page>& prepare(int width, int height);

    int pageForPath(const std::string& xpath, int targetOffset = -1) const;
    std::pair<std::string, int> firstAnchorOnPage(int pageIndex) const;
    std::string firstPathOnPage(int pageIndex) const;

    size_t getPageCount() const { return m_pages.empty() ? 1 : m_pages.size(); }
    const Page& getPage(size_t index) const;

    const std::string& getTitle() const { return m_title; }
    const std::vector<std::string>& getChapters() const { return m_chapters; }
    const std::vector<Block>& getBlocks() const { return m_blocks; }

private:
    std::string m_title;
    std::string m_fontUrl;
    std::vector<std::string> m_chapters;
    std::string m_rawHtml;

    std::string m_baseUrl;
    std::string m_systemFont;
    std::shared_ptr<core::Config> m_config;

    std::vector<Block> m_blocks;
    FontResolver m_fontResolver;

    std::shared_ptr<FontFace> m_bodyFont;
    std::shared_ptr<FontFace> m_bodyFallback;
    std::shared_ptr<FontFace> m_smallFont;
    std::shared_ptr<FontFace> m_smallFallback;

    std::vector<Page> m_pages;

    void parseChapterJson(const yyjson_val* chapterVal);
};

} // namespace kinnovel::reader
