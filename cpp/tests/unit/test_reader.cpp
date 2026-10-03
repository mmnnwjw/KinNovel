#include <cassert>
#include <iostream>
#include <vector>
#include <string>
#include <cstring>
#include <filesystem>

#include "kinnovel/reader/DomModel.hpp"
#include "kinnovel/reader/HtmlParser.hpp"
#include "kinnovel/reader/WoffNormalizer.hpp"
#include "kinnovel/reader/FontEngine.hpp"

using namespace kinnovel::reader;

void testHtmlParser() {
    std::cout << "[TEST] Running testHtmlParser..." << std::endl;

    // 1. UTF8 content not mojibake
    auto b1 = HtmlParser::extractBlocks("<p>第一章 中文正文</p>");
    assert(b1.size() == 1);
    assert(b1[0].text == "第一章 中文正文");

    // 2. Parent block is not duplicated
    auto b2 = HtmlParser::extractBlocks("<div><h1>标题</h1><p>正文</p></div>");
    assert(b2.size() == 2);
    assert(b2[0].text == "标题");
    assert(b2[0].kind == BlockKind::Heading);
    assert(b2[0].level == 1);
    assert(b2[1].text == "正文");
    assert(b2[1].kind == BlockKind::Text);

    // 3. Image alt is not added to text
    auto b3 = HtmlParser::extractBlocks("<p>前文</p><img src=\"cover.jpg\" alt=\"封面说明\"><p>后文</p>");
    assert(b3.size() == 3);
    assert(b3[0].kind == BlockKind::Text && b3[0].text == "前文");
    assert(b3[1].kind == BlockKind::Image && b3[1].sourceUrl == "cover.jpg");
    assert(b3[2].kind == BlockKind::Text && b3[2].text == "后文");

    // 4. Inline image keeps DOM order and parent offsets
    auto b4 = HtmlParser::extractBlocks("<p>前<img src=\"cover.jpg\">后</p>");
    assert(b4.size() == 3);
    assert(b4[0].kind == BlockKind::Text && b4[0].text == "前" && b4[0].path == "./p[1]" && b4[0].offset == 0);
    assert(b4[1].kind == BlockKind::Image && b4[1].path == "./p[1]/img[1]" && b4[1].offset == 1);
    assert(b4[2].kind == BlockKind::Text && b4[2].text == "后" && b4[2].path == "./p[1]" && b4[2].offset == 1);

    // 5. XPath preserves order
    auto b5 = HtmlParser::extractBlocks("<p>一</p><p>二</p><h2>三</h2>");
    assert(b5.size() == 3);
    assert(b5[0].path == "./p[1]" && b5[0].offset == 0);
    assert(b5[1].path == "./p[2]" && b5[1].offset == 0);
    assert(b5[2].path == "./h2[1]" && b5[2].offset == 0);

    // 6. Invisible format chars are stripped
    auto b6 = HtmlParser::extractBlocks("<p>破折\xE2\x80\x8B号\xEF\xBB\xBF测试\xC2\xAD文本\xE2\x81\xA0。</p>");
    assert(b6.size() == 1);
    assert(b6[0].text == "破折号测试文本。");

    std::cout << "  -> testHtmlParser PASSED" << std::endl;
}

void testWoffNormalizer() {
    std::cout << "[TEST] Running testWoffNormalizer..." << std::endl;

    // Construct a synthetic uncompressed WOFF1 stream
    std::string tagA = "AAAA";
    std::string payloadA = "abc";
    std::string tagB = "BBBB";
    std::string payloadB = "12345";

    uint16_t numTables = 2;
    std::vector<uint8_t> woff(44 + numTables * 20);

    // wOFF signature
    std::memcpy(woff.data(), "wOFF", 4);
    // flavor \x00\x01\x00\x00
    woff[4] = 0; woff[5] = 1; woff[6] = 0; woff[7] = 0;
    // numTables = 2
    woff[12] = 0; woff[13] = 2;

    size_t tableOffset = 44 + numTables * 20;

    // Table 1: AAAA
    size_t rec1 = 44;
    std::memcpy(woff.data() + rec1, tagA.data(), 4);
    woff[rec1 + 4] = (tableOffset >> 24) & 0xFF;
    woff[rec1 + 5] = (tableOffset >> 16) & 0xFF;
    woff[rec1 + 6] = (tableOffset >> 8) & 0xFF;
    woff[rec1 + 7] = tableOffset & 0xFF;
    // compLength = 3
    woff[rec1 + 11] = 3;
    // origLength = 3
    woff[rec1 + 15] = 3;
    woff.insert(woff.end(), payloadA.begin(), payloadA.end());
    // pad to 4 bytes
    while (woff.size() % 4 != 0) woff.push_back(0);

    // Table 2: BBBB
    tableOffset = woff.size();
    size_t rec2 = 44 + 20;
    std::memcpy(woff.data() + rec2, tagB.data(), 4);
    woff[rec2 + 4] = (tableOffset >> 24) & 0xFF;
    woff[rec2 + 5] = (tableOffset >> 16) & 0xFF;
    woff[rec2 + 6] = (tableOffset >> 8) & 0xFF;
    woff[rec2 + 7] = tableOffset & 0xFF;
    // compLength = 5
    woff[rec2 + 11] = 5;
    // origLength = 5
    woff[rec2 + 15] = 5;
    woff.insert(woff.end(), payloadB.begin(), payloadB.end());
    while (woff.size() % 4 != 0) woff.push_back(0);

    assert(WoffNormalizer::isWoff(woff));

    std::vector<uint8_t> sfnt;
    bool ok = WoffNormalizer::normalize(woff, sfnt);
    assert(ok);
    (void)ok;
    assert(sfnt.size() > 12 + 2 * 16);

    // Check numTables in SFNT header
    uint16_t sfntNumTables = (sfnt[4] << 8) | sfnt[5];
    assert(sfntNumTables == 2);
    (void)sfntNumTables;

    std::cout << "  -> testWoffNormalizer PASSED" << std::endl;
}

void testFontEngine() {
    std::cout << "[TEST] Running testFontEngine..." << std::endl;

    // Load fixture font
    std::string fontPath = "cpp/tests/fixtures/testfont.ttf";
    if (!std::filesystem::exists(fontPath)) {
        fontPath = "../tests/fixtures/testfont.ttf";
    }
    if (!std::filesystem::exists(fontPath)) {
        std::cerr << "Warning: testfont.ttf not found, skipping font engine test." << std::endl;
        return;
    }

    auto font = FontEngine::instance().loadFont(fontPath, 34);
    assert(font != nullptr);
    assert(font->getSize() == 34);

    // Check available glyphs
    assert(font->isGlyphAvailable(U'中'));
    assert(font->isGlyphAvailable(U'文'));
    assert(font->isGlyphAvailable(U'A'));
    assert(font->isGlyphAvailable(U'1'));
    assert(font->isGlyphAvailable(U' '));

    // Check missing glyph mapping to notdef
    assert(!font->isGlyphAvailable(0x10FFFF));

    // Check metrics
    double wZh = font->getCharAdvance(U'中');
    assert(wZh > 0.0);
    (void)wZh;
    double wA = font->getCharAdvance(U'A');
    assert(wA > 0.0);
    (void)wA;

    // Split font runs
    auto runs = FontEngine::instance().splitFontRuns("中文A1", font, nullptr);
    assert(!runs.empty());

    std::cout << "  -> testFontEngine PASSED" << std::endl;
}

int main() {
    testHtmlParser();
    testWoffNormalizer();
    testFontEngine();
    std::cout << "ALL READER UNIT TESTS PASSED!" << std::endl;
    return 0;
}
