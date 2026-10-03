#pragma once

#include <string>
#include <vector>
#include <cstdint>

namespace kinnovel::core {

class Sha256 {
public:
    Sha256();
    void update(const uint8_t* data, size_t length);
    void update(const std::string& str);
    std::string finalHex();

    static std::string hashHex(const std::string& str);
    static std::string hashHex(const uint8_t* data, size_t length);
    static std::string hash(const std::string& str) { return hashHex(str); }

private:
    void transform(const uint8_t* chunk);

    uint32_t m_state[8];
    uint64_t m_bitlen = 0;
    uint8_t m_buffer[64];
    size_t m_buflen = 0;
};

} // namespace kinnovel::core
