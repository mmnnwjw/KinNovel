#pragma once

#include "kinnovel/network/RateLimiter.hpp"
#include "kinnovel/network/WebSocketClient.hpp"
#include "kinnovel/network/HttpTransport.hpp"

#include <string>
#include <vector>
#include <deque>
#include <functional>
#include <mutex>
#include <memory>

namespace kinnovel::network {

constexpr size_t MAX_SIGNALR_RECORD_BYTES = 64 * 1024 * 1024;

class SignalRClient {
public:
    using TokenProvider = std::function<std::string()>;

    SignalRClient(std::string server,
                  TokenProvider tokenProvider = nullptr,
                  bool strictTls = false,
                  int requestLimit = 9,
                  double requestWindow = 5.5,
                  int timeout = 30,
                  std::string visitorId = "");

    virtual ~SignalRClient();

    void setServer(const std::string& server);
    std::string invoke(const std::string& method,
                       const std::string& paramsJson = "{}",
                       bool useGzip = true,
                       int timeoutSeconds = -1);

    void close();

    const std::string& getVisitorId() const { return m_visitorId; }
    const std::string& getServer() const { return m_server; }
    RateLimiter& getRateLimiter() { return m_rateLimit; }

    // Factory method for creating WebSocket client (allows mocking in tests)
    virtual std::unique_ptr<WebSocketClient> createWebSocket(
        const std::string& url,
        const std::map<std::string, std::string>& headers,
        int timeoutSeconds,
        bool strictTls);

    virtual std::string negotiate(const std::string& token);

    // Helpers exposed for unit tests
    std::string decodeResponse(const std::string& value);
    std::vector<std::string> receiveMessages(WebSocketClient* ws);

private:
    std::string m_server;
    TokenProvider m_tokenProvider;
    bool m_strictTls;
    int m_timeout;
    RateLimiter m_rateLimit;
    std::string m_visitorId;

    std::recursive_mutex m_lock;
    std::unique_ptr<WebSocketClient> m_socket;
    std::vector<uint8_t> m_recordBuffer;
    std::deque<std::string> m_notifications;

    void connectLocked();
    void closeLocked();
    void ensureConnectedLocked();
    void handleServerMessage(const std::string& msgJson);
    std::string invokeOnce(const std::string& invocationJson,
                           const std::string& invocationId,
                           const std::string& method,
                           int timeoutSeconds);
};

} // namespace kinnovel::network
