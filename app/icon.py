"""Draws app-icon.png (1024x1024), the source for `npx tauri icon`, and prints the same mark as
SVG for the in-app logo in src/icons.ts.

The mark (D-029): an anvil side on, after the reference picture Dzyan sent on 2026-11-25: a long
horn to the left, the step block, the face with its overhanging heel, a waist with deep curves on
both sides, and two feet with an arch between them. Thin lines in the background colour mark the
edges that make it read as an anvil (the horn's underside, the step block, the face plate, the
waist, the arch). Since 2026-11-26 (his pick, number 3 of docs/logo-options): analyser bars
rise off the face like sparks, and two beamed eighth notes are cut into the body.

The anvil is written in the reference picture's pixels (540 x 360) and scaled into the logo's 32
units, so it can be compared with it directly; the bars and the notes are in logo units. One set
of shapes makes both the icon and the SVG. Standard library only.

    python -I icon.py && npx tauri icon app-icon.png -o src-tauri/icons
    (then delete the android, ios, Square*, StoreLogo and 64x64 files it also writes)
"""
import math
import struct
import zlib

# ---------------------------------------------------------------- the shapes (reference pixels)
K = 30.0 / 440.0  # reference pixels to logo units: the anvil is 30 units long


def R(x, y):
    return (1.0 + (x - 50) * K, 8.6 + (y - 72) * K)


# The silhouette: ('M' | 'L', point) or ('C', control, control, point).
ANVIL = [
    ('M', (52, 94)),                                  # the horn's point
    ('C', (70, 88), (120, 87), (186, 87)),            # the horn's top
    ('L', (186, 80)),                                 # the step block
    ('L', (214, 80)),
    ('L', (214, 72)),
    ('L', (490, 72)),                                 # the face
    ('L', (490, 90)),                                 # the heel's end
    ('C', (430, 92), (372, 114), (356, 160)),         # under the heel, into the waist
    ('C', (346, 196), (366, 240), (424, 256)),        # down and out to the right foot
    ('L', (424, 288)),
    ('L', (342, 288)),
    ('C', (338, 248), (248, 248), (244, 288)),        # the arch between the feet
    ('L', (160, 288)),
    ('L', (160, 256)),
    ('C', (214, 242), (240, 214), (240, 182)),        # up the waist's left side
    ('C', (180, 176), (96, 150), (52, 94)),           # under the horn, back to its point
]
# Lines cut into it, drawn as strokes.
EDGE = 0.46  # their width, in logo units
LINES = [
    [('M', (110, 128)), ('C', (136, 146), (162, 155), (184, 158))],            # the horn's underside
    [('M', (189, 85)), ('L', (189, 167))],                                      # the step block
    [('M', (216, 83)), ('L', (216, 172))],
    [('M', (216, 83)), ('L', (468, 83))],                                       # the face plate
    [('M', (250, 188)), ('C', (250, 214), (228, 240), (190, 254))],            # the waist, left
    [('M', (366, 123)), ('C', (350, 146), (340, 186), (356, 220)), ('C', (366, 240), (388, 256), (414, 264))],  # the waist, right
    [('M', (236, 266)), ('C', (252, 232), (334, 232), (350, 266))],            # over the arch
]
# The analyser bars over the face, in logo units: left edge, width, bottom, heights, corner radius.
BARS_X, BARS_STEP, BAR_W, BARS_BOTTOM, BAR_R = 13.07, 1.95, 1.4, 7.7, 0.3
BAR_HEIGHTS = [3.0, 5.0, 7.5, 6.0, 4.0, 6.5, 4.5, 2.5, 1.5]
# Two beamed eighth notes cut into the body, in logo units: the heads (tilted ellipses), the
# stems rising from the heads' right sides, and the beam joining the stem tops.
HEADS = [(14.57, 14.65), (18.87, 14.15)]
HEAD_RX, HEAD_RY, HEAD_TILT = 1.2, 0.875, -20.0
STEM_W, STEM_H, STEM_AT, BEAM_H = 0.5, 4.0, 1.1, 1.0


def bar_rects():
    """(x, y, w, h) of each bar."""
    return [(BARS_X + k * BARS_STEP, BARS_BOTTOM - h, BAR_W, h) for k, h in enumerate(BAR_HEIGHTS)]


def note_parts():
    """The notes as polygons: two heads, two stems, the beam."""
    a = math.radians(HEAD_TILT)
    parts, tops = [], []
    for cx, cy in HEADS:
        parts.append([(cx + HEAD_RX * math.cos(t) * math.cos(a) - HEAD_RY * math.sin(t) * math.sin(a),
                       cy + HEAD_RX * math.cos(t) * math.sin(a) + HEAD_RY * math.sin(t) * math.cos(a))
                      for t in (2 * math.pi * k / 48 for k in range(48))])
        sx = cx + STEM_AT
        parts.append([(sx - STEM_W, cy - STEM_H), (sx, cy - STEM_H), (sx, cy), (sx - STEM_W, cy)])
        tops.append((sx, cy - STEM_H))
    (ax, ay), (bx, by) = tops
    parts.append([(ax - STEM_W, ay), (bx, by), (bx, by + BEAM_H), (ax - STEM_W, ay + BEAM_H)])
    return parts


def rounded_rect(x, y, w, h, r):
    pts = []
    for cx, cy, a0 in ((x + w - r, y + r, -90), (x + w - r, y + h - r, 0), (x + r, y + h - r, 90), (x + r, y + r, 180)):
        for k in range(9):
            a = math.radians(a0 + 90 * k / 8)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


# ---------------------------------------------------------------- geometry
def path_points(ops, n=40):
    pts, cur = [], None
    for op, *p in ops:
        p = [R(*q) for q in p]
        if op in 'ML':
            cur = p[0]
            pts.append(cur)
        else:
            (x0, y0), (x1, y1), (x2, y2), (x3, y3) = cur, *p
            for k in range(1, n + 1):
                t = k / n
                a, b, c, e = (1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t * t, t ** 3
                pts.append((a * x0 + b * x1 + c * x2 + e * x3, a * y0 + b * y1 + c * y2 + e * y3))
            cur = p[2]
    return pts


def svg():
    def f(v):
        return f'{round(v, 2):g}'

    def d(ops):
        return ''.join(op + ' '.join(f'{f(x)} {f(y)}' for x, y in (R(*q) for q in p)) for op, *p in ops)

    def poly(pts):
        return 'M' + 'L'.join(f'{f(x)} {f(y)}' for x, y in pts) + 'Z'

    lines = ''.join(f'<path d="{d(ops)}"/>' for ops in LINES)
    bars = ''.join(f'<rect x="{f(x)}" y="{f(y)}" width="{f(w)}" height="{f(h)}" rx="{BAR_R}"/>' for x, y, w, h in bar_rects())
    heads = ''.join(f'<ellipse cx="{f(x)}" cy="{f(y)}" rx="{HEAD_RX}" ry="{HEAD_RY}" transform="rotate({f(HEAD_TILT)} {f(x)} {f(y)})"/>'
                    for x, y in HEADS)
    parts = note_parts()  # head, stem, head, stem, beam; the heads are drawn as ellipses above
    rest = ''.join(f'<path d="{poly(parts[i])}"/>' for i in (1, 3, 4))
    # Cropped to the mark, so the top bar can show it large. The cuts are drawn in the top bar's colour.
    return (f'<svg viewBox="0.4 -0.4 31.2 24.3"><path d="{d(ANVIL)}Z" fill="currentColor"/><g fill="currentColor">{bars}</g>'
            f'<g fill="none" stroke-linecap="round" stroke-linejoin="round" stroke-width="{EDGE}" style="stroke:var(--surface)">{lines}</g>'
            f'<g style="fill:var(--surface)">{heads}{rest}</g></svg>')


# ---------------------------------------------------------------- drawing the icon
N = 2048  # drawn at 2x, then averaged down to 1024 for smooth edges
S = 29.0 * N / 1024  # pixels per logo unit
CX, CY = 16.0, 11.8  # logo point that lands in the middle of the icon (the bars top to the feet)
BG = (18, 23, 28)
GREEN = (33, 163, 102)  # --accent, #21a366


def px(u, v):
    return (u - CX) * S + N / 2, (v - CY) * S + N / 2


def fill(mask, polygon):
    poly = [px(u, v) for u, v in polygon]
    ys = [p[1] for p in poly]
    for yy in range(max(0, int(min(ys))), min(N, int(max(ys)) + 2)):
        y = yy + 0.5
        xs = sorted(x0 + (y - y0) * (x1 - x0) / (y1 - y0) for (x0, y0), (x1, y1) in zip(poly, poly[1:] + poly[:1]) if (y0 <= y < y1) or (y1 <= y < y0))
        for a, b in zip(xs[::2], xs[1::2]):
            for xx in range(max(0, int(a + 0.5)), min(N, int(b + 0.5))):
                mask[yy * N + xx] = 1


def stroke(mask, pts, width):
    r = width / 2 * S
    for (u0, v0), (u1, v1) in zip(pts, pts[1:]):
        n = max(1, int(math.hypot(u1 - u0, v1 - v0) / 0.05))
        for k in range(n + 1):
            x, y = px(u0 + (u1 - u0) * k / n, v0 + (v1 - v0) * k / n)
            for yy in range(max(0, int(y - r)), min(N, int(y + r) + 2)):
                dy = yy + 0.5 - y
                if abs(dy) > r:
                    continue
                w = math.sqrt(r * r - dy * dy)
                for xx in range(max(0, int(x - w)), min(N, int(x + w) + 2)):
                    mask[yy * N + xx] = 1


def in_rounded_square(x, y):
    m, r = N * 0.02, N * 0.2
    cx = min(max(x, m + r), N - m - r)
    cy = min(max(y, m + r), N - m - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def main():
    body, cut = bytearray(N * N), bytearray(N * N)
    fill(body, path_points(ANVIL))
    for rect in bar_rects():
        fill(body, rounded_rect(*rect, BAR_R))
    for ops in LINES:
        stroke(cut, path_points(ops), EDGE)
    for part in note_parts():
        fill(cut, part)

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
                    c = GREEN if body[k] and not cut[k] else BG
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
    print(svg())


main()
