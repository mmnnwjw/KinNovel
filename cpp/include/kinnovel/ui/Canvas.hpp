#pragma once

#include "kinnovel/ui/Theme.hpp"
#include "kinnovel/ui/FontSet.hpp"
#include "kinnovel/reader/FontEngine.hpp"

#include <vector>
#include <string>
#include <memory>
#include <cstdint>

#include "kinnovel/core/Types.hpp"

namespace kinnovel::ui {

using Rect = kinnovel::Rect;

struct HeaderState {
    int height = 0;
    std::string left;
    std::string right;
};

class Canvas {
public:
    Canvas(int width, int height, Theme theme, const FontSet* fonts = nullptr);
    ~Canvas() = default;

    int getWidth() const { return m_width; }
    int getHeight() const { return m_height; }
    const Theme& getTheme() const { return m_theme; }
    void setTheme(const Theme& theme) { m_theme = theme; }
    const FontSet* getFonts() const { return m_fonts; }

    const std::vector<uint8_t>& getBuffer() const { return m_buffer; }
    std::vector<uint8_t>& getBuffer() { return m_buffer; }

    void clear(uint8_t color);
    void setPixel(int x, int y, uint8_t color);
    uint8_t getPixel(int x, int y) const;

    void drawLine(int x0, int y0, int x1, int y1, uint8_t color, int thickness = 1);
    void drawRect(int x, int y, int w, int h, uint8_t color, bool fill = false);
    void drawRoundedRect(int x, int y, int w, int h, int radius, uint8_t outlineColor, uint8_t fillColor, int thickness = 1);
    void drawRoundedRect(int x, int y, int w, int h, int radius, uint8_t outlineColor, int thickness = 1);
    void drawCircle(int cx, int cy, int radius, uint8_t color, bool fill = false);
    void drawBitmap(int x, int y, const uint8_t* data, int w, int h);
    void paste(const uint8_t* data, int x, int y, int w, int h);

    // Text rendering & metrics
    void drawText(int x, int y, const std::string& text, reader::FontFace* font, uint8_t color);
    void drawText(int x, int y, const std::string& text, reader::FontFace* font);
    void drawTextFallback(int x, int y, const std::string& text, reader::FontFace* font, reader::FontFace* fallback, uint8_t color);
    void drawTextFallback(int x, int y, const std::string& text, reader::FontFace* font, reader::FontFace* fallback);
    void drawCenteredText(int cx, int cy, const std::string& text, reader::FontFace* font, uint8_t color);
    void drawCenteredText(int cx, int cy, const std::string& text, reader::FontFace* font);

    double measureTextWidth(const std::string& text, reader::FontFace* font, reader::FontFace* fallback = nullptr);
    Rect textBBox(const std::string& text, reader::FontFace* font);

    std::string fitText(const std::string& text, reader::FontFace* font, int maxWidth);
    std::vector<std::string> wrap(const std::string& text, reader::FontFace* font, int maxWidth);

    // High level Kindle UI widgets
    void button(const Rect& rect, const std::string& label, bool active = true, reader::FontFace* font = nullptr);
    int header(const std::string& title, const std::string& left = "返回", const std::string& right = "主页",
               reader::FontFace* titleFont = nullptr, reader::FontFace* tinyFont = nullptr);
    int compactHeader(const std::string& title, const std::string& progress = "",
                      reader::FontFace* tinyFont = nullptr);

    std::vector<std::pair<std::string, Rect>> popup(
        const std::vector<std::string>& lines,
        const std::vector<std::string>& buttons,
        reader::FontFace* font = nullptr,
        reader::FontFace* btnFont = nullptr);

    const HeaderState& getHeaderState() const { return m_headerState; }
    void setHeaderState(const HeaderState& state) { m_headerState = state; }

private:
    int m_width;
    int m_height;
    Theme m_theme;
    const FontSet* m_fonts = nullptr;
    std::vector<uint8_t> m_buffer;
    HeaderState m_headerState;

    reader::FontFace* defaultFont(reader::FontFace* font) const;
    void drawBackIcon(int cx, int cy, int size, uint8_t color);
    void drawHomeIcon(int cx, int cy, int size, uint8_t color);
};

} // namespace kinnovel::ui
