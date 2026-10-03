#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace kinnovel {

struct Point {
    int x = 0;
    int y = 0;
};

struct Rect {
    int x = 0;
    int y = 0;
    int width = 0;
    int height = 0;

    bool contains(int px, int py) const {
        return px >= x && px < x + width && py >= y && py < y + height;
    }
};

} // namespace kinnovel
