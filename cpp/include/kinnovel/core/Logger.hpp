#pragma once

#include <string>
#include <mutex>
#include <fstream>

namespace kinnovel {

enum class LogLevel {
    Debug,
    Info,
    Warn,
    Error
};

class Logger {
public:
    static Logger& instance();

    void init(const std::string& logFilePath);
    void log(LogLevel level, const std::string& tag, const std::string& message);
    void close();

    static void debug(const std::string& tag, const std::string& msg);
    static void info(const std::string& tag, const std::string& msg);
    static void warn(const std::string& tag, const std::string& msg);
    static void error(const std::string& tag, const std::string& msg);

private:
    Logger() = default;
    ~Logger();
    void rotateIfNeeded();

    std::string m_logPath;
    std::ofstream m_fileStream;
    std::mutex m_mutex;
    bool m_initialized = false;
};

} // namespace kinnovel
