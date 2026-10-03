#include "kinnovel/network/SessionStore.hpp"
#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Utils.hpp"
#include "yyjson.h"

#include <filesystem>
#include <fstream>

namespace kinnovel::network {

SessionStore::SessionStore(std::string path)
    : m_path(std::move(path)) {
    if (m_path.empty()) {
        m_path = core::Config::getCacheDir() + "/session.json";
    }
    load();
}

bool SessionStore::load() {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_data.clear();

    if (!std::filesystem::exists(m_path)) {
        return false;
    }

    std::ifstream in(m_path, std::ios::binary);
    if (!in) return false;
    std::string content((std::istreambuf_iterator<char>(in)),
                         std::istreambuf_iterator<char>());
    in.close();

    yyjson_doc* doc = yyjson_read(content.data(), content.size(), 0);
    if (!doc) return false;
    yyjson_val* root = yyjson_doc_get_root(doc);
    if (yyjson_is_obj(root)) {
        size_t idx, max;
        yyjson_val *key, *val;
        yyjson_obj_foreach(root, idx, max, key, val) {
            const char* k = yyjson_get_str(key);
            if (!k) continue;

            if (yyjson_is_str(val)) {
                m_data[k] = yyjson_get_str(val);
            } else if (yyjson_is_num(val)) {
                m_data[k] = std::to_string(yyjson_get_num(val));
            } else if (yyjson_is_bool(val)) {
                m_data[k] = yyjson_get_bool(val) ? "true" : "false";
            } else if (yyjson_is_obj(val) || yyjson_is_arr(val)) {
                char* serialized = yyjson_val_write(val, 0, nullptr);
                if (serialized) {
                    m_data[k] = serialized;
                    free(serialized);
                }
            }
        }
    }
    yyjson_doc_free(doc);
    return true;
}

bool SessionStore::save() {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    yyjson_mut_doc* doc = yyjson_mut_doc_new(nullptr);
    yyjson_mut_val* root = yyjson_mut_obj(doc);
    yyjson_mut_doc_set_root(doc, root);

    for (const auto& [k, v] : m_data) {
        if (k == "TokenUpdatedAt" || k == "Id") {
            try {
                double num = std::stod(v);
                yyjson_mut_obj_add_real(doc, root, k.c_str(), num);
                continue;
            } catch (...) {}
        }
        if (k == "User" && !v.empty() && (v[0] == '{' || v[0] == '[')) {
            yyjson_doc* subDoc = yyjson_read(v.data(), v.size(), 0);
            if (subDoc) {
                yyjson_mut_val* copyVal = yyjson_val_mut_copy(doc, yyjson_doc_get_root(subDoc));
                yyjson_mut_obj_add_val(doc, root, k.c_str(), copyVal);
                yyjson_doc_free(subDoc);
                continue;
            }
        }
        yyjson_mut_obj_add_str(doc, root, k.c_str(), v.c_str());
    }

    char* jsonStr = yyjson_mut_write(doc, YYJSON_WRITE_PRETTY, nullptr);
    bool ok = false;
    if (jsonStr) {
        ok = core::Utils::atomicWrite(m_path, jsonStr);
        free(jsonStr);
    }
    yyjson_mut_doc_free(doc);
    return ok;
}

std::string SessionStore::getString(const std::string& key, const std::string& defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_data.find(key);
    return (it != m_data.end()) ? it->second : defaultVal;
}

double SessionStore::getDouble(const std::string& key, double defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_data.find(key);
    if (it != m_data.end()) {
        try {
            return std::stod(it->second);
        } catch (...) {}
    }
    return defaultVal;
}

int SessionStore::getInt(const std::string& key, int defaultVal) const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_data.find(key);
    if (it != m_data.end()) {
        try {
            return std::stoi(it->second);
        } catch (...) {}
    }
    return defaultVal;
}

void SessionStore::setString(const std::string& key, const std::string& value) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_data[key] = value;
    save();
}

void SessionStore::setDouble(const std::string& key, double value) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_data[key] = std::to_string(value);
    save();
}

void SessionStore::setInt(const std::string& key, int value) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_data[key] = std::to_string(value);
    save();
}

void SessionStore::clearCredentials() {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_data.erase("Token");
    m_data.erase("RefreshToken");
    m_data.erase("TokenUpdatedAt");
    m_data.erase("User");
    save();
}

std::string SessionStore::getUserJson() const {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    auto it = m_data.find("User");
    return (it != m_data.end()) ? it->second : "{}";
}

void SessionStore::setUserJson(const std::string& json) {
    std::lock_guard<std::recursive_mutex> lock(m_mutex);
    m_data["User"] = json;
    save();
}

} // namespace kinnovel::network
