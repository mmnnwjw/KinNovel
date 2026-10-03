#include "kinnovel/ui/Canvas.hpp"
#include "kinnovel/core/Charset.hpp"
#include "kinnovel/core/Utils.hpp"

#include <cmath>
#include <ctime>
#include <algorithm>
#include <iomanip>
#include <sstream>

namespace kinnovel::ui {

Canvas::Canvas(int width, int height, Theme theme, const FontSet* fonts)
    : m_width(width)
    , m_height(height)
    , m_theme(theme)
    , m_fonts(fonts)
    , m_buffer(static_cast<size_t>(width * height), theme.background) {
}

reader::FontFace* Canvas::defaultFont(reader::FontFace* font) const {
    if (font) return font;
    if (m_fonts && m_fonts->body) return m_fonts->body.get();
    return nullptr;
}

void Canvas::clear(uint8_t color) {
    std::fill(m_buffer.begin(), m_buffer.end(), color);
}

void Canvas::setPixel(int x, int y, uint8_t color) {
    if (x >= 0 && x < m_width && y >= 0 && y < m_height) {
        m_buffer[static_cast<size_t>(y * m_width + x)] = color;
    }
}

uint8_t Canvas::getPixel(int x, int y) const {
    if (x >= 0 && x < m_width && y >= 0 && y < m_height) {
        return m_buffer[static_cast<size_t>(y * m_width + x)];
    }
    return m_theme.background;
}

void Canvas::drawLine(int x0, int y0, int x1, int y1, uint8_t color, int thickness) {
    if (thickness <= 1) {
        int dx = std::abs(x1 - x0);
        int dy = -std::abs(y1 - y0);
        int sx = x0 < x1 ? 1 : -1;
        int sy = y0 < y1 ? 1 : -1;
        int err = dx + dy;

        while (true) {
            setPixel(x0, y0, color);
            if (x0 == x1 && y0 == y1) break;
            int e2 = 2 * err;
            if (e2 >= dy) {
                err += dy;
                x0 += sx;
            }
            if (e2 <= dx) {
                err += dx;
                y0 += sy;
            }
        }
        return;
    }

    int half = thickness / 2;
    for (int t = -half; t <= half; ++t) {
        if (std::abs(x1 - x0) > std::abs(y1 - y0)) {
            drawLine(x0, y0 + t, x1, y1 + t, color, 1);
        } else {
            drawLine(x0 + t, y0, x1 + t, y1, color, 1);
        }
    }
}

void Canvas::drawRect(int x, int y, int w, int h, uint8_t color, bool fill) {
    if (w <= 0 || h <= 0) return;
    if (fill) {
        int xStart = std::max(0, x);
        int xEnd = std::min(m_width, x + w);
        int yStart = std::max(0, y);
        int yEnd = std::min(m_height, y + h);

        for (int py = yStart; py < yEnd; ++py) {
            uint8_t* row = &m_buffer[static_cast<size_t>(py * m_width + xStart)];
            std::fill(row, row + (xEnd - xStart), color);
        }
    } else {
        drawLine(x, y, x + w - 1, y, color, 1);
        drawLine(x, y + h - 1, x + w - 1, y + h - 1, color, 1);
        drawLine(x, y, x, y + h - 1, color, 1);
        drawLine(x + w - 1, y, x + w - 1, y + h - 1, color, 1);
    }
}

void Canvas::drawRoundedRect(int x, int y, int w, int h, int radius, uint8_t outlineColor, uint8_t fillColor, int thickness) {
    if (w <= 0 || h <= 0) return;
    int r = std::min(radius, std::min(w / 2, h / 2));
    if (r <= 0) {
        drawRect(x, y, w, h, fillColor, true);
        if (thickness > 0) {
            for (int t = 0; t < thickness; ++t) {
                drawRect(x + t, y + t, w - 2 * t, h - 2 * t, outlineColor, false);
            }
        }
        return;
    }

    int cx0 = x + r;
    int cx1 = x + w - 1 - r;
    int cy0 = y + r;
    int cy1 = y + h - 1 - r;

    int rOuterSq = r * r;
    int rInner = std::max(0, r - thickness);
    int rInnerSq = rInner * rInner;

    int yMin = std::max(0, y);
    int yMax = std::min(m_height - 1, y + h - 1);
    int xMin = std::max(0, x);
    int xMax = std::min(m_width - 1, x + w - 1);

    for (int py = yMin; py <= yMax; ++py) {
        for (int px = xMin; px <= xMax; ++px) {
            int dx = 0;
            int dy = 0;
            bool isCorner = false;

            if (px < cx0 && py < cy0) {
                dx = px - cx0; dy = py - cy0; isCorner = true;
            } else if (px > cx1 && py < cy0) {
                dx = px - cx1; dy = py - cy0; isCorner = true;
            } else if (px < cx0 && py > cy1) {
                dx = px - cx0; dy = py - cy1; isCorner = true;
            } else if (px > cx1 && py > cy1) {
                dx = px - cx1; dy = py - cy1; isCorner = true;
            }

            if (isCorner) {
                int distSq = dx * dx + dy * dy;
                if (distSq > rOuterSq) {
                    continue;
                }
                if (distSq >= rInnerSq) {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = outlineColor;
                } else {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = fillColor;
                }
            } else {
                bool isBorder = (px < x + thickness) || (px > x + w - 1 - thickness) ||
                                (py < y + thickness) || (py > y + h - 1 - thickness);
                if (isBorder) {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = outlineColor;
                } else {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = fillColor;
                }
            }
        }
    }
}

void Canvas::drawRoundedRect(int x, int y, int w, int h, int radius, uint8_t outlineColor, int thickness) {
    if (w <= 0 || h <= 0) return;
    int r = std::min(radius, std::min(w / 2, h / 2));
    int cx0 = x + r;
    int cx1 = x + w - 1 - r;
    int cy0 = y + r;
    int cy1 = y + h - 1 - r;

    int rOuterSq = r * r;
    int rInner = std::max(0, r - thickness);
    int rInnerSq = rInner * rInner;

    int yMin = std::max(0, y);
    int yMax = std::min(m_height - 1, y + h - 1);
    int xMin = std::max(0, x);
    int xMax = std::min(m_width - 1, x + w - 1);

    for (int py = yMin; py <= yMax; ++py) {
        for (int px = xMin; px <= xMax; ++px) {
            int dx = 0;
            int dy = 0;
            bool isCorner = false;

            if (px < cx0 && py < cy0) {
                dx = px - cx0; dy = py - cy0; isCorner = true;
            } else if (px > cx1 && py < cy0) {
                dx = px - cx1; dy = py - cy0; isCorner = true;
            } else if (px < cx0 && py > cy1) {
                dx = px - cx0; dy = py - cy1; isCorner = true;
            } else if (px > cx1 && py > cy1) {
                dx = px - cx1; dy = py - cy1; isCorner = true;
            }

            if (isCorner) {
                int distSq = dx * dx + dy * dy;
                if (distSq <= rOuterSq && distSq >= rInnerSq) {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = outlineColor;
                }
            } else {
                bool isBorder = (px < x + thickness) || (px > x + w - 1 - thickness) ||
                                (py < y + thickness) || (py > y + h - 1 - thickness);
                if (isBorder) {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = outlineColor;
                }
            }
        }
    }
}

void Canvas::drawCircle(int cx, int cy, int radius, uint8_t color, bool fill) {
    if (radius <= 0) return;
    if (fill) {
        int rSq = radius * radius;
        int yMin = std::max(0, cy - radius);
        int yMax = std::min(m_height - 1, cy + radius);
        for (int py = yMin; py <= yMax; ++py) {
            int dy = py - cy;
            int dxMax = static_cast<int>(std::sqrt(rSq - dy * dy));
            int xMin = std::max(0, cx - dxMax);
            int xMax = std::min(m_width - 1, cx + dxMax);
            uint8_t* row = &m_buffer[static_cast<size_t>(py * m_width + xMin)];
            std::fill(row, row + (xMax - xMin + 1), color);
        }
    } else {
        int x = 0;
        int y = radius;
        int d = 3 - 2 * radius;

        auto plot8 = [&](int px, int py) {
            setPixel(cx + px, cy + py, color);
            setPixel(cx - px, cy + py, color);
            setPixel(cx + px, cy - py, color);
            setPixel(cx - px, cy - py, color);
            setPixel(cx + py, cy + px, color);
            setPixel(cx - py, cy + px, color);
            setPixel(cx + py, cy - px, color);
            setPixel(cx - py, cy - px, color);
        };

        plot8(x, y);
        while (y >= x) {
            x++;
            if (d > 0) {
                y--;
                d = d + 4 * (x - y) + 10;
            } else {
                d = d + 4 * x + 6;
            }
            plot8(x, y);
        }
    }
}

void Canvas::drawBitmap(int x, int y, const uint8_t* data, int w, int h) {
    paste(data, x, y, w, h);
}

void Canvas::paste(const uint8_t* data, int x, int y, int w, int h) {
    if (!data || w <= 0 || h <= 0) return;

    int xStart = std::max(0, x);
    int xEnd = std::min(m_width, x + w);
    int yStart = std::max(0, y);
    int yEnd = std::min(m_height, y + h);

    for (int py = yStart; py < yEnd; ++py) {
        int srcY = py - y;
        int copyWidth = xEnd - xStart;
        const uint8_t* srcRow = data + srcY * w + (xStart - x);
        uint8_t* dstRow = &m_buffer[static_cast<size_t>(py * m_width + xStart)];
        std::copy(srcRow, srcRow + copyWidth, dstRow);
    }
}

void Canvas::drawText(int x, int y, const std::string& text, reader::FontFace* font, uint8_t color) {
    font = defaultFont(font);
    if (!font || text.empty()) return;

    FT_Face face = font->getFtFace();
    if (!face) return;

    int ascender = face->size->metrics.ascender >> 6;
    int baselineY = y + ascender;
    int penX = x;

    auto cps = core::Charset::utf8ToCodepoints(text);
    for (uint32_t cp : cps) {
        if (cp == ' ' || cp == '\t' || cp == 0x3000) {
            penX += static_cast<int>(font->getCharAdvance(cp));
            continue;
        }

        FT_UInt glyphIndex = FT_Get_Char_Index(face, cp);
        if (glyphIndex == 0) {
            penX += static_cast<int>(font->getCharAdvance(cp));
            continue;
        }

        FT_Error err = FT_Load_Glyph(face, glyphIndex, FT_LOAD_RENDER | FT_LOAD_TARGET_NORMAL);
        if (err != 0) {
            penX += static_cast<int>(font->getCharAdvance(cp));
            continue;
        }

        FT_GlyphSlot slot = face->glyph;
        FT_Bitmap& bmp = slot->bitmap;
        int gx = penX + slot->bitmap_left;
        int gy = baselineY - slot->bitmap_top;

        for (unsigned int r = 0; r < bmp.rows; ++r) {
            int py = gy + r;
            if (py < 0 || py >= m_height) continue;
            for (unsigned int c = 0; c < bmp.width; ++c) {
                int px = gx + c;
                if (px < 0 || px >= m_width) continue;
                uint8_t alpha = bmp.buffer[r * bmp.pitch + c];
                if (alpha == 0) continue;
                if (alpha == 255) {
                    m_buffer[static_cast<size_t>(py * m_width + px)] = color;
                } else {
                    uint8_t bg = m_buffer[static_cast<size_t>(py * m_width + px)];
                    m_buffer[static_cast<size_t>(py * m_width + px)] =
                        static_cast<uint8_t>((bg * (255 - alpha) + color * alpha) / 255);
                }
            }
        }

        penX += static_cast<int>(font->getCharAdvance(cp));
    }
}

void Canvas::drawText(int x, int y, const std::string& text, reader::FontFace* font) {
    drawText(x, y, text, font, m_theme.foreground);
}

void Canvas::drawTextFallback(int x, int y, const std::string& text, reader::FontFace* font,
                              reader::FontFace* fallback, uint8_t color) {
    font = defaultFont(font);
    if (!font || text.empty()) return;

    if (!fallback) {
        drawText(x, y, text, font, color);
        return;
    }

    auto fontShared = std::shared_ptr<reader::FontFace>(font, [](reader::FontFace*){});
    auto fallbackShared = std::shared_ptr<reader::FontFace>(fallback, [](reader::FontFace*){});

    auto runs = reader::FontEngine::instance().splitFontRuns(text, fontShared, fallbackShared);
    int penX = x;
    for (const auto& run : runs) {
        if (!run.text.empty() && run.font) {
            drawText(penX, y, run.text, run.font.get(), color);
            penX += static_cast<int>(std::round(reader::FontEngine::instance().measureText(run.text, run.font, nullptr)));
        }
    }
}

void Canvas::drawTextFallback(int x, int y, const std::string& text, reader::FontFace* font,
                              reader::FontFace* fallback) {
    drawTextFallback(x, y, text, font, fallback, m_theme.foreground);
}

void Canvas::drawCenteredText(int cx, int cy, const std::string& text, reader::FontFace* font, uint8_t color) {
    font = defaultFont(font);
    if (!font || text.empty()) return;

    double w = measureTextWidth(text, font);
    int h = font->getSize();
    int x = cx - static_cast<int>(std::round(w / 2.0));
    int y = cy - h / 2;
    drawText(x, y, text, font, color);
}

void Canvas::drawCenteredText(int cx, int cy, const std::string& text, reader::FontFace* font) {
    drawCenteredText(cx, cy, text, font, m_theme.foreground);
}

double Canvas::measureTextWidth(const std::string& text, reader::FontFace* font, reader::FontFace* fallback) {
    font = defaultFont(font);
    if (!font || text.empty()) return 0.0;
    auto fontShared = std::shared_ptr<reader::FontFace>(font, [](reader::FontFace*){});
    std::shared_ptr<reader::FontFace> fallbackShared;
    if (fallback) {
        fallbackShared = std::shared_ptr<reader::FontFace>(fallback, [](reader::FontFace*){});
    }
    return reader::FontEngine::instance().measureText(text, fontShared, fallbackShared);
}

Rect Canvas::textBBox(const std::string& text, reader::FontFace* font) {
    font = defaultFont(font);
    int w = static_cast<int>(std::round(measureTextWidth(text, font)));
    int h = font ? font->getSize() : 20;
    return { 0, 0, w, h };
}

std::string Canvas::fitText(const std::string& text, reader::FontFace* font, int maxWidth) {
    font = defaultFont(font);
    if (!font || text.empty() || maxWidth <= 0) return "";

    if (measureTextWidth(text, font) <= maxWidth) {
        return text;
    }

    std::string suffix = "…";
    double suffixWidth = measureTextWidth(suffix, font);
    if (suffixWidth > maxWidth) return "";

    auto cps = core::Charset::utf8ToCodepoints(text);
    while (!cps.empty()) {
        std::string cand = core::Charset::codepointsToUtf8(cps) + suffix;
        if (measureTextWidth(cand, font) <= maxWidth) {
            return cand;
        }
        cps.pop_back();
    }
    return suffix;
}

std::vector<std::string> Canvas::wrap(const std::string& text, reader::FontFace* font, int maxWidth) {
    font = defaultFont(font);
    if (!font || maxWidth <= 0) return { text };

    std::string clean = core::Charset::cleanText(text);
    std::vector<std::string> output;

    std::istringstream stream(clean);
    std::string paragraph;
    while (std::getline(stream, paragraph)) {
        if (!paragraph.empty() && paragraph.back() == '\r') {
            paragraph.pop_back();
        }

        std::string current;
        auto cps = core::Charset::utf8ToCodepoints(paragraph);
        for (uint32_t cp : cps) {
            std::string ch = core::Charset::codepointToUtf8(cp);
            std::string candidate = current + ch;
            if (!current.empty() && measureTextWidth(candidate, font) > maxWidth) {
                output.push_back(current);
                current = ch;
            } else {
                current = candidate;
            }
        }
        output.push_back(current);
    }

    if (output.empty()) {
        output.push_back("");
    }
    return output;
}

void Canvas::button(const Rect& rect, const std::string& label, bool active, reader::FontFace* font) {
    font = font ? font : (m_fonts && m_fonts->small ? m_fonts->small.get() : defaultFont(nullptr));
    uint8_t fill = active ? m_theme.inverseBg : m_theme.background;
    uint8_t textFill = active ? m_theme.inverseFg : m_theme.muted;

    drawRoundedRect(rect.x, rect.y, rect.width, rect.height, 10, m_theme.foreground, fill, 2);
    drawCenteredText(rect.x + rect.width / 2, rect.y + rect.height / 2, label, font, textFill);
}

int Canvas::header(const std::string& title, const std::string& left, const std::string& right,
                   reader::FontFace* titleFont, reader::FontFace* tinyFont) {
    int height = std::max(72, static_cast<int>(m_height * 0.085));
    titleFont = titleFont ? titleFont : (m_fonts && m_fonts->title ? m_fonts->title.get() : defaultFont(nullptr));
    tinyFont = tinyFont ? tinyFont : (m_fonts && m_fonts->tiny ? m_fonts->tiny.get() : defaultFont(nullptr));

    drawRect(0, 0, m_width, height, m_theme.light, true);

    std::time_t now = std::time(nullptr);
    std::tm tmNow{};
    localtime_r(&now, &tmNow);
    char timeBuf[16];
    std::strftime(timeBuf, sizeof(timeBuf), "%H:%M", &tmNow);
    std::string status = timeBuf;

    auto level = core::Utils::batteryLevel();
    if (level.has_value()) {
        status += " · " + std::to_string(level.value()) + "%";
    }

    int statusWidth = static_cast<int>(std::round(measureTextWidth(status, tinyFont)));
    int homeCx = m_width - std::max(34, height / 2);
    int statusX = std::max(m_width / 2 + 60, homeCx - 36 - statusWidth);

    int statusY = height / 2 - (tinyFont ? tinyFont->getSize() / 2 : 12);
    drawText(statusX, statusY, status, tinyFont, m_theme.foreground);

    int titleWidth = std::max(120, m_width - 360 - statusWidth);
    std::string fittedTitle = fitText(title, titleFont, titleWidth);
    drawCenteredText(m_width / 2, height / 2, fittedTitle, titleFont, m_theme.foreground);

    if (!left.empty()) {
        drawBackIcon(std::max(34, height / 2), height / 2, std::max(14, std::min(24, height / 4)), m_theme.foreground);
    }
    if (!right.empty()) {
        drawHomeIcon(m_width - std::max(34, height / 2), height / 2, std::max(13, std::min(22, height / 4)), m_theme.foreground);
    }

    m_headerState = { height, left, right };
    return height;
}

int Canvas::compactHeader(const std::string& title, const std::string& progress, reader::FontFace* tinyFont) {
    int height = std::max(40, static_cast<int>(m_height * 0.035));
    tinyFont = tinyFont ? tinyFont : (m_fonts && m_fonts->tiny ? m_fonts->tiny.get() : defaultFont(nullptr));

    drawRect(0, 0, m_width, height, m_theme.background, true);

    std::time_t now = std::time(nullptr);
    std::tm tmNow{};
    localtime_r(&now, &tmNow);
    char timeBuf[16];
    std::strftime(timeBuf, sizeof(timeBuf), "%H:%M", &tmNow);
    std::string status = timeBuf;

    auto level = core::Utils::batteryLevel();
    if (level.has_value()) {
        status += " · " + std::to_string(level.value()) + "%";
    }

    int margin = std::max(10, static_cast<int>(m_width * 0.012));
    int statusWidth = static_cast<int>(std::round(measureTextWidth(status, tinyFont)));
    int statusX = m_width - margin - statusWidth;
    int textY = (height - (tinyFont ? tinyFont->getSize() : 20)) / 2;

    drawText(statusX, textY, status, tinyFont, m_theme.foreground);

    std::string label = title.empty() ? "阅读" : title;
    if (!progress.empty()) {
        label += "  " + progress;
    }

    int maxW = std::max(80, statusX - margin * 3);
    drawText(margin, textY, fitText(label, tinyFont, maxW), tinyFont, m_theme.foreground);

    drawLine(0, height - 1, m_width - 1, height - 1, m_theme.mid, 1);

    m_headerState = { height, "", "" };
    return height;
}

void Canvas::drawBackIcon(int cx, int cy, int size, uint8_t color) {
    drawLine(cx + size, cy, cx - size, cy, color, 5);
    drawLine(cx + size, cy, cx, cy - size, color, 5);
    drawLine(cx + size, cy, cx, cy + size, color, 5);
}

void Canvas::drawHomeIcon(int cx, int cy, int size, uint8_t color) {
    int roofY = cy - size;
    int wallY = cy + size;
    drawLine(cx - size, cy, cx, roofY, color, 5);
    drawLine(cx, roofY, cx + size, cy, color, 5);
    drawLine(cx - size + 3, cy, cx - size + 3, wallY, color, 4);
    drawLine(cx + size - 3, cy, cx + size - 3, wallY, color, 4);
    drawLine(cx - size + 3, wallY, cx + size - 3, wallY, color, 4);
}

std::vector<std::pair<std::string, Rect>> Canvas::popup(
    const std::vector<std::string>& lines,
    const std::vector<std::string>& buttons,
    reader::FontFace* font,
    reader::FontFace* btnFont) {

    font = font ? font : (m_fonts && m_fonts->small ? m_fonts->small.get() : defaultFont(nullptr));
    btnFont = btnFont ? btnFont : (m_fonts && m_fonts->body ? m_fonts->body.get() : defaultFont(nullptr));

    int width = static_cast<int>(m_width * 0.78);
    int lineHeight = std::max((font ? font->getSize() : 30) + 10, 52);
    int height = std::max(180, 72 + lineHeight * std::max(1, static_cast<int>(lines.size())) + 70 * (!buttons.empty() ? 1 : 0));

    int x = (m_width - width) / 2;
    int y = (m_height - height) / 2;

    drawRoundedRect(x, y, width, height, 18, m_theme.foreground, m_theme.background, 3);

    int textY = y + 46;
    for (const auto& line : lines) {
        auto wrapped = wrap(line, font, width - 40);
        for (const auto& wline : wrapped) {
            drawCenteredText(m_width / 2, textY, wline, font, m_theme.foreground);
            textY += lineHeight;
        }
    }

    std::vector<std::pair<std::string, Rect>> rects;
    if (buttons.empty()) {
        return rects;
    }

    int gap = 16;
    int btnW = static_cast<int>((width - 32 - gap * (static_cast<int>(buttons.size()) - 1)) / buttons.size());
    int btnH = 52;
    int bx = x + 16;
    int by = y + height - btnH - 16;

    for (const auto& label : buttons) {
        Rect r{ bx, by, btnW, btnH };
        button(r, label, true, btnFont);
        rects.push_back({ label, r });
        bx += btnW + gap;
    }

    return rects;
}

} // namespace kinnovel::ui
