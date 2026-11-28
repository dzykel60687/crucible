// Crucible's window: top bar, track headers, timeline, bottom panel. Views draw from `state`
// (the engine's truth) and change it only through `run(command)`.
import { askConvert, closeLayer, confirmUnsaved, layerOpen, markPrompt, menu, newProject, openExport, openProject, openSettings, pickFiles, save, saveAs, textPrompt, type Item } from './dialogs'
import { chosenOutput, watchDevices } from './devices'
import { beats, ids, inTauri, PPQ, q, transportName, type Clip, type Track } from './engine'
import { icon } from './icons'
import { fxLabel, loadKinds, openEffectEditor, openEffectPicker } from './effects'
import { inst, instButton, instMenu, keyButton, openInstrumentPicker, toggleLive, watchInstruments } from './instruments'
import { cycleLayout, loadLayout } from './layout'
import { deleteSelectedNote, drawMeters, renderPanel, renderTabs, selectTab, setCommandNames } from './panels'
import { bpb, changed, clip, created, dbToPos, dragNumber, esc, flush, fmtDb, onChange, owe, pointerHeld, posToDb, redraw, run, selectClips, state, toast, toggleMute, toggleSolo, track, ui, view } from './store'
import { advance, device, settling, togglePlay as transportToggle, tp } from './transport'
import { clampScroll, draw, dropTarget, follow, hooks, pasteTarget, rowH, trackColor, zoom } from './timeline'
import { frameDrawn, startTrail } from './trail'

// First, so that anything that goes wrong while the window builds itself is on record.
startTrail()

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
  </div>
  <span class="sep"></span>
  <div class="field edit" id="f-tempo" title="Tempo: drag, or click to type"><span class="k">Tempo</span><span class="v"></span></div>
  <div class="field edit" id="f-sig" title="Time signature: click to change"><span class="k">Time sig</span><span class="v"></span></div>
  <div class="field position" title="Position: minutes and seconds"><span class="k">Position</span><span class="v"></span></div>
  <span class="spacer"></span>
  <div class="safety">
    <div class="field headroom" title="Highest peak since play started. Click to reset."><span class="k">Peak</span><span class="v" style="display:flex;align-items:center;gap:8px"><span id="peak"></span><span class="clip-light" id="clip"></span></span></div>
  </div>
  <span class="sep"></span>
  <div class="device" id="device"><span class="dot"></span><span id="dev-text"></span><span class="xruns" id="xruns"></span></div>
  <button class="btn icon" id="gear" title="Settings (Ctrl+,)">${icon.gear}</button>`

const el = {
  projName: $('#proj .name'), proj: $('#proj'), play: $('#t-play'),
  tempo: $('#f-tempo .v'), sig: $('#f-sig .v'), pos: $('.position .v'), peak: $('#peak'), clip: $('#clip'),
  devBox: $('#device'), dev: $('#dev-text'), xruns: $('#xruns'),
}
/** Which icon the play button shows. Its contents are replaced only when this changes: replacing
 *  them on every update put a new element under the pointer mid-click, and the click was lost. */
let playIcon: boolean | null = null

function updateTop() {
  const p = state.project
  el.projName.textContent = p.name
  el.proj.classList.toggle('is-dirty', state.dirty)
  el.proj.title = state.dirty ? 'Unsaved changes' : state.folder ?? 'Not saved yet'
  el.tempo.textContent = p.tempo_bpm.toFixed(p.tempo_bpm % 1 ? 2 : 0)
  el.sig.textContent = `${p.time_signature[0]}/${p.time_signature[1]}`
  if (playIcon !== tp.playing) { playIcon = tp.playing; el.play.innerHTML = tp.playing ? icon.stop : icon.play }
  el.play.classList.toggle('on', tp.playing)
  // While playing: what the driver really gave. Otherwise: the output that will be used and the
  // rate Windows runs it at.
  const out = chosenOutput()
  el.dev.textContent = device
    ? `${device.sample_rate / 1000} kHz · ${device.driver} · ${device.buffer_frames} · ${device.buffer_ms.toFixed(1)} ms`
    : out ? `${out.name}${out.rate ? ` · ${out.rate / 1000} kHz` : ''}` : 'No audio output'
  el.devBox.title = (device ? `Playing through ${device.device}, ${device.format}. ` : out ? `${out.name}. Opens when you press play. ` : 'No audio output is connected. ') + 'Click for audio settings.'
  el.devBox.classList.toggle('none', !device && !out)
  updateReadouts()
}

/** The parts of the top bar that move while playing. */
function updateReadouts() {
  // Minutes, seconds and tenths, nothing else (asked for 2026-11-26; it showed bar.beat.sixteenth
  // first, which means little for a song that was not made to a click).
  const tenths = Math.floor(tp.pos * 600 / state.project.tempo_bpm + 1e-6)
  const pos = `${Math.floor(tenths / 600)}:${String(Math.floor(tenths / 10) % 60).padStart(2, '0')}.${tenths % 10}`
  if (el.pos.textContent !== pos) el.pos.textContent = pos
  el.peak.textContent = Number.isFinite(tp.peakDb) ? `${tp.peakDb.toFixed(1)}` : '—'
  el.clip.classList.toggle('hot', tp.clipped)
  const drops = tp.dropouts ? `${tp.dropouts} dropout${tp.dropouts === 1 ? '' : 's'}` : ''
  if (el.xruns.textContent !== drops) el.xruns.textContent = drops
}

// top bar actions
el.proj.onclick = () => {
  const r = el.proj.getBoundingClientRect()
  projectMenu({ x: r.left, y: r.bottom + 4 })
}
function projectMenu(at: { x: number; y: number }) {
  const items: Item[] = [
    { label: 'New project', kbd: 'Ctrl+N', action: newProject },
    { label: 'Open…', kbd: 'Ctrl+O', action: openProject },
    'sep',
    { label: 'Save', kbd: 'Ctrl+S', action: save, disabled: !state.dirty && !!state.folder },
    { label: 'Save as…', kbd: 'Ctrl+Shift+S', action: saveAs },
    { label: 'Import audio…', kbd: 'Ctrl+I', action: () => { const t = pasteTarget(); importPick(t.track, t.at) } },
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

// Closing the window with unsaved changes asks first (D-042). Before 2026-11-26 it just closed.
if (inTauri) {
  import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
    const w = getCurrentWindow()
    let asking = false
    w.onCloseRequested(async (ev) => {
      if (!state.dirty) return
      ev.preventDefault()
      if (asking) return
      asking = true
      const go = await confirmUnsaved()
      asking = false
      if (go) await w.destroy()
    })
  })
}

$('#t-start').onclick = () => { tp.pos = 0; changed() }
el.play.onclick = togglePlay
$('.headroom').onclick = () => { tp.peakDb = -Infinity; tp.clipped = false; changed() }
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
        <div class="row1"><span class="kind" title="${t.kind === 'audio' ? 'Audio track' : 'Instrument track'}">${t.kind === 'audio' ? icon.wave : icon.keys}</span><span class="tname">${esc(t.name)}</span>${t.kind === 'instrument' ? instHtml(t) : ''}</div>
        <div class="row2">
          <button class="tbtn m ${t.mute ? 'on' : ''}" title="Mute (M)">M</button>
          <button class="tbtn s ${t.solo ? 'on' : ''}" title="Solo (S)">S</button>
          <div class="hfader" title="Volume: drag, Shift for fine, double-click for 0 dB" style="--p:${dbToPos(t.gain_db)}"><div class="rail"></div><div class="fill"></div><div class="thumb"></div></div>
          <span class="knob-num gain" title="Volume: drag, Shift for fine, double-click for 0 dB">${fmtDb(t.gain_db)} dB</span>
          <button class="tbtn fx ${t.effects.length ? 'on' : ''}" title="${t.effects.length ? `Effects: ${t.effects.map(fxLabel).join(', ')}` : 'Add an effect'}">FX</button>
        </div>
      </div>
    </div>`).join('') + `<div class="add-track"><button class="btn" data-add="audio">${icon.plus} Audio</button><button class="btn" data-add="instrument">${icon.plus} Instrument</button></div>`

  rows.querySelectorAll<HTMLElement>('.track').forEach((row) => {
    const id = +row.dataset.id!
    const t = track(id)!
    row.onpointerdown = () => { if (ui.selTrack !== id) { ui.selTrack = id; changed() } }
    row.oncontextmenu = (e) => { e.preventDefault(); ui.selTrack = id; changed(); trackMenu(t, { x: e.clientX, y: e.clientY }) }
    $('.tname', row).ondblclick = () => rename(t)
    $('.m', row).onclick = (e) => { e.stopPropagation(); toggleMute(id) }
    $('.s', row).onclick = (e) => { e.stopPropagation(); toggleSolo(id) }
    // An instrument track: its plugin, and whether the keyboards play it (D-044).
    const ib = row.querySelector<HTMLElement>('.tinst'), kb = row.querySelector<HTMLElement>('.tbtn.k')
    if (ib) ib.onclick = (e) => { e.stopPropagation(); const r = ib.getBoundingClientRect(); instMenu(t, { x: r.left, y: r.bottom + 4 }) }
    if (kb) kb.onclick = (e) => { e.stopPropagation(); toggleLive(t, e.ctrlKey || e.shiftKey) }
    // Effects from the track itself (asked for 2026-11-26); pan is on the mixer strip.
    $('.fx', row).onclick = (e) => { e.stopPropagation(); const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); fxMenu(id, { x: r.left, y: r.bottom + 4 }) }
    const g = $('.gain', row), hf = $('.hfader', row)
    // The slider is the mixer's fader lying down: the same travel, the same command.
    hf.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return
      e.preventDefault(); e.stopPropagation()
      try { hf.setPointerCapture(e.pointerId) } catch { /* synthetic event */ }
      const start = dbToPos(t.gain_db), x0 = e.clientX, w = Math.max(1, hf.clientWidth - 8)
      let db = t.gain_db
      const mv = (m: PointerEvent) => {
        db = posToDb(Math.min(1, Math.max(0, start + (m.clientX - x0) / w * (m.shiftKey ? 0.1 : 1))))
        hf.style.setProperty('--p', String(dbToPos(db)))
        g.textContent = `${fmtDb(db)} dB`
      }
      const up = () => {
        hf.removeEventListener('pointermove', mv); hf.removeEventListener('pointerup', up); hf.removeEventListener('pointercancel', up)
        if (db !== t.gain_db) run(`track.gain #${id} ${db}`)
      }
      hf.addEventListener('pointermove', mv); hf.addEventListener('pointerup', up); hf.addEventListener('pointercancel', up)
    })
    hf.addEventListener('dblclick', (e) => { e.stopPropagation(); run(`track.gain #${id} 0`) })
    dragNumber(g, () => t.gain_db, { min: -60, max: 24, perPx: 0.25, step: 0.1, reset: 0, preview: (v) => (g.textContent = `${fmtDb(v)} dB`), commit: (v) => run(`track.gain #${id} ${+v.toFixed(1)}`) })
  })
  rows.querySelectorAll<HTMLButtonElement>('[data-add]').forEach((b) => (b.onclick = () => addTrack(b.dataset.add as 'audio' | 'instrument')))
}

/** The instrument and keyboard buttons of an instrument track's header. */
function instHtml(t: Track): string {
  const b = instButton(t), k = keyButton(t)
  return `<button class="tinst ${b.cls}" title="${esc(b.title)}">${esc(b.label)}</button>`
    + (t.instrument ? `<button class="tbtn k ${k.on ? 'on' : ''} ${k.on && inst.hit ? 'hit' : ''}" title="${esc(k.title)}">${icon.keys}</button>` : '')
}

function trackMenu(t: Track, at: { x: number; y: number }) {
  const i = state.project.tracks.findIndex((x) => x.id === t.id), n = state.project.tracks.length
  menu(at, [
    { label: 'Rename', kbd: 'F2', action: () => rename(t) },
    { label: 'Move up', action: () => run(`track.move #${t.id} ${i}`), disabled: i === 0 },
    { label: 'Move down', action: () => run(`track.move #${t.id} ${i + 2}`), disabled: i === n - 1 },
    { label: 'Import audio at playhead…', kbd: 'Ctrl+I', action: () => importPick(t.id, pasteTarget().at) },
    ...(t.kind === 'instrument' ? [{ label: t.instrument ? `Instrument: ${t.instrument.name}…` : 'Choose instrument…', action: () => instMenu(t, at) } as Item] : []),
    { label: 'Effects…', action: () => fxMenu(t.id, at) },
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
    // An instrument track is no use without an instrument: ask for it straight away.
    if (t && kind === 'instrument') void openInstrumentPicker(t)
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
  const at = Math.round(tp.pos * PPQ)
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
    { label: 'Import audio here…', kbd: 'Ctrl+I', action: () => importPick(trackId, at) },
    ...(t?.kind === 'instrument' ? [{ label: 'New MIDI clip here', action: () => runSelecting(`clip.add #${t.id} ${Math.floor(at / bpb()) * bpb()} ${bpb()}`) }] : []),
    { label: 'Select all clips', kbd: 'Ctrl+A', action: () => { selectClips(state.project.clips.map((c) => c.id)); changed() } },
  ])
}

// ------------------------------------------------------------------ audio import
/** Import files and place them at `at`: the first on `trackId` if that is an audio track, every
 *  other one on a new audio track named after its file. A project that was never saved takes
 *  them too (D-042): the engine keeps their copies until the first save. */
async function importFiles(paths: string[], trackId: number | null, at: number) {
  if (!paths.length) return
  let target = track(trackId)?.kind === 'audio' ? trackId : null
  const made: number[] = []
  // Once the person has agreed to convert, the rest of the files in this drop are converted too.
  let convert = false
  for (const path of paths) {
    let [imp] = await run(`audio.import ${q(path)}${convert ? ' --convert' : ''}`, { handles: ['E5017'] })
    if (imp && !imp.ok && imp.code === 'E5017') {
      // A file at another rate: ask, instead of refusing it. Its rate is in the message.
      const rate = +(imp.message?.match(/is (\d+) Hz/)?.[1] ?? 0), secs = +(imp.message?.match(/([\d.]+) s long/)?.[1] ?? 0)
      const name = path.split(/[\\/]/).pop() ?? path
      const audioClips = state.project.clips.some((c) => c.content.kind === 'audio' && c.content.frames)
      const answer = await askConvert([[name, rate, secs]], state.project.sample_rate, 'import', !audioClips)
      if (answer === 'project') {
        if (!(await run(`project.rate ${rate}`))[0]?.ok) break
        ;[imp] = await run(`audio.import ${q(path)}`)
      } else if (answer === 'convert') {
        convert = true
        ;[imp] = await run(`audio.import ${q(path)} --convert`)
      } else break
    }
    if (!imp?.ok) break
    const converted = imp.data?.converted === true
    // Never stack a file on top of audio already there: use a new track instead.
    const p = state.project, lenBeats = imp.data.frames / ((imp.data.rate * 60) / p.tempo_bpm)
    if (target !== null && p.clips.some((c) => c.track === target && c.start / PPQ < at + lenBeats && (c.start + c.length) / PPQ > at)) target = null
    if (target === null) {
      const [added] = await run(`track.add ${q(String(imp.data.source).replace(/\.\w+$/, ''))} --kind audio`)
      if (!added?.ok) break
      target = added.data.id
    }
    made.push(...created(await run(`clip.place #${target} ${at} ${q(imp.data.source)}${converted ? ' --convert' : ''}`)))
    target = null
  }
  if (made.length) { selectClips(made); changed() }
}

/** Import (Ctrl+I, the menus, the button on an empty audio track): straight to the system's file
 *  picker. Before 2026-11-26 it asked for a path to be typed. */
async function importPick(trackId: number | null, at: number) {
  const files = await pickFiles('Import audio')
  if (files.length) await importFiles(files, trackId, at)
}
hooks.addAudio = (trackId) => importPick(trackId, 0)

/** The FX button on a track: its effects, and adding one. Straight to the picker when it has none. */
function fxMenu(trackId: number, at: { x: number; y: number }) {
  const t = track(trackId)
  if (!t) return
  if (!t.effects.length) return openEffectPicker(t)
  menu(at, [
    ...t.effects.map((f, i): Item => ({ label: `${fxLabel(f)}…`, small: f.bypass ? 'bypassed' : `slot ${i + 1}`, action: () => openEffectEditor(t.id, i + 1) })),
    'sep',
    { label: 'Add effect…', action: () => openEffectPicker(t), disabled: t.effects.length >= 8 },
  ])
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
  if (tp.playing) follow()
  draw(); drawMeters(); updateReadouts()
  const ms = performance.now() - t0
  perf.frames++; perf.totalMs += ms; perf.worstMs = Math.max(perf.worstMs, ms)
  frameDrawn(ms)
  if (settling()) requestAnimationFrame(frame)
  else looping = false
}
if (import.meta.env.DEV) Object.assign(window, { __crucible: { perf, draw, drawMeters, state: () => state, ui, tp, run, changed, flush, advance, togglePlay } })

// ------------------------------------------------------------------ render on change
onChange(() => {
  if (ui.selTrack !== null && !track(ui.selTrack)) ui.selTrack = null
  if ([...ui.selClips].some((i) => !clip(i))) selectClips([...ui.selClips].filter((i) => clip(i)))
  if (ui.selClip !== null && !clip(ui.selClip)) ui.selClip = null
  clampScroll()
  rows.style.transform = `translateY(${-ui.scrollY}px)`
  // With a mouse button held, only the canvas is redrawn. Rebuilding the page now would replace
  // the element being clicked or dragged; it is rebuilt when the button is released.
  if (pointerHeld()) { owe(); draw(); return }
  updateTop()
  renderHeaders()
  renderTabs()
  renderPanel()
  draw()
})

// ------------------------------------------------------------------ keyboard
addEventListener('keydown', (e) => {
  // F9 marks the moment in the log. It works everywhere, also while typing or in a dialog.
  if (e.key === 'F9' && !e.repeat) { e.preventDefault(); return markPrompt() }
  if (e.target instanceof Element && e.target.matches('input, select, textarea')) return
  if (e.key === 'Escape' && layerOpen()) return closeLayer()
  if (layerOpen()) return
  const ctrl = e.ctrlKey || e.metaKey
  const sel = ui.selTrack
  const k = e.key.toLowerCase()
  const handled = () => e.preventDefault()
  // Selected text (the Console's output, a message) is the person's to copy to the Windows
  // clipboard. Copy, cut and select-all act on clips only when no text is selected. Before
  // 2026-11-25 Ctrl+C always copied clips, so copying an error message from the Console failed.
  const text = getSelection()
  if (ctrl && (k === 'c' || k === 'x' || k === 'a') && text && !text.isCollapsed && text.toString().trim()) return
  if (ctrl) {
    if (k === 'z' && !e.shiftKey) { handled(); run('undo') }
    else if ((k === 'z' && e.shiftKey) || k === 'y') { handled(); run('redo') }
    else if (k === 's' && e.shiftKey) { handled(); saveAs() }
    else if (k === 's') { handled(); save() }
    else if (k === 'o') { handled(); openProject() }
    else if (k === 'n') { handled(); newProject() }
    else if (k === 'e' && e.shiftKey) { handled(); openExport() }
    else if (k === 'i') { handled(); const t = pasteTarget(); importPick(t.track, t.at) }
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
    case 'Home': case 'Enter': handled(); tp.pos = 0; changed(); break
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
  await loadKinds()
  ui.selTrack = state.project.tracks[0]?.id ?? null
  watchDevices()
  watchInstruments()
  document.fonts?.ready.then(() => changed()) // canvases redraw once the bundled fonts are in
  const help = await run('help', { quiet: true })
  setCommandNames([...(help[0]?.text ?? '').matchAll(/^\s{2}([a-z][\w.]*)/gm)].map((m) => m[1]))
  changed()
}
start()
