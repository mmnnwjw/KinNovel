#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Sha256.hpp"
#include "kinnovel/core/Logger.hpp"

#include <filesystem>
#include <fstream>
#include <sstream>
#include <iomanip>
#include <algorithm>
#include <chrono>
#include <mutex>
#include <glob.h>
#include <fcntl.h>
#include <unistd.h>
#include <utime.h>

namespace fs = std::filesystem;

namespace kinnovel::core {

namespace {
std::mutex s_writeLockMutex;
}

bool Utils::atomicWrite(const std::string& path, const uint8_t* data, size_t size) {
    std::lock_guard<std::mutex> lock(s_writeLockMutex);
    try {
        fs::path target(path);
        if (target.has_parent_path()) {
            fs::create_directories(target.parent_path());
        }

        std::string tmpPath = path + ".tmp";
        int fd = ::open(tmpPath.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (fd < 0) return false;

        size_t written = 0;
        while (written < size) {
            ssize_t n = ::write(fd, data + written, size - written);
            if (n <= 0) {
                ::close(fd);
                ::unlink(tmpPath.c_str());
                return false;
            }
            written += n;
        }

        ::fsync(fd);
        ::close(fd);

        std::error_code ec;
        fs::rename(tmpPath, target, ec);
        if (ec) {
            ::unlink(tmpPath.c_str());
            return false;
        }
        return true;
    } catch (...) {
        return false;
    }
}

bool Utils::atomicWrite(const std::string& path, const std::string& data) {
    return atomicWrite(path, reinterpret_cast<const uint8_t*>(data.data()), data.size());
}

std::string Utils::stableCacheName(const std::string& value, const std::string& suffix) {
    std::string h = Sha256::hash(value);
    return h + suffix;
}

std::string Utils::absoluteUrl(const std::string& base, const std::string& value) {
    if (value.empty()) return base;
    if (value.rfind("http://", 0) == 0 || value.rfind("https://", 0) == 0) {
        return value;
    }

    if (value.rfind("//", 0) == 0) {
        size_t protoEnd = base.find("://");
        std::string proto = (protoEnd != std::string::npos) ? base.substr(0, protoEnd + 1) : "https:";
        return proto + value;
    }

    size_t schemePos = base.find("://");
    if (schemePos == std::string::npos) return value;

    size_t hostStart = schemePos + 3;
    size_t hostEnd = base.find('/', hostStart);

    if (value[0] == '/') {
        if (hostEnd == std::string::npos) {
            return base + value;
        }
        return base.substr(0, hostEnd) + value;
    }

    std::string basePath = (hostEnd != std::string::npos) ? base.substr(0, base.rfind('/') + 1) : (base + "/");
    return basePath + value;
}

std::string Utils::urlEncode(const std::string& value) {
    std::ostringstream escaped;
    escaped.fill('0');
    escaped << std::hex;

    for (unsigned char c : value) {
        if (std::isalnum(c) || c == '-' || c == '_' || c == '.' || c == '~') {
            escaped << c;
        } else {
            escaped << '%' << std::setw(2) << std::uppercase << static_cast<int>(c);
        }
    }
    return escaped.str();
}

std::string Utils::urlDecode(const std::string& value) {
    std::string result;
    result.reserve(value.size());
    for (size_t i = 0; i < value.size(); ++i) {
        if (value[i] == '%' && i + 2 < value.size()) {
            int h1 = value[i + 1];
            int h2 = value[i + 2];
            auto fromHex = [](int h) -> int {
                if (h >= '0' && h <= '9') return h - '0';
                if (h >= 'a' && h <= 'f') return h - 'a' + 10;
                if (h >= 'A' && h <= 'F') return h - 'A' + 10;
                return -1;
            };
            int d1 = fromHex(h1);
            int d2 = fromHex(h2);
            if (d1 >= 0 && d2 >= 0) {
                result.push_back(static_cast<char>((d1 << 4) | d2));
                i += 2;
                continue;
            }
        } else if (value[i] == '+') {
            result.push_back(' ');
            continue;
        }
        result.push_back(value[i]);
    }
    return result;
}

std::string Utils::formatBytes(uint64_t bytes) {
    if (bytes < 1024) {
        return std::to_string(bytes) + " B";
    }
    double val = static_cast<double>(bytes);
    const char* units[] = {"KB", "MB", "GB", "TB"};
    int unitIdx = 0;
    while (val >= 1024.0 && unitIdx < 3) {
        val /= 1024.0;
        unitIdx++;
    }
    std::ostringstream oss;
    oss << std::fixed << std::setprecision(1) << val << " " << units[unitIdx - 1];
    return oss.str();
}

std::string Utils::trim(const std::string& s) {
    size_t start = 0;
    while (start < s.size() && std::isspace(static_cast<unsigned char>(s[start]))) {
        start++;
    }
    if (start == s.size()) return "";
    size_t end = s.size();
    while (end > start && std::isspace(static_cast<unsigned char>(s[end - 1]))) {
        end--;
    }
    return s.substr(start, end - start);
}

std::vector<std::string> Utils::split(const std::string& s, char delimiter) {
    std::vector<std::string> tokens;
    std::string token;
    std::istringstream tokenStream(s);
    while (std::getline(tokenStream, token, delimiter)) {
        tokens.push_back(token);
    }
    return tokens;
}

std::string Utils::formatTime(const std::string& isoValue) {
    if (isoValue.size() >= 16) {
        std::string s = isoValue.substr(0, 16);
        std::replace(s.begin(), s.end(), 'T', ' ');
        return s;
    }
    return isoValue;
}

void Utils::touch(const std::string& path) {
    ::utime(path.c_str(), nullptr);
}

uint64_t Utils::directorySize(const std::string& path) {
    uint64_t total = 0;
    try {
        if (!fs::exists(path) || !fs::is_directory(path)) return 0;
        for (const auto& entry : fs::recursive_directory_iterator(path, fs::directory_options::skip_permission_denied)) {
            if (entry.is_regular_file()) {
                total += entry.file_size();
            }
        }
    } catch (...) {}
    return total;
}

uint64_t Utils::pruneCache(const std::string& path, uint64_t limitBytes) {
    struct FileEntry {
        fs::path p;
        uint64_t size;
        fs::file_time_type mtime;
    };

    std::vector<FileEntry> files;
    uint64_t currentSize = 0;

    try {
        if (!fs::exists(path) || !fs::is_directory(path)) return 0;
        for (const auto& entry : fs::recursive_directory_iterator(path, fs::directory_options::skip_permission_denied)) {
            if (entry.is_regular_file()) {
                uint64_t sz = entry.file_size();
                currentSize += sz;
                files.push_back({entry.path(), sz, entry.last_write_time()});
            }
        }
    } catch (...) {}

    if (currentSize <= limitBytes) {
        return currentSize;
    }

    std::sort(files.begin(), files.end(), [](const FileEntry& a, const FileEntry& b) {
        return a.mtime < b.mtime; // Oldest first
    });

    for (const auto& f : files) {
        if (currentSize <= limitBytes) break;
        std::error_code ec;
        fs::remove(f.p, ec);
        if (!ec) {
            currentSize = (currentSize >= f.size) ? (currentSize - f.size) : 0;
        }
    }

    return currentSize;
}

void Utils::clearCache(const std::string& path) {
    try {
        if (fs::exists(path)) {
            for (const auto& entry : fs::directory_iterator(path)) {
                fs::remove_all(entry.path());
            }
        }
    } catch (...) {}
}

std::optional<int> Utils::batteryLevel() {
    static int cachedLevel = -1;
    static auto lastCheck = std::chrono::steady_clock::time_point::min();

    auto now = std::chrono::steady_clock::now();
    if (cachedLevel >= 0 && std::chrono::duration_cast<std::chrono::seconds>(now - lastCheck).count() < 60) {
        return cachedLevel;
    }

    glob_t globResult;
    if (glob("/sys/class/power_supply/*/capacity", GLOB_NOSORT, nullptr, &globResult) == 0) {
        for (size_t i = 0; i < globResult.gl_pathc; ++i) {
            std::ifstream file(globResult.gl_pathv[i]);
            int cap = 0;
            if (file >> cap) {
                cachedLevel = std::max(0, std::min(100, cap));
                lastCheck = now;
                globfree(&globResult);
                return cachedLevel;
            }
        }
        globfree(&globResult);
    }
    return std::nullopt;
}

double Utils::monotonicSeconds() {
    auto now = std::chrono::steady_clock::now().time_since_epoch();
    return std::chrono::duration<double>(now).count();
}

void Utils::ensureDirectories(const std::string& appDir) {
    try {
        fs::create_directories(appDir);
        fs::create_directories(appDir + "/cache");
        fs::create_directories(appDir + "/cache/fonts");
        fs::create_directories(appDir + "/cache/images");
        fs::create_directories(appDir + "/cache/chapters");
        fs::create_directories(appDir + "/data");
    } catch (...) {}
}

} // namespace kinnovel::core
