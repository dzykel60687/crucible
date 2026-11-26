// The arrange timeline: one canvas, drawn only when something changed (or every frame while the
// transport moves). Plain Canvas 2D, no scene graph.
//
// Mouse model (D-027). Every gesture previews locally while the button is down and sends ONE
// engine command on release, so a drag is one undo step and nothing is half-applied:
//   click clip            select it (Shift adds, Ctrl toggles)
//   drag clip body        move the selection in time and across tracks (snaps; Alt = free)
//   Ctrl + drag           copy instead of move
//   drag clip edge        trim that edge (content stays put in time)
//   drag on empty space   rubber-band select; a plain click there moves the playhead
//   double-click empty    new 1-bar MIDI clip on an instrument track
//   double-click clip     open it in the piano roll or clip editor
//   right-click           context menu
import { tp } from './transport'
import { clipPeaks, levelAt } from './wave'
import { beats, ids, PPQ, type Clip } from './engine'
import { bpb, changed, clip, created, redraw, run, selectClips, state, ui } from './store'

// Track colours: cool and muted, so the accent and the red record/clip warnings stand out.
export const COLORS = ['#5aa2ff', '#a78bfa', '#2dd4bf', '#e6b450', '#f47c96', '#7bd88f', '#94a3ff', '#c9a27e']
export const trackColor = (id: number) => COLORS[(id - 1) % COLORS.length]
export const RULER_H = 30
const EDGE = 7 // px from a clip edge that grabs the edge instead of the body
const MIN_PPB = 3, MAX_PPB = 400

export const hooks = {
  openClip: (_c: Clip) => {},
  clipMenu: (_c: Clip, _at: { x: number; y: number }) => {},
  emptyMenu: (_trackId: number | null, _beat: number, _at: { x: number; y: number }) => {},
}

const cv = document.getElementById('timeline') as HTMLCanvasElement
const ctx = cv.getContext('2d', { alpha: false })!
let W = 0, H = 0, dpr = 1

const css = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim()
let C = palette()
function palette() {
  return {
    bg: css('--bg'), surface: css('--surface'), line: css('--line'), lineS: css('--line-strong'), text: css('--text'), dim: css('--text-dim'),
    faint: css('--text-faint'), accent: css('--accent'), rec: css('--rec'), font: css('--font'), mono: css('--mono'), rowH: parseFloat(css('--row-h')) || 72,
  }
}
/** Re-read design tokens: after an accent or layout change. */
export function refreshPalette() { C = palette(); clampScroll(); changed() }
export const rowH = () => C.rowH

new ResizeObserver(() => {
  dpr = window.devicePixelRatio || 1
  W = cv.clientWidth; H = cv.clientHeight
  cv.width = Math.round(W * dpr); cv.height = Math.round(H * dpr)
  clampScroll()
  draw()
}).observe(cv)

const x = (beat: number) => beat * ui.pxPerBeat - ui.scrollX
const beatAt = (px: number) => (px + ui.scrollX) / ui.pxPerBeat
const rowTop = (i: number) => RULER_H + i * C.rowH - ui.scrollY
const rowAt = (py: number) => Math.floor((py - RULER_H + ui.scrollY) / C.rowH)

function contentHeight() { return state.project.tracks.length * C.rowH + 64 }
export function clampScroll() {
  ui.scrollY = Math.max(0, Math.min(ui.scrollY, Math.max(0, contentHeight() - (H - RULER_H))))
  ui.scrollX = Math.max(0, ui.scrollX)
}

/** Snap a beat position to the grid unless snapping is off or Alt is held. */
export function snap(b: number, free = false): number {
  if (free || ui.snap === 0) return Math.round(b * 64) / 64
  return Math.round(b / ui.snap) * ui.snap
}

// ------------------------------------------------------------------ drag state
type Drag =
  | { kind: 'move'; ids: number[]; anchor: number; grab: number; x0: number; y0: number; row0: number; started: boolean; dBeats: number; dRows: number; copy: boolean; toggle: number | null; bad: boolean }
  | { kind: 'trim'; id: number; side: 'start' | 'end'; edge: number }
  | { kind: 'marquee'; x0: number; y0: number; x1: number; y1: number; additive: boolean; base: Set<number> }
  | { kind: 'scrub' }
let drag: Drag | null = null

function rr(x0: number, y0: number, w: number, h: number, r: number) {
  ctx.beginPath()
  ctx.roundRect(x0, y0, w, h, Math.max(0, Math.min(r, w / 2, h / 2)))
}

// ------------------------------------------------------------------ drawing
export function draw() {
  if (!W) return
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  const tracks = state.project.tracks
  const per = bpb()
  ctx.fillStyle = C.bg
  ctx.fillRect(0, 0, W, H)

  ctx.save()
  ctx.beginPath(); ctx.rect(0, RULER_H, W, H - RULER_H); ctx.clip()
  tracks.forEach((t, i) => {
    const y = rowTop(i)
    if (y > H || y + C.rowH < RULER_H) return
    if (t.id === ui.selTrack) { ctx.fillStyle = '#ffffff06'; ctx.fillRect(0, y, W, C.rowH) }
    ctx.fillStyle = C.line; ctx.fillRect(0, y + C.rowH - 1, W, 1)
  })

  // grid: bars always, beats when there is room, sixteenths when zoomed in
  const first = Math.floor(beatAt(0)), last = Math.ceil(beatAt(W))
  const showBeats = ui.pxPerBeat >= 10, show16 = ui.pxPerBeat >= 90
  const barStep = ui.pxPerBeat * per < 24 ? (ui.pxPerBeat * per < 8 ? 8 : 4) : 1
  for (let b = first; b <= last; b++) {
    const bar = b / per, isBar = Number.isInteger(bar)
    if ((!isBar && !showBeats) || (isBar && bar % barStep !== 0)) continue
    ctx.fillStyle = isBar ? C.lineS : C.line
    ctx.globalAlpha = isBar ? 0.9 : 0.55
    ctx.fillRect(Math.round(x(b)), RULER_H, 1, H)
    if (show16) { ctx.globalAlpha = 0.22; for (let k = 1; k < 4; k++) ctx.fillRect(Math.round(x(b + k / 4)), RULER_H, 1, H) }
  }
  ctx.globalAlpha = 1

  // clips: real ones, with the dragged ones drawn where they would land
  const moving = drag?.kind === 'move' && drag.started ? drag : null
  for (const c of state.project.clips) {
    const i = tracks.findIndex((t) => t.id === c.track)
    if (i < 0) continue
    const isMoving = moving?.ids.includes(c.id) ?? false
    if (isMoving) {
      drawClip(c, i, c.start / PPQ, c.length / PPQ, c.offset / PPQ, moving!.copy ? 1 : 0.28, false)
      const j = i + moving!.dRows
      if (j >= 0 && j < tracks.length) drawClip(c, j, c.start / PPQ + moving!.dBeats, c.length / PPQ, c.offset / PPQ, moving!.bad ? 0.45 : 0.9, true, moving!.bad)
      continue
    }
    let s = c.start / PPQ, len = c.length / PPQ, off = c.offset / PPQ
    if (drag?.kind === 'trim' && drag.id === c.id) {
      if (drag.side === 'start') { const d = drag.edge - s; s += d; off += d; len -= d } else len = drag.edge - s
    }
    drawClip(c, i, s, len, off, 1, ui.selClips.has(c.id))
  }

  if (drag?.kind === 'marquee') {
    const [x0, x1] = [Math.min(drag.x0, drag.x1), Math.max(drag.x0, drag.x1)]
    const [y0, y1] = [Math.min(drag.y0, drag.y1), Math.max(drag.y0, drag.y1)]
    ctx.fillStyle = C.accent + '1a'; ctx.strokeStyle = C.accent; ctx.lineWidth = 1
    ctx.fillRect(x0 - ui.scrollX, y0 - ui.scrollY, x1 - x0, y1 - y0)
    ctx.strokeRect(x0 - ui.scrollX + 0.5, y0 - ui.scrollY + 0.5, x1 - x0, y1 - y0)
  }
  ctx.restore()

  // ruler
  ctx.fillStyle = C.surface; ctx.fillRect(0, 0, W, RULER_H)
  ctx.fillStyle = C.line; ctx.fillRect(0, RULER_H - 1, W, 1)
  ctx.font = `500 10.5px ${C.mono}`; ctx.textBaseline = 'middle'
  for (let b = Math.floor(first / per) * per; b <= last; b += per) {
    const bar = Math.round(b / per)
    if (bar % barStep !== 0) continue
    const px = Math.round(x(b))
    ctx.fillStyle = C.lineS; ctx.fillRect(px, 14, 1, RULER_H - 14)
    ctx.fillStyle = C.dim; ctx.fillText(String(bar + 1), px + 5, 20)
  }

  // playhead
  const ph = Math.round(x(tp.pos))
  if (ph >= -1 && ph <= W + 1) {
    ctx.fillStyle = C.accent
    ctx.fillRect(ph, 0, 1.5, H)
    ctx.beginPath(); ctx.moveTo(ph - 5, 0); ctx.lineTo(ph + 6.5, 0); ctx.lineTo(ph + 0.75, 8); ctx.closePath(); ctx.fill()
  }
}

/** One clip at a position in beats, with `offset` = how far into its content it starts. */
function drawClip(c: Clip, row: number, start: number, len: number, offset: number, alpha: number, sel: boolean, bad = false) {
  const t = state.project.tracks[row]
  const cx = x(start), cw = len * ui.pxPerBeat
  const y = rowTop(row) + 4, h = C.rowH - 9
  if (cx > W || cx + cw < 0 || y > H || y + h < RULER_H || cw <= 0) return
  const col = trackColor(t.id)
  ctx.globalAlpha = alpha * (t.mute ? 0.4 : 1)
  rr(cx + 0.5, y + 0.5, cw - 1, h, 6)
  ctx.fillStyle = col + (sel ? '40' : '2e'); ctx.fill()
  ctx.lineWidth = sel ? 2 : 1
  ctx.strokeStyle = bad ? C.rec : sel ? C.text : col + '99'; ctx.stroke()
  ctx.save(); rr(cx + 0.5, y + 0.5, cw - 1, h, 6); ctx.clip()
  const titleH = h > 40 ? 16 : 0
  if (titleH) {
    ctx.fillStyle = col + '55'; ctx.fillRect(cx, y, cw, titleH)
    ctx.fillStyle = C.text; ctx.font = `600 10.5px ${C.font}`; ctx.textBaseline = 'middle'
    if (cw > 30) ctx.fillText(c.name, cx + 6, y + 8.5, cw - 12)
  }
  const by = y + titleH + 2, bh = h - titleH - 5
  ctx.fillStyle = col
  ctx.beginPath() // one path, one fill: far cheaper than a fillRect per pixel or note
  if (c.content.kind === 'audio') {
    const pk = clipPeaks(c) // null while an imported file's peaks load: the clip draws empty for a moment
    const mid = by + bh / 2, gain = Math.pow(10, c.gain_db / 20)
    for (let px = Math.max(cx, 0); pk && px < Math.min(cx + cw, W); px++) {
      const b0 = (px - cx) / ui.pxPerBeat + offset
      const a = Math.min(1, levelAt(pk, b0, b0 + 1 / ui.pxPerBeat) * gain) * bh * 0.48
      ctx.rect(px, mid - a, 1, a * 2 || 1)
    }
  } else if (c.content.notes.length) {
    let lo = 127, hi = 0
    for (const n of c.content.notes) { lo = Math.min(lo, n.pitch); hi = Math.max(hi, n.pitch) }
    const span = Math.max(12, hi - lo + 1), nh = Math.max(2, Math.min(5, bh / span))
    const off = offset * PPQ, end = off + len * PPQ
    for (const n of c.content.notes) {
      if (n.start + n.length <= off || n.start >= end) continue
      const s = Math.max(n.start, off), e = Math.min(n.start + n.length, end)
      const nx = cx + ((s - off) / PPQ) * ui.pxPerBeat
      if (nx > W) continue
      const ny = by + bh - ((n.pitch - lo + 0.5) / span) * bh - nh / 2
      ctx.rect(nx, ny, Math.max(1.5, ((e - s) / PPQ) * ui.pxPerBeat - 1), nh)
    }
  }
  ctx.fill()
  ctx.restore()
  ctx.globalAlpha = 1
}

/** Keep the playhead in view while playing, page by page (no constant scrolling). */
export function follow() {
  const px = x(tp.pos)
  if (px > W * 0.92 || px < 0) ui.scrollX = Math.max(0, tp.pos * ui.pxPerBeat - W * 0.08)
}

export function zoom(factor: number, anchorPx = W / 2) {
  const beat = beatAt(anchorPx)
  ui.pxPerBeat = Math.min(MAX_PPB, Math.max(MIN_PPB, ui.pxPerBeat * factor))
  ui.scrollX = Math.max(0, beat * ui.pxPerBeat - anchorPx)
  redraw()
}

/** Whether moving these clips by `dRows` tracks keeps every one on a track of its kind. The engine
 *  refuses a bad drop anyway; this lets the drag show it before the button is released. */
function dropFits(idList: number[], dRows: number): boolean {
  const tracks = state.project.tracks
  return idList.every((id) => {
    const c = clip(id)
    const t = c && tracks[tracks.findIndex((x) => x.id === c.track) + dRows]
    return !!c && !!t && (t.kind === 'audio') === (c.content.kind === 'audio')
  })
}

// ------------------------------------------------------------------ hit testing
type Hit = { clip?: Clip; edge?: 'start' | 'end'; row: number }
function hit(px: number, py: number): Hit {
  const row = rowAt(py)
  const t = state.project.tracks[row]
  if (!t || py < RULER_H) return { row: -1 }
  // Topmost first: later clips draw over earlier ones.
  for (let k = state.project.clips.length - 1; k >= 0; k--) {
    const c = state.project.clips[k]
    if (c.track !== t.id) continue
    const x0 = x(c.start / PPQ), x1 = x((c.start + c.length) / PPQ)
    if (px < x0 || px > x1) continue
    const room = x1 - x0 > EDGE * 3
    return { clip: c, row, edge: room && px - x0 <= EDGE ? 'start' : room && x1 - px <= EDGE ? 'end' : undefined }
  }
  return { row }
}

// ------------------------------------------------------------------ input
cv.addEventListener('wheel', (e) => {
  e.preventDefault()
  if (e.ctrlKey || e.metaKey) return zoom(Math.exp(-e.deltaY * 0.0022), e.offsetX)
  if (e.shiftKey) ui.scrollX += e.deltaY
  else { ui.scrollY += e.deltaY; ui.scrollX += e.deltaX }
  clampScroll(); redraw()
}, { passive: false })

cv.addEventListener('pointerdown', (e) => {
  ui.focus = 'timeline'
  if (e.button !== 0) return
  try { cv.setPointerCapture(e.pointerId) } catch { /* pointer already gone: the drag still works inside the canvas */ }
  if (e.offsetY < RULER_H) {
    drag = { kind: 'scrub' }
    tp.pos = Math.max(0, snap(beatAt(e.offsetX), e.altKey))
    redraw()
    return
  }
  const h = hit(e.offsetX, e.offsetY)
  if (h.row >= 0) ui.selTrack = state.project.tracks[h.row].id
  const c = h.clip
  if (!c) {
    const additive = e.shiftKey || e.ctrlKey || e.metaKey
    const px = e.offsetX + ui.scrollX, py = e.offsetY + ui.scrollY
    drag = { kind: 'marquee', x0: px, y0: py, x1: px, y1: py, additive, base: additive ? new Set(ui.selClips) : new Set() }
    if (!additive) selectClips([])
    changed()
    return
  }
  if (h.edge) {
    if (!ui.selClips.has(c.id)) selectClips([c.id])
    drag = { kind: 'trim', id: c.id, side: h.edge, edge: (h.edge === 'start' ? c.start : c.start + c.length) / PPQ }
    changed()
    return
  }
  // Selection rules: plain click selects just this (on release, if it was already selected);
  // Shift adds; Ctrl toggles (on release, so Ctrl+drag can copy instead).
  let toggle: number | null = null
  if (e.ctrlKey || e.metaKey) { if (ui.selClips.has(c.id)) toggle = c.id; else selectClips([...ui.selClips, c.id]) }
  else if (e.shiftKey) selectClips([...ui.selClips, c.id])
  else if (!ui.selClips.has(c.id)) selectClips([c.id])
  else { ui.selClip = c.id; toggle = -c.id } // already selected: collapse to it on a plain click
  drag = { kind: 'move', ids: [...ui.selClips], anchor: c.start / PPQ, grab: beatAt(e.offsetX), x0: e.offsetX, y0: e.offsetY, row0: h.row, started: false, dBeats: 0, dRows: 0, copy: false, toggle, bad: false }
  changed()
})

cv.addEventListener('pointermove', (e) => {
  if (!drag) {
    if (e.buttons) return
    const h = e.offsetY < RULER_H ? null : hit(e.offsetX, e.offsetY)
    cv.style.cursor = e.offsetY < RULER_H ? 'text' : h?.edge ? 'ew-resize' : h?.clip ? 'grab' : 'default'
    return
  }
  autoScroll(e.offsetX)
  if (drag.kind === 'scrub') { tp.pos = Math.max(0, snap(beatAt(e.offsetX), e.altKey)); redraw(); return }
  if (drag.kind === 'marquee') { drag.x1 = e.offsetX + ui.scrollX; drag.y1 = e.offsetY + ui.scrollY; marqueeSelect(drag); redraw(); return }
  if (drag.kind === 'trim') {
    const c = clip(drag.id)
    if (c) drag.edge = clampTrim(c, drag.side, snap(beatAt(e.offsetX), e.altKey))
    cv.style.cursor = 'ew-resize'
    redraw()
    return
  }
  // move
  if (!drag.started && Math.hypot(e.offsetX - drag.x0, e.offsetY - drag.y0) < 4) return
  drag.started = true
  const raw = beatAt(e.offsetX) - drag.grab
  drag.dBeats = Math.max(snap(drag.anchor + raw, e.altKey) - drag.anchor, -earliest(drag.ids))
  drag.dRows = rowAt(e.offsetY) - drag.row0
  drag.copy = e.ctrlKey || e.metaKey
  drag.bad = !dropFits(drag.ids, drag.dRows)
  cv.style.cursor = drag.bad ? 'not-allowed' : drag.copy ? 'copy' : 'grabbing'
  redraw()
})

const finish = async (e: PointerEvent) => {
  const d = drag
  drag = null
  if (!d) return
  if (d.kind === 'scrub') return
  if (d.kind === 'marquee') {
    if (Math.hypot(d.x1 - d.x0, d.y1 - d.y0) < 4) {
      // A plain click on empty space: put the playhead there.
      tp.pos = Math.max(0, snap(beatAt(d.x0 - ui.scrollX), e.altKey))
    }
    changed()
    return
  }
  if (d.kind === 'trim') {
    const c = clip(d.id)
    const old = c ? (d.side === 'start' ? c.start : c.start + c.length) / PPQ : d.edge
    if (c && Math.abs(old - d.edge) > 1e-9) await run(`clip.trim #${c.id} --${d.side} ${+d.edge.toFixed(6)}`)
    changed()
    return
  }
  if (!d.started) {
    if (d.toggle !== null && d.toggle > 0) selectClips([...ui.selClips].filter((i) => i !== d.toggle))
    else if (d.toggle !== null) selectClips([-d.toggle])
    changed()
    return
  }
  if (e.ctrlKey || e.metaKey) d.copy = true // Ctrl pressed late still means copy
  if (Math.abs(d.dBeats) < 1e-9 && d.dRows === 0 && !d.copy) { changed(); return }
  const by = `--by ${+d.dBeats.toFixed(6)}`, rows = d.dRows ? ` --tracks ${d.dRows}` : ''
  if (d.copy) {
    const r = await run(`clip.copy ${ids(d.ids)} ${by}${rows}`)
    const made = created(r)
    if (made.length) selectClips(made)
  } else await run(`clip.move ${ids(d.ids)} ${by}${rows}`)
  changed()
}
cv.addEventListener('pointerup', finish)
cv.addEventListener('pointercancel', () => { drag = null; changed() })

cv.addEventListener('dblclick', async (e) => {
  const h = hit(e.offsetX, e.offsetY)
  if (h.clip) return hooks.openClip(h.clip)
  const t = state.project.tracks[h.row]
  if (!t) return
  if (t.kind === 'instrument') {
    const per = bpb()
    const at = Math.floor(beatAt(e.offsetX) / per) * per
    const r = await run(`clip.add #${t.id} ${at} ${per}`)
    const id = r[0]?.data?.id
    if (id) { selectClips([id]); changed() }
  }
})

cv.addEventListener('contextmenu', (e) => {
  e.preventDefault()
  const h = hit(e.offsetX, e.offsetY)
  if (h.clip) {
    if (!ui.selClips.has(h.clip.id)) selectClips([h.clip.id])
    changed()
    hooks.clipMenu(h.clip, { x: e.clientX, y: e.clientY })
  } else {
    const t = state.project.tracks[h.row]
    if (t) ui.selTrack = t.id
    hooks.emptyMenu(t?.id ?? null, snap(beatAt(e.offsetX)), { x: e.clientX, y: e.clientY })
  }
})

function earliest(list: number[]) {
  let m = Infinity
  for (const id of list) { const c = clip(id); if (c) m = Math.min(m, c.start / PPQ) }
  return Number.isFinite(m) ? m : 0
}

/** Keep a trim inside what the content allows: not before bar 1, not past the source, not empty. */
function clampTrim(c: Clip, side: 'start' | 'end', b: number): number {
  const minLen = 1 / 16
  if (side === 'start') {
    const lo = Math.max(0, (c.start - c.offset) / PPQ)
    return Math.min(Math.max(b, lo), (c.start + c.length) / PPQ - minLen)
  }
  const hi = c.content.kind === 'audio' ? (c.start - c.offset + c.content.source_length) / PPQ : Infinity
  return Math.max(Math.min(b, hi), c.start / PPQ + minLen)
}

function marqueeSelect(d: Extract<Drag, { kind: 'marquee' }>) {
  const [b0, b1] = [Math.min(d.x0, d.x1) / ui.pxPerBeat, Math.max(d.x0, d.x1) / ui.pxPerBeat]
  const r0 = Math.floor((Math.min(d.y0, d.y1) - RULER_H) / C.rowH), r1 = Math.floor((Math.max(d.y0, d.y1) - RULER_H) / C.rowH)
  const inside = state.project.clips.filter((c) => {
    const row = state.project.tracks.findIndex((t) => t.id === c.track)
    return row >= r0 && row <= r1 && c.start / PPQ < b1 && (c.start + c.length) / PPQ > b0
  })
  selectClips([...d.base, ...inside.map((c) => c.id)])
}

function autoScroll(px: number) {
  if (px > W - 24) ui.scrollX += 14
  else if (px < 24 && ui.scrollX > 0) ui.scrollX = Math.max(0, ui.scrollX - 14)
}

/** Where pasted clips go: the playhead, on the selected track (or the first). */
/** The track and snapped beat under a point in page coordinates, for file drops. */
export function dropTarget(clientX: number, clientY: number): { track: number | null; at: number } {
  const r = cv.getBoundingClientRect()
  const row = rowAt(clientY - r.top)
  return { track: state.project.tracks[row]?.id ?? null, at: Math.max(0, snap(beatAt(clientX - r.left))) }
}

export function pasteTarget(): { track: number | null; at: number } {
  return { track: ui.selTrack ?? state.project.tracks[0]?.id ?? null, at: beats(Math.round(tp.pos * PPQ)) }
}
