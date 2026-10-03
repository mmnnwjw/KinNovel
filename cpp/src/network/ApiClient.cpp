#include "kinnovel/network/ApiClient.hpp"
#include "kinnovel/network/HttpTransport.hpp"
#include "kinnovel/core/Sha256.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Logger.hpp"
#include "yyjson.h"

#include <openssl/rand.h>
#include <chrono>
#include <fstream>
#include <sstream>
#include <iomanip>
#include <filesystem>

namespace kinnovel::network {

namespace {

std::string randomHex(size_t bytesCount) {
    std::vector<uint8_t> buf(bytesCount);
    RAND_bytes(buf.data(), static_cast<int>(bytesCount));
    std::ostringstream oss;
    oss << std::hex << std::setfill('0');
    for (uint8_t b : buf) {
        oss << std::setw(2) << static_cast<int>(b);
    }
    return oss.str();
}

double currentTimestamp() {
    auto now = std::chrono::system_clock::now();
    return std::chrono::duration<double>(now.time_since_epoch()).count();
}

} // namespace

ApiClient::ApiClient(std::shared_ptr<core::Config> config,
                     std::shared_ptr<SessionStore> session)
    : m_config(std::move(config)),
      m_session(std::move(session)) {
    if (!m_config) {
        m_config = std::make_shared<core::Config>();
    }
    if (!m_session) {
        m_session = std::make_shared<SessionStore>();
    }

    m_server = m_config->getString("api_server");
    while (!m_server.empty() && m_server.back() == '/') {
        m_server.pop_back();
    }

    std::string visitorPath = core::Config::getCacheDir() + "/visitor-id";
    if (std::filesystem::exists(visitorPath)) {
        std::ifstream in(visitorPath);
        in >> m_visitorId;
    }
    if (m_visitorId.empty()) {
        m_visitorId = randomHex(16);
        core::Utils::atomicWrite(visitorPath, m_visitorId);
    }

    int reqLimit = m_config->getInt("request_limit", 9);
    double reqWindow = m_config->getDouble("request_window_ms", 5500.0) / 1000.0;
    bool strictTls = m_config->getBool("strict_tls", false);

    m_hub = std::make_shared<SignalRClient>(
        m_server,
        [this]() { return getAccessToken(); },
        strictTls,
        reqLimit,
        reqWindow,
        30,
        m_visitorId
    );
}

void ApiClient::setServer(const std::string& server) {
    m_server = server;
    while (!m_server.empty() && m_server.back() == '/') {
        m_server.pop_back();
    }
    m_config->setString("api_server", m_server);
    m_hub->setServer(m_server);
}

std::string ApiClient::getUser() const {
    return m_session->getUserJson();
}

int ApiClient::getUserId() const {
    std::string userJson = getUser();
    yyjson_doc* doc = yyjson_read(userJson.data(), userJson.size(), 0);
    if (!doc) return 0;
    yyjson_val* root = yyjson_doc_get_root(doc);
    yyjson_val* idVal = yyjson_obj_get(root, "Id");
    if (!idVal) idVal = yyjson_obj_get(root, "id");
    int id = idVal ? yyjson_get_int(idVal) : 0;
    yyjson_doc_free(doc);
    return id;
}

bool ApiClient::hasRefreshToken() const {
    return !m_session->getString("RefreshToken").empty();
}

std::string ApiClient::httpApi(const std::string& path,
                               const std::string& payloadJson,
                               const std::string& method,
                               const std::string& token) {
    std::string url = (path.rfind("http://", 0) == 0 || path.rfind("https://", 0) == 0)
                      ? path : (m_server + path);

    std::map<std::string, std::string> headers = {
        {"Accept", "application/json"},
        {"User-Agent", "KinNovel/0.1"},
        {"x-id", m_visitorId}
    };
    if (!payloadJson.empty()) {
        headers["Content-Type"] = "application/json";
    }
    if (!token.empty()) {
        headers["Authorization"] = "Bearer " + token;
    }

    bool strictTls = m_config->getBool("strict_tls", false);
    HttpResponse resp = HttpTransport::request(url, method, payloadJson, headers, 30, strictTls);

    yyjson_doc* doc = yyjson_read(resp.body.data(), resp.body.size(), 0);
    if (!doc) {
        if (resp.statusCode >= 400) {
            throw ApiError("HTTP " + std::to_string(resp.statusCode), resp.statusCode);
        }
        throw TransportError("响应不是 JSON");
    }

    yyjson_val* root = yyjson_doc_get_root(doc);
    yyjson_val* succVal = yyjson_obj_get(root, "Success");
    if (!succVal) succVal = yyjson_obj_get(root, "success");
    bool success = succVal ? yyjson_get_bool(succVal) : (resp.statusCode < 400);

    if (resp.statusCode >= 400 || !success) {
        yyjson_val* msgVal = yyjson_obj_get(root, "Msg");
        if (!msgVal) msgVal = yyjson_obj_get(root, "msg");
        std::string msg = msgVal && yyjson_is_str(msgVal) ? yyjson_get_str(msgVal) : ("HTTP " + std::to_string(resp.statusCode));

        yyjson_val* stVal = yyjson_obj_get(root, "Status");
        if (!stVal) stVal = yyjson_obj_get(root, "status");
        int st = stVal ? yyjson_get_int(stVal) : resp.statusCode;

        yyjson_doc_free(doc);
        throw ApiError(msg, st);
    }

    yyjson_val* respVal = yyjson_obj_get(root, "Response");
    if (!respVal) respVal = yyjson_obj_get(root, "response");

    std::string out;
    if (respVal && yyjson_is_str(respVal)) {
        out = yyjson_get_str(respVal);
    } else if (respVal) {
        char* ser = yyjson_val_write(respVal, 0, nullptr);
        if (ser) {
            out = ser;
            free(ser);
        }
    }
    yyjson_doc_free(doc);
    return out;
}

std::string ApiClient::login(const std::string& email, const std::string& password) {
    std::string pwdHash = core::Sha256::hash(password);
    std::string payload = "{\"email\":\"" + email + "\",\"password\":\"" + pwdHash + "\"}";

    std::string respStr = httpApi("/api/user/login", payload, "POST");

    yyjson_doc* doc = yyjson_read(respStr.data(), respStr.size(), 0);
    if (!doc) {
        throw ApiError("登录响应缺少凭据");
    }
    yyjson_val* root = yyjson_doc_get_root(doc);
    yyjson_val* tokenVal = yyjson_obj_get(root, "Token");
    if (!tokenVal) tokenVal = yyjson_obj_get(root, "token");

    yyjson_val* refreshVal = yyjson_obj_get(root, "RefreshToken");
    if (!refreshVal) refreshVal = yyjson_obj_get(root, "refreshToken");

    if (!tokenVal || !refreshVal || !yyjson_is_str(tokenVal) || !yyjson_is_str(refreshVal)) {
        yyjson_doc_free(doc);
        throw ApiError("登录响应缺少 Token 或 RefreshToken");
    }

    std::string token = yyjson_get_str(tokenVal);
    std::string refresh = yyjson_get_str(refreshVal);
    yyjson_doc_free(doc);

    m_session->setString("Token", token);
    m_session->setString("RefreshToken", refresh);
    m_session->setDouble("TokenUpdatedAt", currentTimestamp());

    std::string user = getMyInfo();
    m_session->setUserJson(user);
    return user;
}

std::string ApiClient::refreshAccessToken() {
    std::string refresh = m_session->getString("RefreshToken");
    if (refresh.empty()) return "";

    std::lock_guard<std::mutex> lock(m_refreshMutex);
    std::string token = m_session->getString("Token");
    double updated = m_session->getDouble("TokenUpdatedAt");
    if (!token.empty() && (currentTimestamp() - updated) < 25.0) {
        return token;
    }

    std::string payload = "{\"token\":\"" + refresh + "\"}";
    try {
        std::string newToken = httpApi("/api/user/refresh_token", payload, "POST");
        if (!newToken.empty()) {
            m_session->setString("Token", newToken);
            m_session->setDouble("TokenUpdatedAt", currentTimestamp());
            return newToken;
        }
    } catch (const ApiError& exc) {
        if (exc.getStatus() == -100 || exc.getStatus() == 404) {
            m_session->clearCredentials();
        }
        throw;
    }
    return "";
}

std::string ApiClient::getAccessToken() {
    std::string token = m_session->getString("Token");
    double updated = m_session->getDouble("TokenUpdatedAt");
    if (!token.empty() && (currentTimestamp() - updated) < 25.0) {
        return token;
    }
    try {
        return refreshAccessToken();
    } catch (...) {
        return "";
    }
}

std::string ApiClient::refreshUser() {
    if (!hasRefreshToken()) return "";
    std::string user = getMyInfo();
    m_session->setUserJson(user);
    return user;
}

std::string ApiClient::invoke(const std::string& method, const std::string& paramsJson) {
    try {
        return m_hub->invoke(method, paramsJson);
    } catch (const ApiError& exc) {
        if (exc.getStatus() != 401) {
            throw;
        }
        m_session->setString("Token", "");
        m_session->setDouble("TokenUpdatedAt", 0);
        std::string newToken = refreshAccessToken();
        if (newToken.empty()) {
            throw;
        }
        m_hub->close();
        return m_hub->invoke(method, paramsJson);
    }
}

std::string ApiClient::getBookList(int page, int size, const std::string& keywords,
                                   const std::string& order, bool ignoreJapanese,
                                   bool ignoreAi, int categoryId) {
    std::ostringstream oss;
    oss << "{\"Page\":" << page << ",\"Size\":" << size
        << ",\"Order\":\"" << order << "\""
        << ",\"IgnoreJapanese\":" << (ignoreJapanese ? "true" : "false")
        << ",\"IgnoreAI\":" << (ignoreAi ? "true" : "false");
    if (!keywords.empty()) {
        oss << ",\"KeyWords\":\"" << keywords << "\"";
    }
    if (categoryId >= 0) {
        oss << ",\"CategoryId\":" << categoryId;
    }
    oss << "}";
    return invoke("GetBookList", oss.str());
}

std::string ApiClient::getBookCategories(const std::string& bookType) {
    return invoke("GetBookCategories", "{\"Type\":\"" + bookType + "\"}");
}

std::string ApiClient::getRank(int days) {
    return invoke("GetRank", "{\"Days\":" + std::to_string(days) + "}");
}

std::string ApiClient::getAnnouncementList(int page, int size) {
    return invoke("GetAnnouncementList", "{\"Page\":" + std::to_string(page) + ",\"Size\":" + std::to_string(size) + "}");
}

std::string ApiClient::getAnnouncementDetail(int announcementId) {
    return invoke("GetAnnouncementDetail", "{\"Id\":" + std::to_string(announcementId) + "}");
}

std::string ApiClient::getBookInfo(int bookId) {
    return invoke("GetBookInfo", "{\"Id\":" + std::to_string(bookId) + "}");
}

std::string ApiClient::getBookListByIds(const std::vector<int>& ids, const std::string& bookType) {
    if (ids.size() > 24) {
        throw ApiError("单次最多请求 24 本书", 400);
    }
    std::ostringstream oss;
    oss << "{\"Ids\":[";
    for (size_t i = 0; i < ids.size(); ++i) {
        if (i > 0) oss << ",";
        oss << ids[i];
    }
    oss << "]";
    if (!bookType.empty()) {
        oss << ",\"Type\":\"" << bookType << "\"";
    }
    oss << "}";
    return invoke("GetBookListByIds", oss.str());
}

std::string ApiClient::getBooksBySeries(const std::string& seriesName, int page, int size,
                                        const std::string& order, bool ignoreJapanese,
                                        bool ignoreAi) {
    std::ostringstream oss;
    oss << "{\"SeriesName\":\"" << seriesName << "\""
        << ",\"Page\":" << page << ",\"Size\":" << size
        << ",\"Order\":\"" << order << "\""
        << ",\"IgnoreJapanese\":" << (ignoreJapanese ? "true" : "false")
        << ",\"IgnoreAI\":" << (ignoreAi ? "true" : "false") << "}";
    return invoke("GetBooksBySeries", oss.str());
}

std::string ApiClient::getNovelContent(int bookId, int sortNum, const std::string& convert) {
    std::string cacheKey = std::to_string(bookId) + "_" + std::to_string(sortNum);
    std::string cacheDir = core::Config::getCacheDir() + "/chapters";
    std::string cacheFile = cacheDir + "/" + core::Utils::stableCacheName(cacheKey, ".json");

    std::ostringstream oss;
    oss << "{\"Bid\":" << bookId << ",\"SortNum\":" << sortNum;
    if (!convert.empty()) {
        oss << ",\"Convert\":\"" << convert << "\"";
    }
    oss << "}";
    std::string params = oss.str();

    try {
        std::string res = invoke("GetNovelContent", params);
        core::Utils::atomicWrite(cacheFile, res);
        core::Utils::touch(cacheFile);
        return res;
    } catch (const TransportError& exc) {
        // Offline / network failure fallback
        if (std::filesystem::exists(cacheFile)) {
            std::ifstream in(cacheFile, std::ios::binary);
            if (in) {
                return std::string((std::istreambuf_iterator<char>(in)),
                                   std::istreambuf_iterator<char>());
            }
        }
        throw;
    }
}

std::string ApiClient::saveReadPosition(int bookId, int chapterId, const std::string& xpath) {
    std::ostringstream oss;
    oss << "{\"Bid\":" << bookId << ",\"Cid\":" << chapterId << ",\"XPath\":\"" << (xpath.empty() ? "." : xpath) << "\"}";
    return invoke("SaveReadPosition", oss.str());
}

std::string ApiClient::getReadHistory() {
    return invoke("GetReadHistory", "{}");
}

std::string ApiClient::clearReadHistory() {
    return invoke("ClearReadHistory", "{}");
}

std::string ApiClient::getMyInfo() {
    return invoke("GetMyInfo", "{}");
}

std::string ApiClient::getNotifications(int page, int size) {
    return invoke("GetNotifications", "{\"Page\":" + std::to_string(page) + ",\"Size\":" + std::to_string(size) + "}");
}

std::string ApiClient::markNotifications(const std::vector<int>& ids) {
    std::ostringstream oss;
    oss << "{\"Ids\":[";
    for (size_t i = 0; i < ids.size(); ++i) {
        if (i > 0) oss << ",";
        oss << ids[i];
    }
    oss << "]}";
    return invoke("MarkNotifications", oss.str());
}

std::string ApiClient::getBookShelf() {
    return invoke("GetBookShelf", "{}");
}

std::string ApiClient::saveBookShelf(const std::string& itemsJson, const std::string& version) {
    return invoke("SaveBookShelf", "{\"data\":" + itemsJson + ",\"ver\":\"" + version + "\"}");
}

std::string ApiClient::signIn() {
    return invoke("SignIn", "{}");
}

std::string ApiClient::getShop() {
    return invoke("GetShop", "{}");
}

std::string ApiClient::getMyItems() {
    return invoke("GetMyItems", "{}");
}

std::string ApiClient::buyShopItem(const std::string& key, int quantity) {
    return invoke("BuyShopItem", "{\"Key\":\"" + key + "\",\"Quantity\":" + std::to_string(quantity) + "}");
}

std::string ApiClient::getComments(const std::string& commentType, int targetId, int page) {
    std::ostringstream oss;
    oss << "{\"Type\":\"" << commentType << "\",\"Id\":" << targetId << ",\"Page\":" << page << "}";
    return invoke("GetComments", oss.str());
}

bool ApiClient::prefetchImage(const std::string& imageUrl) {
    if (imageUrl.empty()) return false;
    std::string fullUrl = core::Utils::absoluteUrl(m_server, imageUrl);
    std::string dest = core::Config::getCacheDir() + "/images/" + core::Utils::stableCacheName(fullUrl, ".img");
    if (std::filesystem::exists(dest)) {
        core::Utils::touch(dest);
        return true;
    }
    bool strictTls = m_config->getBool("strict_tls", false);
    return HttpTransport::downloadFile(fullUrl, dest, 30, strictTls);
}

bool ApiClient::prefetchFont(const std::string& fontUrl) {
    if (fontUrl.empty()) return false;
    std::string fullUrl = core::Utils::absoluteUrl(m_server, fontUrl);
    std::string dest = core::Config::getCacheDir() + "/fonts/" + core::Utils::stableCacheName(fullUrl, ".font");
    if (std::filesystem::exists(dest)) {
        core::Utils::touch(dest);
        return true;
    }
    bool strictTls = m_config->getBool("strict_tls", false);
    return HttpTransport::downloadFile(fullUrl, dest, 30, strictTls);
}

void ApiClient::pruneCacheIfNeeded() {
    int limitMb = m_config->getInt("cache_limit_mb", 512);
    uint64_t limitBytes = static_cast<uint64_t>(limitMb) * 1024 * 1024;
    std::string cacheDir = core::Config::getCacheDir();
    core::Utils::pruneCache(cacheDir + "/chapters", limitBytes / 3);
    core::Utils::pruneCache(cacheDir + "/images", limitBytes / 3);
    core::Utils::pruneCache(cacheDir + "/fonts", limitBytes / 3);
}

} // namespace kinnovel::network
