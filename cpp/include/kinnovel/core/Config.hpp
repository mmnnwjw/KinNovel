#pragma once

#include <string>
#include <map>
#include <mutex>
#include <vector>

namespace kinnovel::core {

struct HomeOrderItem {
    std::string key;
    std::string label;
    int order = 0;
};

class Config {
public:
    explicit Config(const std::string& configPath = "");
    ~Config() = default;

    bool load();
    bool save();

    std::string getString(const std::string& key, const std::string& defaultVal = "") const;
    int getInt(const std::string& key, int defaultVal = 0) const;
    double getDouble(const std::string& key, double defaultVal = 0.0) const;
    bool getBool(const std::string& key, bool defaultVal = false) const;

    void setString(const std::string& key, const std::string& value, bool autoSave = true);
    void setInt(const std::string& key, int value, bool autoSave = true);
    void setDouble(const std::string& key, double value, bool autoSave = true);
    void setBool(const std::string& key, bool value, bool autoSave = true);

    void set(const std::string& key, const std::string& value) { setString(key, value, false); }
    void set(const std::string& key, const char* value) { setString(key, std::string(value), false); }
    void set(const std::string& key, int value) { setInt(key, value, false); }
    void set(const std::string& key, double value) { setDouble(key, value, false); }
    void set(const std::string& key, bool value) { setBool(key, value, false); }

    std::map<std::string, int> getHomeOrder() const;
    void setHomeOrder(const std::map<std::string, int>& order, bool autoSave = true);

    const std::string& getPath() const { return m_path; }

    static std::string getAppDir();
    static std::string getCacheDir();
    static std::string getDataDir();

private:
    void initDefaults();

    std::string m_path;
    mutable std::recursive_mutex m_mutex;

    std::map<std::string, std::string> m_stringMap;
    std::map<std::string, int> m_intMap;
    std::map<std::string, double> m_doubleMap;
    std::map<std::string, bool> m_boolMap;
    std::map<std::string, int> m_homeOrder;
};

} // namespace kinnovel::core
