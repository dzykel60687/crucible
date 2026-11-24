// Menus and dialogs: project menu, track menu, settings (one page), export, plugin picker,
// open/save. One level deep, never nested (05-gui-design-brief.md).
import { demo, devices, plugins } from './demo'
import { PPQ, q, type Track } from './engine'
import { icon } from './icons'
import { changed, esc, run, selectClips, state, toast, ui } from './store'
import { refreshPalette } from './timeline'
import { applyLayout, currentLayout, LAYOUTS } from './layout'

const layer = document.getElementById('layer')!
export const closeLayer = () => layer.replaceChildren()
export const layerOpen = () => layer.childElementCount > 0

export type Item = { label: string; kbd?: string; small?: string; action: () => void; disabled?: boolean; danger?: boolean } | 'sep'

export function menu(at: { x: number; y: number }, items: Item[]) {
  closeLayer()
  const scrim = document.createElement('div')
  scrim.style.cssText = 'position:fixed;inset:0'
  scrim.onpointerdown = closeLayer
  const m = document.createElement('div')
  m.className = 'menu'
  for (const it of items) {
    if (it === 'sep') { m.appendChild(document.createElement('hr')); continue }
    const b = document.createElement('button')
    b.disabled = !!it.disabled
    if (it.danger) b.className = 'danger'
    b.innerHTML = `<span>${esc(it.label)}</span>${it.kbd ? `<kbd>${esc(it.kbd)}</kbd>` : it.small ? `<small>${esc(it.small)}</small>` : ''}`
    b.onclick = () => { closeLayer(); it.action() }
    m.appendChild(b)
  }
  layer.append(scrim, m)
  const r = m.getBoundingClientRect()
  m.style.left = `${Math.min(at.x, innerWidth - r.width - 8)}px`
  m.style.top = `${Math.min(at.y, innerHeight - r.height - 8)}px`
}

export function dialog(title: string, html: string, opts: { wide?: boolean; footer?: string; tag?: string } = {}): HTMLElement {
  closeLayer()
  const scrim = document.createElement('div')
  scrim.className = 'scrim'
  scrim.onpointerdown = closeLayer
  const d = document.createElement('div')
  d.className = 'dialog' + (opts.wide ? ' wide' : '')
  d.innerHTML = `<header><h2>${esc(title)}</h2>${opts.tag ?? ''}<button class="btn icon" data-close title="Close (Esc)">${icon.close}</button></header>${html}${opts.footer ? `<footer>${opts.footer}</footer>` : ''}`
  d.querySelectorAll<HTMLElement>('[data-close]').forEach((b) => (b.onclick = closeLayer))
  layer.append(scrim, d)
  return d
}

const DEMO = '<span class="tag demo">demo</span>'

// ------------------------------------------------------------------ open / save
export function pathPrompt(mode: 'open' | 'save' | 'new') {
  const titles = { open: 'Open project', save: 'Save project as', new: 'New project' }
  const d = dialog(titles[mode], `<div class="content">
      ${mode === 'new'
        ? `<label class="row"><span>Name</span><input class="input" value="Untitled"></label>`
        : `<label class="row"><span>Project folder</span><input class="input" spellcheck="false" placeholder="C:\\Music\\My Song" value="${esc(state.folder ?? '')}"></label>
           <div class="row"><span></span><div class="sub">A project is a folder holding project.json and its audio. A folder picker comes with the file-dialog plugin.</div></div>`}
      ${mode === 'open' && state.dirty ? `<label class="row"><span>Unsaved changes</span><span><button class="switch" data-discard></button> <small style="color:var(--text-faint);margin-left:8px">discard them</small></span></label>` : ''}
    </div>`, { footer: `<button class="btn" data-close>Cancel</button><button class="btn primary" data-go>${mode === 'open' ? 'Open' : mode === 'new' ? 'Create' : 'Save'}</button>` })
  const input = d.querySelector('input')!
  input.focus(); input.select()
  const discard = d.querySelector<HTMLButtonElement>('[data-discard]')
  if (discard) discard.onclick = () => discard.classList.toggle('on')
  const go = async () => {
    const v = input.value.trim()
    if (!v) return
    const cmd = mode === 'new' ? `project.new ${q(v)}` : mode === 'open' ? `project.open ${q(v)}${discard?.classList.contains('on') ? ' --discard' : ''}` : `project.save ${q(v)}`
    const r = await run(cmd)
    if (r.every((x) => x.ok)) { closeLayer(); if (mode !== 'save') selectClips([]); toast('info', r.at(-1)?.text?.split('\n')[0] ?? 'Done') }
  }
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = go
  input.onkeydown = (e) => { if (e.key === 'Enter') go(); e.stopPropagation() }
}

/** One text field, OK and Cancel. Calls done with the trimmed value if it changed. */
export function textPrompt(title: string, value: string, done: (v: string) => void) {
  const d = dialog(title, `<div class="content"><input class="input" value="${esc(value)}" spellcheck="false"></div>`, { footer: '<button class="btn" data-close>Cancel</button><button class="btn primary" data-go>OK</button>' })
  const input = d.querySelector('input')!
  input.focus(); input.select()
  const go = () => { const v = input.value.trim(); closeLayer(); if (v && v !== value) done(v) }
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = go
  input.onkeydown = (e) => { e.stopPropagation(); if (e.key === 'Enter') go(); if (e.key === 'Escape') closeLayer() }
}

// ------------------------------------------------------------------ plugin picker
export function openPluginPicker(t: Track, kind: 'Instrument' | 'Effect') {
  const list = plugins.filter((p) => p.kind === kind)
  const d = dialog(kind === 'Instrument' ? `Instrument for ${t.name}` : `Add effect to ${t.name}`, `<div class="content">
      <label style="position:relative"><input class="input" placeholder="Search plugins" style="padding-left:34px"><span style="position:absolute;left:10px;top:8px;width:18px;color:var(--text-faint)">${icon.search}</span></label>
      <div class="plugin-list">${list.map((p, i) => `<button data-i="${i}"><span class="pi">${icon.plug}</span><span><b>${esc(p.name)}</b><small>${esc(p.vendor)} · ${esc(p.note)}</small></span></button>`).join('')}</div>
    </div>`, { tag: DEMO, footer: `<span class="note">Real list comes from the plugin scan (crucible plugin.scan). Each plugin loads in its own process.</span><button class="btn" data-close>Cancel</button>` })
  const input = d.querySelector('input')!
  input.focus()
  input.oninput = () => d.querySelectorAll<HTMLButtonElement>('.plugin-list button').forEach((b) => (b.style.display = b.textContent!.toLowerCase().includes(input.value.toLowerCase()) ? '' : 'none'))
  input.onkeydown = (e) => e.stopPropagation()
  d.querySelectorAll<HTMLButtonElement>('.plugin-list button').forEach((b) => (b.onclick = () => {
    const p = list[+b.dataset.i!]
    if (kind === 'Instrument') demo.instrument.set(t.id, p.name)
    else demo.inserts.set(t.id, [...(demo.inserts.get(t.id) ?? []), p.name])
    closeLayer(); changed()
  }))
}

// ------------------------------------------------------------------ export
export function openExport() {
  const p = state.project
  // Ranges in beats. Loop comes from the transport stand-in; selection is the selected clips' span.
  const sel = state.project.clips.filter((c) => ui.selClips.has(c.id))
  const ranges: Record<string, [number, number] | null> = {
    song: null,
    loop: demo.loop ? [demo.loopStart, demo.loopEnd] : null,
    selection: sel.length ? [Math.min(...sel.map((c) => c.start)) / PPQ, Math.max(...sel.map((c) => c.start + c.length)) / PPQ] : null,
  }
  const sep = state.folder?.includes('/') ? '/' : '\\'
  const file = state.folder ? `${state.folder}${sep}${p.name}.wav` : ''
  const d = dialog('Export', `<div class="content">
      <div class="row"><span>Format</span><span style="color:var(--text-dim)">WAV, stereo, ${p.sample_rate} Hz (the project rate, no resampling)</span></div>
      <div class="row"><span>Bit depth</span><div class="seg" data-g="bits"><button data-v="16">16-bit</button><button data-v="24" class="on">24-bit</button><button data-v="32f">32-bit float</button></div></div>
      <div class="row"><span>Range</span><div class="seg" data-g="range"><button data-v="song" class="on">Whole song</button><button data-v="loop" ${ranges.loop ? '' : 'disabled title="Loop is off"'}>Loop</button><button data-v="selection" ${ranges.selection ? '' : 'disabled title="No clips selected"'}>Selection</button></div></div>
      <div class="row"><span>Dither</span><label style="color:var(--text-dim)" data-dither>Off: 24-bit needs none</label></div>
      <div class="row"><span>File</span><input class="input" value="${esc(file)}" placeholder="full path, e.g. C:\\Music\\Song.wav" spellcheck="false"></div>
      <div class="row"><span></span><span style="color:var(--text-faint);font-size:var(--fs-s)">Audio clips only: MIDI is silent until instruments are hosted. Nothing is normalised or limited.</span></div>
    </div>`, { footer: `<span class="note">Lossless and untouched by default.</span><button class="btn" data-close>Cancel</button><button class="btn primary" data-go>${icon.export} Export</button>` })
  const pick = (g: string) => d.querySelector<HTMLElement>(`[data-g="${g}"] .on`)!.dataset.v!
  let dither = true
  const showDither = () => {
    const el = d.querySelector<HTMLElement>('[data-dither]')!
    if (pick('bits') !== '16') { el.textContent = `Off: ${pick('bits') === '24' ? '24-bit' : 'float'} needs none`; return }
    el.innerHTML = `<input type="checkbox" ${dither ? 'checked' : ''}> TPDF dither, applied once at the end`
    el.querySelector('input')!.onchange = (e) => { dither = (e.target as HTMLInputElement).checked }
  }
  d.querySelectorAll<HTMLElement>('.seg').forEach((seg) => seg.querySelectorAll('button').forEach((b) => (b.onclick = () => {
    seg.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b))
    showDither()
  })))
  const input = d.querySelector<HTMLInputElement>('input.input')!
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = async () => {
    const path = input.value.trim().replace(/^"(.*)"$/, '$1')
    // Absolute only: C:\..., \\server\share\... or /...; a bare name would land wherever the program started.
    if (!/^([A-Za-z]:[\\/]|\\\\|\/)/.test(path)) { input.focus(); toast('error', 'Give the full path of the file to write', 'For example C:\\Music\\Song.wav'); return }
    const bits = pick('bits'), r = ranges[pick('range')]
    const line = `project.export ${q(path.endsWith('.wav') ? path : path + '.wav')} --bits ${bits}${bits === '16' && !dither ? ' --dither off' : ''}${r ? ` --start ${+r[0].toFixed(6)} --end ${+r[1].toFixed(6)}` : ''}`
    closeLayer()
    const [rep] = await run(line)
    if (rep?.ok) toast(rep.data.clipped ? 'error' : 'info', rep.data.clipped ? 'Exported, with clipping' : 'Exported', rep.text ?? '', rep.data.clipped ? 'W5020' : undefined)
  }
}

// ------------------------------------------------------------------ settings: one page
const SECTIONS = ['Audio', 'MIDI', 'Plugins', 'Project', 'Appearance', 'Keyboard', 'Diagnostics']
/** Sections whose controls really work. The others show planned settings with demo values. */
const LIVE = new Set(['Project', 'Appearance', 'Keyboard'])
const ACCENTS = ['#45c08f', '#5aa2ff', '#2dd4bf', '#a78bfa', '#e6b450', '#f47c96']
const prefs = { driver: 0, output: 0, input: 0, buffer: 2, exclusive: true, accent: '#45c08f', reduceMotion: false }

export const SHORTCUTS: [string, string][] = [
  ['Play / stop', 'Space'], ['Record', 'R'], ['Return to start', 'Home'], ['Loop on/off', 'L'], ['Metronome', 'K'],
  ['Undo', 'Ctrl+Z'], ['Redo', 'Ctrl+Shift+Z'], ['Save', 'Ctrl+S'], ['Export', 'Ctrl+Shift+E'], ['Settings', 'Ctrl+,'],
  ['Copy / cut / paste clips', 'Ctrl+C / X / V'], ['Duplicate clips', 'Ctrl+D'], ['Split at playhead', 'Ctrl+E'],
  ['Delete clips (or note)', 'Del'], ['Select all clips', 'Ctrl+A'], ['Nudge clips by grid / bar', '← → / Shift'],
  ['Move clips: drag · copy: Ctrl+drag', 'mouse'], ['Trim: drag a clip edge · no snap: Alt', 'mouse'],
  ['Import audio (or drop files on the window)', 'Ctrl+I'], ['Add track', 'Ctrl+T'], ['Rename track', 'F2'], ['Mute / solo track', 'M / S'], ['Remove track', 'Ctrl+Del'],
  ['Select track above / below', '↑ / ↓'], ['Zoom in / out', '+ / − or Ctrl+wheel'], ['Mixer / piano roll / clip / console', '1 / 2 / 3 / `'],
  ['Show / hide panel', 'B'], ['Next layout', 'Ctrl+Shift+L'],
]

export function openSettings(section = 'Audio') {
  const d = dialog('Settings', `<div class="settings"><nav>${SECTIONS.map((s) => `<button data-s="${s}">${s}</button>`).join('')}</nav><div class="pane"></div></div>`, { wide: true })
  const pane = d.querySelector<HTMLElement>('.pane')!
  const show = (s: string) => {
    d.querySelectorAll<HTMLButtonElement>('nav button').forEach((b) => b.classList.toggle('on', b.dataset.s === s))
    pane.innerHTML = (LIVE.has(s) ? '' : '<div><span class="tag demo">demo values: not wired to devices yet</span></div>') + sectionHtml(s)
    pane.scrollTop = 0
    wire(s, pane)
  }
  d.querySelectorAll<HTMLButtonElement>('nav button').forEach((b) => (b.onclick = () => show(b.dataset.s!)))
  show(section)
}

const sel = (opts: string[], i: number, attr: string) => `<select class="input" ${attr}>${opts.map((o, k) => `<option value="${k}" ${k === i ? 'selected' : ''} ${o.includes('not in this build') ? 'disabled' : ''}>${esc(o)}</option>`).join('')}</select>`

function sectionHtml(s: string): string {
  const p = state.project
  const lat = (devices.buffers[prefs.buffer] / p.sample_rate * 1000).toFixed(1)
  switch (s) {
    case 'Audio': return `<h3>Audio device</h3>
      <label class="row"><span>Driver</span>${sel(devices.drivers, prefs.driver, 'data-p="driver"')}</label>
      <label class="row"><span>Output</span>${sel(devices.outputs, prefs.output, 'data-p="output"')}</label>
      <label class="row"><span>Input</span>${sel(devices.inputs, prefs.input, 'data-p="input"')}</label>
      <div class="row"><span>Buffer</span><div class="seg" data-buf>${devices.buffers.map((b, i) => `<button class="${i === prefs.buffer ? 'on' : ''}" data-i="${i}">${b}</button>`).join('')}</div></div>
      <div class="row"><span></span><div class="sub">${devices.buffers[prefs.buffer]} samples at ${p.sample_rate / 1000} kHz = ${lat} ms each way</div></div>
      <div class="row"><span>Exclusive mode</span><span><button class="switch ${prefs.exclusive ? 'on' : ''}" data-x="exclusive"></button></span></div>
      <div class="row"><span></span><div class="sub">Takes the device over for the lowest latency. Other programs go silent meanwhile.</div></div>
      <h3>Safety</h3>
      <div class="row"><span>Device rate</span><span style="color:var(--ok)">Matches the project (${p.sample_rate} Hz). Nothing is resampled.</span></div>
      <div class="row"><span>Test</span><span><button class="btn solid" data-act="audio.test">Run timing test</button></span></div>`
    case 'MIDI': return `<h3>MIDI inputs</h3><div class="list">${devices.midi.map((m, i) => `<div><span class="grow">${esc(m.name)}<br><small>Notes go to the selected instrument track</small></span><button class="switch ${m.on ? 'on' : ''}" data-midi="${i}"></button></div>`).join('')}</div>
      <div class="row"><span>Record from</span>${sel(['All enabled inputs', ...devices.midi.map((m) => m.name)], 0, '')}</div>`
    case 'Plugins': return `<h3>VST3 folders</h3><div class="list">
        <div><span class="grow">C:\\Program Files\\Common Files\\VST3<br><small>Standard location · always scanned</small></span></div>
        <div><span class="grow">D:\\Plugins\\VST3<br><small>Added by you</small></span><button class="btn">Remove</button></div></div>
      <div><button class="btn solid">${icon.plus} Add folder</button> <button class="btn solid" data-act="plugin.scan">Rescan</button></div>
      <h3>Found</h3><div class="list">${plugins.map((pl) => `<div><span class="grow">${esc(pl.name)}<br><small>${esc(pl.vendor)} · ${pl.kind}</small></span><span class="tag live">OK</span></div>`).join('')}</div>`
    case 'Project': return `<h3>This project</h3>
      <label class="row"><span>Name</span><input class="input" data-cmd="project.rename" value="${esc(p.name)}"></label>
      <label class="row"><span>Sample rate</span><select class="input" data-cmd="project.rate">${[44100, 48000, 88200, 96000, 176400, 192000].map((r) => `<option ${r === p.sample_rate ? 'selected' : ''}>${r}</option>`).join('')}</select></label>
      <label class="row"><span>Tempo</span><input class="input" data-cmd="project.tempo" value="${p.tempo_bpm}"></label>
      <label class="row"><span>Time signature</span><input class="input" data-cmd="project.signature" value="${p.time_signature.join(' ')}"></label>
      <div class="row"><span>Folder</span><span style="color:var(--text-dim);user-select:text">${esc(state.folder ?? 'Not saved yet')}</span></div>
      <div class="row"><span>Crash journal</span><span style="color:var(--text-dim)">${state.journal ? 'On. Edits since the last save survive a crash.' : 'Starts after the first save.'}</span></div>
      <div class="row"><span></span><div class="sub" style="color:var(--ok)">These settings are live: each change runs an engine command.</div></div>`
    case 'Appearance': return `<h3>Layout</h3>
      <div class="layout-cards">${LAYOUTS.map((l) => `<button data-layout="${l.id}" class="${l.id === currentLayout() ? 'on' : ''}">${l.sketch}<b>${esc(l.name)}</b><small>${esc(l.note)}</small></button>`).join('')}</div>
      <div class="row"><span></span><div class="sub">Remembered on this computer. Ctrl+Shift+L switches to the next one.</div></div>
      <h3>Look</h3>
      <div class="row"><span>Theme</span><div class="seg"><button class="on">Dark</button><button disabled title="Later, through the same design tokens">Light</button></div></div>
      <div class="row"><span>Accent</span><div class="swatches">${ACCENTS.map((c) => `<button style="--c:${c}" class="${c === prefs.accent ? 'on' : ''}" data-accent="${c}"></button>`).join('')}</div></div>
      <div class="row"><span>Reduce motion</span><span><button class="switch ${prefs.reduceMotion ? 'on' : ''}" data-x="reduceMotion"></button></span></div>
      <div class="row"><span></span><div class="sub">Turns off the few 120 ms transitions. Nothing in Crucible animates while idle either way.</div></div>`
    case 'Keyboard': return `<h3>Shortcuts</h3><div class="keys">${SHORTCUTS.map(([a, k]) => `<span>${esc(a)}</span><kbd>${esc(k)}</kbd>`).join('')}</div>`
    case 'Diagnostics': return `<h3>When something goes wrong</h3>
      <div class="row"><span>Logs</span><span style="color:var(--text-dim);user-select:text;font-family:var(--mono);font-size:12px">%LOCALAPPDATA%\\Crucible\\logs</span></div>
      <div class="row"><span>Report</span><span><button class="btn solid" data-act="report">Create report file</button></span></div>
      <div class="row"><span></span><div class="sub">Bundles the machine check and recent logs into one file to send. For now run <code>crucible report</code>.</div></div>
      <div class="row"><span>Error codes</span><span style="color:var(--text-dim)">Every failure shows a code like E2004 with what to try.</span></div>`
  }
  return ''
}

function wire(s: string, pane: HTMLElement) {
  pane.querySelectorAll<HTMLSelectElement>('select[data-p]').forEach((el) => (el.onchange = () => { (prefs as any)[el.dataset.p!] = +el.value }))
  pane.querySelectorAll<HTMLButtonElement>('[data-buf] button').forEach((b) => (b.onclick = () => { prefs.buffer = +b.dataset.i!; refresh(s, pane) }))
  pane.querySelectorAll<HTMLButtonElement>('[data-x]').forEach((b) => (b.onclick = () => {
    const k = b.dataset.x as 'exclusive' | 'reduceMotion'
    prefs[k] = !prefs[k]
    b.classList.toggle('on', prefs[k])
    if (k === 'reduceMotion') document.documentElement.style.setProperty('--t', prefs.reduceMotion ? '0ms' : '120ms')
  }))
  pane.querySelectorAll<HTMLButtonElement>('[data-midi]').forEach((b) => (b.onclick = () => { const m = devices.midi[+b.dataset.midi!]; m.on = !m.on; b.classList.toggle('on', m.on) }))
  pane.querySelectorAll<HTMLButtonElement>('[data-layout]').forEach((b) => (b.onclick = () => { applyLayout(b.dataset.layout!); refresh(s, pane) }))
  pane.querySelectorAll<HTMLButtonElement>('[data-accent]').forEach((b) => (b.onclick = () => {
    prefs.accent = b.dataset.accent!
    document.documentElement.style.setProperty('--accent', prefs.accent)
    document.documentElement.style.setProperty('--accent-soft', prefs.accent + '1f')
    document.documentElement.style.setProperty('--accent-hover', prefs.accent)
    refreshPalette(); refresh(s, pane)
  }))
  pane.querySelectorAll<HTMLButtonElement>('[data-act]').forEach((b) => (b.onclick = () => toast('info', `Run crucible ${b.dataset.act}`, 'Machine commands are in crucible.exe today. They join the window when the device layer moves into the engine.')))
  pane.querySelectorAll<HTMLInputElement | HTMLSelectElement>('[data-cmd]').forEach((el) => {
    const send = async () => {
      const v = el.value.trim()
      const arg = el.dataset.cmd === 'project.signature' ? v.split(/[\s/]+/).join(' ') : q(v)
      await run(`${el.dataset.cmd} ${arg}`)
      refresh(s, pane)
    }
    el.onchange = send
    el.onkeydown = (e) => { if (e.key === 'Enter') (el as HTMLInputElement).blur(); e.stopPropagation() }
  })
}

function refresh(s: string, pane: HTMLElement) { pane.innerHTML = (LIVE.has(s) ? '' : '<div><span class="tag demo">demo values: not wired to devices yet</span></div>') + sectionHtml(s); wire(s, pane) }
