#include "kinnovel/reader/ReaderDocument.hpp"
#include "kinnovel/reader/HtmlParser.hpp"
#include "kinnovel/core/Charset.hpp"
#include "kinnovel/core/Logger.hpp"

#include <unordered_set>
#include <algorithm>
#include <cmath>

namespace kinnovel::reader {

namespace {

static const std::unordered_set<uint32_t> LINE_START_FORBIDDEN = {
    0xFF0C, // ，
    0x3002, // 。
    0x3001, // 、
    0xFF1B, // ；
    0xFF1A, // ：
    0xFF1F, // ？
    0xFF01, // ！
    ',', '.', '!', '?', ';', ':', '\'', '"', ')', ']',
    0x3011, // 】
    0x300B, // 》
    0x201D, // ”
    0x2019, // ’
    '%',
    0x2026, // …
    0x2014, // —
    0x00B7  // ·
};

static const std::unordered_set<uint32_t> LINE_END_FORBIDDEN = {
    0xFF08, // （
    0x300A, // 《
    0x3010, // 【
    0x300C, // 「
    0x300E, // 『
    0x201C, // “
    0x2018, // ‘
    '(', '[', '{'
};

bool isWhitespace(uint32_t cp) {
    return cp == ' ' || cp == '\t' || cp == '\r' || cp == '\n' || cp == 0x3000;
}

std::string renderCodepoints(const std::vector<uint32_t>& cps, bool removeTrailingSpaces) {
    if (cps.empty()) return "";
    size_t end = cps.size();
    if (removeTrailingSpaces) {
        while (end > 0 && isWhitespace(cps[end - 1])) {
            end--;
        }
    }
    if (end == 0) return "";
    std::vector<uint32_t> sub(cps.begin(), cps.begin() + end);
    return core::Charset::codepointsToUtf8(sub);
}

} // namespace

std::vector<WrappedLine> LayoutEngine::wrapLineParts(
    const std::string& text,
    const std::shared_ptr<FontFace>& font,
    double maxWidth,
    const std::shared_ptr<FontFace>& fallbackFont,
    bool removeTrailingSpaces) {

    auto cps = core::Charset::utf8ToCodepoints(text);
    for (uint32_t& cp : cps) {
        if (cp == 0x00A0) cp = ' ';
    }

    if (cps.empty()) {
        return { { "", 0 } };
    }

    auto measureSingle = [&](uint32_t c) -> double {
        auto f = FontEngine::instance().getCharFont(font, fallbackFont, c);
        return f ? f->getCharAdvance(c) : 0.0;
    };

    auto measure = [&](size_t start, size_t count) -> double {
        double w = 0.0;
        for (size_t k = 0; k < count; ++k) {
            w += measureSingle(cps[start + k]);
        }
        return w;
    };

    std::vector<WrappedLine> lines;
    std::vector<uint32_t> current;
    size_t current_start = 0;
    double current_width = 0.0;
    size_t index = 0;

    while (index < cps.size()) {
        uint32_t character = cps[index];
        if (character == '\n') {
            lines.push_back({ renderCodepoints(current, removeTrailingSpaces), static_cast<int>(current_start) });
            current.clear();
            current_width = 0.0;
            index++;
            current_start = index;
            continue;
        }

        if (LINE_START_FORBIDDEN.count(character)) {
            size_t run_end = index + 1;
            while (run_end < cps.size() && LINE_START_FORBIDDEN.count(cps[run_end])) {
                run_end++;
            }
            size_t run_len = run_end - index;
            double run_width = measure(index, run_len);

            if (!current.empty() && current_width + run_width > maxWidth) {
                if (run_len == 1) {
                    current.push_back(character);
                    lines.push_back({ renderCodepoints(current, removeTrailingSpaces), static_cast<int>(current_start) });
                    current.clear();
                    current_width = 0.0;
                    index = run_end;
                    continue;
                }
                auto cut = current;
                size_t carry_start = cut.size();
                while (carry_start > 0 && LINE_END_FORBIDDEN.count(cut[carry_start - 1])) {
                    carry_start--;
                }
                std::vector<uint32_t> carry(cut.begin() + carry_start, cut.end());
                cut.resize(carry_start);
                if (!cut.empty()) {
                    lines.push_back({ renderCodepoints(cut, removeTrailingSpaces), static_cast<int>(current_start) });
                }
                current_start += carry_start;
                current = carry;
                current.insert(current.end(), cps.begin() + index, cps.begin() + run_end);
                current_width = 0.0;
                for (uint32_t c : current) current_width += measureSingle(c);
            } else {
                if (current.empty()) {
                    current_start = index;
                }
                current.insert(current.end(), cps.begin() + index, cps.begin() + run_end);
                current_width += run_width;
            }
            index = run_end;
            continue;
        }

        double character_width = measureSingle(character);
        if (!current.empty() && current_width + character_width > maxWidth) {
            auto cut = current;
            size_t carry_start = cut.size();
            while (carry_start > 0 && LINE_END_FORBIDDEN.count(cut[carry_start - 1])) {
                carry_start--;
            }
            std::vector<uint32_t> carry(cut.begin() + carry_start, cut.end());
            cut.resize(carry_start);
            if (!cut.empty()) {
                lines.push_back({ renderCodepoints(cut, removeTrailingSpaces), static_cast<int>(current_start) });
            }
            current_start += carry_start;
            current = carry;
            current.push_back(character);
            current_width = 0.0;
            for (uint32_t c : current) current_width += measureSingle(c);
        } else {
            if (current.empty()) {
                current_start = index;
            }
            current.push_back(character);
            current_width += character_width;
        }
        index++;
    }

    lines.push_back({ renderCodepoints(current, removeTrailingSpaces), static_cast<int>(current_start) });
    return lines;
}

std::vector<std::string> LayoutEngine::wrapLine(
    const std::string& text,
    const std::shared_ptr<FontFace>& font,
    double maxWidth,
    const std::shared_ptr<FontFace>& fallbackFont,
    bool removeTrailingSpaces) {

    auto parts = wrapLineParts(text, font, maxWidth, fallbackFont, removeTrailingSpaces);
    std::vector<std::string> res;
    res.reserve(parts.size());
    for (const auto& p : parts) {
        res.push_back(p.text);
    }
    return res;
}

ReaderDocument::ReaderDocument(const yyjson_val* chapterVal,
                               std::string baseUrl,
                               std::string systemFont,
                               std::shared_ptr<core::Config> config)
    : m_baseUrl(std::move(baseUrl)),
      m_systemFont(std::move(systemFont)),
      m_config(std::move(config)),
      m_fontResolver(m_systemFont) {
    parseChapterJson(chapterVal);
    m_blocks = HtmlParser::extractBlocks(m_rawHtml, m_baseUrl);
}

ReaderDocument::~ReaderDocument() = default;

void ReaderDocument::parseChapterJson(const yyjson_val* chapterVal) {
    if (!chapterVal || !yyjson_is_obj(chapterVal)) {
        return;
    }

    yyjson_val* titleVal = yyjson_obj_get(chapterVal, "Title");
    if (titleVal && yyjson_is_str(titleVal)) {
        m_title = yyjson_get_str(titleVal);
    }

    yyjson_val* fontVal = yyjson_obj_get(chapterVal, "Font");
    if (fontVal && yyjson_is_str(fontVal)) {
        m_fontUrl = yyjson_get_str(fontVal);
    }

    yyjson_val* contentVal = yyjson_obj_get(chapterVal, "Content");
    if (contentVal && yyjson_is_str(contentVal)) {
        m_rawHtml = yyjson_get_str(contentVal);
    }

    yyjson_val* chaptersVal = yyjson_obj_get(chapterVal, "Chapters");
    if (chaptersVal && yyjson_is_arr(chaptersVal)) {
        size_t idx, max;
        yyjson_val* item;
        yyjson_arr_foreach(chaptersVal, idx, max, item) {
            if (yyjson_is_str(item)) {
                m_chapters.push_back(yyjson_get_str(item));
            }
        }
    }
}

const std::vector<Page>& ReaderDocument::prepare(int width, int height) {
    m_pages.clear();

    int fontSize = m_config ? m_config->getInt("font_size", 48) : 48;
    bool strictTls = m_config ? m_config->getBool("strict_tls", false) : false;

    m_bodyFont = m_fontResolver.resolve(m_fontUrl, m_baseUrl, fontSize, strictTls);
    m_bodyFallback = m_fontResolver.getSystemFont(fontSize);

    int smallSize = std::max(20, static_cast<int>(fontSize * 0.82));
    m_smallFont = m_fontResolver.resolve(m_fontUrl, m_baseUrl, smallSize, strictTls);
    m_smallFallback = m_fontResolver.getSystemFont(smallSize);

    int margin = m_config ? m_config->getInt("reader_margin", 34) : 34;
    int usableWidth = std::max(120, width - 2 * margin);
    int usableHeight = std::max(160, height - 2 * margin);

    double lineSpacing = m_config ? m_config->getDouble("line_spacing", 1.42) : 1.42;
    int bodyFontSize = m_bodyFont ? m_bodyFont->getSize() : fontSize;
    int lineHeight = std::max(1, static_cast<int>(bodyFontSize * lineSpacing));
    int headingGap = static_cast<int>(lineHeight * 0.5);
    bool firstLineIndent = m_config ? m_config->getBool("first_line_indent", true) : true;

    struct FootnoteEntry {
        std::string text;
        std::string path;
        int offset = 0;
    };
    std::vector<FootnoteEntry> footnoteLines;

    Page currentPage;
    int y = 0;

    auto newPage = [&]() {
        if (!currentPage.empty()) {
            m_pages.push_back(currentPage);
        }
        currentPage.clear();
        y = 0;
    };

    auto addLine = [&](const std::string& text,
                       const std::shared_ptr<FontFace>& font,
                       const std::shared_ptr<FontFace>& fallbackFont,
                       bool indent,
                       int gapBefore,
                       int gapAfter,
                       int baseOffset,
                       const std::string& path) {

        auto curFont = font ? font : m_bodyFont;
        auto curFallback = fallbackFont ? fallbackFont : m_bodyFallback;
        int curSize = curFont ? curFont->getSize() : 28;
        int actualHeight = std::max(lineHeight, static_cast<int>(curSize * lineSpacing));

        if (y + gapBefore + actualHeight > usableHeight && !currentPage.empty()) {
            newPage();
        }
        y += gapBefore;

        std::string prefix = (indent && firstLineIndent) ? "\xE3\x80\x80\xE3\x80\x80" : "";
        auto wrapped = LayoutEngine::wrapLineParts(prefix + text, curFont, usableWidth, curFallback);

        for (const auto& line : wrapped) {
            if (y + actualHeight > usableHeight && !currentPage.empty()) {
                newPage();
            }

            LayoutItem item;
            item.type = "text";
            item.text = line.text;
            item.x = margin;
            item.y = margin + y;
            item.font = curFont;
            item.fallbackFont = curFallback;
            item.size = curSize;
            item.path = path;
            // Prefix "　　" is 2 unicode characters
            int prefixLenChars = prefix.empty() ? 0 : 2;
            item.offset = baseOffset + std::max(0, line.lineStartOffset - prefixLenChars);

            currentPage.push_back(std::move(item));
            y += actualHeight;
        }
        y += gapAfter;
    };

    for (const auto& block : m_blocks) {
        if (block.kind == BlockKind::Image) {
            int imageHeight = std::min(static_cast<int>(usableHeight * 0.62),
                                      static_cast<int>(usableWidth * 0.72));
            if (y + imageHeight > usableHeight && !currentPage.empty()) {
                newPage();
            }
            LayoutItem item;
            item.type = "image";
            item.url = block.sourceUrl;
            item.x = margin;
            item.y = margin + y;
            item.width = usableWidth;
            item.height = imageHeight;
            item.path = block.path;
            item.offset = block.offset;

            currentPage.push_back(std::move(item));
            y += imageHeight + static_cast<int>(lineHeight * 0.5);
            continue;
        }

        if (block.kind == BlockKind::Footnote) {
            footnoteLines.push_back({ block.text, block.path, block.offset });
            continue;
        }

        if (block.kind == BlockKind::Heading) {
            double scale = std::max(1.08, 1.30 - block.level * 0.05);
            int headSize = static_cast<int>(bodyFontSize * scale);
            auto hFont = m_fontResolver.resolve(m_fontUrl, m_baseUrl, headSize, strictTls);
            auto hFallback = m_fontResolver.getSystemFont(headSize);

            int gapB = (!currentPage.empty()) ? headingGap : 0;
            int gapA = static_cast<int>(lineHeight * 0.35);

            addLine(block.text, hFont, hFallback, false, gapB, gapA, block.offset, block.path);
        } else {
            int gapA = static_cast<int>(lineHeight * 0.22);
            addLine(block.text, m_bodyFont, m_bodyFallback, true, 0, gapA, block.offset, block.path);
        }
    }

    if (!footnoteLines.empty()) {
        addLine("注释", m_smallFont, m_smallFallback, false, headingGap, 0, 0, ".");
        for (const auto& fn : footnoteLines) {
            int gapA = static_cast<int>(lineHeight * 0.15);
            addLine(fn.text, m_smallFont, m_smallFallback, false, 0, gapA, fn.offset, fn.path);
        }
    }

    if (!currentPage.empty() || m_pages.empty()) {
        m_pages.push_back(std::move(currentPage));
    }

    return m_pages;
}

int ReaderDocument::pageForPath(const std::string& xpath, int targetOffset) const {
    if (xpath.empty()) return 0;

    if (targetOffset >= 0) {
        int firstPage = -1;
        int bestPage = -1;
        int bestOffset = -1;

        for (size_t index = 0; index < m_pages.size(); ++index) {
            for (const auto& item : m_pages[index]) {
                if (item.path != xpath) continue;

                if (firstPage == -1) firstPage = static_cast<int>(index);
                if (item.offset > targetOffset) continue;

                if (bestOffset == -1 || item.offset > bestOffset) {
                    bestPage = static_cast<int>(index);
                    bestOffset = item.offset;
                }
            }
        }
        if (bestPage != -1) return bestPage;
        if (firstPage != -1) return firstPage;
        return 0;
    }

    for (size_t index = 0; index < m_pages.size(); ++index) {
        for (const auto& item : m_pages[index]) {
            if (item.path == xpath) return static_cast<int>(index);
        }
    }
    return 0;
}

std::pair<std::string, int> ReaderDocument::firstAnchorOnPage(int pageIndex) const {
    if (m_pages.empty()) return { ".", 0 };
    int idx = std::max(0, std::min(pageIndex, static_cast<int>(m_pages.size()) - 1));
    for (const auto& item : m_pages[idx]) {
        if (!item.path.empty()) {
            return { item.path, item.offset };
        }
    }
    return { ".", 0 };
}

std::string ReaderDocument::firstPathOnPage(int pageIndex) const {
    if (m_pages.empty()) return ".";
    int idx = std::max(0, std::min(pageIndex, static_cast<int>(m_pages.size()) - 1));
    for (const auto& item : m_pages[idx]) {
        if (!item.path.empty()) {
            return item.path;
        }
    }
    return ".";
}

const Page& ReaderDocument::getPage(size_t index) const {
    static const Page emptyPage;
    if (index < m_pages.size()) {
        return m_pages[index];
    }
    return emptyPage;
}

} // namespace kinnovel::reader
