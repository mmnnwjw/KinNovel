#include <cassert>
#include <iostream>
#include <fstream>
#include <string>
#include <vector>
#include <filesystem>

#include "kinnovel/reader/ReaderDocument.hpp"
#include "kinnovel/reader/FontEngine.hpp"
#include "kinnovel/core/Config.hpp"
#include "yyjson.h"

using namespace kinnovel::reader;
using namespace kinnovel::core;

std::string readFile(const std::string& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        throw std::runtime_error("Could not open file: " + path);
    }
    return std::string((std::istreambuf_iterator<char>(in)),
                       std::istreambuf_iterator<char>());
}

std::string findPath(const std::string& relPath) {
    std::vector<std::string> prefixes = {"", "../", "../../", "../../../", "../../../../"};
    for (const auto& p : prefixes) {
        if (std::filesystem::exists(p + relPath)) return p + relPath;
        if (relPath.rfind("cpp/", 0) == 0) {
            std::string sub = relPath.substr(4);
            if (std::filesystem::exists(p + sub)) return p + sub;
        }
    }
    return relPath;
}

void testGoldenWrap() {
    std::cout << "[TEST] Running testGoldenWrap..." << std::endl;

    std::string fontPath = findPath("cpp/tests/fixtures/testfont.ttf");
    auto font = FontEngine::instance().loadFont(fontPath, 34);
    assert(font != nullptr);

    std::string wrapJsonPath = findPath("cpp/tests/golden/golden_wrap.json");
    std::string jsonStr = readFile(wrapJsonPath);

    yyjson_doc* doc = yyjson_read(jsonStr.data(), jsonStr.size(), 0);
    assert(doc != nullptr);
    yyjson_val* root = yyjson_doc_get_root(doc);
    assert(yyjson_is_arr(root));

    size_t idx, max;
    yyjson_val* caseVal;
    yyjson_arr_foreach(root, idx, max, caseVal) {
        std::string desc = yyjson_get_str(yyjson_obj_get(caseVal, "description"));
        std::string text = yyjson_get_str(yyjson_obj_get(caseVal, "text"));
        double maxW = yyjson_get_num(yyjson_obj_get(caseVal, "max_width"));
        yyjson_val* expectedParts = yyjson_obj_get(caseVal, "expected_parts");

        auto parts = LayoutEngine::wrapLineParts(text, font, maxW);
        size_t expCount = yyjson_arr_size(expectedParts);

        if (parts.size() != expCount) {
            std::cerr << "FAIL Case [" << desc << "]: expected " << expCount << " lines, got " << parts.size() << std::endl;
            assert(parts.size() == expCount);
        }

        for (size_t i = 0; i < expCount; ++i) {
            yyjson_val* exp = yyjson_arr_get(expectedParts, i);
            std::string expText = yyjson_get_str(yyjson_obj_get(exp, "text"));
            int expStart = yyjson_get_int(yyjson_obj_get(exp, "line_start"));

            if (parts[i].text != expText || parts[i].lineStartOffset != expStart) {
                std::cerr << "FAIL Case [" << desc << "] line " << i << ":\n"
                          << "  Expected: '" << expText << "' (offset " << expStart << ")\n"
                          << "  Got:      '" << parts[i].text << "' (offset " << parts[i].lineStartOffset << ")\n";
                assert(parts[i].text == expText);
                assert(parts[i].lineStartOffset == expStart);
            }
        }
    }

    yyjson_doc_free(doc);
    std::cout << "  -> testGoldenWrap PASSED (100% parity with Python wrap_line)" << std::endl;
}

void testGoldenChapterPagination() {
    std::cout << "[TEST] Running testGoldenChapterPagination..." << std::endl;

    std::string fontPath = findPath("cpp/tests/fixtures/testfont.ttf");
    std::string chapterJsonPath = findPath("cpp/tests/golden/golden_chapter.json");
    std::string jsonStr = readFile(chapterJsonPath);

    yyjson_doc* doc = yyjson_read(jsonStr.data(), jsonStr.size(), 0);
    assert(doc != nullptr);
    yyjson_val* root = yyjson_doc_get_root(doc);

    yyjson_val* chapterInput = yyjson_obj_get(root, "chapter_input");
    assert(chapterInput != nullptr);

    auto cfg = std::make_shared<Config>();
    cfg->set("font_size", 34);
    cfg->set("line_spacing", 1.42);
    cfg->set("reader_margin", 34);
    cfg->set("first_line_indent", true);
    cfg->set("strict_tls", false);

    ReaderDocument readerDoc(chapterInput, "https://example.com", fontPath, cfg);
    const auto& pages = readerDoc.prepare(800, 1000);

    int expPageCount = yyjson_get_int(yyjson_obj_get(root, "page_count"));
    assert(static_cast<int>(pages.size()) == expPageCount);
    (void)expPageCount;

    yyjson_val* expPages = yyjson_obj_get(root, "pages");
    assert(yyjson_is_arr(expPages));

    for (size_t p = 0; p < pages.size(); ++p) {
        yyjson_val* expPage = yyjson_arr_get(expPages, p);
        int expItemCount = yyjson_get_int(yyjson_obj_get(expPage, "item_count"));
        const auto& page = pages[p];

        if (static_cast<int>(page.size()) != expItemCount) {
            std::cerr << "FAIL Page " << p << ": expected " << expItemCount << " items, got " << page.size() << std::endl;
            assert(static_cast<int>(page.size()) == expItemCount);
        }

        yyjson_val* expItems = yyjson_obj_get(expPage, "items");
        for (size_t it = 0; it < page.size(); ++it) {
            const auto& item = page[it];
            yyjson_val* expItem = yyjson_arr_get(expItems, it);

            std::string expType = yyjson_get_str(yyjson_obj_get(expItem, "type"));
            int expX = yyjson_get_int(yyjson_obj_get(expItem, "x"));
            int expY = yyjson_get_int(yyjson_obj_get(expItem, "y"));
            std::string expPath = yyjson_get_str(yyjson_obj_get(expItem, "path"));
            int expOffset = yyjson_get_int(yyjson_obj_get(expItem, "offset"));

            assert(item.type == expType);
            assert(item.x == expX);
            assert(item.y == expY);
            assert(item.path == expPath);
            assert(item.offset == expOffset);
            (void)expX; (void)expY; (void)expOffset;

            if (item.type == "text") {
                std::string expText = yyjson_get_str(yyjson_obj_get(expItem, "text"));
                int expSize = yyjson_get_int(yyjson_obj_get(expItem, "size"));
                if (item.text != expText || item.size != expSize) {
                    std::cerr << "FAIL Page " << p << " Item " << it << ":\n"
                              << "  Expected: '" << expText << "' (size " << expSize << ")\n"
                              << "  Got:      '" << item.text << "' (size " << item.size << ")\n";
                    assert(item.text == expText);
                    assert(item.size == expSize);
                }
            } else if (item.type == "image") {
                std::string expUrl = yyjson_get_str(yyjson_obj_get(expItem, "url"));
                int expW = yyjson_get_int(yyjson_obj_get(expItem, "width"));
                int expH = yyjson_get_int(yyjson_obj_get(expItem, "height"));
                assert(item.url == expUrl);
                assert(item.width == expW);
                assert(item.height == expH);
                (void)expW; (void)expH;
            }
        }
    }

    // Verify firstAnchorOnPage and pageForPath
    auto anchor0 = readerDoc.firstAnchorOnPage(0);
    assert(readerDoc.pageForPath(anchor0.first, anchor0.second) == 0);

    auto anchor1 = readerDoc.firstAnchorOnPage(1);
    assert(readerDoc.pageForPath(anchor1.first, anchor1.second) == 1);

    auto anchor2 = readerDoc.firstAnchorOnPage(2);
    assert(readerDoc.pageForPath(anchor2.first, anchor2.second) == 2);

    yyjson_doc_free(doc);
    std::cout << "  -> testGoldenChapterPagination PASSED (100% parity with Python ReaderDocument)" << std::endl;
}

int main() {
    testGoldenWrap();
    testGoldenChapterPagination();
    std::cout << "ALL GOLDEN TESTS PASSED!" << std::endl;
    return 0;
}
