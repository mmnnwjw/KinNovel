#pragma once

#include "kinnovel/network/HttpTransport.hpp"

#include <string>
#include <vector>
#include <map>
#include <mutex>
#include <memory>
#include <cstdint>

namespace kinnovel::network {

constexpr size_t MAX_WEBSOCKET_MESSAGE_BYTES = 32 * 1024 * 1024;

class WebSocketClient {
public:
    WebSocketClient(std::string url,
                    std::map<std::string, std::string> headers = {},
                    int timeoutSeconds = 30,
                    bool strictTls = false);

    virtual ~WebSocketClient();

    virtual void connect();
    virtual void sendText(const std::string& text);
    virtual void sendPong(const std::vector<uint8_t>& data);
    virtual std::pair<int, std::vector<uint8_t>> receive();
    virtual void close();

    bool isConnected() const { return m_connected; }

private:
    std::string m_url;
    std::map<std::string, std::string> m_headers;
    int m_timeoutSeconds;
    bool m_strictTls;

    int m_sockfd = -1;
    void* m_sslCtx = nullptr; // SSL_CTX*
    void* m_ssl = nullptr;    // SSL*
    bool m_isSecure = false;
    bool m_connected = false;

    std::vector<uint8_t> m_buffer;
    std::mutex m_sendMutex;

    void recvExact(uint8_t* dest, size_t count);
    void sendFrame(uint8_t opcode, const uint8_t* payload, size_t length);
    void sendRaw(const uint8_t* data, size_t len);
    size_t recvRaw(uint8_t* buf, size_t len);
};

} // namespace kinnovel::network
