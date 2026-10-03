#pragma once

#include <string>
#include <map>
#include <mutex>
#include <memory>

namespace kinnovel::network {

class SessionStore {
public:
    explicit SessionStore(std::string path = "");
    ~SessionStore() = default;

    bool load();
    bool save();

    std::string getString(const std::string& key, const std::string& defaultVal = "") const;
    double getDouble(const std::string& key, double defaultVal = 0.0) const;
    int getInt(const std::string& key, int defaultVal = 0) const;

    void setString(const std::string& key, const std::string& value);
    void setDouble(const std::string& key, double value);
    void setInt(const std::string& key, int value);

    void clearCredentials();

    std::string getUserJson() const;
    void setUserJson(const std::string& json);

    const std::string& getPath() const { return m_path; }

private:
    std::string m_path;
    mutable std::recursive_mutex m_mutex;
    std::map<std::string, std::string> m_data;
};

} // namespace kinnovel::network
