#pragma once

#include "kinnovel/reader/DomModel.hpp"
#include <string>
#include <vector>
#include <memory>
#include <map>

namespace kinnovel::reader {

struct DomNode {
    std::string tag;
    std::string text;
    std::string tail;
    std::map<std::string, std::string> attributes;
    std::vector<std::shared_ptr<DomNode>> children;
    std::weak_ptr<DomNode> parent;

    std::string getAttr(const std::string& key) const {
        auto it = attributes.find(key);
        return (it != attributes.end()) ? it->second : "";
    }
};

class HtmlParser {
public:
    static std::shared_ptr<DomNode> parse(const std::string& htmlContent);
    static std::string relativeXPath(const std::shared_ptr<DomNode>& node, const std::shared_ptr<DomNode>& root);
    static std::vector<Block> extractBlocks(const std::string& htmlContent, const std::string& baseUrl = "");
};

} // namespace kinnovel::reader
