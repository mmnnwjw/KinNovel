#pragma once

#include <string>
#include <vector>
#include <cstdint>

namespace kinnovel::core {

class Charset {
public:
    // UTF-8 decoding / encoding
    static std::vector<uint32_t> utf8ToCodepoints(const std::string& str);
    static std::string codepointToUtf8(uint32_t cp);
    static std::string codepointsToUtf8(const std::vector<uint32_t>& codepoints);

    // Strips invisible characters: U+200B, U+200C, U+200D, U+FEFF, U+00AD, U+2060
    static std::string cleanText(const std::string& text);

    // Collapses internal whitespace [ \t\r\f\v]+ to single space and trims edges
    static std::string collapseWhitespace(const std::string& text);

    // Check if character is invisible
    static bool isInvisible(uint32_t cp);

    // Chinese conversion: "t2s" (Traditional to Simplified) or "s2t" (Simplified to Traditional)
    static std::string convertChinese(const std::string& text, const std::string& mode);
};

} // namespace kinnovel::core
