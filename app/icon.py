"""Draws app-icon.png (1024x1024), the source for `npx tauri icon`.

The mark matches the in-app logo in src/icons.ts: a crucible (rim with a pouring lip, tapered
body) with a sound wave cut through it, on a dark rounded square. Standard library only, so it
runs anywhere Python does. Shapes are in the logo's 32-unit coordinates.

    python -I icon.py && npx tauri icon app-icon.png -o src-tauri/icons
"""
import math
import struct
import zlib

N = 2048  # drawn at 2x, then averaged down to 1024 for smooth edges
S = 28 * N / 1024  # pixels per logo unit
CX, CY = 16.0, 16.7  # logo point that lands in the middle of the icon

BG = (18, 23, 28)
GREEN = (69, 192, 143)  # --accent


def px(u, v):
    return (u - CX) * S + N / 2, (v - CY) * S + N / 2


mask_body = bytearray(N * N)  # 1 = crucible
mask_cut = bytearray(N * N)  # 1 = wave cut out of it


def stamp(mask, u, v, r_units):
    x, y = px(u, v)
    r = r_units * S
    for yy in range(max(0, int(y - r)), min(N, int(y + r) + 2)):
        dy = yy + 0.5 - y
        if abs(dy) > r:
            continue
        w = math.sqrt(r * r - dy * dy)
        for xx in range(max(0, int(x - w)), min(N, int(x + w) + 2)):
            mask[yy * N + xx] = 1


def stroke(mask, pts, width_units, step=0.04):
    for (u0, v0), (u1, v1) in zip(pts, pts[1:]):
        n = max(1, int(math.hypot(u1 - u0, v1 - v0) / step))
        for k in range(n + 1):
            t = k / n
            stamp(mask, u0 + (u1 - u0) * t, v0 + (v1 - v0) * t, width_units / 2)


# rim with a lip on the right
stroke(mask_body, [(4.5, 8.2), (25.8, 8.2), (28.0, 6.6)], 2.4, step=0.25)

# body: tapered sides, rounded bottom corners (radius 3.8)
TOP, BOT, R = 10.6, 26.8, 3.8
def side(v):  # left edge at height v (units); the right edge mirrors it about x=16
    return 6.6 + (v - TOP) / 13.0 * 2.3
for yy in range(N):
    v = (yy + 0.5 - N / 2) / S + CY
    if v < TOP or v > BOT:
        continue
    left = side(min(v, BOT))
    if v > BOT - R:  # rounded corner
        cxl = side(BOT - R) + R
        dy = v - (BOT - R)
        left = max(left, cxl - math.sqrt(max(0.0, R * R - dy * dy)))
    right = 32 - left
    x0, _ = px(left, v)
    x1, _ = px(right, v)
    for xx in range(max(0, int(x0)), min(N, int(x1) + 1)):
        mask_body[yy * N + xx] = 1

# the wave: extremes joined by half-cosines, like the SVG path's smooth curves
extremes = [(9.2, 18.2), (11.6, 14.6), (14.0, 21.8), (16.4, 12.8), (18.8, 21.8), (21.2, 14.6), (23.6, 18.2)]
wave = []
for (u0, v0), (u1, v1) in zip(extremes, extremes[1:]):
    for k in range(40):
        t = k / 40
        wave.append((u0 + (u1 - u0) * t, v0 + (v1 - v0) * (1 - math.cos(math.pi * t)) / 2))
wave.append(extremes[-1])
stroke(mask_cut, wave, 1.7, step=0.05)


def in_rounded_square(x, y):
    m, r = N * 0.02, N * 0.2
    cx = min(max(x, m + r), N - m - r)
    cy = min(max(y, m + r), N - m - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


rows = []
for j in range(0, N, 2):
    row = bytearray([0])
    for i in range(0, N, 2):
        acc = [0, 0, 0, 0]
        for dy in (0, 1):
            for dx in (0, 1):
                x, y = i + dx, j + dy
                if not in_rounded_square(x + 0.5, y + 0.5):
                    continue
                k = y * N + x
                c = GREEN if mask_body[k] and not mask_cut[k] else BG
                acc[0] += c[0]
                acc[1] += c[1]
                acc[2] += c[2]
                acc[3] += 255
        row += bytes(a // 4 for a in acc)
    rows.append(bytes(row))


def chunk(t, d):
    return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xFFFFFFFF)


png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', N // 2, N // 2, 8, 6, 0, 0, 0))
png += chunk(b'IDAT', zlib.compress(b''.join(rows), 9)) + chunk(b'IEND', b'')
open('app-icon.png', 'wb').write(png)
print('app-icon.png written')
