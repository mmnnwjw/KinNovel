#include "kinnovel/core/Logger.hpp"

#include <iostream>
#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>
#include <filesystem>
#include <vector>

namespace kinnovel {

Logger& Logger::instance() {
    static Logger s_instance;
    return s_instance;
}

Logger::~Logger() {
    close();
}

void Logger::init(const std::string& logFilePath) {
    std::lock_guard<std::mutex> lock(m_mutex);
    m_logPath = logFilePath;
    try {
        std::filesystem::path p(m_logPath);
        if (p.has_parent_path()) {
            std::filesystem::create_directories(p.parent_path());
        }
        rotateIfNeeded();
        m_fileStream.open(m_logPath, std::ios::app);
        m_initialized = true;
    } catch (...) {
        // Fallback to stderr if file cannot be opened
    }
}

void Logger::rotateIfNeeded() {
    try {
        if (!std::filesystem::exists(m_logPath)) return;
        auto sz = std::filesystem::file_size(m_logPath);
        if (sz > 1024 * 1024) {
            std::ifstream in(m_logPath, std::ios::binary);
            if (in.is_open()) {
                in.seekg(-512 * 1024, std::ios::end);
                std::vector<char> buffer(512 * 1024);
                in.read(buffer.data(), buffer.size());
                in.close();

                std::ofstream out(m_logPath, std::ios::binary | std::ios::trunc);
                if (out.is_open()) {
                    out.write(buffer.data(), buffer.size());
                    out.close();
                }
            }
        }
    } catch (...) {}
}

void Logger::close() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_fileStream.is_open()) {
        m_fileStream.flush();
        m_fileStream.close();
    }
    m_initialized = false;
}

void Logger::log(LogLevel level, const std::string& tag, const std::string& message) {
    std::lock_guard<std::mutex> lock(m_mutex);
    auto now = std::chrono::system_clock::now();
    auto in_time_t = std::chrono::system_clock::to_time_t(now);
    std::tm tm_buf{};
#if defined(_WIN32)
    localtime_s(&tm_buf, &in_time_t);
#else
    localtime_r(&in_time_t, &tm_buf);
#endif

    std::ostringstream ss;
    ss << std::put_time(&tm_buf, "%Y-%m-%d %H:%M:%S");

    const char* lvlStr = "INFO";
    switch (level) {
        case LogLevel::Debug: lvlStr = "DEBUG"; break;
        case LogLevel::Info:  lvlStr = "INFO"; break;
        case LogLevel::Warn:  lvlStr = "WARN"; break;
        case LogLevel::Error: lvlStr = "ERROR"; break;
    }

    std::string formatted = "[" + ss.str() + "] [" + lvlStr + "] [" + tag + "] " + message;

    std::cout << formatted << std::endl;
    if (m_initialized && m_fileStream.is_open()) {
        m_fileStream << formatted << "\n";
        m_fileStream.flush();
    }
}

void Logger::debug(const std::string& tag, const std::string& msg) {
    instance().log(LogLevel::Debug, tag, msg);
}

void Logger::info(const std::string& tag, const std::string& msg) {
    instance().log(LogLevel::Info, tag, msg);
}

void Logger::warn(const std::string& tag, const std::string& msg) {
    instance().log(LogLevel::Warn, tag, msg);
}

void Logger::error(const std::string& tag, const std::string& msg) {
    instance().log(LogLevel::Error, tag, msg);
}

} // namespace kinnovel
