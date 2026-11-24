// Crucible's window: top bar, track headers, timeline, bottom panel. Views draw from `state`
// (the engine's truth) and change it only through `run(command)`.
import { closeLayer, layerOpen, menu, openExport, openPluginPicker, openSettings, pathPrompt, textPrompt, type Item } from './dialogs'
import { demo, seedPlugins, settling } from './demo'
import { beats, ids, inTauri, PPQ, q, transportName, type Clip } from './engine'
import { icon } from './icons'
import { loadKinds } from './effects'
import { cycleLayout, loadLayout } from './layout'
import { deleteSelectedNote, drawMeters, renderPanel, renderTabs, selectTab, setCommandNames } from './panels'
import { bpb, changed, clip, created, dragNumber, esc, flush, fmtDb, fmtPan, onChange, redraw, run, selectClips, state, toast, toggleMute, toggleSolo, track, ui, view } from './store'
import { advance, device, togglePlay as transportToggle } from './transport'
import { clampScroll, draw, dropTarget, follow, hooks, pasteTarget, rowH, trackColor, zoom } from './timeline'

loadLayout()

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
  el.dev.textContent = device ? `${p.sample_rate / 1000} kHz · ${device.driver} · ${device.buffer_frames} · ${device.buffer_ms.toFixed(1)} ms` : `${p.sample_rate / 1000} kHz`
  el.dev.title = device ? device.device : 'The audio device opens when you press play. Choose it with the audio.output command.'
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
    { label: 'Export…', kbd: 'Ctrl+Shift+E', action: openExport },
    'sep',
    { label: 'Undo', small: state.undo ?? 'nothing', action: () => run('undo'), disabled: !state.undo },
    { label: 'Redo', small: state.redo ?? 'nothing', action: () => run('redo'), disabled: !state.redo },
    { label: 'History', action: () => { run('history'); selectTab('console') } },
    'sep',
    { label: 'Layout…', kbd: 'Ctrl+Shift+L', action: () => openSettings('Appearance') },
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
$('#headers').insertAdjacentHTML('afterbegin', `<div class="ruler-label"><span>Tracks <span id="track-count"></span></span><button class="snap-btn" id="snap" title="Grid for moving and trimming clips. Hold Alt while dragging to ignore it."></button></div>`)
const SNAPS: [string, number][] = [['Bar', -1], ['Beat', 1], ['1/8', 0.5], ['1/16', 0.25], ['Off', 0]]
const snapLabel = () => (ui.snap === bpb() ? 'Bar' : SNAPS.find(([, v]) => v === ui.snap)?.[0] ?? `${ui.snap}`)
$('#snap').onclick = (e) => {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu({ x: r.left, y: r.bottom + 4 }, SNAPS.map(([label, v]) => ({ label: `Snap: ${label}`, action: () => { ui.snap = v < 0 ? bpb() : v; changed() } })))
}

function renderHeaders() {
  const tracks = state.project.tracks
  $('#track-count').textContent = String(tracks.length)
  $('#snap').textContent = `Snap ${snapLabel()}`
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
    $('.m', row).onclick = (e) => { e.stopPropagation(); toggleMute(id) }
    $('.s', row).onclick = (e) => { e.stopPropagation(); toggleSolo(id) }
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
    { label: 'Import audio file at playhead…', kbd: 'Ctrl+I', action: () => importPrompt(t.id, pasteTarget().at) },
    'sep',
    { label: 'Remove track', kbd: 'Ctrl+Del', danger: true, action: () => run(`track.remove #${t.id}`) },
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
  const h = rowH()
  if (i * h < ui.scrollY) ui.scrollY = i * h
  else if ((i + 1) * h > ui.scrollY + viewH) ui.scrollY = (i + 1) * h - viewH
  clampScroll()
}

$('#headers').addEventListener('wheel', (e) => { e.preventDefault(); ui.scrollY += e.deltaY; clampScroll(); redraw() }, { passive: false })
view.redraw = () => { rows.style.transform = `translateY(${-ui.scrollY}px)`; draw(); updateReadouts() }

// ------------------------------------------------------------------ clip editing (all real engine commands)
const selIds = () => [...ui.selClips].filter((i) => clip(i))
async function runSelecting(line: string) {
  const r = await run(line)
  const made = created(r)
  if (made.length) { selectClips(made); changed() }
}
function copyClips() { const s = selIds(); if (s.length) run(`edit.copy ${ids(s)}`) }
function cutClips() { const s = selIds(); if (s.length) { run(`edit.cut ${ids(s)}`); selectClips([]) } }
function pasteClips(trackId?: number | null, at?: number) {
  const t = pasteTarget()
  const tr = trackId ?? t.track
  if (tr == null) return toast('info', 'Add a track first')
  runSelecting(`edit.paste #${tr} ${at ?? t.at}`)
}
function duplicateClips() { const s = selIds(); if (s.length) runSelecting(`clip.copy ${ids(s)}`) }
function deleteClips() { const s = selIds(); if (s.length) { run(`clip.remove ${ids(s)}`); selectClips([]) } }
function splitClips() {
  const at = Math.round(demo.pos * PPQ)
  let s = selIds()
  if (!s.length) s = state.project.clips.filter((c) => c.track === ui.selTrack && c.start < at && c.start + c.length > at).map((c) => c.id)
  if (!s.length) return toast('info', 'Nothing to split', 'Put the playhead over a clip, select it, then Ctrl+E.')
  runSelecting(`clip.split ${ids(s)} ${beats(at)}`)
}
function nudge(dir: number, bar: boolean) {
  const s = selIds()
  if (!s.length) return false
  const step = bar ? bpb() : ui.snap || 0.25
  const first = Math.min(...s.map((i) => clip(i)!.start)) / PPQ
  const by = Math.max(dir * step, -first)
  if (by) run(`clip.move ${ids(s)} --by ${+by.toFixed(6)}`)
  return true
}
function renameClip(c: Clip) {
  textPrompt('Rename clip', c.name, (name) => run(`clip.rename #${c.id} ${q(name)}`))
}

hooks.openClip = (c) => { selectClips([c.id]); selectTab(c.content.kind === 'midi' ? 'piano' : 'clip') }
hooks.clipMenu = (c, at) => {
  const n = ui.selClips.size
  const many = n > 1 ? ` ${n} clips` : ''
  menu(at, [
    { label: `Cut${many}`, kbd: 'Ctrl+X', action: cutClips },
    { label: `Copy${many}`, kbd: 'Ctrl+C', action: copyClips },
    { label: 'Paste at playhead', kbd: 'Ctrl+V', action: () => pasteClips() },
    { label: `Duplicate${many}`, kbd: 'Ctrl+D', action: duplicateClips },
    { label: 'Split at playhead', kbd: 'Ctrl+E', action: splitClips },
    'sep',
    { label: 'Open in editor', action: () => hooks.openClip(c) },
    { label: 'Rename…', action: () => renameClip(c), disabled: n > 1 },
    ...(c.content.kind === 'midi' ? [{ label: 'Quantize to 1/16', action: () => run(`clip.quantize #${c.id}`) }] : []),
    'sep',
    { label: `Delete${many}`, kbd: 'Del', danger: true, action: deleteClips },
  ])
}
hooks.emptyMenu = (trackId, at, pos) => {
  const t = track(trackId)
  menu(pos, [
    { label: 'Paste here', kbd: 'Ctrl+V', action: () => pasteClips(trackId, at) },
    { label: 'Import audio file here…', kbd: 'Ctrl+I', action: () => importPrompt(trackId, at) },
    ...(t?.kind === 'instrument' ? [{ label: 'New MIDI clip here', action: () => runSelecting(`clip.add #${t.id} ${Math.floor(at / bpb()) * bpb()} ${bpb()}`) }] : []),
    { label: 'Select all clips', kbd: 'Ctrl+A', action: () => { selectClips(state.project.clips.map((c) => c.id)); changed() } },
  ])
}

// ------------------------------------------------------------------ audio import
/** Import files and place them at `at`: the first on `trackId` if that is an audio track, every
 *  other one on a new audio track named after its file. A project needs a folder first, because
 *  imported audio is copied into it. */
async function importFiles(paths: string[], trackId: number | null, at: number) {
  if (!paths.length) return
  if (!state.folder) {
    toast('info', 'Save the project first', 'Imported audio is copied into the project folder, so it needs one. Choose a folder, then import again.')
    pathPrompt('save')
    return
  }
  let target = track(trackId)?.kind === 'audio' ? trackId : null
  const made: number[] = []
  for (const path of paths) {
    const [imp] = await run(`audio.import ${q(path)}`)
    if (!imp?.ok) break
    // Never stack a file on top of audio already there: use a new track instead.
    const p = state.project, lenBeats = imp.data.frames / ((imp.data.rate * 60) / p.tempo_bpm)
    if (target !== null && p.clips.some((c) => c.track === target && c.start / PPQ < at + lenBeats && (c.start + c.length) / PPQ > at)) target = null
    if (target === null) {
      const [added] = await run(`track.add ${q(String(imp.data.source).replace(/\.\w+$/, ''))} --kind audio`)
      if (!added?.ok) break
      target = added.data.id
    }
    made.push(...created(await run(`clip.place #${target} ${at} ${q(imp.data.source)}`)))
    target = null
  }
  if (made.length) { selectClips(made); changed() }
}

function importPrompt(trackId: number | null, at: number) {
  textPrompt('Import audio: WAV, MP3 or FLAC (full path; "Copy as path" from Explorer works)', '', (v) => {
    const path = v.trim().replace(/^"(.*)"$/, '$1')
    if (path) importFiles([path], trackId, at)
  })
}

// Files dropped on the window (Tauri gives real paths; a plain browser does not).
if (inTauri) {
  import('@tauri-apps/api/webview').then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((ev) => {
    if (ev.payload.type !== 'drop') return
    const dpr = devicePixelRatio || 1
    const t = dropTarget(ev.payload.position.x / dpr, ev.payload.position.y / dpr)
    importFiles(ev.payload.paths, t.track, t.at)
  }))
}

// ------------------------------------------------------------------ panel resize
const panelEl = $('#panel')
panelEl.insertAdjacentHTML('afterbegin', '<div class="resize-h" title="Drag to resize"></div>')
$('.resize-h', panelEl).addEventListener('pointerdown', (e) => {
  if (document.documentElement.dataset.layout === 'side') {
    const w0 = panelEl.clientWidth, x0 = e.clientX
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    const mv = (m: PointerEvent) => document.documentElement.style.setProperty('--side-w', `${Math.max(320, Math.min(innerWidth - 480, w0 + x0 - m.clientX))}px`)
    const up = () => { removeEventListener('pointermove', mv); removeEventListener('pointerup', up); changed() }
    addEventListener('pointermove', mv); addEventListener('pointerup', up)
    return
  }
  const h0 = $('#panel-body').clientHeight, y0 = e.clientY
  ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  const mv = (m: PointerEvent) => document.documentElement.style.setProperty('--panel-h', `${Math.max(160, Math.min(innerHeight - 260, h0 + y0 - m.clientY))}px`)
  const up = () => { removeEventListener('pointermove', mv); removeEventListener('pointerup', up); changed() }
  addEventListener('pointermove', mv); addEventListener('pointerup', up)
})

// ------------------------------------------------------------------ transport loop
// The draw loop runs only while something moves: playing, or meters falling back after stop.
let last = 0, looping = false
async function togglePlay() {
  await transportToggle()
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
  advance(dt)
  if (demo.playing) follow()
  draw(); drawMeters(); updateReadouts()
  const ms = performance.now() - t0
  perf.frames++; perf.totalMs += ms; perf.worstMs = Math.max(perf.worstMs, ms)
  if (settling()) requestAnimationFrame(frame)
  else looping = false
}
if (import.meta.env.DEV) Object.assign(window, { __crucible: { perf, draw, drawMeters, state: () => state, ui, demo, run, changed, flush, advance, togglePlay } })

// ------------------------------------------------------------------ render on change
onChange(() => {
  seedPlugins(state.project.tracks)
  if (ui.selTrack !== null && !track(ui.selTrack)) ui.selTrack = null
  if ([...ui.selClips].some((i) => !clip(i))) selectClips([...ui.selClips].filter((i) => clip(i)))
  if (ui.selClip !== null && !clip(ui.selClip)) ui.selClip = null
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
    else if (k === 'e' && e.shiftKey) { handled(); openExport() }
    else if (k === 'i') { handled(); const t = pasteTarget(); importPrompt(t.track, t.at) }
    else if (k === 'e') { handled(); splitClips() }
    else if (k === 'c') { handled(); copyClips() }
    else if (k === 'x') { handled(); cutClips() }
    else if (k === 'v') { handled(); pasteClips() }
    else if (k === 'd') { handled(); duplicateClips() }
    else if (k === 'a') { handled(); selectClips(state.project.clips.map((c) => c.id)); changed() }
    else if (k === 'l' && e.shiftKey) { handled(); toast('info', `Layout: ${cycleLayout().name}`) }
    else if ((k === 'delete' || k === 'backspace') && sel) { handled(); run(`track.remove #${sel}`) }
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
    case 'm': case 'M': if (sel) toggleMute(sel); break
    case 's': case 'S': if (sel) toggleSolo(sel); break
    case 'Delete': case 'Backspace':
      handled()
      if (ui.focus === 'piano' && deleteSelectedNote()) break
      if (ui.selClips.size) deleteClips()
      else if (sel) toast('info', 'Ctrl+Delete removes the track', 'Delete on its own removes selected clips.')
      break
    case 'Escape': if (ui.selClips.size) { selectClips([]); changed() } break
    case 'ArrowLeft': case 'ArrowRight': if (nudge(e.key === 'ArrowLeft' ? -1 : 1, e.shiftKey)) handled(); break
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
  await run('project.state', { quiet: true })
  // An empty engine gets the demo song, built by the engine itself in one undoable step.
  if (!state.project.tracks.length) await run('project.demo', { quiet: true })
  await loadKinds()
  ui.selTrack = state.project.tracks[2]?.id ?? null
  const first = state.project.clips.find((c) => c.track === ui.selTrack)
  if (first) selectClips([first.id])
  document.fonts?.ready.then(() => changed()) // canvases redraw once the bundled fonts are in
  const help = await run('help', { quiet: true })
  setCommandNames([...(help[0]?.text ?? '').matchAll(/^\s{2}([a-z][\w.]*)/gm)].map((m) => m[1]))
  changed()
}
start()
