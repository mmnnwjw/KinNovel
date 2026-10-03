#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace kinnovel::reader {

class WoffNormalizer {
public:
    // Check if the data begins with 'wOFF' magic
    static bool isWoff(const uint8_t* data, size_t size);
    static bool isWoff(const std::vector<uint8_t>& data);

    // Unpack WOFF1 container to TTF/OTF SFNT stream
    // Returns true on success, filling outSfnt
    static bool normalize(const uint8_t* data, size_t size, std::vector<uint8_t>& outSfnt);
    static bool normalize(const std::vector<uint8_t>& data, std::vector<uint8_t>& outSfnt);

    // Normalizes file at inputPath. If it is WOFF1, converts and writes to inputPath + (.ttf/.otf),
    // returning the path to the converted font. If not WOFF1, returns inputPath.
    static std::string normalizeFile(const std::string& inputPath);
};

} // namespace kinnovel::reader
