#pragma once

#include <string>
#include <vector>

namespace kinnovel::reader {

enum class BlockKind {
    Text,
    Heading,
    Image,
    Footnote
};

struct Block {
    BlockKind kind = BlockKind::Text;
    std::string text;
    int level = 0;          // Heading level (1..6)
    std::string path = "."; // Relative XPath (e.g., "./p[1]", "./p[1]/img[1]")
    int offset = 0;         // Text character offset
    std::string sourceUrl;  // For images
};

} // namespace kinnovel::reader
