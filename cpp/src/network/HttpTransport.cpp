#include "kinnovel/network/HttpTransport.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Logger.hpp"

#include <curl/curl.h>
#include <zlib.h>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <cstring>

namespace kinnovel::network {

std::vector<uint8_t> gunzipLimited(const uint8_t* data, size_t size, size_t limit) {
    if (!data || size == 0) return {};

    z_stream strm;
    std::memset(&strm, 0, sizeof(strm));
    strm.next_in = const_cast<Bytef*>(data);
    strm.avail_in = static_cast<uInt>(size);

    // 16 + MAX_WBITS enables automatic gzip decoding
    if (inflateInit2(&strm, 16 + MAX_WBITS) != Z_OK) {
        throw TransportError("Failed to initialize zlib for gzip decompression");
    }

    std::vector<uint8_t> out;
    out.reserve(std::min(limit, size * 4));
    uint8_t chunk[32768];

    int ret = Z_OK;
    while (ret != Z_STREAM_END) {
        strm.next_out = chunk;
        strm.avail_out = sizeof(chunk);

        ret = inflate(&strm, Z_NO_FLUSH);
        if (ret != Z_OK && ret != Z_STREAM_END) {
            inflateEnd(&strm);
            throw TransportError("Gzip decompression error: " + std::to_string(ret));
        }

        size_t have = sizeof(chunk) - strm.avail_out;
        if (out.size() + have > limit) {
            inflateEnd(&strm);
            throw TransportError("gzip 响应超过 8MB 上限");
        }
        out.insert(out.end(), chunk, chunk + have);

        if (ret == Z_OK && strm.avail_in == 0 && strm.avail_out > 0) {
            break;
        }
    }

    inflateEnd(&strm);
    return out;
}

std::string base64Decode(const std::string& in) {
    static const int b64_table[256] = {
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,62,-1,-1,-1,63,
        52,53,54,55,56,57,58,59,60,61,-1,-1,-1, 0,-1,-1,
        -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,10,11,12,13,14,
        15,16,17,18,19,20,21,22,23,24,25,-1,-1,-1,-1,-1,
        -1,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,
        41,42,43,44,45,46,47,48,49,50,51,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1
    };

    std::string out;
    out.reserve(in.size() * 3 / 4);

    int val = 0;
    int valb = -8;
    for (unsigned char c : in) {
        if (std::isspace(c)) continue;
        if (c == '=') break;
        if (b64_table[c] == -1) continue;
        val = (val << 6) + b64_table[c];
        valb += 6;
        if (valb >= 0) {
            out.push_back(static_cast<char>((val >> valb) & 0xFF));
            valb -= 8;
        }
    }
    return out;
}

namespace {

size_t writeCallback(void* contents, size_t size, size_t nmemb, void* userp) {
    size_t total = size * nmemb;
    auto* s = static_cast<std::string*>(userp);
    s->append(static_cast<const char*>(contents), total);
    return total;
}

size_t headerCallback(char* buffer, size_t size, size_t nitems, void* userdata) {
    size_t total = size * nitems;
    auto* headers = static_cast<std::map<std::string, std::string>*>(userdata);
    std::string line(buffer, total);
    size_t colon = line.find(':');
    if (colon != std::string::npos) {
        std::string key = core::Utils::trim(line.substr(0, colon));
        std::string val = core::Utils::trim(line.substr(colon + 1));
        (*headers)[key] = val;
    }
    return total;
}

} // namespace

void HttpTransport::globalInit() {
    curl_global_init(CURL_GLOBAL_DEFAULT);
}

void HttpTransport::globalCleanup() {
    curl_global_cleanup();
}

HttpResponse HttpTransport::request(const std::string& url,
                                    const std::string& method,
                                    const std::string& body,
                                    const std::map<std::string, std::string>& headers,
                                    int timeoutSeconds,
                                    bool strictTls) {
    CURL* curl = curl_easy_init();
    if (!curl) {
        throw TransportError("Failed to initialize libcurl");
    }

    HttpResponse response;

    curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, static_cast<long>(timeoutSeconds));
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 15L);
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);

    if (method == "POST") {
        curl_easy_setopt(curl, CURLOPT_POST, 1L);
        curl_easy_setopt(curl, CURLOPT_POSTFIELDS, body.c_str());
        curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE, static_cast<long>(body.size()));
    } else if (method == "GET") {
        curl_easy_setopt(curl, CURLOPT_HTTPGET, 1L);
    } else {
        curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, method.c_str());
        if (!body.empty()) {
            curl_easy_setopt(curl, CURLOPT_POSTFIELDS, body.c_str());
            curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE, static_cast<long>(body.size()));
        }
    }

    if (!strictTls) {
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 0L);
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 0L);
    } else {
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 1L);
        curl_easy_setopt(curl, CURLOPT_SSL_VERIFYHOST, 2L);

        // Check for bundled CA certs
        const std::vector<std::string> caPaths = {
            "res/cacert.pem",
            "/mnt/us/extensions/kinnovel/res/cacert.pem",
            "/etc/ssl/certs/ca-certificates.crt"
        };
        for (const auto& cap : caPaths) {
            if (std::filesystem::exists(cap)) {
                curl_easy_setopt(curl, CURLOPT_CAINFO, cap.c_str());
                break;
            }
        }
    }

    struct curl_slist* chunk = nullptr;
    for (const auto& [k, v] : headers) {
        std::string headerLine = k + ": " + v;
        chunk = curl_slist_append(chunk, headerLine.c_str());
    }
    if (chunk) {
        curl_easy_setopt(curl, CURLOPT_HTTPHEADER, chunk);
    }

    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, writeCallback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &response.body);
    curl_easy_setopt(curl, CURLOPT_HEADERFUNCTION, headerCallback);
    curl_easy_setopt(curl, CURLOPT_HEADERDATA, &response.headers);

    CURLcode res = curl_easy_perform(curl);
    if (res != CURLE_OK) {
        if (chunk) curl_slist_free_all(chunk);
        std::string errStr = curl_easy_strerror(res);
        curl_easy_cleanup(curl);
        throw TransportError("HTTP request failed: " + errStr);
    }

    long httpCode = 0;
    curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &httpCode);
    response.statusCode = static_cast<int>(httpCode);

    if (chunk) curl_slist_free_all(chunk);
    curl_easy_cleanup(curl);

    return response;
}

bool HttpTransport::downloadFile(const std::string& url,
                                 const std::string& destinationPath,
                                 int timeoutSeconds,
                                 bool strictTls) {
    try {
        auto resp = request(url, "GET", "", {{"User-Agent", "KinNovel/0.1"}}, timeoutSeconds, strictTls);
        if (resp.statusCode != 200 || resp.body.empty()) {
            return false;
        }
        return core::Utils::atomicWrite(destinationPath, resp.body);
    } catch (...) {
        return false;
    }
}

} // namespace kinnovel::network
