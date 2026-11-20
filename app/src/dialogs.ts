// Menus and dialogs: project menu, track menu, settings (one page), export, plugin picker,
// open/save. One level deep, never nested (05-gui-design-brief.md).
import { demo, devices, plugins } from './demo'
import { q, type Track } from './engine'
import { icon } from './icons'
import { changed, esc, run, state, toast } from './store'
import { refreshPalette } from './timeline'

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

function dialog(title: string, html: string, opts: { wide?: boolean; footer?: string; tag?: string } = {}): HTMLElement {
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
    if (r.every((x) => x.ok)) { closeLayer(); if (mode !== 'save') { demo.clips = []; demo.seeded.clear() } toast('info', r.at(-1)?.text?.split('\n')[0] ?? 'Done') }
  }
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = go
  input.onkeydown = (e) => { if (e.key === 'Enter') go(); e.stopPropagation() }
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
  const d = dialog('Export', `<div class="content">
      <div class="row"><span>Format</span><div class="seg" data-g="fmt"><button class="on">WAV</button><button>FLAC</button><button>AIFF</button><button>MP3</button></div></div>
      <div class="row"><span>Bit depth</span><div class="seg" data-g="bits"><button>16-bit</button><button class="on">24-bit</button><button>32-bit float</button></div></div>
      <div class="row"><span>Sample rate</span><select class="input"><option>${p.sample_rate} Hz (project rate, no resampling)</option>${[44100, 48000, 96000].filter((r) => r !== p.sample_rate).map((r) => `<option>${r} Hz (resampled)</option>`).join('')}</select></div>
      <div class="row"><span>Range</span><div class="seg"><button class="on">Whole song</button><button>Loop</button><button>Selection</button></div></div>
      <div class="row"><span>Tail</span><select class="input"><option>Until silence (max 10 s)</option><option>None</option><option>2 seconds</option></select></div>
      <div class="row"><span>Loudness</span><div class="seg"><button class="on">Untouched</button><button>Peak −1 dBTP</button><button>−14 LUFS</button></div></div>
      <div class="row"><span>Dither</span><span style="color:var(--text-dim)" data-dither>Off: 24-bit needs none</span></div>
      <div class="row"><span>File</span><input class="input" value="${esc(p.name)}.wav" spellcheck="false"></div>
    </div>`, { tag: DEMO, footer: `<span class="note">Lossless and untouched by default.</span><button class="btn" data-close>Cancel</button><button class="btn primary" data-go>${icon.export} Export</button>` })
  d.querySelectorAll<HTMLElement>('.seg').forEach((seg) => seg.querySelectorAll('button').forEach((b) => (b.onclick = () => {
    seg.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b))
    if (seg.dataset.g === 'bits') d.querySelector('[data-dither]')!.textContent = b.textContent === '16-bit' ? 'TPDF, applied once at the end' : `Off: ${b.textContent} needs none`
    if (seg.dataset.g === 'fmt') { const f = d.querySelector<HTMLInputElement>('.row:last-child input')!; f.value = f.value.replace(/\.\w+$/, '.' + b.textContent!.toLowerCase()) }
  })))
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = () => { closeLayer(); toast('info', 'Export is not built yet', 'It arrives with the render engine (M5). This dialog shows the planned choices.') }
}

// ------------------------------------------------------------------ settings: one page
const SECTIONS = ['Audio', 'MIDI', 'Plugins', 'Project', 'Appearance', 'Keyboard', 'Diagnostics']
const ACCENTS = ['#ff7a2f', '#5aa2ff', '#3fd18b', '#c084fc', '#f5c542', '#ff5c8a']
const prefs = { driver: 0, output: 0, input: 0, buffer: 2, exclusive: true, accent: '#ff7a2f', reduceMotion: false }

export const SHORTCUTS: [string, string][] = [
  ['Play / stop', 'Space'], ['Record', 'R'], ['Return to start', 'Home'], ['Loop on/off', 'L'], ['Metronome', 'K'],
  ['Undo', 'Ctrl+Z'], ['Redo', 'Ctrl+Shift+Z'], ['Save', 'Ctrl+S'], ['Export', 'Ctrl+E'], ['Settings', 'Ctrl+,'],
  ['Add track', 'Ctrl+T'], ['Rename track', 'F2'], ['Mute / solo track', 'M / S'], ['Remove track', 'Del'],
  ['Select track above / below', '↑ / ↓'], ['Zoom in / out', '+ / −'], ['Mixer / piano roll / clip / console', '1 / 2 / 3 / `'], ['Show / hide panel', 'B'],
]

export function openSettings(section = 'Audio') {
  const d = dialog('Settings', `<div class="settings"><nav>${SECTIONS.map((s) => `<button data-s="${s}">${s}</button>`).join('')}</nav><div class="pane"></div></div>`, { wide: true, tag: DEMO })
  const pane = d.querySelector<HTMLElement>('.pane')!
  const show = (s: string) => {
    d.querySelectorAll<HTMLButtonElement>('nav button').forEach((b) => b.classList.toggle('on', b.dataset.s === s))
    pane.innerHTML = sectionHtml(s)
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
    case 'Appearance': return `<h3>Look</h3>
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
  pane.querySelectorAll<HTMLButtonElement>('[data-accent]').forEach((b) => (b.onclick = () => {
    prefs.accent = b.dataset.accent!
    document.documentElement.style.setProperty('--accent', prefs.accent)
    document.documentElement.style.setProperty('--accent-soft', prefs.accent + '26')
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

function refresh(s: string, pane: HTMLElement) { pane.innerHTML = sectionHtml(s); wire(s, pane) }
