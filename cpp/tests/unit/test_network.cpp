#include <cassert>
#include <iostream>
#include <vector>
#include <string>
#include <chrono>
#include <thread>
#include <zlib.h>
#include <filesystem>
#include <fstream>

#include "kinnovel/network/RateLimiter.hpp"
#include "kinnovel/network/HttpTransport.hpp"
#include "kinnovel/network/WebSocketClient.hpp"
#include "kinnovel/network/SignalRClient.hpp"
#include "kinnovel/network/SessionStore.hpp"
#include "kinnovel/network/ApiClient.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Config.hpp"
#include "yyjson.h"

using namespace kinnovel::network;
using namespace kinnovel::core;

std::string gzipCompress(const std::string& in) {
    z_stream strm;
    std::memset(&strm, 0, sizeof(strm));
    deflateInit2(&strm, Z_DEFAULT_COMPRESSION, Z_DEFLATED, 16 + MAX_WBITS, 8, Z_DEFAULT_STRATEGY);

    std::vector<uint8_t> out(in.size() + 128);
    strm.next_in = reinterpret_cast<Bytef*>(const_cast<char*>(in.data()));
    strm.avail_in = static_cast<uInt>(in.size());
    strm.next_out = out.data();
    strm.avail_out = static_cast<uInt>(out.size());

    deflate(&strm, Z_FINISH);
    size_t outSize = out.size() - strm.avail_out;
    deflateEnd(&strm);

    // Base64 encode
    static const char* tbl = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string b64;
    for (size_t i = 0; i < outSize; i += 3) {
        uint32_t b = (out[i] << 16);
        if (i + 1 < outSize) b |= (out[i + 1] << 8);
        if (i + 2 < outSize) b |= out[i + 2];
        b64.push_back(tbl[(b >> 18) & 0x3F]);
        b64.push_back(tbl[(b >> 12) & 0x3F]);
        b64.push_back((i + 1 < outSize) ? tbl[(b >> 6) & 0x3F] : '=');
        b64.push_back((i + 2 < outSize) ? tbl[b & 0x3F] : '=');
    }
    return b64;
}

void testRateLimiter() {
    std::cout << "[TEST] Running testRateLimiter..." << std::endl;
    // 3 requests per 0.2 seconds
    RateLimiter limiter(3, 0.2);

    auto start = std::chrono::steady_clock::now();
    limiter.wait();
    limiter.wait();
    limiter.wait();
    auto d1 = std::chrono::steady_clock::now() - start;
    // First 3 requests should be virtually immediate (< 50ms)
    assert(std::chrono::duration_cast<std::chrono::milliseconds>(d1).count() < 50);
    (void)d1;

    // 4th request must wait for window to clear
    limiter.wait();
    auto d2 = std::chrono::steady_clock::now() - start;
    assert(std::chrono::duration_cast<std::chrono::milliseconds>(d2).count() >= 180);
    (void)d2;

    std::cout << "  -> testRateLimiter PASSED" << std::endl;
}

void testGzipAndBase64() {
    std::cout << "[TEST] Running testGzipAndBase64..." << std::endl;

    std::string original = "{\"Data\":[{\"Id\":1,\"Title\":\"测试\"}],\"TotalPages\":1}";
    std::string encoded = gzipCompress(original);

    SignalRClient client("https://example.test");
    std::string decoded = client.decodeResponse(encoded);
    assert(decoded == original);

    // Gunzip limited error test
    std::string huge(2000, 'X');
    std::string hugeGz = gzipCompress(huge);
    std::string rawGz = base64Decode(hugeGz);
    try {
        gunzipLimited(reinterpret_cast<const uint8_t*>(rawGz.data()), rawGz.size(), 100);
        assert(false && "Should have thrown limit exception");
    } catch (const TransportError&) {
        // Expected
    }

    std::cout << "  -> testGzipAndBase64 PASSED" << std::endl;
}

// Mock WebSocket for SignalR testing
class MockWs : public WebSocketClient {
public:
    MockWs(std::vector<std::string> incomingQueue = {}, bool failOnReceive = false)
        : WebSocketClient("ws://mock"),
          m_incoming(std::move(incomingQueue)),
          m_failOnReceive(failOnReceive) {}

    void connect() override {
        m_connected = true;
    }

    void sendText(const std::string& text) override {
        m_sent.push_back(text);
        if (text.find("\"target\"") != std::string::npos) {
            size_t idPos = text.find("\"invocationId\":\"");
            if (idPos != std::string::npos) {
                size_t idEnd = text.find('"', idPos + 16);
                std::string invId = text.substr(idPos + 16, idEnd - (idPos + 16));
                if (m_autoRespond) {
                    std::string resp = "{\"type\":3,\"invocationId\":\"" + invId +
                                       "\",\"result\":{\"success\":true,\"response\":{\"ok\":true}}}\x1e";
                    m_incoming.push_back(resp);
                } else if (m_errorRespond) {
                    std::string resp = "{\"type\":3,\"invocationId\":\"" + invId +
                                       "\",\"error\":\"unauthorized\"}\x1e";
                    m_incoming.push_back(resp);
                }
            }
        }
    }

    std::pair<int, std::vector<uint8_t>> receive() override {
        if (m_failOnReceive) {
            m_failOnReceive = false; // Next call can succeed
            throw TransportError("Connection reset by peer");
        }
        if (m_incoming.empty()) {
            throw TransportError("No more mock data");
        }
        std::string next = m_incoming.front();
        m_incoming.erase(m_incoming.begin());
        return { 1, std::vector<uint8_t>(next.begin(), next.end()) };
    }

    void close() override {
        m_connected = false;
    }

    std::vector<std::string> m_sent;
    std::vector<std::string> m_incoming;
    bool m_autoRespond = false;
    bool m_errorRespond = false;
    bool m_failOnReceive = false;
    bool m_connected = false;
};

class MockSignalRClient : public SignalRClient {
public:
    MockSignalRClient(std::function<std::unique_ptr<WebSocketClient>()> wsFactory)
        : SignalRClient("https://example.test"),
          m_wsFactory(std::move(wsFactory)) {}

    std::string negotiate(const std::string&) override {
        return "mock_connection_token";
    }

    std::unique_ptr<WebSocketClient> createWebSocket(
        const std::string&,
        const std::map<std::string, std::string>&,
        int,
        bool) override {
        return m_wsFactory();
    }

    std::function<std::unique_ptr<WebSocketClient>()> m_wsFactory;
};

void testSignalRTransportRetry() {
    std::cout << "[TEST] Running testSignalRTransportRetry..." << std::endl;

    // Simulate handshake response "{}\x1e"
    // Then fail on first invoke receive, succeed on second attempt
    int attempt = 0;
    auto factory = [&attempt]() -> std::unique_ptr<WebSocketClient> {
        attempt++;
        if (attempt == 1) {
            auto ws = std::make_unique<MockWs>(std::vector<std::string>{"{}\x1e"}, true);
            ws->m_autoRespond = false;
            return ws;
        } else {
            auto ws = std::make_unique<MockWs>(std::vector<std::string>{"{}\x1e"}, false);
            ws->m_autoRespond = true;
            return ws;
        }
    };

    MockSignalRClient client(factory);
    // Transport failure on attempt 0 -> reconnects and succeeds on attempt 1
    std::string res = client.invoke("GetOnlineInfo", "{}");
    assert(res.find("\"ok\":true") != std::string::npos);

    std::cout << "  -> testSignalRTransportRetry PASSED" << std::endl;
}

void testSignalRApiErrorNoRetry() {
    std::cout << "[TEST] Running testSignalRApiErrorNoRetry..." << std::endl;

    auto factory = []() -> std::unique_ptr<WebSocketClient> {
        auto ws = std::make_unique<MockWs>(std::vector<std::string>{"{}\x1e"});
        ws->m_errorRespond = true;
        return ws;
    };

    MockSignalRClient client(factory);
    try {
        client.invoke("GetMyInfo", "{}");
        assert(false && "Should have thrown 401 ApiError");
    } catch (const ApiError& exc) {
        assert(exc.getStatus() == 401);
    }

    std::cout << "  -> testSignalRApiErrorNoRetry PASSED" << std::endl;
}

void testOfflineChapterDiskFallback() {
    std::cout << "[TEST] Running testOfflineChapterDiskFallback..." << std::endl;

    std::string cacheDir = Config::getCacheDir() + "/chapters";
    std::filesystem::create_directories(cacheDir);

    int bookId = 9999;
    int sortNum = 1;
    std::string cacheKey = std::to_string(bookId) + "_" + std::to_string(sortNum);
    std::string cacheFile = cacheDir + "/" + Utils::stableCacheName(cacheKey, ".json");

    std::string cachedChapter = "{\"Title\":\"离线正文\",\"Content\":\"<p>这是离线缓存的内容</p>\"}";
    Utils::atomicWrite(cacheFile, cachedChapter);

    auto cfg = std::make_shared<Config>();
    cfg->set("api_server", "https://invalid.server.offline.test");
    ApiClient api(cfg);

    // Network call will fail and fall back to disk cache
    std::string content = api.getNovelContent(bookId, sortNum);
    assert(content == cachedChapter);

    // Clean up test cache
    std::filesystem::remove(cacheFile);

    std::cout << "  -> testOfflineChapterDiskFallback PASSED" << std::endl;
}

void testCachePruning() {
    std::cout << "[TEST] Running testCachePruning..." << std::endl;

    std::string testDir = Config::getCacheDir() + "/test_prune";
    std::filesystem::create_directories(testDir);

    std::string f1 = testDir + "/file1.bin";
    std::string f2 = testDir + "/file2.bin";
    std::string f3 = testDir + "/file3.bin";

    std::string data10k(10240, 'A');
    Utils::atomicWrite(f1, data10k);
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
    Utils::atomicWrite(f2, data10k);
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
    Utils::atomicWrite(f3, data10k);

    assert(Utils::directorySize(testDir) >= 30720);

    // Prune to 15KB (should remove f1 and f2, keeping newest f3)
    Utils::pruneCache(testDir, 15000);
    assert(Utils::directorySize(testDir) <= 15000);
    assert(std::filesystem::exists(f3));
    assert(!std::filesystem::exists(f1));

    std::filesystem::remove_all(testDir);
    std::cout << "  -> testCachePruning PASSED" << std::endl;
}

void testSessionStore() {
    std::cout << "[TEST] Running testSessionStore..." << std::endl;

    std::string testPath = Config::getCacheDir() + "/test_session.json";
    if (std::filesystem::exists(testPath)) {
        std::filesystem::remove(testPath);
    }

    SessionStore session(testPath);
    session.setString("Token", "tok_abc");
    session.setString("RefreshToken", "ref_123");
    session.setDouble("TokenUpdatedAt", 123456.0);
    session.setUserJson("{\"Id\":42,\"NickName\":\"测试用户\"}");

    // Reload from file
    SessionStore session2(testPath);
    assert(session2.getString("Token") == "tok_abc");
    assert(session2.getString("RefreshToken") == "ref_123");
    assert(session2.getDouble("TokenUpdatedAt") == 123456.0);
    assert(session2.getUserJson().find("\"NickName\":\"测试用户\"") != std::string::npos);

    session2.clearCredentials();
    assert(session2.getString("Token").empty());
    assert(session2.getString("RefreshToken").empty());

    std::filesystem::remove(testPath);
    std::cout << "  -> testSessionStore PASSED" << std::endl;
}

int main() {
    testRateLimiter();
    testGzipAndBase64();
    testSignalRTransportRetry();
    testSignalRApiErrorNoRetry();
    testOfflineChapterDiskFallback();
    testCachePruning();
    testSessionStore();
    std::cout << "ALL NETWORK TESTS PASSED!" << std::endl;
    return 0;
}
