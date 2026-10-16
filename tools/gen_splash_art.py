"""Draw the splash pixel art on a small grid and print it as text rows.
'#' black, 'o' dark grey, '+' light grey, '.' background."""
W, H = 48, 36
g = [['.'] * W for _ in range(H)]


def put(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        g[y][x] = c


def hline(x0, x1, y, c):
    for x in range(x0, x1 + 1):
        put(x, y, c)


def vline(x, y0, y1, c):
    for y in range(y0, y1 + 1):
        put(x, y, c)


def rect(x0, y0, x1, y1, c):
    for y in range(y0, y1 + 1):
        hline(x0, x1, y, c)


# --- night sky: crescent moon, twinkles, dots
moon = ["...###.", ".##+...", ".#+....", "#+.....", "#+.....", "#+.....", ".#+....", ".##+...", "...###."]
for y, row in enumerate(moon):
    for x, ch in enumerate(row):
        if ch != '.':
            put(37 + x, 1 + y, ch)
for (sx, sy) in [(7, 4), (27, 2), (45, 12)]:
    put(sx, sy, 'o'); put(sx - 1, sy, '+'); put(sx + 1, sy, '+'); put(sx, sy - 1, '+'); put(sx, sy + 1, '+')
for (sx, sy) in [(14, 9), (33, 7), (2, 13), (20, 4)]:
    put(sx, sy, 'o')

# --- cat head peeking over the book
x0, x1, top = 15, 32, 10      # head box (ears start at `top`)
# ears: triangles, black outline, dark inner ear
for i in range(5):
    y = top + i
    # left ear: outer edge vertical at x0, inner edge diagonal
    put(x0, y, '#'); put(x0 + 1 + i, y, '#')
    hline(x0 + 1, x0 + i, y, 'o' if i >= 2 else '+')
    put(x1, y, '#'); put(x1 - 1 - i, y, '#')
    hline(x1 - i, x1 - 1, y, 'o' if i >= 2 else '+')
hline(x0 + 6, x1 - 6, top + 4, '#')                  # top of the head between the ears
# face
rect(x0 + 1, top + 5, x1 - 1, top + 11, '+')
vline(x0, top + 5, top + 11, '#'); vline(x1, top + 5, top + 11, '#')
# closed happy eyes  ^ ^
for ex in (x0 + 4, x1 - 6):
    put(ex, top + 8, '#'); put(ex + 1, top + 7, '#'); put(ex + 2, top + 8, '#')
# nose + mouth
hline(23, 24, top + 9, '#')
put(22, top + 10, 'o'); put(25, top + 10, 'o')
# cheeks
put(x0 + 2, top + 10, 'o'); put(x1 - 2, top + 10, 'o')
# whiskers
for dy in (9, 11):
    hline(x0 - 4, x0 - 1, top + dy, 'o'); hline(x1 + 1, x1 + 4, top + dy, 'o')

# --- open book
by = top + 12                  # page tops (row 22)
bx0, bx1 = 3, 44
sp0, sp1 = 23, 24
bottom = by + 10
for x in range(bx0, bx1 + 1):
    if sp0 <= x <= sp1:
        continue
    outer = x - bx0 if x < sp0 else bx1 - x
    y_top = by - (1 if outer < 6 else 0)          # outer corners lifted
    put(x, y_top, '#')
    rect(x, y_top + 1, x, bottom - 2, '.')
    put(x, bottom - 1, 'o')                       # page stack
    put(x, bottom, '#')                           # cover edge
vline(bx0, by - 1, bottom, '#'); vline(bx1, by - 1, bottom, '#')
vline(sp0, by, bottom + 1, '#'); vline(sp1, by, bottom + 1, '#')
hline(sp0 - 2, sp1 + 2, bottom + 1, '#')
# lines of text
for i, (l, r) in enumerate([(13, 11), (15, 14), (10, 15), (14, 8)]):
    y = by + 2 + i * 2
    hline(bx0 + 3, bx0 + 2 + l, y, 'o')
    hline(sp1 + 3, sp1 + 2 + r, y, 'o')
# paws resting on the page tops
for px in (16, 27):
    hline(px + 1, px + 3, by - 1, '#')
    put(px, by, '#'); hline(px + 1, px + 3, by, '+'); put(px + 4, by, '#')
    put(px, by + 1, '#'); hline(px + 1, px + 3, by + 1, '+'); put(px + 4, by + 1, '#')
    put(px + 2, by + 1, 'o')
    hline(px + 1, px + 3, by + 2, '#')

rows = [''.join(r) for r in g]
while rows and set(rows[-1]) == {'.'}:
    rows.pop()
print('\n'.join(rows))
