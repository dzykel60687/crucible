// The arrange timeline: one canvas, drawn only when something changed (or every frame while the
// transport moves). Plain Canvas 2D, no scene graph. Positions are in beats (quarter notes).
import { demo, PEAKS_PER_BEAT, type Clip } from './demo'
import { bpb, changed, redraw, state, ui } from './store'

export const COLORS = ['#ff7a2f', '#5aa2ff', '#3fd18b', '#c084fc', '#f5c542', '#ff5c8a', '#2dd4bf', '#a3a3ff']
export const trackColor = (id: number) => COLORS[(id - 1) % COLORS.length]
export const ROW_H = 72
export const RULER_H = 30
const MIN_PPB = 3, MAX_PPB = 400

const cv = document.getElementById('timeline') as HTMLCanvasElement
const ctx = cv.getContext('2d', { alpha: false })!
let W = 0, H = 0, dpr = 1
let onOpenClip: (c: Clip) => void = () => {}
export const setOnOpenClip = (f: (c: Clip) => void) => (onOpenClip = f)

const css = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim()
let C = palette()
function palette() {
  return { bg: css('--bg'), surface: css('--surface'), s2: css('--surface-2'), s3: css('--surface-3'), line: css('--line'), lineS: css('--line-strong'), text: css('--text'), dim: css('--text-dim'), faint: css('--text-faint'), accent: css('--accent'), rec: css('--rec'), font: css('--font'), mono: css('--mono') }
}
export function refreshPalette() { C = palette(); changed() }

new ResizeObserver(() => {
  dpr = window.devicePixelRatio || 1
  W = cv.clientWidth; H = cv.clientHeight
  cv.width = Math.round(W * dpr); cv.height = Math.round(H * dpr)
  clampScroll()
  draw()
}).observe(cv)

const x = (beat: number) => (beat * ui.pxPerBeat) - ui.scrollX
const beatAt = (px: number) => (px + ui.scrollX) / ui.pxPerBeat
export const visibleBeats = () => W / ui.pxPerBeat

function contentHeight() { return state.project.tracks.length * ROW_H + 64 }
export function clampScroll() {
  ui.scrollY = Math.max(0, Math.min(ui.scrollY, Math.max(0, contentHeight() - (H - RULER_H))))
  ui.scrollX = Math.max(0, ui.scrollX)
}

function rr(x0: number, y0: number, w: number, h: number, r: number) {
  ctx.beginPath()
  ctx.roundRect(x0, y0, w, h, Math.min(r, w / 2, h / 2))
}

export function draw() {
  if (!W) return
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  const tracks = state.project.tracks
  const per = bpb()
  ctx.fillStyle = C.bg
  ctx.fillRect(0, 0, W, H)

  // rows
  ctx.save()
  ctx.beginPath(); ctx.rect(0, RULER_H, W, H - RULER_H); ctx.clip()
  tracks.forEach((t, i) => {
    const y = RULER_H + i * ROW_H - ui.scrollY
    if (y > H || y + ROW_H < RULER_H) return
    if (t.id === ui.selTrack) { ctx.fillStyle = '#ffffff06'; ctx.fillRect(0, y, W, ROW_H) }
    ctx.fillStyle = C.line; ctx.fillRect(0, y + ROW_H - 1, W, 1)
  })

  // loop region
  if (demo.loop) {
    ctx.fillStyle = '#ff7a2f0d'
    ctx.fillRect(x(demo.loopStart), RULER_H, (demo.loopEnd - demo.loopStart) * ui.pxPerBeat, H)
  }

  // grid
  const first = Math.floor(beatAt(0)), last = Math.ceil(beatAt(W))
  const showBeats = ui.pxPerBeat >= 10, show16 = ui.pxPerBeat >= 90
  const barStep = ui.pxPerBeat * per < 24 ? (ui.pxPerBeat * per < 8 ? 8 : 4) : 1
  for (let b = first; b <= last; b++) {
    const bar = b / per
    const isBar = Number.isInteger(bar)
    if (!isBar && !showBeats) continue
    if (isBar && bar % barStep !== 0) continue
    ctx.fillStyle = isBar ? C.lineS : C.line
    ctx.globalAlpha = isBar ? 0.9 : 0.55
    ctx.fillRect(Math.round(x(b)), RULER_H, 1, H)
    if (show16) {
      ctx.globalAlpha = 0.22
      for (let k = 1; k < 4; k++) ctx.fillRect(Math.round(x(b + k / 4)), RULER_H, 1, H)
    }
  }
  ctx.globalAlpha = 1

  // clips
  for (const c of demo.clips) {
    const i = tracks.findIndex((t) => t.id === c.track)
    if (i < 0) continue
    const t = tracks[i]
    const cx = x(c.start), cw = c.len * ui.pxPerBeat
    const y = RULER_H + i * ROW_H - ui.scrollY + 4, h = ROW_H - 9
    if (cx > W || cx + cw < 0 || y > H || y + h < RULER_H) continue
    const col = trackColor(t.id)
    const sel = c.id === ui.selClip
    ctx.globalAlpha = t.mute ? 0.4 : 1
    rr(cx + 0.5, y + 0.5, cw - 1, h, 6)
    ctx.fillStyle = col + '2e'; ctx.fill()
    ctx.lineWidth = sel ? 2 : 1
    ctx.strokeStyle = sel ? C.text : col + '99'; ctx.stroke()
    // title strip
    ctx.save(); rr(cx + 0.5, y + 0.5, cw - 1, h, 6); ctx.clip()
    ctx.fillStyle = col + '55'; ctx.fillRect(cx, y, cw, 16)
    ctx.fillStyle = C.text; ctx.font = `600 10.5px ${C.font}`; ctx.textBaseline = 'middle'
    if (cw > 30) ctx.fillText(c.name, cx + 6, y + 8.5, cw - 12)
    const by = y + 18, bh = h - 21
    if (c.peaks) {
      ctx.fillStyle = col
      const mid = by + bh / 2
      const step = Math.max(1, Math.floor(PEAKS_PER_BEAT / ui.pxPerBeat))
      const x0 = Math.max(cx, 0), x1 = Math.min(cx + cw, W)
      ctx.beginPath() // one path, one fill: far cheaper than a fillRect per pixel
      for (let px = x0; px < x1; px += 1) {
        const k = Math.floor(((px - cx) / ui.pxPerBeat) * PEAKS_PER_BEAT)
        let v = 0
        for (let j = 0; j < step; j++) v = Math.max(v, c.peaks[k + j] ?? 0)
        const a = v * bh * 0.48
        ctx.rect(px, mid - a, 1, a * 2 || 1)
      }
      ctx.fill()
    } else if (c.notes?.length) {
      let lo = 127, hi = 0
      for (const n of c.notes) { lo = Math.min(lo, n.pitch); hi = Math.max(hi, n.pitch) }
      const span = Math.max(12, hi - lo + 1), nh = Math.max(2, Math.min(5, bh / span))
      ctx.fillStyle = col
      ctx.beginPath()
      for (const n of c.notes) {
        const nx = cx + n.start * ui.pxPerBeat
        if (nx > W || nx + n.len * ui.pxPerBeat < 0) continue
        const ny = by + bh - ((n.pitch - lo + 0.5) / span) * bh - nh / 2
        ctx.rect(nx, ny, Math.max(1.5, n.len * ui.pxPerBeat - 1), nh)
      }
      ctx.fill()
    }
    ctx.restore()
    ctx.globalAlpha = 1
  }
  ctx.restore()

  // ruler
  ctx.fillStyle = C.surface; ctx.fillRect(0, 0, W, RULER_H)
  ctx.fillStyle = C.line; ctx.fillRect(0, RULER_H - 1, W, 1)
  if (demo.loop) {
    ctx.fillStyle = demo.loop ? C.accent : C.faint
    ctx.globalAlpha = 0.85
    rr(x(demo.loopStart), 3, (demo.loopEnd - demo.loopStart) * ui.pxPerBeat, 6, 3); ctx.fill()
    ctx.globalAlpha = 1
  }
  ctx.font = `500 10.5px ${C.mono}`; ctx.textBaseline = 'middle'
  for (let b = Math.floor(first / per) * per; b <= last; b += per) {
    const bar = Math.round(b / per)
    if (bar % barStep !== 0) continue
    const px = Math.round(x(b))
    ctx.fillStyle = C.lineS; ctx.fillRect(px, 14, 1, RULER_H - 14)
    ctx.fillStyle = C.dim; ctx.fillText(String(bar + 1), px + 5, 20)
  }

  // playhead
  const ph = Math.round(x(demo.pos))
  if (ph >= -1 && ph <= W + 1) {
    ctx.fillStyle = demo.recording && demo.playing ? C.rec : C.accent
    ctx.fillRect(ph, 0, 1.5, H)
    ctx.beginPath(); ctx.moveTo(ph - 5, 0); ctx.lineTo(ph + 6.5, 0); ctx.lineTo(ph + 0.75, 8); ctx.closePath(); ctx.fill()
  }
}

/** Keep the playhead in view while playing, page by page (no constant scrolling). */
export function follow() {
  const px = x(demo.pos)
  if (px > W * 0.92 || px < 0) { ui.scrollX = Math.max(0, demo.pos * ui.pxPerBeat - W * 0.08); }
}

export function zoom(factor: number, anchorPx = W / 2) {
  const beat = beatAt(anchorPx)
  ui.pxPerBeat = Math.min(MAX_PPB, Math.max(MIN_PPB, ui.pxPerBeat * factor))
  ui.scrollX = Math.max(0, beat * ui.pxPerBeat - anchorPx)
  redraw()
}

// ------------------------------------------------------------------ input
function hit(px: number, py: number): { clip?: Clip; trackIndex: number } {
  const i = Math.floor((py - RULER_H + ui.scrollY) / ROW_H)
  const t = state.project.tracks[i]
  if (!t) return { trackIndex: -1 }
  const b = beatAt(px)
  const clip = demo.clips.find((c) => c.track === t.id && b >= c.start && b < c.start + c.len)
  return { clip, trackIndex: i }
}

cv.addEventListener('wheel', (e) => {
  e.preventDefault()
  if (e.ctrlKey || e.metaKey) return zoom(Math.exp(-e.deltaY * 0.0022), e.offsetX)
  if (e.shiftKey) ui.scrollX += e.deltaY
  else { ui.scrollY += e.deltaY; ui.scrollX += e.deltaX }
  clampScroll(); redraw()
}, { passive: false })

cv.addEventListener('pointerdown', (e) => {
  if (e.button !== 0) return
  cv.setPointerCapture(e.pointerId)
  if (e.offsetY < RULER_H) {
    const scrub = (px: number) => { demo.pos = Math.max(0, Math.round(beatAt(px) * 4) / 4); redraw() }
    scrub(e.offsetX)
    const mv = (m: PointerEvent) => scrub(m.offsetX)
    const up = () => { cv.removeEventListener('pointermove', mv); cv.removeEventListener('pointerup', up) }
    cv.addEventListener('pointermove', mv); cv.addEventListener('pointerup', up)
    return
  }
  const h = hit(e.offsetX, e.offsetY)
  if (h.trackIndex >= 0) ui.selTrack = state.project.tracks[h.trackIndex].id
  ui.selClip = h.clip?.id ?? null
  changed()
  const clip = h.clip
  if (!clip) return
  const grab = beatAt(e.offsetX) - clip.start
  const mv = (m: PointerEvent) => {
    const snap = m.altKey ? 1 / 16 : 1
    clip.start = Math.max(0, Math.round((beatAt(m.offsetX) - grab) / snap) * snap)
    const j = Math.floor((m.offsetY - RULER_H + ui.scrollY) / ROW_H)
    const t = state.project.tracks[j]
    if (t && (t.kind === 'audio') === (clip.kind === 'audio')) clip.track = t.id
    redraw()
  }
  const up = () => { cv.removeEventListener('pointermove', mv); cv.removeEventListener('pointerup', up); changed() }
  cv.addEventListener('pointermove', mv); cv.addEventListener('pointerup', up)
})

cv.addEventListener('dblclick', (e) => {
  const h = hit(e.offsetX, e.offsetY)
  if (h.clip) onOpenClip(h.clip)
})

cv.addEventListener('pointermove', (e) => {
  if (e.buttons) return
  cv.style.cursor = e.offsetY < RULER_H ? 'text' : hit(e.offsetX, e.offsetY).clip ? 'grab' : 'default'
})
