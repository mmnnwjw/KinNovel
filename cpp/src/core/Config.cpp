#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/core/Logger.hpp"
#include "third_party/yyjson.h"

#include <fstream>
#include <sstream>
#include <filesystem>

namespace fs = std::filesystem;

namespace kinnovel::core {

Config::Config(const std::string& configPath) {
    initDefaults();
    if (!configPath.empty()) {
        m_path = configPath;
    } else {
        m_path = "bin/config.json";
    }
    load();
}

void Config::initDefaults() {
    m_stringMap = {
        {"api_server", "https://api.lightnovel.life"},
        {"account_email", ""},
        {"account_password", ""},
        {"screen_protocol", "auto"},
        {"framebuffer", "/dev/fb0"},
        {"font_path", "/usr/java/lib/fonts/STHeitiMedium.ttf"},
        {"convert", ""}
    };

    m_intMap = {
        {"font_size", 48},
        {"reader_margin", 34},
        {"request_limit", 9},
        {"request_window_ms", 5500},
        {"cache_limit_mb", 192}
    };

    m_doubleMap = {
        {"line_spacing", 1.42}
    };

    m_boolMap = {
        {"page_flash", false},
        {"page_turn_animation", true},
        {"reader_guide_dismissed", false},
        {"night_mode", false},
        {"justify", false},
        {"first_line_indent", true},
        {"ignore_japanese", false},
        {"ignore_ai", false},
        {"prefetch_chapters", false},
        {"strict_tls", true},
        {"check_update", true}
    };

    m_homeOrder = {
        {"shelf", 0},
        {"history", 1},
        {"rank", 2},
        {"browse", 3},
        {"account", 4},
        {"settings", 5},
        {"about", 6},
        {"exit", 7},
        {"announcements", -1},
        {"notifications", -1},
        {"shop", -1}
    };
}

bool Config::load() {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    std::ifstream in(m_path, std::ios::binary);
    if (!in.is_open()) return false;

    std::stringstream buffer;
    buffer << in.rdbuf();
    std::string jsonStr = buffer.str();

    yyjson_doc* doc = yyjson_read(jsonStr.c_str(), jsonStr.size(), 0);
    if (!doc) return false;

    yyjson_val* root = yyjson_doc_get_root(doc);
    if (yyjson_is_obj(root)) {
        // String fields
        for (auto& [key, val] : m_stringMap) {
            yyjson_val* v = yyjson_obj_get(root, key.c_str());
            if (v && yyjson_is_str(v)) {
                val = yyjson_get_str(v);
            }
        }

        // Int fields
        for (auto& [key, val] : m_intMap) {
            yyjson_val* v = yyjson_obj_get(root, key.c_str());
            if (v && yyjson_is_int(v)) {
                val = yyjson_get_int(v);
            }
        }

        // Double fields
        for (auto& [key, val] : m_doubleMap) {
            yyjson_val* v = yyjson_obj_get(root, key.c_str());
            if (v && yyjson_is_num(v)) {
                val = yyjson_get_num(v);
            }
        }

        // Bool fields
        for (auto& [key, val] : m_boolMap) {
            yyjson_val* v = yyjson_obj_get(root, key.c_str());
            if (v && yyjson_is_bool(v)) {
                val = yyjson_get_bool(v);
            }
        }

        // Home order
        yyjson_val* homeOrderVal = yyjson_obj_get(root, "home_order");
        if (homeOrderVal && yyjson_is_obj(homeOrderVal)) {
            yyjson_obj_iter iter;
            yyjson_obj_iter_init(homeOrderVal, &iter);
            yyjson_val* k = nullptr;
            yyjson_val* v = nullptr;
            while ((k = yyjson_obj_iter_next(&iter))) {
                v = yyjson_obj_iter_get_val(k);
                if (yyjson_is_int(v)) {
                    m_homeOrder[yyjson_get_str(k)] = yyjson_get_int(v);
                }
            }
        }
    }

    yyjson_doc_free(doc);
    return true;
}

bool Config::save() {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    yyjson_mut_doc* doc = yyjson_mut_doc_new(nullptr);
    yyjson_mut_val* root = yyjson_mut_obj(doc);
    yyjson_mut_doc_set_root(doc, root);

    for (const auto& [k, v] : m_stringMap) {
        if (k == "convert" && v.empty()) {
            yyjson_mut_obj_add_null(doc, root, k.c_str());
        } else {
            yyjson_mut_obj_add_str(doc, root, k.c_str(), v.c_str());
        }
    }
    for (const auto& [k, v] : m_intMap) {
        yyjson_mut_obj_add_int(doc, root, k.c_str(), v);
    }
    for (const auto& [k, v] : m_doubleMap) {
        yyjson_mut_obj_add_real(doc, root, k.c_str(), v);
    }
    for (const auto& [k, v] : m_boolMap) {
        yyjson_mut_obj_add_bool(doc, root, k.c_str(), v);
    }

    yyjson_mut_val* homeOrderVal = yyjson_mut_obj(doc);
    for (const auto& [k, v] : m_homeOrder) {
        yyjson_mut_obj_add_int(doc, homeOrderVal, k.c_str(), v);
    }
    yyjson_mut_obj_add_val(doc, root, "home_order", homeOrderVal);

    size_t len = 0;
    char* jsonStr = yyjson_mut_write(doc, YYJSON_WRITE_PRETTY, &len);
    bool ok = false;
    if (jsonStr) {
        ok = atomicWrite(m_path, reinterpret_cast<const uint8_t*>(jsonStr), len);
        free(jsonStr);
    }
    yyjson_mut_doc_free(doc);
    return ok;
}

std::string Config::getString(const std::string& key, const std::string& defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_stringMap.find(key);
    return (it != m_stringMap.end()) ? it->second : defaultVal;
}

int Config::getInt(const std::string& key, int defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_intMap.find(key);
    return (it != m_intMap.end()) ? it->second : defaultVal;
}

double Config::getDouble(const std::string& key, double defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_doubleMap.find(key);
    return (it != m_doubleMap.end()) ? it->second : defaultVal;
}

bool Config::getBool(const std::string& key, bool defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_boolMap.find(key);
    return (it != m_boolMap.end()) ? it->second : defaultVal;
}

void Config::setString(const std::string& key, const std::string& value, bool autoSave) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_stringMap[key] = value;
    if (autoSave) save();
}

void Config::setInt(const std::string& key, int value, bool autoSave) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_intMap[key] = value;
    if (autoSave) save();
}

void Config::setDouble(const std::string& key, double value, bool autoSave) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_doubleMap[key] = value;
    if (autoSave) save();
}

void Config::setBool(const std::string& key, bool value, bool autoSave) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_boolMap[key] = value;
    if (autoSave) save();
}

std::map<std::string, int> Config::getHomeOrder() const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    return m_homeOrder;
}

void Config::setHomeOrder(const std::map<std::string, int>& order, bool autoSave) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_homeOrder = order;
    if (autoSave) save();
}

std::string Config::getAppDir() {
    const char* env = std::getenv("KINNOVEL_APP_DIR");
    if (env && *env) {
        return env;
    }
    if (std::filesystem::exists("/mnt/us/extensions/kinnovel")) {
        return "/mnt/us/extensions/kinnovel";
    }
    return "./.kinnovel";
}

std::string Config::getCacheDir() {
    return getAppDir() + "/cache";
}

std::string Config::getDataDir() {
    return getAppDir() + "/data";
}

} // namespace kinnovel::core
