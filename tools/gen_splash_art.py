"""Splash pixel art: flat books on a shelf (style of the LightNovelShelf web icon:
rounded flat shapes, two books standing, one leaning), rasterised onto a coarse grid.
'#' black, 'o' dark grey, '+' light grey, '.' background. Prints the rows."""
import math

W, H = 44, 34
g = [['.'] * W for _ in range(H)]


def inside_round_rect(px, py, x0, y0, x1, y1, r):
    if not (x0 <= px <= x1 and y0 <= py <= y1):
        return False
    cx = min(max(px, x0 + r), x1 - r)
    cy = min(max(py, y0 + r), y1 - r)
    return (px - cx) ** 2 + (py - cy) ** 2 <= r * r


def fill(shape, c):
    for y in range(H):
        for x in range(W):
            if shape(x + 0.5, y + 0.5):
                g[y][x] = c


def rect(x0, y0, x1, y1, r, c):
    fill(lambda px, py: inside_round_rect(px, py, x0, y0, x1, y1, r), c)


def leaning(cx, cy, w, h, deg, r, c):
    """Rounded rect of size w x h centred at (cx, cy), rotated clockwise by deg."""
    a = math.radians(deg)
    ca, sa = math.cos(a), math.sin(a)

    def shape(px, py):
        dx, dy = px - cx, py - cy
        lx = dx * ca + dy * sa
        ly = -dx * sa + dy * ca
        return inside_round_rect(lx, ly, -w / 2, -h / 2, w / 2, h / 2, r)
    fill(shape, c)


base = 31                       # shelf top (books stand on it)
# book 1: narrow, black, a little shorter
rect(3, 7, 10, base, 2.2, '#')
# book 2: wide, dark grey, tallest; light label band near the top and a thin band at the bottom
rect(11.5, 1, 22, base, 2.2, 'o')
rect(13.5, 5, 20, 7, 0.0, '+')
rect(13.5, 25.5, 20, 26.5, 0.0, '+')
# book 1 spine line
rect(5, 10, 8, 11, 0.0, 'o')
# book 3: light grey, leaning against book 2 (its foot rests on the shelf)
h3 = 22.0
deg = -math.degrees(math.atan(0.5))  # 2:1 slope = clean pixel steps
foot_x, foot_y = 35.5, base - 1.2  # lowest corner level with the others
# centre = foot moved half the height along the tilted axis
cx = foot_x + math.sin(math.radians(deg)) * h3 / 2
cy = foot_y - math.cos(math.radians(deg)) * h3 / 2
leaning(cx, cy, 7.5, h3, deg, 2.2, "+")
# shelf
# (no shelf line: the web icon has none)

rows = [''.join(r) for r in g]
while rows and set(rows[0]) == {'.'}:
    rows.pop(0)
while rows and set(rows[-1]) == {'.'}:
    rows.pop()
# trim empty columns on both sides so the art is centred by its content
left = min(len(r) - len(r.lstrip('.')) for r in rows)
right = min(len(r) - len(r.rstrip('.')) for r in rows)
rows = [r[left:len(r) - right] for r in rows]
print('\n'.join(rows))
