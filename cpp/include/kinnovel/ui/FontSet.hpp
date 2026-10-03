#pragma once

#include "kinnovel/reader/FontEngine.hpp"
#include <memory>
#include <string>
#include <unordered_map>
#include <algorithm>
#include <cmath>

namespace kinnovel::ui {

class FontSet {
public:
    FontSet() = default;

    void init(const std::string& fontPath, int screenWidth = 1072, int screenHeight = 1448) {
        double scale = 1.0;
        if (screenWidth > 0 && screenHeight > 0) {
            double sw = static_cast<double>(screenWidth) / 1072.0;
            double sh = static_cast<double>(screenHeight) / 1448.0;
            scale = std::max(0.75, std::min(1.15, std::min(sw, sh)));
        }

        auto loadScaled = [&](int size) -> std::shared_ptr<reader::FontFace> {
            int scaled = std::max(12, static_cast<int>(std::round(size * scale)));
            return reader::FontEngine::instance().loadFont(fontPath, scaled);
        };

        hero = loadScaled(82);
        title = loadScaled(50);
        body = loadScaled(38);
        small = loadScaled(31);
        tiny = loadScaled(25);

        font96 = loadScaled(96);
        font48 = loadScaled(48);
        font36 = loadScaled(36);
        font28 = loadScaled(28);

        m_namedFonts["hero"] = hero;
        m_namedFonts["title"] = title;
        m_namedFonts["body"] = body;
        m_namedFonts["small"] = small;
        m_namedFonts["tiny"] = tiny;

        m_sizedFonts[96] = font96;
        m_sizedFonts[48] = font48;
        m_sizedFonts[36] = font36;
        m_sizedFonts[28] = font28;
    }

    std::shared_ptr<reader::FontFace> hero;
    std::shared_ptr<reader::FontFace> title;
    std::shared_ptr<reader::FontFace> body;
    std::shared_ptr<reader::FontFace> small;
    std::shared_ptr<reader::FontFace> tiny;

    std::shared_ptr<reader::FontFace> font96;
    std::shared_ptr<reader::FontFace> font48;
    std::shared_ptr<reader::FontFace> font36;
    std::shared_ptr<reader::FontFace> font28;

    std::shared_ptr<reader::FontFace> get(const std::string& name) const {
        auto it = m_namedFonts.find(name);
        if (it != m_namedFonts.end()) {
            return it->second;
        }
        return body;
    }

    std::shared_ptr<reader::FontFace> get(int size) const {
        auto it = m_sizedFonts.find(size);
        if (it != m_sizedFonts.end()) {
            return it->second;
        }
        return body;
    }

private:
    std::unordered_map<std::string, std::shared_ptr<reader::FontFace>> m_namedFonts;
    std::unordered_map<int, std::shared_ptr<reader::FontFace>> m_sizedFonts;
};

} // namespace kinnovel::ui
