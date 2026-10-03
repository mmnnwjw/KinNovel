#include "kinnovel/reader/HtmlParser.hpp"
#include "kinnovel/core/Charset.hpp"
#include "kinnovel/core/Utils.hpp"

#include <set>
#include <algorithm>
#include <cctype>
#include <sstream>

namespace kinnovel::reader {

namespace {

const std::set<std::string> BLOCK_TAGS = {
    "p", "div", "section", "article", "blockquote", "h1", "h2", "h3",
    "h4", "h5", "h6", "li", "dt", "dd", "pre", "figcaption", "aside"
};

const std::set<std::string> HEADING_TAGS = {"h1", "h2", "h3", "h4", "h5", "h6"};
const std::set<std::string> SKIP_TAGS = {"script", "style", "iframe", "object", "embed", "svg", "canvas"};
const std::set<std::string> VOID_TAGS = {"br", "img", "hr", "input", "meta", "link"};

std::string decodeHtmlEntities(const std::string& input) {
    std::string out;
    out.reserve(input.size());

    size_t i = 0;
    while (i < input.size()) {
        if (input[i] == '&') {
            size_t semi = input.find(';', i);
            if (semi != std::string::npos && semi - i <= 10) {
                std::string ent = input.substr(i + 1, semi - i - 1);
                if (ent == "nbsp") {
                    out += "\xC2\xA0"; // U+00A0
                    i = semi + 1;
                    continue;
                } else if (ent == "lt") {
                    out += '<';
                    i = semi + 1;
                    continue;
                } else if (ent == "gt") {
                    out += '>';
                    i = semi + 1;
                    continue;
                } else if (ent == "amp") {
                    out += '&';
                    i = semi + 1;
                    continue;
                } else if (ent == "quot") {
                    out += '"';
                    i = semi + 1;
                    continue;
                } else if (ent == "apos") {
                    out += '\'';
                    i = semi + 1;
                    continue;
                } else if (!ent.empty() && ent[0] == '#') {
                    try {
                        uint32_t cp = 0;
                        if (ent.size() > 1 && (ent[1] == 'x' || ent[1] == 'X')) {
                            cp = std::stoul(ent.substr(2), nullptr, 16);
                        } else {
                            cp = std::stoul(ent.substr(1), nullptr, 10);
                        }
                        out += core::Charset::codepointToUtf8(cp);
                        i = semi + 1;
                        continue;
                    } catch (...) {}
                }
            }
        }
        out.push_back(input[i]);
        i++;
    }
    return out;
}

std::string toLower(std::string s) {
    for (char& c : s) c = std::tolower(static_cast<unsigned char>(c));
    return s;
}

} // namespace

std::shared_ptr<DomNode> HtmlParser::parse(const std::string& htmlContent) {
    auto root = std::make_shared<DomNode>();
    root->tag = "div";
    root->attributes["id"] = "kinnovel-root";

    std::shared_ptr<DomNode> current = root;
    size_t i = 0;
    size_t len = htmlContent.size();

    while (i < len) {
        if (htmlContent[i] == '<') {
            // Check if comment or DOCTYPE
            if (i + 3 < len && htmlContent.substr(i, 4) == "<!--") {
                size_t endComment = htmlContent.find("-->", i + 4);
                if (endComment != std::string::npos) {
                    i = endComment + 3;
                } else {
                    break;
                }
                continue;
            }

            size_t tagEnd = htmlContent.find('>', i);
            if (tagEnd == std::string::npos) break;

            std::string tagStr = htmlContent.substr(i + 1, tagEnd - i - 1);
            i = tagEnd + 1;

            if (tagStr.empty()) continue;

            if (tagStr[0] == '/') {
                // Closing tag
                std::string closingTag = toLower(tagStr.substr(1));
                size_t spacePos = closingTag.find_first_of(" \t\r\n");
                if (spacePos != std::string::npos) {
                    closingTag = closingTag.substr(0, spacePos);
                }

                auto p = current->parent.lock();
                if (p && current->tag == closingTag) {
                    current = p;
                } else {
                    // Try to unwind to matching tag
                    auto walker = current;
                    while (walker && walker != root && walker->tag != closingTag) {
                        walker = walker->parent.lock();
                    }
                    if (walker && walker->parent.lock()) {
                        current = walker->parent.lock();
                    }
                }
            } else {
                // Opening or self-closing tag
                bool selfClosing = false;
                if (!tagStr.empty() && tagStr.back() == '/') {
                    selfClosing = true;
                    tagStr.pop_back();
                }

                std::istringstream iss(tagStr);
                std::string tagName;
                iss >> tagName;
                tagName = toLower(tagName);

                if (SKIP_TAGS.count(tagName)) {
                    // Skip content until closing tag
                    std::string endTag = "</" + tagName + ">";
                    size_t closePos = htmlContent.find(endTag, i);
                    if (closePos != std::string::npos) {
                        i = closePos + endTag.size();
                    }
                    continue;
                }

                auto node = std::make_shared<DomNode>();
                node->tag = tagName;
                node->parent = current;

                // Parse attributes
                std::string attrPart;
                std::getline(iss, attrPart);
                size_t ap = 0;
                while (ap < attrPart.size()) {
                    while (ap < attrPart.size() && std::isspace(static_cast<unsigned char>(attrPart[ap]))) ap++;
                    if (ap >= attrPart.size()) break;

                    size_t nameStart = ap;
                    while (ap < attrPart.size() && attrPart[ap] != '=' && !std::isspace(static_cast<unsigned char>(attrPart[ap]))) ap++;
                    std::string attrName = toLower(attrPart.substr(nameStart, ap - nameStart));

                    while (ap < attrPart.size() && std::isspace(static_cast<unsigned char>(attrPart[ap]))) ap++;
                    std::string attrVal;
                    if (ap < attrPart.size() && attrPart[ap] == '=') {
                        ap++;
                        while (ap < attrPart.size() && std::isspace(static_cast<unsigned char>(attrPart[ap]))) ap++;
                        if (ap < attrPart.size()) {
                            if (attrPart[ap] == '"' || attrPart[ap] == '\'') {
                                char quote = attrPart[ap++];
                                size_t valStart = ap;
                                while (ap < attrPart.size() && attrPart[ap] != quote) ap++;
                                attrVal = attrPart.substr(valStart, ap - valStart);
                                if (ap < attrPart.size()) ap++;
                            } else {
                                size_t valStart = ap;
                                while (ap < attrPart.size() && !std::isspace(static_cast<unsigned char>(attrPart[ap]))) ap++;
                                attrVal = attrPart.substr(valStart, ap - valStart);
                            }
                        }
                    }

                    // Security & cleanup filtering
                    if (attrName.rfind("on", 0) != 0 && attrName != "style" && attrName != "srcset") {
                        if (tagName == "a" && attrName == "href") {
                            std::string lowerVal = toLower(attrVal);
                            if (lowerVal.rfind("javascript:", 0) == 0 || lowerVal.rfind("data:", 0) == 0) {
                                continue;
                            }
                        }
                        node->attributes[attrName] = decodeHtmlEntities(attrVal);
                    }
                }

                current->children.push_back(node);

                if (!selfClosing && !VOID_TAGS.count(tagName)) {
                    current = node;
                }
            }
        } else {
            // Text content
            size_t nextTag = htmlContent.find('<', i);
            if (nextTag == std::string::npos) nextTag = len;

            std::string text = decodeHtmlEntities(htmlContent.substr(i, nextTag - i));
            i = nextTag;

            if (current->children.empty()) {
                current->text += text;
            } else {
                current->children.back()->tail += text;
            }
        }
    }

    return root;
}

std::string HtmlParser::relativeXPath(const std::shared_ptr<DomNode>& node, const std::shared_ptr<DomNode>& root) {
    if (!node || node == root) return ".";

    std::string id = node->getAttr("id");
    if (!id.empty()) {
        std::string escaped;
        for (char c : id) {
            if (c == '"') escaped += "\\\"";
            else escaped += c;
        }
        return "//*[@id=\"" + escaped + "\"]";
    }

    std::vector<std::string> steps;
    auto current = node;

    while (current && current != root) {
        auto parent = current->parent.lock();
        if (!parent) break;

        int index = 1;
        for (const auto& sibling : parent->children) {
            if (sibling == current) break;
            if (sibling->tag == current->tag) {
                index++;
            }
        }

        steps.push_back(current->tag + "[" + std::to_string(index) + "]");
        current = parent;
    }

    std::reverse(steps.begin(), steps.end());
    if (steps.empty()) return ".";

    std::string res = "./";
    for (size_t s = 0; s < steps.size(); ++s) {
        if (s > 0) res += "/";
        res += steps[s];
    }
    return res;
}

namespace {

void flushText(std::string& buffer, int& offset, BlockKind kind, int level, const std::string& path, std::vector<Block>& blocks) {
    std::string cleaned = core::Charset::cleanText(buffer);
    buffer.clear();
    if (cleaned.empty()) return;

    Block b;
    b.kind = kind;
    b.text = cleaned;
    b.level = level;
    b.path = path;
    b.offset = offset;
    blocks.push_back(b);

    offset += static_cast<int>(cleaned.size());
}

void appendNode(const std::shared_ptr<DomNode>& node,
                BlockKind kind,
                int level,
                const std::string& path,
                std::string& buffer,
                int& offset,
                const std::shared_ptr<DomNode>& root,
                const std::string& baseUrl,
                std::vector<Block>& blocks) {
    if (!node->text.empty()) {
        buffer += node->text;
    }

    for (const auto& child : node->children) {
        const std::string& tag = child->tag;

        if (tag == "img") {
            flushText(buffer, offset, kind, level, path, blocks);
            std::string src = child->getAttr("src");
            if (src.empty()) src = child->getAttr("data-system-image-url");
            src = core::absoluteUrl(baseUrl, src);

            if (!src.empty()) {
                Block b;
                b.kind = BlockKind::Image;
                b.path = HtmlParser::relativeXPath(child, root);
                b.offset = offset;
                b.sourceUrl = src;
                blocks.push_back(b);
            }
        } else if (tag == "br") {
            buffer += "\n";
        } else if (BLOCK_TAGS.count(tag)) {
            flushText(buffer, offset, kind, level, path, blocks);

            BlockKind childKind = BlockKind::Text;
            int childLevel = 0;
            if (HEADING_TAGS.count(tag) && tag.size() > 1 && std::isdigit(tag[1])) {
                childKind = BlockKind::Heading;
                childLevel = tag[1] - '0';
            } else if (tag == "aside") {
                childKind = BlockKind::Footnote;
            }

            std::string childPath = HtmlParser::relativeXPath(child, root);
            std::string childBuffer;
            int childOffset = 0;

            appendNode(child, childKind, childLevel, childPath, childBuffer, childOffset, root, baseUrl, blocks);
            flushText(childBuffer, childOffset, childKind, childLevel, childPath, blocks);
        } else {
            appendNode(child, kind, level, path, buffer, offset, root, baseUrl, blocks);
        }

        if (!child->tail.empty()) {
            buffer += child->tail;
        }
    }
}

} // namespace

std::vector<Block> HtmlParser::extractBlocks(const std::string& htmlContent, const std::string& baseUrl) {
    auto root = parse(htmlContent);
    std::vector<Block> blocks;

    std::string buffer;
    int offset = 0;

    appendNode(root, BlockKind::Text, 0, ".", buffer, offset, root, baseUrl, blocks);
    flushText(buffer, offset, BlockKind::Text, 0, ".", blocks);

    if (blocks.empty() && !buffer.empty()) {
        std::string cleaned = core::Charset::cleanText(buffer);
        if (!cleaned.empty()) {
            Block b;
            b.kind = BlockKind::Text;
            b.text = cleaned;
            b.path = ".";
            blocks.push_back(b);
        }
    }
    return blocks;
}

} // namespace kinnovel::reader
