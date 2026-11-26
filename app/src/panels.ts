// Bottom panel: Mixer, Piano roll, Clip editor, Console. One shown at a time.
import { tp } from './transport'
import { clipPeaks, levelAt } from './wave'
import { icon } from './icons'
import { effectMenu, fxLabel, openEffectEditor, openEffectPicker } from './effects'
import { bpb, changed, clip, dbToPos, dragNumber, esc, fmtDb, fmtPan, posToDb, run, state, toggleMute, toggleSolo, ui, type Tab } from './store'
import { trackColor } from './timeline'
import { beats, PPQ, q, type Clip, type Track } from './engine'

const body = document.getElementById('panel-body')!
const tabsEl = document.getElementById('panel-tabs')!
const panelEl = document.getElementById('panel')!
let shownTab: Tab | null = null
const accent = () => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()
let commandNames: string[] = []
export const setCommandNames = (n: string[]) => (commandNames = n)

const TABS: [Tab, string][] = [['mixer', 'Mixer'], ['piano', 'Piano roll'], ['clip', 'Clip editor'], ['console', 'Console']]

export function selectTab(t: Tab) {
  if (ui.tab === t && ui.panelOpen) { renderPanel(); return }
  ui.tab = t
  ui.panelOpen = true
  changed()
}

export function renderTabs() {
  tabsEl.innerHTML = TABS.map(([k, label]) => `<button class="tab ${ui.tab === k && ui.panelOpen ? 'on' : ''}" data-tab="${k}">${label}</button>`).join('')
    + `<span class="spacer"></span><span class="hint">${hint()}</span><button class="btn icon collapse" title="Show or hide the panel (B)">${icon.chevron}</button>`
  tabsEl.querySelectorAll<HTMLButtonElement>('.tab').forEach((b) => (b.onclick = () => {
    const t = b.dataset.tab as Tab
    if (ui.tab === t && ui.panelOpen) ui.panelOpen = false
    else { ui.tab = t; ui.panelOpen = true }
    changed()
  }))
  tabsEl.querySelector<HTMLButtonElement>('.collapse')!.onclick = () => { ui.panelOpen = !ui.panelOpen; changed() }
  panelEl.classList.toggle('collapsed', !ui.panelOpen)
}

function hint() {
  switch (ui.tab) {
    case 'mixer': return 'Volume and pan: drag · double-click resets · Shift for fine'
    case 'piano': return 'Draw tool adds notes · double-click a note deletes it · Del removes the selected one'
    case 'clip': return 'Clip gain is applied before the effects · drag the value'
    case 'console': return 'Every button above is one of these commands'
  }
}

export function renderPanel() {
  if (!ui.panelOpen) { shownTab = null; return }
  const enter = shownTab !== ui.tab
  shownTab = ui.tab
  // Keep the console's half-typed input across redraws.
  const typed = body.querySelector<HTMLInputElement>('.console input')?.value
  const view = document.createElement('div')
  view.className = 'panel-view' + (enter ? ' enter' : '')
  body.replaceChildren(view)
  if (ui.tab === 'mixer') mixer(view)
  else if (ui.tab === 'piano') piano(view)
  else if (ui.tab === 'clip') clipEditor(view)
  else consoleView(view, enter, typed)
}

// ------------------------------------------------------------------ mixer
const meterCanvases = new Map<number, HTMLCanvasElement>()

function mixer(view: HTMLElement) {
  meterCanvases.clear()
  const wrap = document.createElement('div')
  wrap.className = 'mixer'
  for (const t of state.project.tracks) wrap.appendChild(strip(t))
  wrap.appendChild(strip(null))
  view.appendChild(wrap)
  drawMeters()
}

function strip(t: Track | null): HTMLElement {
  const id = t?.id ?? 0
  const gain = t ? t.gain_db : state.project.master_gain_db
  const el = document.createElement('div')
  el.className = 'strip' + (t ? '' : ' master') + (t && t.id === ui.selTrack ? ' sel' : '')
  el.style.setProperty('--c', t ? trackColor(t.id) : 'var(--accent)')
  const effects = t?.effects ?? []
  el.innerHTML = `
    <div class="sname"><i></i><span>${esc(t ? t.name : 'Master')}</span></div>
    <div class="inserts">
      ${effects.map((f, i) => `<button class="insert filled ${f.bypass ? 'off' : ''}" data-fx="${i + 1}" title="${esc(fxLabel(f))}${f.bypass ? ' (bypassed)' : ''}: click to edit, right-click for more">${esc(fxLabel(f))}</button>`).join('')}
      ${t && effects.length < 8 ? '<button class="insert empty" data-slot="fx">+ effect</button>' : ''}
    </div>
    ${t ? `<svg class="pan-knob" viewBox="0 0 34 34" title="Pan"></svg>` : '<div style="height:34px;display:grid;place-items:center;font-size:10.5px;color:var(--text-faint)">STEREO</div>'}
    <div class="fader-area">
      <div class="fader" title="${t ? 'Volume' : 'Master volume'}: drag, Shift for fine, double-click for 0 dB" style="--p:${1 - dbToPos(gain)}"><div class="rail"></div><div class="cap"></div></div>
      <canvas class="meter"></canvas>
    </div>
    <div class="db">${fmtDb(gain)} dB</div>
    <div class="ms">${t ? `<button class="tbtn m ${t.mute ? 'on' : ''}" title="Mute">M</button><button class="tbtn s ${t.solo ? 'on' : ''}" title="Solo">S</button>` : '<span class="tbtn" style="visibility:hidden"></span>'}</div>`
  el.onclick = () => { if (t && ui.selTrack !== t.id) { ui.selTrack = t.id; changed() } }
  meterCanvases.set(id, el.querySelector('canvas')!)

  // fader: absolute position, command on release
  // The cap's place comes from a CSS variable, so it is right in the very first frame. Placing it
  // a frame late made every cap jump whenever the mixer was redrawn (found 2026-11-25).
  const fader = el.querySelector<HTMLElement>('.fader')!, dbEl = el.querySelector<HTMLElement>('.db')!
  const place = (db: number) => {
    fader.style.setProperty('--p', String(1 - dbToPos(db)))
    dbEl.textContent = `${fmtDb(db)} dB`
  }
  const target = t ? `track.gain #${t.id}` : 'master.gain'
  fader.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return
    e.preventDefault(); e.stopPropagation()
    try { fader.setPointerCapture(e.pointerId) } catch { /* synthetic event */ }
    let db = gain
    const startPos = dbToPos(gain), y0 = e.clientY, h = fader.clientHeight - 12
    const mv = (m: PointerEvent) => { db = posToDb(Math.min(1, Math.max(0, startPos + (y0 - m.clientY) / h * (m.shiftKey ? 0.1 : 1)))); place(db) }
    const up = () => {
      fader.removeEventListener('pointermove', mv); fader.removeEventListener('pointerup', up); fader.removeEventListener('pointercancel', up)
      if (db !== gain) run(`${target} ${db}`)
    }
    fader.addEventListener('pointermove', mv); fader.addEventListener('pointerup', up); fader.addEventListener('pointercancel', up)
  })
  fader.addEventListener('dblclick', () => run(`${target} 0`))

  if (t) {
    const knob = el.querySelector<SVGSVGElement>('.pan-knob')!
    const drawKnob = (p: number) => {
      const a = (deg: number) => { const r = (deg - 90) * Math.PI / 180; return [17 + 13 * Math.cos(r), 17 + 13 * Math.sin(r)] }
      const [x0, y0] = a(0), [x1, y1] = a(p * 135)
      const [bx0, by0] = a(-135), [bx1, by1] = a(135)
      knob.innerHTML = `<path d="M${bx0} ${by0} A13 13 0 1 1 ${bx1} ${by1}" stroke="var(--surface-4)" stroke-width="3" fill="none" stroke-linecap="round"/>
        ${Math.abs(p) > 0.005 ? `<path d="M${x0} ${y0} A13 13 0 0 ${p > 0 ? 1 : 0} ${x1} ${y1}" stroke="var(--c)" stroke-width="3" fill="none" stroke-linecap="round"/>` : ''}
        <circle cx="17" cy="17" r="8.5" fill="var(--surface-4)"/><text x="17" y="20.5" text-anchor="middle" font-size="9" fill="var(--text-dim)" font-family="var(--mono)">${fmtPan(p)}</text>`
    }
    drawKnob(t.pan)
    let p = t.pan
    knob.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return
      e.preventDefault(); e.stopPropagation()
      try { knob.setPointerCapture(e.pointerId) } catch { /* synthetic event */ }
      const y0 = e.clientY, p0 = t.pan
      p = p0 // a plain click must not resend a value left over from an earlier drag
      const mv = (m: PointerEvent) => { p = Math.max(-1, Math.min(1, p0 + (y0 - m.clientY) / 120 * (m.shiftKey ? 0.1 : 1))); drawKnob(p) }
      const up = () => {
        knob.removeEventListener('pointermove', mv); knob.removeEventListener('pointerup', up); knob.removeEventListener('pointercancel', up)
        if (Math.round(p * 100) !== Math.round(p0 * 100)) run(`track.pan #${t.id} ${Math.round(p * 100)}`)
      }
      knob.addEventListener('pointermove', mv); knob.addEventListener('pointerup', up); knob.addEventListener('pointercancel', up)
    })
    knob.addEventListener('dblclick', () => run(`track.pan #${t.id} 0`))
    el.querySelector<HTMLButtonElement>('.m')!.onclick = (e) => { e.stopPropagation(); toggleMute(t.id) }
    el.querySelector<HTMLButtonElement>('.s')!.onclick = (e) => { e.stopPropagation(); toggleSolo(t.id) }
    el.querySelectorAll<HTMLButtonElement>('.insert').forEach((b) => {
      const slot = Number(b.dataset.fx)
      b.onclick = (e) => {
        e.stopPropagation()
        if (slot) openEffectEditor(t.id, slot)
        else openEffectPicker(t)
      }
      if (slot) b.oncontextmenu = (e) => { e.preventDefault(); e.stopPropagation(); effectMenu(t, slot, { x: e.clientX, y: e.clientY }) }
    })
  }
  return el
}

const gradients = new WeakMap<CanvasRenderingContext2D, { h: number; g: CanvasGradient }>()
function meterGradient(c: CanvasRenderingContext2D, h: number): CanvasGradient {
  const hit = gradients.get(c)
  if (hit && hit.h === h) return hit.g
  const g = c.createLinearGradient(0, h, 0, 0)
  g.addColorStop(0, '#2fbf7a'); g.addColorStop(0.72, '#3fd18b'); g.addColorStop(0.86, '#f5c542'); g.addColorStop(1, '#ff4757')
  gradients.set(c, { h, g })
  return g
}

/** Draw every visible meter. Called each frame only while something moves. */
export function drawMeters() {
  for (const [id, cv] of meterCanvases) {
    if (!cv.isConnected) continue
    const w = cv.clientWidth, h = cv.clientHeight, dpr = devicePixelRatio || 1
    if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr) }
    const c = cv.getContext('2d')!
    c.setTransform(dpr, 0, 0, dpr, 0, 0)
    c.fillStyle = '#0a0c10'; c.fillRect(0, 0, w, h)
    const lv = tp.levels.get(id) ?? { l: 0, r: 0, pl: 0, pr: 0 }
    const toY = (lin: number) => { const db = 20 * Math.log10(Math.max(lin, 1e-6)); return h - Math.max(0, Math.min(1, (db + 60) / 66)) * h }
    const bw = (w - 3) / 2
    const g = meterGradient(c, h)
    ;[[lv.l, lv.pl], [lv.r, lv.pr]].forEach(([v, p], k) => {
      const x0 = 1 + k * (bw + 1), y = toY(v)
      c.fillStyle = g; c.fillRect(x0, y, bw, h - y)
      if (p > 0.002) { c.fillStyle = p >= 1 ? '#ff4757' : '#e7e9ee'; c.fillRect(x0, toY(p), bw, 1.5) }
    })
    c.fillStyle = '#ffffff22'; c.fillRect(0, toY(1), w, 1)
  }
}

// ------------------------------------------------------------------ piano roll
// Notes are engine data. Gestures preview locally and send one note command on release.
//   click note: select · drag: move (pitch and time, snaps) · drag its right end: resize
//   double-click note: delete · Draw tool: click empty space to add · velocity lane: drag a stem
let pianoTool: 'select' | 'draw' = 'select'
let pianoGrid = 0.25
/** Selected note, by content-relative start tick and pitch (indices change after every command). */
let selNote: { clip: number; pitch: number; start: number } | null = null
/** Highest pitch shown in each clip's piano roll, once the wheel has scrolled it. */
const pitchTop = new Map<number, number>()
const GRIDS: [string, number][] = [['1/16', 0.25], ['1/8', 0.5], ['1/4', 1], ['Off', 0]]

function midiClip(): Clip | undefined {
  const sel = state.project.clips.find((c) => c.id === ui.selClip && c.content.kind === 'midi')
  return sel ?? state.project.clips.find((c) => c.track === ui.selTrack && c.content.kind === 'midi')
}

/** Delete the selected note, if the piano roll has one. Returns whether it did. */
export function deleteSelectedNote(): boolean {
  const c = selNote && clip(selNote.clip)
  if (!c || !selNote) return false
  const s = selNote
  selNote = null
  run(`note.remove #${c.id} ${s.pitch} ${beats(s.start - c.offset)}`)
  return true
}

const sm = (svg: string) => svg.replace('<svg', '<svg width="16" height="16"')

function piano(view: HTMLElement) {
  const c = midiClip()
  const notes = c?.content.kind === 'midi' ? c.content.notes : []
  view.innerHTML = `<div class="editor">
    <div class="toolbar">
      <span class="title">${c ? esc(c.name) : 'Piano roll'}</span>
      <div class="seg"><button data-t="select" class="${pianoTool === 'select' ? 'on' : ''}" title="Select and move (V)">${sm(icon.pointer)}</button><button data-t="draw" class="${pianoTool === 'draw' ? 'on' : ''}" title="Draw notes (D)">${sm(icon.pencil)}</button></div>
      <div class="seg" data-grid>${GRIDS.map(([l, g]) => `<button data-g="${g}" class="${g === pianoGrid ? 'on' : ''}">${l}</button>`).join('')}</div>
      <button class="btn solid quant" ${c ? '' : 'disabled'}>Quantize</button>
      <span class="muted">${c ? `${notes.length} notes · ${+(c.length / PPQ / bpb()).toFixed(2)} bars · drag to move, drag the end to resize, double-click to delete, wheel scrolls pitch` : ''}</span>
    </div>
    ${c ? '<canvas tabindex="0"></canvas>' : '<div class="empty-note">No MIDI clip selected.<br>Double-click a MIDI clip in the timeline, or double-click empty space on an instrument track to make one.</div>'}
  </div>`
  view.querySelectorAll<HTMLButtonElement>('[data-t]').forEach((b) => (b.onclick = () => { pianoTool = b.dataset.t as typeof pianoTool; renderPanel() }))
  view.querySelectorAll<HTMLButtonElement>('[data-g]').forEach((b) => (b.onclick = () => { pianoGrid = +b.dataset.g!; renderPanel() }))
  if (!c) return
  view.querySelector<HTMLButtonElement>('.quant')!.onclick = () => run(`clip.quantize #${c.id} --grid ${pianoGrid || 0.25}`)
  const cv = view.querySelector('canvas')!
  const KEYS = 52, RUL = 22, VEL = 54
  const off = c.offset, len = c.length
  const visible = notes.filter((n) => n.start + n.length > off && n.start < off + len)
  // Pitch window: as many rows as fit at --piano-row or taller, centred on the notes until the
  // wheel scrolls it. Squeezing every note in made rows 4 px tall on a laptop, too small to hit.
  let nLo = 127, nHi = 0
  for (const n of visible) { nLo = Math.min(nLo, n.pitch); nHi = Math.max(nHi, n.pitch) }
  const centre = visible.length ? (nLo + nHi) / 2 : 60
  const minRow = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--piano-row')) || 11
  let lo = 0, hi = 127
  const font = getComputedStyle(document.documentElement).getPropertyValue('--mono').trim()
  const g = () => {
    const w = cv.clientWidth, h = cv.clientHeight
    const rows = Math.max(1, Math.min(128, Math.floor((h - RUL - VEL) / minRow)))
    hi = Math.max(rows - 1, Math.min(127, pitchTop.get(c.id) ?? Math.round(centre + rows / 2)))
    lo = hi - rows + 1
    return { w, h, rowH: (h - RUL - VEL) / rows, ppb: (w - KEYS) / (len / PPQ) }
  }
  cv.addEventListener('wheel', (e) => {
    e.preventDefault()
    g()
    pitchTop.set(c.id, hi - Math.sign(e.deltaY) * 3) // three semitones a notch; g() clamps
    drawRoll()
  }, { passive: false })
  const snapB = (b: number, free: boolean) => (free || !pianoGrid ? Math.round(b * 64) / 64 : Math.round(b / pianoGrid) * pianoGrid)
  // live preview of a drag: which note, and where it would go
  let preview: { pitch: number; start: number; length: number; velocity: number } | null = null
  let previewOf: { pitch: number; start: number } | null = null

  const drawRoll = () => {
    const { w, h, rowH, ppb } = g(), dpr = devicePixelRatio || 1
    if (!w || !h) return
    cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr)
    const k = cv.getContext('2d')!
    k.setTransform(dpr, 0, 0, dpr, 0, 0)
    k.fillStyle = '#0e1014'; k.fillRect(0, 0, w, h)
    const col = trackColor(c.track)
    k.font = `10px ${font}`; k.textBaseline = 'middle'
    for (let p = lo; p <= hi; p++) {
      const y = RUL + (hi - p) * rowH, black = [1, 3, 6, 8, 10].includes(p % 12)
      k.fillStyle = black ? '#12151b' : '#161920'; k.fillRect(KEYS, y, w - KEYS, rowH)
      k.fillStyle = black ? '#1b1f27' : '#d9dce3'; k.fillRect(0, y, KEYS - 2, rowH - 0.5)
      if (p % 12 === 0) { k.fillStyle = '#5a6172'; k.fillText(`C${p / 12 - 1}`, KEYS - 26, y + rowH / 2) }
    }
    const per = bpb()
    for (let s = 0; s <= (len / PPQ) * 4; s++) {
      const xx = KEYS + (s / 4) * ppb
      k.fillStyle = s % (per * 4) === 0 ? '#343b4a' : s % 4 === 0 ? '#262b36' : '#1c2029'
      k.fillRect(Math.round(xx), RUL, 1, h - RUL)
      if (s % (per * 4) === 0) { k.fillStyle = '#a1a8b6'; k.fillText(String(Math.floor((c.start / PPQ + s / 4) / per) + 1), xx + 4, RUL / 2) }
    }
    k.fillStyle = '#14171d'; k.fillRect(KEYS, h - VEL, w - KEYS, VEL)
    k.fillStyle = '#262b36'; k.fillRect(KEYS, h - VEL, w - KEYS, 1); k.fillRect(0, RUL - 1, w, 1)
    const drawNote = (n: { pitch: number; start: number; length: number; velocity: number }, sel: boolean, ghost: boolean) => {
      const xx = KEYS + ((n.start - off) / PPQ) * ppb, y = RUL + (hi - n.pitch) * rowH, nw = Math.max(3, (n.length / PPQ) * ppb - 1)
      k.globalAlpha = ghost ? 0.3 : 0.45 + 0.55 * n.velocity / 127
      k.fillStyle = sel ? '#e7e9ee' : col
      k.beginPath(); k.roundRect(xx, y + 1, nw, Math.max(2, rowH - 2), 2); k.fill()
      k.globalAlpha = ghost ? 0.3 : 1
      const vh = (n.velocity / 127) * (VEL - 10)
      k.fillRect(xx, h - 4 - vh, 2, vh); k.fillRect(xx - 2, h - 4 - vh, 6, 2)
      k.globalAlpha = 1
    }
    for (const n of visible) {
      const isSel = !!selNote && selNote.clip === c.id && selNote.pitch === n.pitch && selNote.start === n.start
      const isPrev = !!previewOf && previewOf.pitch === n.pitch && previewOf.start === n.start
      drawNote(n, isSel && !preview, isPrev && !!preview)
    }
    if (preview) drawNote(preview, true, false)
    const ph = tp.pos - c.start / PPQ
    if (ph >= 0 && ph <= len / PPQ) { k.fillStyle = accent(); k.fillRect(KEYS + ph * ppb, 0, 1.5, h - VEL) }
  }
  new ResizeObserver(drawRoll).observe(cv)

  const noteAt = (px: number, py: number) => {
    const { rowH, ppb } = g()
    const tick = off + ((px - KEYS) / ppb) * PPQ, pitch = hi - Math.floor((py - RUL) / rowH)
    for (let i = visible.length - 1; i >= 0; i--) {
      const n = visible[i]
      if (n.pitch !== pitch || tick < n.start || tick >= n.start + n.length) continue
      const endPx = KEYS + ((n.start + n.length - off) / PPQ) * ppb
      return { n, edge: endPx - px <= 6 && (n.length / PPQ) * ppb > 12 }
    }
    return null
  }
  const rel = (t: number) => beats(t - off) // content tick -> beats from clip start

  cv.addEventListener('pointerdown', (e) => {
    ui.focus = 'piano'
    if (e.button !== 0 || e.offsetX < KEYS) return
    const { h, rowH, ppb } = g()
    try { cv.setPointerCapture(e.pointerId) } catch { /* pointer already gone */ }
    // velocity lane: drag the nearest stem
    if (e.offsetY > h - VEL) {
      const tick = off + ((e.offsetX - KEYS) / ppb) * PPQ
      const n = visible.reduce<typeof visible[0] | null>((best, x) => (Math.abs(x.start - tick) < Math.abs((best?.start ?? Infinity) - tick) ? x : best), null)
      if (!n || Math.abs(((n.start - tick) / PPQ) * ppb) > 8) return
      selNote = { clip: c.id, pitch: n.pitch, start: n.start }
      previewOf = { pitch: n.pitch, start: n.start }
      const setV = (py: number) => { preview = { ...n, velocity: Math.max(1, Math.min(127, Math.round(((h - 4 - py) / (VEL - 10)) * 127))) }; drawRoll() }
      setV(e.offsetY)
      const mv = (m: PointerEvent) => setV(m.offsetY)
      const end = (commit: boolean) => {
        cv.removeEventListener('pointermove', mv); cv.removeEventListener('pointerup', up); cv.removeEventListener('pointercancel', cancel)
        const v = preview?.velocity
        preview = previewOf = null
        if (commit && v && v !== n.velocity) run(`note.edit #${c.id} ${n.pitch} ${rel(n.start)} --velocity ${v}`)
        else drawRoll()
      }
      const up = () => end(true), cancel = () => end(false)
      cv.addEventListener('pointermove', mv); cv.addEventListener('pointerup', up); cv.addEventListener('pointercancel', cancel)
      return
    }
    if (e.offsetY < RUL) return
    const hitN = noteAt(e.offsetX, e.offsetY)
    if (!hitN) {
      selNote = null
      if (pianoTool === 'draw') {
        const b = snapB((e.offsetX - KEYS) / ppb, e.altKey), pitch = hi - Math.floor((e.offsetY - RUL) / rowH)
        const l = pianoGrid || 0.25
        selNote = { clip: c.id, pitch, start: off + Math.round(b * PPQ) }
        run(`note.add #${c.id} ${pitch} ${b} ${l}`)
      } else drawRoll()
      return
    }
    const n = hitN.n
    selNote = { clip: c.id, pitch: n.pitch, start: n.start }
    previewOf = { pitch: n.pitch, start: n.start }
    const x0 = e.offsetX, y0 = e.offsetY
    let moved = false
    const mv = (m: PointerEvent) => {
      if (!moved && Math.hypot(m.offsetX - x0, m.offsetY - y0) < 3) return
      moved = true
      if (hitN.edge) {
        const end = snapB((n.start + n.length - off) / PPQ + (m.offsetX - x0) / ppb, m.altKey)
        preview = { ...n, length: Math.max(PPQ / 64, Math.round((end - (n.start - off) / PPQ) * PPQ)) }
      } else {
        const s = snapB((n.start - off) / PPQ + (m.offsetX - x0) / ppb, m.altKey)
        const dp = -Math.round((m.offsetY - y0) / rowH)
        preview = { ...n, start: off + Math.max(-off, Math.round(s * PPQ)), pitch: Math.max(0, Math.min(127, n.pitch + dp)) }
      }
      drawRoll()
    }
    const end = (commit: boolean) => {
      cv.removeEventListener('pointermove', mv); cv.removeEventListener('pointerup', up); cv.removeEventListener('pointercancel', cancel)
      const p = preview
      preview = previewOf = null
      if (!commit || !moved || !p) { drawRoll(); return }
      const args: string[] = []
      if (p.pitch !== n.pitch) args.push(`--to-pitch ${p.pitch}`)
      if (p.start !== n.start) args.push(`--to-start ${rel(p.start)}`)
      if (p.length !== n.length) args.push(`--length ${beats(p.length)}`)
      if (!args.length) { drawRoll(); return }
      selNote = { clip: c.id, pitch: p.pitch, start: p.start }
      run(`note.edit #${c.id} ${n.pitch} ${rel(n.start)} ${args.join(' ')}`)
    }
    const up = () => end(true), cancel = () => end(false)
    cv.addEventListener('pointermove', mv); cv.addEventListener('pointerup', up); cv.addEventListener('pointercancel', cancel)
  })
  cv.addEventListener('dblclick', (e) => {
    const hitN = noteAt(e.offsetX, e.offsetY)
    if (hitN) { selNote = null; run(`note.remove #${c.id} ${hitN.n.pitch} ${rel(hitN.n.start)}`) }
  })
  cv.addEventListener('pointermove', (e) => {
    if (e.buttons) return
    const hn = e.offsetX >= KEYS && e.offsetY >= RUL ? noteAt(e.offsetX, e.offsetY) : null
    cv.style.cursor = hn?.edge ? 'ew-resize' : hn ? 'grab' : pianoTool === 'draw' && e.offsetX >= KEYS && e.offsetY >= RUL ? 'crosshair' : 'default'
  })
}

// ------------------------------------------------------------------ clip editor
function clipEditor(view: HTMLElement) {
  const c = state.project.clips.find((x) => x.id === ui.selClip && x.content.kind === 'audio') ?? state.project.clips.find((x) => x.track === ui.selTrack && x.content.kind === 'audio')
  const per = bpb()
  const pos = (t: number) => { const b = t / PPQ; return `${Math.floor(b / per) + 1}.${Math.floor(b % per) + 1}.${Math.floor((b % 1) * 4) + 1}` }
  const src = c?.content.kind === 'audio' ? c.content : null
  view.innerHTML = `<div class="editor">
    <div class="toolbar"><span class="title">${c ? esc(c.name) : 'Clip editor'}</span>
      ${c ? `<button class="btn solid" data-split title="Split at the playhead (Ctrl+E)">Split at playhead</button><button class="btn" data-del title="Delete (Del)">Delete</button>` : ''}
      <span class="muted">${src ? (src.frames ? `${esc(src.source)} · ${src.rate / 1000} kHz${src.rate !== state.project.sample_rate ? ` <span class="tag demo" title="The file is kept as it is; its sound is converted while it plays">converted to ${state.project.sample_rate / 1000} kHz</span>` : ''} · ${(src.frames / src.rate).toFixed(2)} s` : `${esc(src.source)} <span class="tag demo">no file</span>`) : ''}</span></div>
    ${c && src ? `<div class="split"><canvas></canvas><div class="props">
      <label>Name <span class="val name-edit" title="Click to rename">${esc(c.name)}</span></label>
      <label>Start <span class="val">${pos(c.start)}</span></label>
      <label>Length <span class="val">${+(c.length / PPQ / per).toFixed(3)} bars</span></label>
      <label>From source <span class="val">${+(c.offset / PPQ).toFixed(3)} beats in</span></label>
      <label>Gain <span class="val knob-num" data-gain title="Drag; double-click for 0 dB">${fmtDb(c.gain_db)} dB</span></label>
      <label>Track <span class="val">${esc(state.project.tracks.find((t) => t.id === c.track)?.name ?? '')}</span></label>
      <label>Sample rate <span class="val">${src && src.frames && src.rate !== state.project.sample_rate ? `file ${src.rate} Hz, converted to ${state.project.sample_rate} Hz` : `${state.project.sample_rate} Hz, not converted`}</span></label>
    </div></div>` : '<div class="empty-note">No audio clip selected.<br>Double-click an audio clip in the timeline.</div>'}
  </div>`
  if (!c || !src) return
  view.querySelector<HTMLButtonElement>('[data-split]')!.onclick = () => run(`clip.split #${c.id} ${beats(Math.round(tp.pos * PPQ))}`)
  view.querySelector<HTMLButtonElement>('[data-del]')!.onclick = () => run(`clip.remove #${c.id}`)
  const nameEl = view.querySelector<HTMLElement>('.name-edit')!
  nameEl.style.cursor = 'text'
  nameEl.onclick = () => {
    const input = document.createElement('input')
    input.className = 'input'; input.value = c.name; input.style.height = '26px'
    nameEl.replaceChildren(input); input.focus(); input.select()
    const done = (ok: boolean) => { if (ok && input.value.trim() && input.value.trim() !== c.name) run(`clip.rename #${c.id} ${q(input.value.trim())}`); else renderPanel() }
    input.onkeydown = (e) => { e.stopPropagation(); if (e.key === 'Enter') done(true); if (e.key === 'Escape') done(false) }
    input.onblur = () => done(true)
  }
  const gainEl = view.querySelector<HTMLElement>('[data-gain]')!
  dragNumber(gainEl, () => c.gain_db, { min: -60, max: 24, perPx: 0.25, step: 0.1, reset: 0, preview: (v) => { gainEl.textContent = `${fmtDb(v)} dB`; drawWave(v) }, commit: (v) => run(`clip.gain #${c.id} ${+v.toFixed(1)}`) })
  const cv = view.querySelector('canvas')!
  const drawWave = (gainDb = c.gain_db) => {
    const w = cv.clientWidth, h = cv.clientHeight, dpr = devicePixelRatio || 1
    if (!w || !h) return
    cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr)
    const k = cv.getContext('2d')!
    k.setTransform(dpr, 0, 0, dpr, 0, 0)
    k.fillStyle = '#0e1014'; k.fillRect(0, 0, w, h)
    const col = trackColor(c.track), pk = clipPeaks(c)
    const mid = h / 2, lenB = c.length / PPQ, offB = c.offset / PPQ, gain = Math.pow(10, gainDb / 20)
    k.fillStyle = '#262b36'; k.fillRect(0, mid, w, 1)
    for (let b = 0; b <= lenB; b++) { k.fillStyle = (b + c.start / PPQ) % per === 0 ? '#343b4a' : '#1c2029'; k.fillRect(Math.round((b / lenB) * w), 0, 1, h) }
    k.fillStyle = col
    k.beginPath()
    for (let px = 0; pk && px < w; px++) {
      const v = levelAt(pk, offB + (px / w) * lenB, offB + ((px + 1) / w) * lenB)
      const a = Math.min(1, v * gain) * (h / 2 - 12)
      k.rect(px, mid - a, 1, a * 2 || 1)
    }
    k.fill()
    if (gain > 1) { k.fillStyle = '#ff475733'; k.fillRect(0, 0, w, 10); k.fillRect(0, h - 10, w, 10) }
    const ph = (tp.pos - c.start / PPQ) / lenB
    if (ph >= 0 && ph <= 1) { k.fillStyle = accent(); k.fillRect(ph * w, 0, 1.5, h) }
  }
  new ResizeObserver(() => drawWave()).observe(cv)
}

// ------------------------------------------------------------------ console
const history: string[] = []
let hIndex = 0

function consoleView(view: HTMLElement, enter: boolean, typed?: string) {
  view.innerHTML = `<div class="console"><div class="out"></div><form><span>›</span><input spellcheck="false" autocomplete="off" placeholder="type a command, e.g. track.add Strings --kind instrument  ·  help  ·  Tab completes"></form></div>`
  const out = view.querySelector<HTMLElement>('.out')!
  out.innerHTML = ui.log.map((l) => `<div class="${l.kind}">${l.kind === 'cmd' ? '› ' : ''}${esc(l.text)}</div>`).join('')
  out.scrollTop = out.scrollHeight
  const input = view.querySelector('input')!
  if (typed) input.value = typed
  if (enter || typed !== undefined) input.focus()
  view.querySelector('form')!.onsubmit = (e) => {
    e.preventDefault()
    const line = input.value.trim()
    if (!line) return
    history.push(line); hIndex = history.length
    input.value = ''
    run(line)
  }
  input.onkeydown = (e) => {
    if (e.key === 'ArrowUp' && hIndex > 0) { input.value = history[--hIndex]; e.preventDefault() }
    else if (e.key === 'ArrowDown') { hIndex = Math.min(history.length, hIndex + 1); input.value = history[hIndex] ?? ''; e.preventDefault() }
    else if (e.key === 'Tab') {
      e.preventDefault()
      const word = input.value.split(' ')[0]
      const m = commandNames.filter((n) => n.startsWith(word))
      if (m.length === 1) input.value = m[0] + ' '
      else if (m.length > 1) { ui.log.push({ kind: 'help', text: m.join('   ') }); renderPanel() }
    }
    e.stopPropagation()
  }
}
