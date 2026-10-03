#pragma once

#include <string>
#include <vector>
#include <map>
#include <stdexcept>
#include <cstdint>

namespace kinnovel::network {

class TransportError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

class ApiError : public TransportError {
public:
    ApiError(const std::string& msg, int status = 500)
        : TransportError(msg), m_status(status) {}
    int getStatus() const { return m_status; }
private:
    int m_status;
};

struct HttpResponse {
    int statusCode = 0;
    std::string body;
    std::map<std::string, std::string> headers;
};

std::vector<uint8_t> gunzipLimited(const uint8_t* data, size_t size, size_t limit = 8 * 1024 * 1024);
std::string base64Decode(const std::string& in);

class HttpTransport {
public:
    static void globalInit();
    static void globalCleanup();

    static HttpResponse request(const std::string& url,
                                const std::string& method = "GET",
                                const std::string& body = "",
                                const std::map<std::string, std::string>& headers = {},
                                int timeoutSeconds = 30,
                                bool strictTls = false);

    static bool downloadFile(const std::string& url,
                             const std::string& destinationPath,
                             int timeoutSeconds = 30,
                             bool strictTls = false);
};

} // namespace kinnovel::network
