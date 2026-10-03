#include <cassert>
#include <iostream>
#include <vector>
#include <string>

#include "kinnovel/core/Sha256.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Charset.hpp"

using namespace kinnovel::core;

void testSha256() {
    std::cout << "[TEST] Running testSha256..." << std::endl;
    // Known SHA256 test vectors
    // "" -> e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    assert(Sha256::hash("") == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");

    // "hello" -> 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
    assert(Sha256::hash("hello") == "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");

    std::cout << "  -> testSha256 PASSED" << std::endl;
}

void testUtils() {
    std::cout << "[TEST] Running testUtils..." << std::endl;

    // URL encode / decode
    std::string raw = "hello world! 123";
    std::string encoded = Utils::urlEncode(raw);
    assert(encoded == "hello%20world%21%20123");
    assert(Utils::urlDecode(encoded) == raw);

    // stableCacheName
    std::string name1 = Utils::stableCacheName("https://example.com/font.woff");
    assert(!name1.empty());
    assert(name1 == Utils::stableCacheName("https://example.com/font.woff"));

    // Absolute URL
    assert(Utils::absoluteUrl("https://example.com/path/a", "b.jpg") == "https://example.com/path/b.jpg");
    assert(Utils::absoluteUrl("https://example.com/path/", "/root.jpg") == "https://example.com/root.jpg");
    assert(Utils::absoluteUrl("https://example.com", "https://other.com/1.png") == "https://other.com/1.png");

    // Format bytes
    assert(Utils::formatBytes(500) == "500 B");
    assert(Utils::formatBytes(1024) == "1.0 KB");
    assert(Utils::formatBytes(1024 * 1024 * 5) == "5.0 MB");

    // String split / trim
    assert(Utils::trim("  abc  \r\n") == "abc");
    auto tokens = Utils::split("a,b,c", ',');
    assert(tokens.size() == 3);
    assert(tokens[0] == "a" && tokens[1] == "b" && tokens[2] == "c");

    std::cout << "  -> testUtils PASSED" << std::endl;
}

void testCharset() {
    std::cout << "[TEST] Running testCharset..." << std::endl;

    // Invisible format characters stripping
    // \u200b \u200c \u200d \ufeff \u00ad \u2060
    std::string input = "破折\xE2\x80\x8B号\xEF\xBB\xBF测试\xC2\xAD文本\xE2\x81\xA0。";
    std::string cleaned = Charset::cleanText(input);
    assert(cleaned == "破折号测试文本。");

    // Whitespace collapse
    std::string spaceText = "  Hello \t\t world  \r\n test  ";
    assert(Charset::collapseWhitespace(spaceText) == "Hello world test");

    // Chinese conversion
    std::string trad = "這是一個測試文本。說話與書本。";
    std::string simp = Charset::convertChinese(trad, "t2s");
    assert(simp == "这是一个测试文本。说话与书本。");

    std::string backTrad = Charset::convertChinese(simp, "s2t");
    assert(backTrad == trad);

    std::cout << "  -> testCharset PASSED" << std::endl;
}

void testConfig() {
    std::cout << "[TEST] Running testConfig..." << std::endl;

    Config cfg;
    // Default values
    assert(cfg.getInt("font_size") == 48);
    assert(cfg.getDouble("line_spacing") == 1.42);
    assert(cfg.getInt("reader_margin") == 34);
    assert(cfg.getBool("first_line_indent") == true);
    assert(cfg.getBool("strict_tls") == false);
    assert(cfg.getInt("cache_limit_mb") == 512);

    // Override
    cfg.set("font_size", 32);
    assert(cfg.getInt("font_size") == 32);

    cfg.set("custom_key", "custom_val");
    assert(cfg.getString("custom_key") == "custom_val");

    std::cout << "  -> testConfig PASSED" << std::endl;
}

int main() {
    testSha256();
    testUtils();
    testCharset();
    testConfig();
    std::cout << "ALL CORE TESTS PASSED!" << std::endl;
    return 0;
}
