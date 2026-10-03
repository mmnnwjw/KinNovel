#include "kinnovel/network/WebSocketClient.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Logger.hpp"

#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/sha.h>
#include <openssl/rand.h>

#include <sys/types.h>
#include <sys/socket.h>
#include <netdb.h>
#include <unistd.h>
#include <fcntl.h>
#include <poll.h>
#include <cstring>
#include <sstream>
#include <filesystem>

namespace kinnovel::network {

namespace {

std::string base64Encode(const uint8_t* data, size_t len) {
    static const char* tbl = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string out;
    out.reserve(((len + 2) / 3) * 4);

    for (size_t i = 0; i < len; i += 3) {
        uint32_t b = (data[i] << 16);
        if (i + 1 < len) b |= (data[i + 1] << 8);
        if (i + 2 < len) b |= data[i + 2];

        out.push_back(tbl[(b >> 18) & 0x3F]);
        out.push_back(tbl[(b >> 12) & 0x3F]);
        out.push_back((i + 1 < len) ? tbl[(b >> 6) & 0x3F] : '=');
        out.push_back((i + 2 < len) ? tbl[b & 0x3F] : '=');
    }
    return out;
}

void parseWsUrl(const std::string& url, bool& secure, std::string& host, int& port, std::string& path) {
    secure = false;
    size_t schemePos = url.find("://");
    if (schemePos == std::string::npos) {
        throw TransportError("Invalid WebSocket URL: " + url);
    }

    std::string scheme = url.substr(0, schemePos);
    if (scheme == "wss") {
        secure = true;
        port = 443;
    } else if (scheme == "ws") {
        secure = false;
        port = 80;
    } else {
        throw TransportError("Unknown WebSocket scheme: " + scheme);
    }

    size_t hostStart = schemePos + 3;
    size_t pathStart = url.find('/', hostStart);
    std::string hostPort;
    if (pathStart == std::string::npos) {
        hostPort = url.substr(hostStart);
        path = "/";
    } else {
        hostPort = url.substr(hostStart, pathStart - hostStart);
        path = url.substr(pathStart);
    }

    size_t colon = hostPort.find(':');
    if (colon != std::string::npos) {
        host = hostPort.substr(0, colon);
        port = std::stoi(hostPort.substr(colon + 1));
    } else {
        host = hostPort;
    }
}

} // namespace

WebSocketClient::WebSocketClient(std::string url,
                                 std::map<std::string, std::string> headers,
                                 int timeoutSeconds,
                                 bool strictTls)
    : m_url(std::move(url)),
      m_headers(std::move(headers)),
      m_timeoutSeconds(timeoutSeconds),
      m_strictTls(strictTls) {
}

WebSocketClient::~WebSocketClient() {
    close();
}

void WebSocketClient::connect() {
    close();

    std::string host;
    int port = 80;
    std::string path;
    parseWsUrl(m_url, m_isSecure, host, port, path);

    // Resolve address
    struct addrinfo hints;
    std::memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    struct addrinfo* res = nullptr;
    std::string portStr = std::to_string(port);
    int rc = getaddrinfo(host.c_str(), portStr.c_str(), &hints, &res);
    if (rc != 0 || !res) {
        throw TransportError("Failed to resolve host: " + host);
    }

    m_sockfd = socket(res->ai_family, res->ai_socktype, res->ai_protocol);
    if (m_sockfd < 0) {
        freeaddrinfo(res);
        throw TransportError("Failed to create socket");
    }

    // Set non-blocking for connect timeout
    int flags = fcntl(m_sockfd, F_GETFL, 0);
    fcntl(m_sockfd, F_SETFL, flags | O_NONBLOCK);

    int connRes = ::connect(m_sockfd, res->ai_addr, res->ai_addrlen);
    if (connRes < 0 && errno == EINPROGRESS) {
        struct pollfd pfd;
        pfd.fd = m_sockfd;
        pfd.events = POLLOUT;
        int p = poll(&pfd, 1, m_timeoutSeconds * 1000);
        if (p <= 0) {
            freeaddrinfo(res);
            close();
            throw TransportError("WebSocket connection timeout: " + host);
        }
        int so_error = 0;
        socklen_t len = sizeof(so_error);
        getsockopt(m_sockfd, SOL_SOCKET, SO_ERROR, &so_error, &len);
        if (so_error != 0) {
            freeaddrinfo(res);
            close();
            throw TransportError("WebSocket connection failed: " + std::string(strerror(so_error)));
        }
    } else if (connRes < 0) {
        freeaddrinfo(res);
        close();
        throw TransportError("WebSocket connection failed: " + std::string(strerror(errno)));
    }
    freeaddrinfo(res);

    // Revert to blocking mode with timeout
    fcntl(m_sockfd, F_SETFL, flags);
    struct timeval tv;
    tv.tv_sec = m_timeoutSeconds;
    tv.tv_usec = 0;
    setsockopt(m_sockfd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(m_sockfd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));

    if (m_isSecure) {
        SSL_CTX* ctx = SSL_CTX_new(TLS_client_method());
        if (!ctx) {
            close();
            throw TransportError("Failed to create SSL context");
        }
        m_sslCtx = ctx;

        if (!m_strictTls) {
            SSL_CTX_set_verify(ctx, SSL_VERIFY_NONE, nullptr);
        } else {
            SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, nullptr);
            const std::vector<std::string> caPaths = {
                "res/cacert.pem",
                "/mnt/us/extensions/kinnovel/res/cacert.pem",
                "/etc/ssl/certs/ca-certificates.crt"
            };
            for (const auto& cap : caPaths) {
                if (std::filesystem::exists(cap)) {
                    SSL_CTX_load_verify_locations(ctx, cap.c_str(), nullptr);
                    break;
                }
            }
        }

        SSL* ssl = SSL_new(ctx);
        if (!ssl) {
            close();
            throw TransportError("Failed to create SSL object");
        }
        m_ssl = ssl;

        SSL_set_tlsext_host_name(ssl, host.c_str());
        SSL_set_fd(ssl, m_sockfd);

        if (SSL_connect(ssl) <= 0) {
            close();
            throw TransportError("TLS handshake failed for: " + host);
        }
    }

    // Generate Sec-WebSocket-Key
    uint8_t randBytes[16];
    RAND_bytes(randBytes, sizeof(randBytes));
    std::string wsKey = base64Encode(randBytes, sizeof(randBytes));

    // Send HTTP upgrade request
    std::ostringstream req;
    req << "GET " << path << " HTTP/1.1\r\n";
    req << "Host: " << host << ":" << port << "\r\n";
    req << "Upgrade: websocket\r\n";
    req << "Connection: Upgrade\r\n";
    req << "Sec-WebSocket-Key: " << wsKey << "\r\n";
    req << "Sec-WebSocket-Version: 13\r\n";
    req << "User-Agent: KinNovel/0.1\r\n";
    for (const auto& [k, v] : m_headers) {
        req << k << ": " << v << "\r\n";
    }
    req << "\r\n";

    std::string reqStr = req.str();
    sendRaw(reinterpret_cast<const uint8_t*>(reqStr.data()), reqStr.size());

    // Read HTTP response headers
    std::string response;
    uint8_t buf[1024];
    while (response.find("\r\n\r\n") == std::string::npos) {
        size_t n = recvRaw(buf, sizeof(buf));
        if (n == 0) {
            close();
            throw TransportError("WebSocket handshake connection closed by server");
        }
        response.append(reinterpret_cast<char*>(buf), n);
        if (response.size() > 65536) {
            close();
            throw TransportError("WebSocket handshake response too large");
        }
    }

    size_t headerEnd = response.find("\r\n\r\n");
    std::string statusLine = response.substr(0, response.find("\r\n"));
    if (statusLine.find(" 101 ") == std::string::npos) {
        close();
        throw TransportError("WebSocket handshake failed: " + statusLine);
    }

    // Verify Sec-WebSocket-Accept
    std::string magic = wsKey + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    unsigned char sha[20];
    SHA1(reinterpret_cast<const unsigned char*>(magic.data()), magic.size(), sha);
    std::string expectedAccept = base64Encode(sha, 20);

    std::string headersPart = response.substr(0, headerEnd);
    bool acceptMatched = (headersPart.find(expectedAccept) != std::string::npos);
    if (!acceptMatched) {
        close();
        throw TransportError("WebSocket handshake verification failed: Sec-WebSocket-Accept mismatch");
    }

    // Any remaining bytes after \r\n\r\n belong to the first WebSocket frame
    if (response.size() > headerEnd + 4) {
        const char* extra = response.data() + headerEnd + 4;
        size_t extraLen = response.size() - (headerEnd + 4);
        m_buffer.assign(extra, extra + extraLen);
    }

    m_connected = true;
}

void WebSocketClient::sendRaw(const uint8_t* data, size_t len) {
    size_t total = 0;
    while (total < len) {
        ssize_t n = 0;
        if (m_isSecure) {
            n = SSL_write(static_cast<SSL*>(m_ssl), data + total, static_cast<int>(len - total));
        } else {
            n = ::send(m_sockfd, data + total, len - total, 0);
        }
        if (n <= 0) {
            throw TransportError("Failed to write to socket");
        }
        total += n;
    }
}

size_t WebSocketClient::recvRaw(uint8_t* buf, size_t len) {
    ssize_t n = 0;
    if (m_isSecure) {
        n = SSL_read(static_cast<SSL*>(m_ssl), buf, static_cast<int>(len));
    } else {
        n = ::recv(m_sockfd, buf, len, 0);
    }
    if (n < 0) {
        throw TransportError("Socket read error / timeout: " + std::string(strerror(errno)));
    }
    return static_cast<size_t>(n);
}

void WebSocketClient::recvExact(uint8_t* dest, size_t count) {
    size_t filled = 0;
    if (!m_buffer.empty()) {
        size_t take = std::min(count, m_buffer.size());
        std::memcpy(dest, m_buffer.data(), take);
        m_buffer.erase(m_buffer.begin(), m_buffer.begin() + take);
        filled += take;
    }

    while (filled < count) {
        size_t n = recvRaw(dest + filled, count - filled);
        if (n == 0) {
            close();
            throw TransportError("WebSocket connection closed");
        }
        filled += n;
    }
}

void WebSocketClient::sendFrame(uint8_t opcode, const uint8_t* payload, size_t length) {
    if (!m_connected || m_sockfd < 0) {
        throw TransportError("WebSocket not connected");
    }

    std::vector<uint8_t> frame;
    frame.reserve(14 + length);

    // FIN = 1 | opcode
    frame.push_back(0x80 | (opcode & 0x0F));

    // Client frames must be masked (bit 7 = 1)
    if (length < 126) {
        frame.push_back(0x80 | static_cast<uint8_t>(length));
    } else if (length <= 0xFFFF) {
        frame.push_back(0x80 | 126);
        frame.push_back(static_cast<uint8_t>((length >> 8) & 0xFF));
        frame.push_back(static_cast<uint8_t>(length & 0xFF));
    } else {
        frame.push_back(0x80 | 127);
        for (int i = 7; i >= 0; --i) {
            frame.push_back(static_cast<uint8_t>((length >> (i * 8)) & 0xFF));
        }
    }

    uint8_t mask[4];
    RAND_bytes(mask, 4);
    frame.insert(frame.end(), mask, mask + 4);

    for (size_t i = 0; i < length; ++i) {
        frame.push_back(payload[i] ^ mask[i % 4]);
    }

    std::lock_guard<std::mutex> lock(m_sendMutex);
    sendRaw(frame.data(), frame.size());
}

void WebSocketClient::sendText(const std::string& text) {
    sendFrame(0x1, reinterpret_cast<const uint8_t*>(text.data()), text.size());
}

void WebSocketClient::sendPong(const std::vector<uint8_t>& data) {
    sendFrame(0xA, data.data(), data.size());
}

std::pair<int, std::vector<uint8_t>> WebSocketClient::receive() {
    std::vector<uint8_t> fragments;
    int messageOpcode = -1;

    while (true) {
        uint8_t head[2];
        recvExact(head, 2);

        bool fin = (head[0] & 0x80) != 0;
        int opcode = head[0] & 0x0F;
        bool masked = (head[1] & 0x80) != 0;
        uint64_t len = head[1] & 0x7F;

        if (len == 126) {
            uint8_t lenBuf[2];
            recvExact(lenBuf, 2);
            len = (static_cast<uint64_t>(lenBuf[0]) << 8) | lenBuf[1];
        } else if (len == 127) {
            uint8_t lenBuf[8];
            recvExact(lenBuf, 8);
            len = 0;
            for (int i = 0; i < 8; ++i) {
                len = (len << 8) | lenBuf[i];
            }
        }

        if (len > MAX_WEBSOCKET_MESSAGE_BYTES) {
            close();
            throw TransportError("WebSocket 消息超过 32MB");
        }

        uint8_t mask[4] = {0};
        if (masked) {
            recvExact(mask, 4);
        }

        std::vector<uint8_t> payload(len);
        if (len > 0) {
            recvExact(payload.data(), len);
            if (masked) {
                for (size_t i = 0; i < len; ++i) {
                    payload[i] ^= mask[i % 4];
                }
            }
        }

        if (opcode == 0x8) { // Close
            close();
            throw TransportError("WebSocket 已被服务器关闭");
        }
        if (opcode == 0x9) { // Ping
            sendPong(payload);
            continue;
        }
        if (opcode == 0xA) { // Pong
            continue;
        }

        if (opcode == 0x1 || opcode == 0x2) {
            messageOpcode = opcode;
            fragments = std::move(payload);
            if (fragments.size() > MAX_WEBSOCKET_MESSAGE_BYTES) {
                close();
                throw TransportError("WebSocket 消息超过 32MB");
            }
        } else if (opcode == 0x0 && messageOpcode != -1) {
            fragments.insert(fragments.end(), payload.begin(), payload.end());
            if (fragments.size() > MAX_WEBSOCKET_MESSAGE_BYTES) {
                close();
                throw TransportError("WebSocket 消息超过 32MB");
            }
        } else {
            continue;
        }

        if (fin) {
            return { messageOpcode, fragments };
        }
    }
}

void WebSocketClient::close() {
    m_connected = false;
    if (m_ssl) {
        SSL_shutdown(static_cast<SSL*>(m_ssl));
        SSL_free(static_cast<SSL*>(m_ssl));
        m_ssl = nullptr;
    }
    if (m_sslCtx) {
        SSL_CTX_free(static_cast<SSL_CTX*>(m_sslCtx));
        m_sslCtx = nullptr;
    }
    if (m_sockfd >= 0) {
        ::close(m_sockfd);
        m_sockfd = -1;
    }
    m_buffer.clear();
}

} // namespace kinnovel::network
