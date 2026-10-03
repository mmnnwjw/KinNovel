#pragma once

#include <string>
#include <vector>
#include <cstdint>
#include <optional>

namespace kinnovel::core {

class Utils {
public:
    static bool atomicWrite(const std::string& path, const std::string& data);
    static bool atomicWrite(const std::string& path, const uint8_t* data, size_t size);

    static std::string stableCacheName(const std::string& value, const std::string& suffix = "");
    static std::string absoluteUrl(const std::string& base, const std::string& value);

    static std::string urlEncode(const std::string& value);
    static std::string urlDecode(const std::string& value);
    static std::string formatBytes(uint64_t bytes);
    static std::string formatTime(const std::string& isoValue);

    static std::string trim(const std::string& s);
    static std::vector<std::string> split(const std::string& s, char delimiter);

    static void touch(const std::string& path);
    static uint64_t directorySize(const std::string& path);
    static uint64_t pruneCache(const std::string& path, uint64_t limitBytes);
    static void clearCache(const std::string& path);

    static std::optional<int> batteryLevel();
    static void ensureDirectories(const std::string& appDir);
};

// Free function forwarders for backward compatibility
inline bool atomicWrite(const std::string& p, const std::string& d) { return Utils::atomicWrite(p, d); }
inline bool atomicWrite(const std::string& p, const uint8_t* d, size_t s) { return Utils::atomicWrite(p, d, s); }
inline std::string stableCacheName(const std::string& v, const std::string& s = "") { return Utils::stableCacheName(v, s); }
inline std::string absoluteUrl(const std::string& b, const std::string& v) { return Utils::absoluteUrl(b, v); }
inline std::string formatTime(const std::string& v) { return Utils::formatTime(v); }
inline void touch(const std::string& p) { Utils::touch(p); }
inline uint64_t directorySize(const std::string& p) { return Utils::directorySize(p); }
inline uint64_t pruneCache(const std::string& p, uint64_t l) { return Utils::pruneCache(p, l); }
inline void clearCache(const std::string& p) { Utils::clearCache(p); }
inline std::optional<int> batteryLevel() { return Utils::batteryLevel(); }
inline void ensureDirectories(const std::string& a) { Utils::ensureDirectories(a); }

} // namespace kinnovel::core
