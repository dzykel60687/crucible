// Crucible's window: top bar, track headers, timeline, bottom panel. Views draw from `state`
// (the engine's truth) and change it only through `run(command)`.
import { closeLayer, layerOpen, menu, openExport, openPluginPicker, openSettings, pathPrompt, type Item } from './dialogs'
import { demo, seedClips, settling, tick } from './demo'
import { q, transportName } from './engine'
import { icon } from './icons'
import { drawMeters, renderPanel, renderTabs, selectTab, setCommandNames } from './panels'
import { bpb, changed, dragNumber, esc, fmtDb, fmtPan, onChange, redraw, run, state, toast, track, ui, view } from './store'
import { clampScroll, draw, follow, ROW_H, setOnOpenClip, trackColor, zoom } from './timeline'

const $ = <T extends HTMLElement = HTMLElement>(sel: string, root: ParentNode = document) => root.querySelector<T>(sel)!

// ------------------------------------------------------------------ top bar (built once, updated in place)
const top = $('#topbar')
top.innerHTML = `
  <span class="logo" title="Crucible">${icon.logo}</span>
  <button class="btn project-btn" id="proj" title="Project: new, open, save, export"><span class="name"></span><span class="dirty"></span>${icon.chevron}</button>
  <span class="sep"></span>
  <div class="group transport">
    <button class="btn" id="t-start" title="Return to start (Home)">${icon.start}</button>
    <button class="btn play" id="t-play" title="Play / stop (Space)">${icon.play}</button>
    <button class="btn rec" id="t-rec" title="Record (R)">${icon.rec}</button>
    <button class="btn" id="t-loop" title="Loop (L)">${icon.loop}</button>
    <button class="btn" id="t-metro" title="Metronome (K)">${icon.metro}</button>
  </div>
  <span class="sep"></span>
  <div class="field edit" id="f-tempo" title="Tempo: drag, or click to type"><span class="k">Tempo</span><span class="v"></span></div>
  <div class="field edit" id="f-sig" title="Time signature: click to change"><span class="k">Time sig</span><span class="v"></span></div>
  <div class="field position" title="Position: bar.beat.sixteenth and time"><span class="k">Position</span><span class="v"></span></div>
  <span class="spacer"></span>
  <div class="safety">
    <div class="field headroom" title="Highest peak since play started. Click to reset."><span class="k">Peak</span><span class="v" style="display:flex;align-items:center;gap:8px"><span id="peak"></span><span class="clip-light" id="clip"></span></span></div>
    <div class="field" title="Short-term loudness"><span class="k">LUFS</span><span class="v" id="lufs"></span></div>
    <button class="btn" id="b-mono" title="Mono check">${icon.mono}<span style="font-size:11.5px">Mono</span></button>
    <button class="btn" id="b-ab" title="Reference track A/B">${icon.ab}</button>
  </div>
  <span class="sep"></span>
  <div class="device" id="device" title="Audio device. Click for settings."><span class="dot"></span><span id="dev-text"></span><span class="xruns" id="xruns"></span></div>
  <button class="btn icon" id="gear" title="Settings (Ctrl+,)">${icon.gear}</button>`

const el = {
  projName: $('#proj .name'), proj: $('#proj'), play: $('#t-play'), rec: $('#t-rec'), loop: $('#t-loop'), metro: $('#t-metro'),
  tempo: $('#f-tempo .v'), sig: $('#f-sig .v'), pos: $('.position .v'), peak: $('#peak'), clip: $('#clip'), lufs: $('#lufs'),
  mono: $('#b-mono'), ab: $('#b-ab'), dev: $('#dev-text'), xruns: $('#xruns'),
}

function updateTop() {
  const p = state.project
  el.projName.textContent = p.name
  el.proj.classList.toggle('is-dirty', state.dirty)
  el.proj.title = state.dirty ? 'Unsaved changes' : state.folder ?? 'Not saved yet'
  el.tempo.textContent = p.tempo_bpm.toFixed(p.tempo_bpm % 1 ? 2 : 0)
  el.sig.textContent = `${p.time_signature[0]}/${p.time_signature[1]}`
  el.play.innerHTML = demo.playing ? icon.stop : icon.play
  el.play.classList.toggle('on', demo.playing)
  el.rec.classList.toggle('on', demo.recording)
  el.loop.classList.toggle('on', demo.loop)
  el.metro.classList.toggle('on', demo.metronome)
  el.mono.classList.toggle('on', demo.mono)
  el.ab.classList.toggle('on', demo.ab)
  el.dev.textContent = `${p.sample_rate / 1000} kHz · 128 · ${(128 / p.sample_rate * 1000).toFixed(1)} ms`
  el.xruns.textContent = demo.xruns ? `${demo.xruns} dropouts` : ''
  updateReadouts()
}

/** The parts of the top bar that move while playing. */
function updateReadouts() {
  const per = bpb(), b = demo.pos
  const bar = Math.floor(b / per) + 1, beat = Math.floor(b % per) + 1, six = Math.floor((b % 1) * 4) + 1
  const secs = b * 60 / state.project.tempo_bpm
  const mm = Math.floor(secs / 60), ss = (secs % 60).toFixed(3).padStart(6, '0')
  el.pos.innerHTML = `${bar}.${beat}.${six}<small>${mm}:${ss}</small>`
  el.peak.textContent = Number.isFinite(demo.peakDb) ? `${demo.peakDb.toFixed(1)}` : '—'
  el.clip.classList.toggle('hot', demo.clipped)
  el.lufs.textContent = Number.isFinite(demo.lufs) ? demo.lufs.toFixed(1) : '—'
}

// top bar actions
el.proj.onclick = () => {
  const r = el.proj.getBoundingClientRect()
  projectMenu({ x: r.left, y: r.bottom + 4 })
}
function projectMenu(at: { x: number; y: number }) {
  const items: Item[] = [
    { label: 'New project…', kbd: 'Ctrl+N', action: () => pathPrompt('new') },
    { label: 'Open…', kbd: 'Ctrl+O', action: () => pathPrompt('open') },
    'sep',
    { label: 'Save', kbd: 'Ctrl+S', action: save, disabled: !state.dirty && !!state.folder },
    { label: 'Save as…', kbd: 'Ctrl+Shift+S', action: () => pathPrompt('save') },
    { label: 'Export…', kbd: 'Ctrl+E', action: openExport },
    'sep',
    { label: 'Undo', small: state.undo ?? 'nothing', action: () => run('undo'), disabled: !state.undo },
    { label: 'Redo', small: state.redo ?? 'nothing', action: () => run('redo'), disabled: !state.redo },
    { label: 'History', action: () => { run('history'); selectTab('console') } },
    'sep',
    { label: 'Settings…', kbd: 'Ctrl+,', action: () => openSettings() },
  ]
  menu(at, items)
}
function save() { if (state.folder) run('project.save'); else pathPrompt('save') }

$('#t-start').onclick = () => { demo.pos = demo.loop ? demo.loopStart : 0; changed() }
el.play.onclick = togglePlay
el.rec.onclick = () => { demo.recording = !demo.recording; if (demo.recording && !demo.playing) togglePlay(); changed() }
el.loop.onclick = () => { demo.loop = !demo.loop; changed() }
el.metro.onclick = () => { demo.metronome = !demo.metronome; changed() }
el.mono.onclick = () => { demo.mono = !demo.mono; changed() }
el.ab.onclick = () => { demo.ab = !demo.ab; toast('info', demo.ab ? 'Reference: B (reference track)' : 'Reference: A (your mix)', 'Demo: no reference track loaded.'); changed() }
$('.headroom').onclick = () => { demo.peakDb = -Infinity; demo.clipped = false; changed() }
$('#device').onclick = () => openSettings('Audio')
$('#gear').onclick = () => openSettings()

dragNumber($('#f-tempo'), () => state.project.tempo_bpm, {
  min: 20, max: 999, perPx: 0.5, step: 1,
  preview: (v) => (el.tempo.textContent = String(v)),
  commit: (v) => run(`project.tempo ${v}`),
  click: () => inlineEdit($('#f-tempo .v'), String(state.project.tempo_bpm), (v) => run(`project.tempo ${q(v)}`)),
})
$('#f-sig').onclick = (e) => {
  const sigs = ['2/4', '3/4', '4/4', '5/4', '6/8', '7/8', '12/8']
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu({ x: r.left, y: r.bottom + 4 }, sigs.map((s) => ({ label: s, action: () => run(`project.signature ${s.replace('/', ' ')}`) })))
}

function inlineEdit(target: HTMLElement, value: string, done: (v: string) => void) {
  const input = document.createElement('input')
  input.className = 'input'
  input.value = value
  input.style.cssText = 'height:24px;width:90px;padding:0 6px;font:inherit'
  const prev = target.innerHTML
  target.replaceChildren(input)
  input.focus(); input.select()
  let finished = false
  const finish = (ok: boolean) => {
    if (finished) return
    finished = true
    target.innerHTML = prev
    if (ok && input.value.trim() && input.value.trim() !== value) done(input.value.trim())
    else changed()
  }
  input.onkeydown = (e) => { e.stopPropagation(); if (e.key === 'Enter') finish(true); if (e.key === 'Escape') finish(false) }
  input.onblur = () => finish(true)
  input.onpointerdown = (e) => e.stopPropagation()
}

// ------------------------------------------------------------------ track headers
const rows = $('#header-rows')
$('#headers').insertAdjacentHTML('afterbegin', `<div class="ruler-label"><span>Tracks</span><span id="track-count"></span></div>`)

function renderHeaders() {
  const tracks = state.project.tracks
  $('#track-count').textContent = String(tracks.length)
  rows.innerHTML = tracks.map((t) => `
    <div class="track ${t.id === ui.selTrack ? 'sel' : ''}" data-id="${t.id}" style="--c:${trackColor(t.id)}">
      <div class="color"></div>
      <div class="body">
        <div class="row1"><span class="kind">${t.kind === 'audio' ? icon.wave : icon.keys}</span><span class="tname">${esc(t.name)}</span>
          ${t.kind === 'instrument' ? `<button class="slot" title="Instrument (demo)">${esc(demo.instrument.get(t.id) ?? 'No instrument')}</button>` : ''}</div>
        <div class="row2">
          <button class="tbtn r ${demo.armed.has(t.id) ? 'on' : ''}" title="Arm for recording (demo)">R</button>
          <button class="tbtn m ${t.mute ? 'on' : ''}" title="Mute (M)">M</button>
          <button class="tbtn s ${t.solo ? 'on' : ''}" title="Solo (S)">S</button>
          <span style="flex:1"></span>
          <span class="knob-num gain" title="Gain: drag, Shift for fine, double-click for 0 dB">${fmtDb(t.gain_db)} dB</span>
          <span class="knob-num pan" title="Pan: drag, double-click to centre">${fmtPan(t.pan)}</span>
        </div>
      </div>
    </div>`).join('') + `<div class="add-track"><button class="btn" data-add="audio">${icon.plus} Audio</button><button class="btn" data-add="instrument">${icon.plus} Instrument</button></div>`

  rows.querySelectorAll<HTMLElement>('.track').forEach((row) => {
    const id = +row.dataset.id!
    const t = track(id)!
    row.onpointerdown = () => { if (ui.selTrack !== id) { ui.selTrack = id; changed() } }
    row.oncontextmenu = (e) => { e.preventDefault(); ui.selTrack = id; changed(); trackMenu(t, { x: e.clientX, y: e.clientY }) }
    $('.tname', row).ondblclick = () => rename(t)
    $('.r', row).onclick = (e) => { e.stopPropagation(); demo.armed.has(id) ? demo.armed.delete(id) : demo.armed.add(id); changed() }
    $('.m', row).onclick = (e) => { e.stopPropagation(); run(`track.mute #${id}`) }
    $('.s', row).onclick = (e) => { e.stopPropagation(); run(`track.solo #${id}`) }
    row.querySelector<HTMLElement>('.slot')?.addEventListener('click', (e) => { e.stopPropagation(); openPluginPicker(t, 'Instrument') })
    const g = $('.gain', row), p = $('.pan', row)
    dragNumber(g, () => t.gain_db, { min: -60, max: 24, perPx: 0.25, step: 0.1, reset: 0, preview: (v) => (g.textContent = `${fmtDb(v)} dB`), commit: (v) => run(`track.gain #${id} ${+v.toFixed(1)}`) })
    dragNumber(p, () => t.pan * 100, { min: -100, max: 100, perPx: 1, step: 1, reset: 0, preview: (v) => (p.textContent = fmtPan(v / 100)), commit: (v) => run(`track.pan #${id} ${Math.round(v)}`) })
  })
  rows.querySelectorAll<HTMLButtonElement>('[data-add]').forEach((b) => (b.onclick = () => addTrack(b.dataset.add as 'audio' | 'instrument')))
}

function trackMenu(t: { id: number; name: string }, at: { x: number; y: number }) {
  const i = state.project.tracks.findIndex((x) => x.id === t.id), n = state.project.tracks.length
  menu(at, [
    { label: 'Rename', kbd: 'F2', action: () => rename(t) },
    { label: 'Move up', action: () => run(`track.move #${t.id} ${i}`), disabled: i === 0 },
    { label: 'Move down', action: () => run(`track.move #${t.id} ${i + 2}`), disabled: i === n - 1 },
    'sep',
    { label: 'Remove track', kbd: 'Del', danger: true, action: () => run(`track.remove #${t.id}`) },
  ])
}

function rename(t: { id: number; name: string }) {
  const nameEl = rows.querySelector<HTMLElement>(`.track[data-id="${t.id}"] .tname`)
  if (nameEl) inlineEdit(nameEl, t.name, (v) => run(`track.rename #${t.id} ${q(v)}`))
}

async function addTrack(kind: 'audio' | 'instrument') {
  const base = kind === 'audio' ? 'Audio' : 'Instrument'
  let k = state.project.tracks.length + 1
  while (state.project.tracks.some((t) => t.name === `${base} ${k}`)) k++
  const r = await run(`track.add ${q(`${base} ${k}`)} --kind ${kind}`)
  if (r[0]?.ok) {
    const t = state.project.tracks.at(-1)
    if (t) { ui.selTrack = t.id; ensureVisible(t.id); changed() }
  }
}

function ensureVisible(id: number) {
  const i = state.project.tracks.findIndex((t) => t.id === id)
  const viewH = $('#timeline-wrap').clientHeight - 30
  if (i * ROW_H < ui.scrollY) ui.scrollY = i * ROW_H
  else if ((i + 1) * ROW_H > ui.scrollY + viewH) ui.scrollY = (i + 1) * ROW_H - viewH
  clampScroll()
}

$('#headers').addEventListener('wheel', (e) => { e.preventDefault(); ui.scrollY += e.deltaY; clampScroll(); redraw() }, { passive: false })
view.redraw = () => { rows.style.transform = `translateY(${-ui.scrollY}px)`; draw(); updateReadouts() }

setOnOpenClip((c) => { ui.selClip = c.id; ui.selTrack = c.track; selectTab(c.kind === 'midi' ? 'piano' : 'clip') })

// ------------------------------------------------------------------ panel resize
const panelEl = $('#panel')
panelEl.insertAdjacentHTML('afterbegin', '<div class="resize-h" title="Drag to resize"></div>')
$('.resize-h', panelEl).addEventListener('pointerdown', (e) => {
  const h0 = $('#panel-body').clientHeight, y0 = e.clientY
  ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  const mv = (m: PointerEvent) => document.documentElement.style.setProperty('--panel-h', `${Math.max(160, Math.min(innerHeight - 260, h0 + y0 - m.clientY))}px`)
  const up = () => { removeEventListener('pointermove', mv); removeEventListener('pointerup', up); changed() }
  addEventListener('pointermove', mv); addEventListener('pointerup', up)
})

// ------------------------------------------------------------------ transport loop
// The draw loop runs only while something moves: playing, or meters falling back after stop.
let last = 0, looping = false
function togglePlay() {
  demo.playing = !demo.playing
  if (!demo.playing) demo.recording = false
  else { demo.peakDb = -Infinity; demo.lufs = -Infinity }
  changed()
  startLoop()
}
function startLoop() {
  if (looping) return
  looping = true
  last = performance.now()
  requestAnimationFrame(frame)
}
// Frame cost, measured: the budget is 8 ms of work per frame (01-vision-and-priorities.md).
export const perf = { frames: 0, totalMs: 0, worstMs: 0 }
function frame(now: number) {
  const t0 = performance.now()
  const dt = Math.min(0.1, (now - last) / 1000)
  last = now
  tick(dt, state.project.tempo_bpm, state.project.tracks, state.project.master_gain_db)
  if (demo.playing) follow()
  draw(); drawMeters(); updateReadouts()
  const ms = performance.now() - t0
  perf.frames++; perf.totalMs += ms; perf.worstMs = Math.max(perf.worstMs, ms)
  if (settling()) requestAnimationFrame(frame)
  else looping = false
}
if (import.meta.env.DEV) Object.assign(window, { __crucible: { perf, draw, drawMeters, state: () => state, ui, demo } })

// ------------------------------------------------------------------ render on change
let firstSeed = false
onChange(() => {
  seedClips(state.project.tracks, firstSeed)
  if (ui.selTrack !== null && !track(ui.selTrack)) ui.selTrack = null
  if (ui.selClip !== null && !demo.clips.some((c) => c.id === ui.selClip && track(c.track))) ui.selClip = null
  clampScroll()
  rows.style.transform = `translateY(${-ui.scrollY}px)`
  updateTop()
  renderHeaders()
  renderTabs()
  renderPanel()
  draw()
})

// ------------------------------------------------------------------ keyboard
addEventListener('keydown', (e) => {
  if (e.target instanceof Element && e.target.matches('input, select, textarea')) return
  if (e.key === 'Escape' && layerOpen()) return closeLayer()
  if (layerOpen()) return
  const ctrl = e.ctrlKey || e.metaKey
  const sel = ui.selTrack
  const k = e.key.toLowerCase()
  const handled = () => e.preventDefault()
  if (ctrl) {
    if (k === 'z' && !e.shiftKey) { handled(); run('undo') }
    else if ((k === 'z' && e.shiftKey) || k === 'y') { handled(); run('redo') }
    else if (k === 's' && e.shiftKey) { handled(); pathPrompt('save') }
    else if (k === 's') { handled(); save() }
    else if (k === 'o') { handled(); pathPrompt('open') }
    else if (k === 'n') { handled(); pathPrompt('new') }
    else if (k === 'e') { handled(); openExport() }
    else if (k === ',') { handled(); openSettings() }
    else if (k === 't') { handled(); addTrack(e.shiftKey ? 'audio' : 'instrument') }
    return
  }
  switch (e.key) {
    case ' ': handled(); togglePlay(); break
    case 'Home': case 'Enter': handled(); demo.pos = 0; changed(); break
    case 'r': case 'R': el.rec.click(); break
    case 'l': case 'L': el.loop.click(); break
    case 'k': case 'K': el.metro.click(); break
    case 'm': case 'M': if (sel) run(`track.mute #${sel}`); break
    case 's': case 'S': if (sel) run(`track.solo #${sel}`); break
    case 'Delete': case 'Backspace': if (sel) run(`track.remove #${sel}`); break
    case 'F2': if (sel) { handled(); rename(track(sel)!) } break
    case 'ArrowUp': case 'ArrowDown': {
      handled()
      const ts = state.project.tracks
      if (!ts.length) break
      const i = ts.findIndex((t) => t.id === sel)
      const j = i < 0 ? 0 : Math.max(0, Math.min(ts.length - 1, i + (e.key === 'ArrowUp' ? -1 : 1)))
      ui.selTrack = ts[j].id; ensureVisible(ts[j].id); changed(); break
    }
    case '+': case '=': zoom(1.25); break
    case '-': case '_': zoom(0.8); break
    case '1': selectTab('mixer'); break
    case '2': selectTab('piano'); break
    case '3': selectTab('clip'); break
    case '`': handled(); selectTab('console'); break
    case 'b': case 'B': ui.panelOpen = !ui.panelOpen; changed(); break
  }
})

// ------------------------------------------------------------------ start
async function start() {
  ui.log.push({ kind: 'help', text: `Crucible console. Connected to the engine through the ${transportName}. Type help.` })
  // Tracks present at start get demo clips by name (Drums, Bass, Keys, Pad, Vox); tracks added
  // later stay empty.
  firstSeed = true
  await run('project.state', { quiet: true })
  if (!state.project.tracks.length) {
    // Empty engine: build a demo song through real engine commands so the layout has content.
    await run('project.new "Demo song"; project.tempo 96; track.add Drums --kind audio; track.add Bass --kind instrument; track.add Keys --kind instrument; track.add Pad --kind instrument; track.add Vox --kind audio; track.gain Pad -4.5; track.pan Keys -20; track.gain Vox -2', { quiet: true })
  }
  changed()
  await new Promise((r) => requestAnimationFrame(r))
  firstSeed = false
  ui.selTrack = state.project.tracks[2]?.id ?? null
  ui.selClip = demo.clips.find((c) => c.track === ui.selTrack)?.id ?? null
  const help = await run('help', { quiet: true })
  setCommandNames([...(help[0]?.text ?? '').matchAll(/^\s{2}([a-z][\w.]*)/gm)].map((m) => m[1]))
  changed()
}
start()
