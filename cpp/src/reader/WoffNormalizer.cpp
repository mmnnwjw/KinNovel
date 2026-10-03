#include "kinnovel/reader/WoffNormalizer.hpp"
#include "kinnovel/core/Utils.hpp"

#include <zlib.h>
#include <cmath>
#include <cstring>
#include <fstream>
#include <algorithm>

namespace kinnovel::reader {

namespace {

inline uint16_t readU16BE(const uint8_t* p) {
    return (static_cast<uint16_t>(p[0]) << 8) | static_cast<uint16_t>(p[1]);
}

inline uint32_t readU32BE(const uint8_t* p) {
    return (static_cast<uint32_t>(p[0]) << 24) |
           (static_cast<uint32_t>(p[1]) << 16) |
           (static_cast<uint32_t>(p[2]) << 8)  |
           static_cast<uint32_t>(p[3]);
}

inline void writeU16BE(uint8_t* p, uint16_t val) {
    p[0] = static_cast<uint8_t>((val >> 8) & 0xFF);
    p[1] = static_cast<uint8_t>(val & 0xFF);
}

inline void writeU32BE(uint8_t* p, uint32_t val) {
    p[0] = static_cast<uint8_t>((val >> 24) & 0xFF);
    p[1] = static_cast<uint8_t>((val >> 16) & 0xFF);
    p[2] = static_cast<uint8_t>((val >> 8) & 0xFF);
    p[3] = static_cast<uint8_t>(val & 0xFF);
}

struct TableEntry {
    uint32_t tag = 0;
    uint32_t checksum = 0;
    std::vector<uint8_t> data;
};

} // namespace

bool WoffNormalizer::isWoff(const uint8_t* data, size_t size) {
    return (data != nullptr && size >= 44 &&
            data[0] == 'w' && data[1] == 'O' && data[2] == 'F' && data[3] == 'F');
}

bool WoffNormalizer::isWoff(const std::vector<uint8_t>& data) {
    return isWoff(data.data(), data.size());
}

bool WoffNormalizer::normalize(const uint8_t* data, size_t size, std::vector<uint8_t>& outSfnt) {
    outSfnt.clear();
    if (!isWoff(data, size)) {
        return false;
    }

    uint32_t flavor = readU32BE(data + 4);
    uint16_t numTables = readU16BE(data + 12);
    if (numTables == 0 || numTables > 128) {
        return false;
    }
    if (size < 44 + static_cast<size_t>(numTables) * 20) {
        return false;
    }

    std::vector<TableEntry> tables;
    tables.reserve(numTables);

    for (uint16_t i = 0; i < numTables; ++i) {
        size_t entryOffset = 44 + static_cast<size_t>(i) * 20;
        uint32_t tag = readU32BE(data + entryOffset);
        uint32_t tableOffset = readU32BE(data + entryOffset + 4);
        uint32_t compLength = readU32BE(data + entryOffset + 8);
        uint32_t origLength = readU32BE(data + entryOffset + 12);
        uint32_t origChecksum = readU32BE(data + entryOffset + 16);

        if (static_cast<size_t>(tableOffset) + compLength > size) {
            return false;
        }

        TableEntry entry;
        entry.tag = tag;
        entry.checksum = origChecksum;
        entry.data.resize(origLength);

        if (compLength < origLength) {
            uLongf destLen = origLength;
            int zres = uncompress(entry.data.data(), &destLen, data + tableOffset, compLength);
            if (zres != Z_OK || destLen != origLength) {
                return false;
            }
        } else {
            if (compLength != origLength) {
                return false;
            }
            std::memcpy(entry.data.data(), data + tableOffset, origLength);
        }

        tables.push_back(std::move(entry));
    }

    // Sort tables by tag in ascending order (standard OpenType requirement)
    std::sort(tables.begin(), tables.end(), [](const TableEntry& a, const TableEntry& b) {
        return a.tag < b.tag;
    });

    // Compute binary search helper fields for SFNT header
    uint32_t entrySelector = (numTables > 0) ? static_cast<uint32_t>(std::log2(numTables)) : 0;
    uint32_t searchRange = (1u << entrySelector) * 16;
    uint32_t rangeShift = static_cast<uint32_t>(numTables) * 16 - searchRange;

    // Header size: 12 bytes + numTables * 16
    size_t headerSize = 12 + static_cast<size_t>(numTables) * 16;
    size_t totalBodySize = 0;
    for (const auto& t : tables) {
        totalBodySize += t.data.size();
        totalBodySize += (4 - (t.data.size() % 4)) % 4; // 4-byte padding
    }

    outSfnt.resize(headerSize + totalBodySize, 0);

    // Write SFNT Offset Table
    writeU32BE(outSfnt.data(), flavor);
    writeU16BE(outSfnt.data() + 4, numTables);
    writeU16BE(outSfnt.data() + 6, static_cast<uint16_t>(searchRange));
    writeU16BE(outSfnt.data() + 8, static_cast<uint16_t>(entrySelector));
    writeU16BE(outSfnt.data() + 10, static_cast<uint16_t>(rangeShift));

    // Write Table Directory and Table Payloads
    size_t currentBodyOffset = headerSize;
    for (size_t i = 0; i < tables.size(); ++i) {
        const auto& t = tables[i];
        size_t recOffset = 12 + i * 16;
        writeU32BE(outSfnt.data() + recOffset, t.tag);
        writeU32BE(outSfnt.data() + recOffset + 4, t.checksum);
        writeU32BE(outSfnt.data() + recOffset + 8, static_cast<uint32_t>(currentBodyOffset));
        writeU32BE(outSfnt.data() + recOffset + 12, static_cast<uint32_t>(t.data.size()));

        std::memcpy(outSfnt.data() + currentBodyOffset, t.data.data(), t.data.size());
        currentBodyOffset += t.data.size();
        size_t pad = (4 - (t.data.size() % 4)) % 4;
        currentBodyOffset += pad;
    }

    return true;
}

bool WoffNormalizer::normalize(const std::vector<uint8_t>& data, std::vector<uint8_t>& outSfnt) {
    return normalize(data.data(), data.size(), outSfnt);
}

std::string WoffNormalizer::normalizeFile(const std::string& inputPath) {
    std::ifstream in(inputPath, std::ios::binary);
    if (!in) {
        return inputPath;
    }
    std::vector<uint8_t> buffer((std::istreambuf_iterator<char>(in)),
                                 std::istreambuf_iterator<char>());
    in.close();

    if (!isWoff(buffer)) {
        return inputPath;
    }

    std::vector<uint8_t> sfnt;
    if (!normalize(buffer, sfnt)) {
        return inputPath;
    }

    // Determine extension: if flavor == 'OTTO' (.otf), else (.ttf)
    uint32_t flavor = readU32BE(buffer.data() + 4);
    std::string ext = (flavor == 0x4F54544F) ? ".otf" : ".ttf";
    std::string targetPath = inputPath + ext;

    if (!core::Utils::atomicWrite(targetPath, sfnt.data(), sfnt.size())) {
        return inputPath;
    }
    return targetPath;
}

} // namespace kinnovel::reader
