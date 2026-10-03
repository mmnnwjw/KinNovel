#include "kinnovel/network/SignalRClient.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Logger.hpp"
#include "yyjson.h"

#include <openssl/rand.h>
#include <chrono>
#include <thread>
#include <sstream>
#include <iomanip>
#include <algorithm>

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

} // namespace

SignalRClient::SignalRClient(std::string server,
                             TokenProvider tokenProvider,
                             bool strictTls,
                             int requestLimit,
                             double requestWindow,
                             int timeout,
                             std::string visitorId)
    : m_server(std::move(server)),
      m_tokenProvider(std::move(tokenProvider)),
      m_strictTls(strictTls),
      m_timeout(timeout),
      m_rateLimit(requestLimit, requestWindow),
      m_visitorId(visitorId.empty() ? randomHex(16) : std::move(visitorId)) {
    while (!m_server.empty() && m_server.back() == '/') {
        m_server.pop_back();
    }
}

SignalRClient::~SignalRClient() {
    close();
}

void SignalRClient::setServer(const std::string& server) {
    std::lock_guard<std::recursive_mutex> lock(m_lock);
    closeLocked();
    m_server = server;
    while (!m_server.empty() && m_server.back() == '/') {
        m_server.pop_back();
    }
}

std::unique_ptr<WebSocketClient> SignalRClient::createWebSocket(
    const std::string& url,
    const std::map<std::string, std::string>& headers,
    int timeoutSeconds,
    bool strictTls) {
    return std::make_unique<WebSocketClient>(url, headers, timeoutSeconds, strictTls);
}

std::string SignalRClient::negotiate(const std::string& token) {
    std::string url = m_server + "/hub/api/negotiate?negotiateVersion=1";
    std::map<std::string, std::string> headers = {
        {"x-id", m_visitorId},
        {"Accept", "application/json"},
        {"User-Agent", "KinNovel/0.1"}
    };
    if (!token.empty()) {
        headers["Authorization"] = "Bearer " + token;
    }

    HttpResponse resp;
    try {
        resp = HttpTransport::request(url, "POST", "", headers, m_timeout, m_strictTls);
    } catch (const TransportError& exc) {
        throw TransportError("Hub 协商网络错误: " + std::string(exc.what()));
    }

    if (resp.statusCode >= 500) {
        throw TransportError("Hub 协商失败 (" + std::to_string(resp.statusCode) + ")");
    }
    if (resp.statusCode >= 400) {
        throw ApiError(resp.body.empty() ? ("Hub 协商失败 (" + std::to_string(resp.statusCode) + ")") : resp.body,
                       resp.statusCode);
    }

    yyjson_doc* doc = yyjson_read(resp.body.data(), resp.body.size(), 0);
    if (!doc) {
        throw TransportError("Hub 协商返回非 JSON 数据");
    }
    yyjson_val* root = yyjson_doc_get_root(doc);
    yyjson_val* tokenVal = yyjson_obj_get(root, "connectionToken");
    if (!tokenVal || !yyjson_is_str(tokenVal)) {
        yyjson_doc_free(doc);
        throw TransportError("Hub 协商响应缺少 connectionToken");
    }
    std::string connectionToken = yyjson_get_str(tokenVal);
    yyjson_doc_free(doc);
    return connectionToken;
}

void SignalRClient::closeLocked() {
    if (m_socket) {
        m_socket->close();
        m_socket.reset();
    }
    m_recordBuffer.clear();
}

void SignalRClient::close() {
    std::lock_guard<std::recursive_mutex> lock(m_lock);
    closeLocked();
}

void SignalRClient::connectLocked() {
    closeLocked();
    std::string token = m_tokenProvider ? m_tokenProvider() : "";
    std::string connectionToken = negotiate(token);

    std::string scheme = (m_server.rfind("https://", 0) == 0) ? "wss://" : "ws://";
    size_t hostStart = (m_server.rfind("https://", 0) == 0) ? 8 : 7;
    std::string host = m_server.substr(hostStart);

    std::string wsUrl = scheme + host + "/hub/api?id=" + core::Utils::urlEncode(connectionToken);
    if (!token.empty()) {
        wsUrl += "&access_token=" + core::Utils::urlEncode(token);
    }

    std::map<std::string, std::string> wsHeaders = {
        {"Origin", "https://www.lightnovel.app"}
    };

    m_socket = createWebSocket(wsUrl, wsHeaders, m_timeout, m_strictTls);
    try {
        m_socket->connect();
    } catch (const std::exception& exc) {
        closeLocked();
        throw TransportError("SignalR 连接失败: " + std::string(exc.what()));
    }

    // Handshake
    m_socket->sendText("{\"protocol\":\"json\",\"version\":1}\x1e");

    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(m_timeout);
    while (std::chrono::steady_clock::now() < deadline) {
        auto msgs = receiveMessages(m_socket.get());
        for (const auto& msg : msgs) {
            if (msg == "{}") {
                return; // Handshake successful
            }
        }
    }

    closeLocked();
    throw TransportError("SignalR 握手超时");
}

void SignalRClient::ensureConnectedLocked() {
    if (!m_socket || !m_socket->isConnected()) {
        connectLocked();
    }
}

std::vector<std::string> SignalRClient::receiveMessages(WebSocketClient* ws) {
    if (!ws) return {};
    auto [opcode, data] = ws->receive();
    m_recordBuffer.insert(m_recordBuffer.end(), data.begin(), data.end());

    if (m_recordBuffer.size() > MAX_SIGNALR_RECORD_BYTES) {
        closeLocked();
        throw TransportError("SignalR 记录超过 64MB");
    }

    std::vector<std::string> messages;
    while (true) {
        auto it = std::find(m_recordBuffer.begin(), m_recordBuffer.end(), 0x1E);
        if (it == m_recordBuffer.end()) {
            break;
        }

        std::string raw(m_recordBuffer.begin(), it);
        m_recordBuffer.erase(m_recordBuffer.begin(), it + 1);

        raw = core::Utils::trim(raw);
        if (!raw.empty()) {
            messages.push_back(std::move(raw));
        }
    }
    return messages;
}

void SignalRClient::handleServerMessage(const std::string& msgJson) {
    yyjson_doc* doc = yyjson_read(msgJson.data(), msgJson.size(), 0);
    if (!doc) return;
    yyjson_val* root = yyjson_doc_get_root(doc);
    if (yyjson_is_obj(root)) {
        yyjson_val* typeVal = yyjson_obj_get(root, "type");
        if (typeVal && yyjson_is_int(typeVal)) {
            int type = yyjson_get_int(typeVal);
            if (type == 1) {
                m_notifications.push_back(msgJson);
                if (m_notifications.size() > 100) {
                    m_notifications.pop_front();
                }
            } else if (type == 6) {
                // Ping -> reply Pong
                if (m_socket && m_socket->isConnected()) {
                    try {
                        m_socket->sendText("{\"type\":6}\x1e");
                    } catch (...) {}
                }
            }
        }
    }
    yyjson_doc_free(doc);
}

std::string SignalRClient::decodeResponse(const std::string& value) {
    try {
        std::string rawBytes = base64Decode(value);
        if (!rawBytes.empty()) {
            auto unzipped = gunzipLimited(reinterpret_cast<const uint8_t*>(rawBytes.data()), rawBytes.size());
            if (!unzipped.empty()) {
                return std::string(reinterpret_cast<const char*>(unzipped.data()), unzipped.size());
            }
        }
    } catch (...) {}
    return value;
}

std::string SignalRClient::invokeOnce(const std::string& invocationJson,
                                      const std::string& invocationId,
                                      const std::string& method,
                                      int timeoutSeconds) {
    std::lock_guard<std::recursive_mutex> lock(m_lock);
    ensureConnectedLocked();

    m_socket->sendText(invocationJson + "\x1e");

    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(timeoutSeconds);
    while (std::chrono::steady_clock::now() < deadline) {
        std::vector<std::string> messages;
        try {
            messages = receiveMessages(m_socket.get());
        } catch (const std::exception& exc) {
            closeLocked();
            throw TransportError(exc.what());
        }

        for (const auto& msgJson : messages) {
            handleServerMessage(msgJson);

            yyjson_doc* doc = yyjson_read(msgJson.data(), msgJson.size(), 0);
            if (!doc) continue;
            yyjson_val* root = yyjson_doc_get_root(doc);

            yyjson_val* idVal = yyjson_obj_get(root, "invocationId");
            if (!idVal || !yyjson_is_str(idVal) || yyjson_get_str(idVal) != invocationId) {
                yyjson_doc_free(doc);
                continue;
            }

            int type = yyjson_get_int(yyjson_obj_get(root, "type"));
            if (type == 3) {
                yyjson_val* errVal = yyjson_obj_get(root, "error");
                if (errVal && yyjson_is_str(errVal)) {
                    std::string errStr = yyjson_get_str(errVal);
                    yyjson_doc_free(doc);
                    std::string errLower = errStr;
                    for (char& c : errLower) c = std::tolower(static_cast<unsigned char>(c));
                    if (errLower.find("unauthorized") != std::string::npos) {
                        throw ApiError(errStr, 401);
                    }
                    throw ApiError(errStr, 500);
                }

                yyjson_val* resultVal = yyjson_obj_get(root, "result");
                if (resultVal && yyjson_is_obj(resultVal)) {
                    yyjson_val* succVal = yyjson_obj_get(resultVal, "success");
                    if (!succVal) succVal = yyjson_obj_get(resultVal, "Success");
                    bool success = succVal ? yyjson_get_bool(succVal) : true;

                    if (!success) {
                        yyjson_val* msgVal = yyjson_obj_get(resultVal, "msg");
                        if (!msgVal) msgVal = yyjson_obj_get(resultVal, "Msg");
                        std::string errorMsg = msgVal && yyjson_is_str(msgVal) ? yyjson_get_str(msgVal) : "请求失败";

                        yyjson_val* stVal = yyjson_obj_get(resultVal, "status");
                        if (!stVal) stVal = yyjson_obj_get(resultVal, "Status");
                        int status = stVal ? yyjson_get_int(stVal) : 500;

                        yyjson_doc_free(doc);
                        throw ApiError(errorMsg, status);
                    }

                    yyjson_val* respVal = yyjson_obj_get(resultVal, "response");
                    if (!respVal) respVal = yyjson_obj_get(resultVal, "Response");

                    std::string finalStr;
                    if (respVal && yyjson_is_str(respVal)) {
                        finalStr = decodeResponse(yyjson_get_str(respVal));
                    } else if (respVal) {
                        char* jsonStr = yyjson_val_write(respVal, 0, nullptr);
                        if (jsonStr) {
                            finalStr = jsonStr;
                            free(jsonStr);
                        }
                    }
                    yyjson_doc_free(doc);
                    return finalStr;
                }

                // If result is direct value
                std::string finalStr;
                if (resultVal && yyjson_is_str(resultVal)) {
                    finalStr = decodeResponse(yyjson_get_str(resultVal));
                } else if (resultVal) {
                    char* jsonStr = yyjson_val_write(resultVal, 0, nullptr);
                    if (jsonStr) {
                        finalStr = jsonStr;
                        free(jsonStr);
                    }
                }
                yyjson_doc_free(doc);
                return finalStr;
            }
            yyjson_doc_free(doc);
        }
    }

    throw TransportError("Hub 调用超时: " + method);
}

std::string SignalRClient::invoke(const std::string& method,
                                  const std::string& paramsJson,
                                  bool useGzip,
                                  int timeoutSeconds) {
    int timeout = (timeoutSeconds > 0) ? timeoutSeconds : m_timeout * 2;
    m_rateLimit.wait();

    std::string invocationId = randomHex(16);
    std::string gzipBoolStr = useGzip ? "true" : "false";
    std::string invocation = "{\"type\":1,\"invocationId\":\"" + invocationId +
                             "\",\"target\":\"" + method +
                             "\",\"arguments\":[" + (paramsJson.empty() ? "{}" : paramsJson) +
                             ",{\"UseGzip\":" + gzipBoolStr + "}]}";

    std::exception_ptr lastError = nullptr;
    for (int attempt = 0; attempt < 2; ++attempt) {
        try {
            return invokeOnce(invocation, invocationId, method, timeout);
        } catch (const ApiError&) {
            throw; // Business error, do not retry
        } catch (const TransportError& exc) {
            lastError = std::current_exception();
            {
                std::lock_guard<std::recursive_mutex> lock(m_lock);
                closeLocked();
            }
            if (attempt == 0) {
                std::this_thread::sleep_for(std::chrono::milliseconds(1000));
                continue;
            }
            std::rethrow_exception(lastError);
        }
    }

    if (lastError) {
        std::rethrow_exception(lastError);
    }
    throw TransportError("Hub 调用失败: " + method);
}

} // namespace kinnovel::network
