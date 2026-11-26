"""Draws the logo and the saved alternatives, and index.html, a page that shows each as an app
icon, at top-bar size and on a light background.

    python -I options.py

History (D-029): Dzyan looked at 25 drawings over three rounds (2026-11-25 and 26). He chose
number 3 as the logo, "Spectrum sparks with a note", and saved 5, 6, 13, 15 and 17 in case the
logo changes again. The numbers are the ones he picked by; the others were dropped.

Each mark is drawn in a 64-unit square in `currentColor`; cut-outs are masks, so they stay
transparent on any background. The logo itself is drawn by app/icon.py, which makes the app icon
and the top-bar logo; a saved alternative that becomes the logo is redrawn there the same way.
"""
import glob
import math
import os
import re

ACCENT, DARK = '#21a366', '#12171c'

# The anvil, in its own 32-unit coordinates (the same shape as app/icon.py).
ANVIL = ('M1.14 10.1C2.36 9.69 5.77 9.62 10.27 9.62L10.27 9.15L12.18 9.15L12.18 8.6L31 8.6L31 9.83'
         'C26.91 9.96 22.95 11.46 21.86 14.6C21.18 17.05 22.55 20.05 26.5 21.15L26.5 23.33L20.91 23.33'
         'C20.64 20.6 14.5 20.6 14.23 23.33L8.5 23.33L8.5 21.15C12.18 20.19 13.95 18.28 13.95 16.1'
         'C9.86 15.69 4.14 13.92 1.14 10.1Z')
EDGES = ['M5.09 12.42C6.86 13.65 8.64 14.26 10.14 14.46', 'M10.48 9.49L10.48 15.08', 'M12.32 9.35L12.32 15.42',
         'M12.32 9.35L29.5 9.35', 'M14.64 16.51C14.64 18.28 13.14 20.05 10.55 21.01',
         'M22.55 12.08C21.45 13.65 20.77 16.37 21.86 18.69C22.55 20.05 24.05 21.15 25.82 21.69',
         'M13.68 21.83C14.77 19.51 20.36 19.51 21.45 21.83']
# The anvil's extent in its own units: x 1.14..31 (centre 16.07), y 8.6 (face) .. 23.33 (feet).
AX, AY = 16.07, 15.97


def f(v):
    return f'{round(v, 2):g}'


def placed(s, cx=32.0, cy=None, face=None, feet=None):
    """A transform that scales the anvil by `s` and centres it on x = cx; vertically either its
    middle at cy, its face at `face`, or its feet at `feet`."""
    tx = cx - AX * s
    if face is not None:
        ty = face - 8.6 * s
    elif feet is not None:
        ty = feet - 23.33 * s
    else:
        ty = (32.0 if cy is None else cy) - AY * s
    return f'translate({f(tx)} {f(ty)}) scale({f(s)})', tx, ty


def mask(key, black, extra=''):
    return (f'<mask id="{key}" maskUnits="userSpaceOnUse" x="-8" y="-8" width="80" height="80">'
            f'<rect x="-8" y="-8" width="80" height="80" fill="#fff"/><g fill="#000">{black}</g>{extra}</mask>')


def anvil(transform, key, cuts=''):
    """The anvil, with its edge lines cut out of it; `cuts` are more shapes, in the 64-unit
    square, cut out of it too."""
    lines = ''.join(f'<path d="{d}"/>' for d in EDGES)
    stroked = (f'<g transform="{transform}" fill="none" stroke="#000" stroke-width="0.46" '
               f'stroke-linecap="round" stroke-linejoin="round">{lines}</g>')
    return mask(key, cuts, stroked), f'<g mask="url(#{key})"><path transform="{transform}" d="{ANVIL}"/></g>'


def knock(key, t, s, gap=2.2):
    """A mask that removes the anvil (placed by `t`, scale `s`) and a gap around it, so whatever
    it masks sits behind the anvil."""
    return (f'<mask id="{key}" maskUnits="userSpaceOnUse" x="-8" y="-8" width="80" height="80">'
            f'<rect x="-8" y="-8" width="80" height="80" fill="#fff"/>'
            f'<path transform="{t}" d="{ANVIL}" fill="#000" stroke="#000" stroke-width="{f(2 * gap / s)}" '
            f'stroke-linejoin="round"/></mask>')


def bars(x0, step, width, bottom, heights, rx=0.6):
    return ''.join(f'<rect x="{f(x0 + k * step)}" y="{f(bottom - h)}" width="{f(width)}" height="{f(h)}" rx="{rx}"/>'
                   for k, h in enumerate(heights))


def beamed(x1, y1, x2, y2, s=1.0, stem=8.0, beam=2.0):
    """Two eighth notes joined by a beam: heads at (x1, y1) and (x2, y2), stems up."""
    rx, ry, w = 2.4 * s, 1.75 * s, 1.0 * s
    out, tops = '', []
    for x, y in ((x1, y1), (x2, y2)):
        out += f'<ellipse cx="{f(x)}" cy="{f(y)}" rx="{f(rx)}" ry="{f(ry)}" transform="rotate(-20 {f(x)} {f(y)})"/>'
        sx = x + 2.2 * s  # right side of the head
        out += f'<rect x="{f(sx - w)}" y="{f(y - stem * s)}" width="{f(w)}" height="{f(stem * s)}"/>'
        tops.append((sx, y - stem * s))
    (ax, ay), (bx, by) = tops
    out += f'<path d="M{f(ax - w)} {f(ay)}L{f(bx)} {f(by)}L{f(bx)} {f(by + beam * s)}L{f(ax - w)} {f(ay + beam * s)}Z"/>'
    return out


# ---------------------------------------------------------------- the logo and the alternatives
OPTIONS = []


def option(number, name, idea):
    def add(fn):
        OPTIONS.append((number, fn.__name__, name, idea, fn))
        return fn
    return add


@option(3, 'Spectrum sparks with a note', 'The logo since 2026-11-26: analyser bars rise off the face like sparks; '
        'two beamed notes are cut into the body.')
def spectrum_note():
    d, body = anvil('translate(-0.14 12) scale(2)', 'sn', cuts=beamed(29, 41.3, 37.6, 40.3))
    return d, body + bars(26, 3.9, 2.8, 27.4, [6, 10, 15, 12, 8, 13, 9, 5, 3])


@option(5, 'Radial spectrum', 'The anvil inside a ring of analyser bars, like a music visualiser.')
def radial_spectrum():
    t, _, _ = placed(1.2)
    d, body = anvil(t, 'rs')
    n, r0, out = 44, 22.5, ''
    for k in range(n):
        a = 2 * math.pi * k / n
        length = 2 + 6.5 * abs(0.55 * math.sin(3 * a + 0.4) + 0.3 * math.sin(7 * a + 1.3) + 0.15 * math.sin(13 * a))
        out += (f'<rect x="{f(r0)}" y="-0.9" width="{f(length)}" height="1.8" rx="0.9" '
                f'transform="translate(32 32) rotate({f(math.degrees(a))})"/>')
    return d, body + out


@option(6, 'Anvil on keys', 'The anvil stands on a piano keyboard.')
def anvil_on_keys():
    t, _, _ = placed(1.65, feet=36.5)
    d, body = anvil(t, 'ak')
    n, x0, y0, y1 = 9, 4.0, 39.0, 53.0
    kw = (60 - x0) / n
    whites = ''.join(f'<rect x="{f(x0 + i * kw + 0.45)}" y="{f(y0)}" width="{f(kw - 0.9)}" height="{f(y1 - y0)}" rx="1"/>'
                     for i in range(n))
    blacks = ''.join(f'<rect x="{f(x0 + b * kw - 1.9)}" y="{f(y0 - 1)}" width="3.8" height="9.5" rx="0.6"/>'
                     for b in (1, 2, 4, 5, 6, 8))
    return d + mask('akk', blacks), body + f'<g mask="url(#akk)">{whites}</g>'


@option(13, 'On the staff', 'The anvil sits on the five lines of a music staff.')
def on_the_staff():
    s = 1.75
    t, _, _ = placed(s)
    d, body = anvil(t, 'os')
    lines = ''.join(f'<rect x="3" y="{f(y - 0.7)}" width="58" height="1.4" rx="0.7"/>' for y in (16, 24, 32, 40, 48))
    gap = f'<path transform="{t}" d="{ANVIL}" stroke="#000" stroke-width="{f(3.2 / s)}" stroke-linejoin="round"/>'
    return d + mask('osk', gap), body + f'<g mask="url(#osk)">{lines}</g>'


@option(15, 'Waveform behind', 'The anvil in front of a big audio waveform.')
def waveform_behind():
    s = 1.55
    t, _, _ = placed(s, cy=34)
    d, body = anvil(t, 'wb')
    n, x0, x1, c = 21, 3.0, 61.0, 34.0
    step, out = (x1 - x0 - 1.9) / (n - 1), ''
    for k in range(n):
        u = k / (n - 1)
        a = 4 + 22 * math.sin(math.pi * u) ** 1.3 * (0.55 + 0.45 * abs(math.sin(2 * math.pi * 1.7 * u + 0.5)))
        out += f'<rect x="{f(x0 + k * step)}" y="{f(c - a)}" width="1.9" height="{f(2 * a)}" rx="0.95"/>'
    return d + knock('wbk', t, s), f'<g mask="url(#wbk)">{out}</g>' + body


@option(17, 'Spectrum behind', 'The anvil in front of tall analyser bars.')
def spectrum_behind():
    s = 1.5
    t, _, _ = placed(s, cy=36)
    d, body = anvil(t, 'sb')
    cols = bars(3.4, 4.6, 3.2, 56, [14, 22, 30, 38, 44, 40, 46, 42, 34, 28, 36, 24, 16], rx=0.8)
    return d + knock('sbk', t, s), f'<g mask="url(#sbk)">{cols}</g>' + body


# ---------------------------------------------------------------- output
def svg(key, defs, body, size=None, color=None):
    attrs = f' width="{size}" height="{size}"' if size else ''
    style = f' color="{color}"' if color else ''
    # Mask ids are made unique per copy, so one page can hold several copies.
    for i in re.findall(r'id="([^"]+)"', defs):
        defs = defs.replace(f'id="{i}"', f'id="{key}-{i}"')
        defs = defs.replace(f'url(#{i})', f'url(#{key}-{i})')
        body = body.replace(f'url(#{i})', f'url(#{key}-{i})')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"{attrs}{style} fill="currentColor">'
            f'<defs>{defs}</defs>{body}</svg>')


def main():
    for old in glob.glob('[0-9][0-9]-*.svg'):
        os.remove(old)
    cards = []
    for n, fn_name, name, idea, fn in OPTIONS:
        defs, body = fn()
        file = f'{n:02d}-{fn_name.replace("_", "-")}.svg'
        open(file, 'w', encoding='utf-8', newline='\n').write(svg('s', defs, body, color=ACCENT) + '\n')
        cards.append(f'''<figure>
  <div class="tile">{svg(f"t{n}", defs, body)}</div>
  <div class="row"><div class="bar">{svg(f"b{n}", defs, body)}<span>Crucible</span></div><div class="light">{svg(f"l{n}", defs, body)}</div></div>
  <figcaption><b>{n}. {name}</b><span>{idea}</span><code>{file}</code></figcaption>
</figure>''')
    page = f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Crucible logo options</title>
<style>
:root {{ --accent: {ACCENT}; --bg: #0d1115; --card: #161c22; --text: #e6ebef; --dim: #8b97a3; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; padding: 24px 16px 48px; background: var(--bg); color: var(--text); font: 14px/1.45 system-ui, sans-serif; }}
h1 {{ font-size: 20px; margin: 0 0 4px; }} p {{ margin: 0 0 20px; color: var(--dim); max-width: 70ch; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 16px; }}
figure {{ margin: 0; background: var(--card); border-radius: 14px; padding: 14px; display: flex; flex-direction: column; gap: 10px; }}
.tile {{ aspect-ratio: 1; border-radius: 22%; background: {DARK}; color: var(--accent); display: grid; place-items: center; }}
.tile svg {{ width: 76%; height: 76%; }}
.row {{ display: flex; gap: 8px; }}
.bar {{ flex: 1; display: flex; align-items: center; gap: 8px; background: {DARK}; border-radius: 8px; padding: 6px 10px; color: var(--accent); }}
.bar svg {{ width: 26px; height: 26px; }} .bar span {{ color: var(--text); font-weight: 600; font-size: 13px; }}
.light {{ background: #f3f5f7; border-radius: 8px; padding: 6px; color: #1d2a33; display: grid; place-items: center; }}
.light svg {{ width: 26px; height: 26px; }}
figcaption {{ display: flex; flex-direction: column; gap: 2px; }} figcaption span {{ color: var(--dim); }}
code {{ color: var(--dim); font-size: 12px; }}
</style></head><body>
<h1>Crucible logo and saved alternatives</h1>
<p>Number 3 is the logo. The others are saved in case it changes; the numbers are the ones they were picked by. Each is shown as the app icon, at top-bar size, and dark on light. <code>options.py</code> draws them all.</p>
<div class="grid">
{chr(10).join(cards)}
</div></body></html>
'''
    open('index.html', 'w', encoding='utf-8', newline='\n').write(page)
    print(f'{len(OPTIONS)} marks written')


main()
