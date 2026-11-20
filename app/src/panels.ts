// Bottom panel: Mixer, Piano roll, Clip editor, Console. One shown at a time.
import { demo, type Clip } from './demo'
import { icon } from './icons'
import { openPluginPicker } from './dialogs'
import { bpb, changed, esc, fmtDb, fmtPan, run, state, ui, type Tab } from './store'
import { trackColor } from './timeline'
import type { Track } from './engine'

const body = document.getElementById('panel-body')!
const tabsEl = document.getElementById('panel-tabs')!
const panelEl = document.getElementById('panel')!
let shownTab: Tab | null = null
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
    case 'mixer': return 'Drag faders and knobs · double-click resets · Shift for fine'
    case 'piano': return 'Double-click a MIDI clip to edit it'
    case 'clip': return 'Double-click an audio clip to edit it'
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

const FADER_EXP = 1.6
const dbToPos = (db: number) => (db <= -100 ? 0 : Math.pow(Math.min(1, Math.max(0, (db + 60) / 72)), 1 / FADER_EXP))
const posToDb = (p: number) => (p <= 0.01 ? -144 : Math.round((72 * Math.pow(p, FADER_EXP) - 60) * 10) / 10)

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
  const inserts = t ? (demo.inserts.get(t.id) ?? []) : []
  const inst = t?.kind === 'instrument' ? (demo.instrument.get(t.id) ?? null) : null
  el.innerHTML = `
    <div class="sname"><i></i><span>${esc(t ? t.name : 'Master')}</span></div>
    <div class="inserts">
      ${t?.kind === 'instrument' ? `<button class="insert ${inst ? 'filled' : 'empty'}" data-slot="inst">${inst ? esc(inst) : '+ instrument'}</button>` : ''}
      ${inserts.map((n) => `<button class="insert filled">${esc(n)}</button>`).join('')}
      ${inserts.length < 4 ? '<button class="insert empty" data-slot="fx">+ effect</button>' : ''}
    </div>
    ${t ? `<svg class="pan-knob" viewBox="0 0 34 34" title="Pan"></svg>` : '<div style="height:34px;display:grid;place-items:center;font-size:10.5px;color:var(--text-faint)">STEREO</div>'}
    <div class="fader-area">
      <div class="fader" title="${t ? 'Gain' : 'Master gain'}"><div class="rail"></div><div class="cap"></div></div>
      <canvas class="meter"></canvas>
    </div>
    <div class="db">${fmtDb(gain)} dB</div>
    <div class="ms">${t ? `<button class="tbtn m ${t.mute ? 'on' : ''}">M</button><button class="tbtn s ${t.solo ? 'on' : ''}">S</button>` : `<button class="tbtn m ${demo.mono ? 'on' : ''}" title="Mono check">Mo</button>`}</div>`
  el.onclick = () => { if (t && ui.selTrack !== t.id) { ui.selTrack = t.id; changed() } }
  meterCanvases.set(id, el.querySelector('canvas')!)

  // fader: absolute position, command on release
  const fader = el.querySelector<HTMLElement>('.fader')!, cap = fader.querySelector<HTMLElement>('.cap')!, dbEl = el.querySelector<HTMLElement>('.db')!
  const place = (db: number) => {
    const h = fader.clientHeight - 12
    cap.style.transform = `translateY(${6 + (1 - dbToPos(db)) * h}px)`
    dbEl.textContent = `${fmtDb(db)} dB`
  }
  requestAnimationFrame(() => place(gain))
  const target = t ? `track.gain #${t.id}` : 'master.gain'
  fader.addEventListener('pointerdown', (e) => {
    e.preventDefault(); e.stopPropagation()
    fader.setPointerCapture(e.pointerId)
    let db = gain
    const startPos = dbToPos(gain), y0 = e.clientY, h = fader.clientHeight - 12
    const mv = (m: PointerEvent) => { db = posToDb(Math.min(1, Math.max(0, startPos + (y0 - m.clientY) / h * (m.shiftKey ? 0.1 : 1)))); place(db) }
    const up = () => {
      fader.removeEventListener('pointermove', mv); fader.removeEventListener('pointerup', up)
      if (db !== gain) run(`${target} ${db}`)
    }
    fader.addEventListener('pointermove', mv); fader.addEventListener('pointerup', up)
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
      e.preventDefault(); e.stopPropagation(); knob.setPointerCapture(e.pointerId)
      const y0 = e.clientY, p0 = t.pan
      const mv = (m: PointerEvent) => { p = Math.max(-1, Math.min(1, p0 + (y0 - m.clientY) / 120 * (m.shiftKey ? 0.1 : 1))); drawKnob(p) }
      const up = () => { knob.removeEventListener('pointermove', mv); knob.removeEventListener('pointerup', up); if (p !== p0) run(`track.pan #${t.id} ${Math.round(p * 100)}`) }
      knob.addEventListener('pointermove', mv); knob.addEventListener('pointerup', up)
    })
    knob.addEventListener('dblclick', () => run(`track.pan #${t.id} 0`))
    el.querySelector<HTMLButtonElement>('.m')!.onclick = (e) => { e.stopPropagation(); run(`track.mute #${t.id}`) }
    el.querySelector<HTMLButtonElement>('.s')!.onclick = (e) => { e.stopPropagation(); run(`track.solo #${t.id}`) }
    el.querySelectorAll<HTMLButtonElement>('.insert').forEach((b) => (b.onclick = (e) => { e.stopPropagation(); openPluginPicker(t, b.dataset.slot === 'inst' ? 'Instrument' : 'Effect') }))
  } else {
    el.querySelector<HTMLButtonElement>('.m')!.onclick = () => { demo.mono = !demo.mono; changed() }
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
    const lv = demo.levels.get(id) ?? { l: 0, r: 0, pl: 0, pr: 0 }
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
let pianoTool: 'select' | 'draw' = 'select'
let selNote = -1

function midiClip(): Clip | undefined {
  const sel = demo.clips.find((c) => c.id === ui.selClip && c.kind === 'midi')
  return sel ?? demo.clips.find((c) => c.track === ui.selTrack && c.kind === 'midi')
}

function piano(view: HTMLElement) {
  const clip = midiClip()
  view.innerHTML = `<div class="editor">
    <div class="toolbar">
      <span class="title">${clip ? esc(clip.name) : 'Piano roll'}</span><span class="tag demo">demo</span>
      <div class="seg"><button data-t="select" class="${pianoTool === 'select' ? 'on' : ''}" title="Select (V)">${icon.pointer.replace('<svg', '<svg width="16" height="16"')}</button><button data-t="draw" class="${pianoTool === 'draw' ? 'on' : ''}" title="Draw (D)">${icon.pencil.replace('<svg', '<svg width="16" height="16"')}</button></div>
      <div class="seg"><button class="on">1/16</button><button>1/8</button><button>1/4</button><button>Off</button></div>
      <button class="btn solid quant">Quantize</button>
      <span class="muted">${clip ? `${clip.notes?.length ?? 0} notes · ${clip.len / bpb()} bars` : ''}</span>
    </div>
    ${clip ? '<canvas></canvas>' : '<div class="empty-note">No MIDI clip selected.<br>Double-click a MIDI clip in the timeline.</div>'}
  </div>`
  view.querySelectorAll<HTMLButtonElement>('[data-t]').forEach((b) => (b.onclick = () => { pianoTool = b.dataset.t as typeof pianoTool; renderPanel() }))
  view.querySelectorAll<HTMLButtonElement>('.seg:nth-of-type(2) button').forEach((b) => (b.onclick = () => {
    b.parentElement!.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b))
  }))
  if (!clip) return
  view.querySelector<HTMLButtonElement>('.quant')!.onclick = () => { clip.notes?.forEach((n) => (n.start = Math.round(n.start * 4) / 4)); changed() }
  const cv = view.querySelector('canvas')!
  const KEYS = 52, RUL = 22, VEL = 54
  const notes = clip.notes ?? []
  let lo = 127, hi = 0
  for (const n of notes) { lo = Math.min(lo, n.pitch); hi = Math.max(hi, n.pitch) }
  if (!notes.length) { lo = 48; hi = 72 }
  lo = Math.max(0, lo - 5); hi = Math.min(127, hi + 5)
  const geo = () => {
    const w = cv.clientWidth, h = cv.clientHeight
    return { w, h, rowH: (h - RUL - VEL) / (hi - lo + 1), ppb: (w - KEYS) / clip.len }
  }
  const drawRoll = () => {
    const { w, h, rowH, ppb } = geo(), dpr = devicePixelRatio || 1
    cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr)
    const c = cv.getContext('2d')!
    c.setTransform(dpr, 0, 0, dpr, 0, 0)
    c.fillStyle = '#0e1014'; c.fillRect(0, 0, w, h)
    const col = trackColor(clip.track)
    for (let p = lo; p <= hi; p++) {
      const y = RUL + (hi - p) * rowH
      const black = [1, 3, 6, 8, 10].includes(p % 12)
      c.fillStyle = black ? '#12151b' : '#161920'; c.fillRect(KEYS, y, w - KEYS, rowH)
      c.fillStyle = black ? '#1b1f27' : '#d9dce3'; c.fillRect(0, y, KEYS - 2, rowH - 0.5)
      if (p % 12 === 0) { c.fillStyle = '#5a6172'; c.font = '9px var(--mono)'; c.textBaseline = 'middle'; c.fillText(`C${p / 12 - 1}`, KEYS - 24, y + rowH / 2) }
    }
    for (let b = 0; b <= clip.len * 4; b++) {
      const x = KEYS + b / 4 * ppb
      c.fillStyle = b % (bpb() * 4) === 0 ? '#343b4a' : b % 4 === 0 ? '#262b36' : '#1c2029'
      c.fillRect(Math.round(x), RUL, 1, h - RUL)
      if (b % (bpb() * 4) === 0) { c.fillStyle = '#a1a8b6'; c.font = '10px var(--mono)'; c.fillText(String((clip.start + b / 4) / bpb() + 1), x + 4, RUL / 2) }
    }
    c.fillStyle = '#14171d'; c.fillRect(KEYS, h - VEL, w - KEYS, VEL)
    c.fillStyle = '#262b36'; c.fillRect(KEYS, h - VEL, w - KEYS, 1); c.fillRect(0, RUL - 1, w, 1)
    notes.forEach((n, i) => {
      const x = KEYS + n.start * ppb, y = RUL + (hi - n.pitch) * rowH, nw = Math.max(3, n.len * ppb - 1)
      c.globalAlpha = 0.45 + 0.55 * n.vel / 127
      c.fillStyle = i === selNote ? '#e7e9ee' : col
      c.beginPath(); c.roundRect(x, y + 1, nw, Math.max(2, rowH - 2), 2); c.fill()
      c.globalAlpha = 1
      const vh = (n.vel / 127) * (VEL - 10)
      c.fillStyle = i === selNote ? '#e7e9ee' : col
      c.fillRect(x, h - 4 - vh, 2, vh); c.fillRect(x - 2, h - 4 - vh, 6, 2)
    })
    const ph = demo.pos - clip.start
    if (ph >= 0 && ph <= clip.len) { c.fillStyle = '#ff7a2f'; c.fillRect(KEYS + ph * ppb, 0, 1.5, h - VEL) }
  }
  new ResizeObserver(drawRoll).observe(cv)
  cv.addEventListener('pointerdown', (e) => {
    const { rowH, ppb } = geo()
    if (e.offsetX < KEYS || e.offsetY < RUL || e.offsetY > cv.clientHeight - VEL) return
    const beat = (e.offsetX - KEYS) / ppb, pitch = hi - Math.floor((e.offsetY - RUL) / rowH)
    const idx = notes.findIndex((n) => n.pitch === pitch && beat >= n.start && beat < n.start + n.len)
    if (idx >= 0) selNote = idx
    else if (pianoTool === 'draw') { notes.push({ pitch, start: Math.floor(beat * 4) / 4, len: 0.25, vel: 96 }); selNote = notes.length - 1; changed() }
    else selNote = -1
    drawRoll()
  })
}

// ------------------------------------------------------------------ clip editor
function clipEditor(view: HTMLElement) {
  const clip = demo.clips.find((c) => c.id === ui.selClip && c.kind === 'audio') ?? demo.clips.find((c) => c.track === ui.selTrack && c.kind === 'audio')
  const per = bpb()
  const pos = (b: number) => `${Math.floor(b / per) + 1}.${Math.floor(b % per) + 1}`
  view.innerHTML = `<div class="editor">
    <div class="toolbar"><span class="title">${clip ? esc(clip.name) : 'Clip editor'}</span><span class="tag demo">demo</span>
      <span class="muted">${clip ? `${esc(clip.name)}.wav · ${state.project.sample_rate / 1000} kHz · 24-bit · stereo` : ''}</span></div>
    ${clip ? `<div class="split"><canvas></canvas><div class="props">
      <label>Start <span class="val">${pos(clip.start)}</span></label>
      <label>Length <span class="val">${(clip.len / per).toFixed(2)} bars</span></label>
      <label>Gain <span class="val">0.0 dB</span></label>
      <label>Fade in <span class="val">10 ms</span></label>
      <label>Fade out <span class="val">40 ms</span></label>
      <label>Source rate <span class="val">${state.project.sample_rate} Hz · no resampling</span></label>
      <label>Track <span class="val">${esc(state.project.tracks.find((t) => t.id === clip.track)?.name ?? '')}</span></label>
    </div></div>` : '<div class="empty-note">No audio clip selected.<br>Double-click an audio clip in the timeline.</div>'}
  </div>`
  if (!clip?.peaks) return
  const cv = view.querySelector('canvas')!
  const drawWave = () => {
    const w = cv.clientWidth, h = cv.clientHeight, dpr = devicePixelRatio || 1
    cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr)
    const c = cv.getContext('2d')!
    c.setTransform(dpr, 0, 0, dpr, 0, 0)
    c.fillStyle = '#0e1014'; c.fillRect(0, 0, w, h)
    const col = trackColor(clip.track), peaks = clip.peaks!
    const mid = h / 2, n = peaks.length
    c.fillStyle = '#262b36'; c.fillRect(0, mid, w, 1)
    for (let b = 0; b <= clip.len; b++) { c.fillStyle = b % per === 0 ? '#343b4a' : '#1c2029'; c.fillRect(Math.round(b / clip.len * w), 0, 1, h) }
    c.fillStyle = col
    for (let x = 0; x < w; x++) {
      const k0 = Math.floor(x / w * n), k1 = Math.max(k0 + 1, Math.floor((x + 1) / w * n))
      let v = 0
      for (let k = k0; k < k1; k++) v = Math.max(v, peaks[k])
      const a = v * (h / 2 - 12)
      c.fillRect(x, mid - a, 1, a * 2 || 1)
    }
    // fades and gain line
    const fi = 0.02 * w, fo = 0.06 * w
    c.strokeStyle = '#e7e9ee'; c.lineWidth = 1.5
    c.beginPath(); c.moveTo(0, h - 6); c.quadraticCurveTo(fi * 0.2, 8, fi, 8); c.lineTo(w - fo, 8); c.quadraticCurveTo(w - fo * 0.2, 8, w, h - 6); c.stroke()
    for (const hx of [fi, w - fo]) { c.fillStyle = '#e7e9ee'; c.fillRect(hx - 4, 4, 8, 8) }
    const ph = (demo.pos - clip.start) / clip.len
    if (ph >= 0 && ph <= 1) { c.fillStyle = '#ff7a2f'; c.fillRect(ph * w, 0, 1.5, h) }
  }
  new ResizeObserver(drawWave).observe(cv)
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
