// Menus and dialogs: project menu, track menu, settings (one page), export, plugin picker,
// open/save. One level deep, never nested (05-gui-design-brief.md).
import { audioHtml, onDevices, refreshDevices, wireAudio } from './devices'
import { exec, PPQ, q } from './engine'
import { icon } from './icons'
import { esc, run, selectClips, state, toast, ui } from './store'
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
/** Ask a question with a few answers. Resolves to the chosen answer's key, or null for Cancel,
 *  Escape or a click outside. */
export function choose(title: string, html: string, answers: [string, string][]): Promise<string | null> {
  return new Promise((resolve) => {
    let done = false
    const finish = (v: string | null) => { if (!done) { done = true; resolve(v) } }
    const d = dialog(title, `<div class="content">${html}</div>`, {
      footer: '<button class="btn" data-close>Cancel</button>' + answers.map(([k, label], i) => `<button class="btn ${i === 0 ? 'primary' : ''}" data-k="${esc(k)}">${esc(label)}</button>`).join(''),
    })
    d.querySelectorAll<HTMLButtonElement>('[data-k]').forEach((b) => (b.onclick = () => { closeLayer(); finish(b.dataset.k!) }))
    // Closed any other way (Cancel, the X, Escape, a click outside): no answer.
    new MutationObserver((_, o) => { if (!d.isConnected) { o.disconnect(); finish(null) } }).observe(layer, { childList: true })
  })
}

/** A file or project at another rate: convert, or not. `files` are [name, rate, seconds]. */
export async function askConvert(files: [string, number, number][], to: number, why: 'import' | 'rate', projectEmpty = false): Promise<'convert' | 'project' | null> {
  const mb = Math.round(files.reduce((m, f) => m + f[2] * to * 2 * 4, 0) / 1_048_576)
  const list = files.slice(0, 4).map(([n, r]) => `<li>${esc(n)}: ${r / 1000} kHz</li>`).join('') + (files.length > 4 ? `<li>and ${files.length - 4} more</li>` : '')
  const up = files.some((f) => f[1] < to)
  const html = `<p style="margin:0">${why === 'import' ? 'This audio is at another sample rate than the project' : 'This project has audio at other sample rates'} (${to / 1000} kHz${why === 'rate' ? ', the new rate' : ''}):</p>
    <ul style="margin:8px 0;padding-left:20px">${list}</ul>
    <p style="margin:0;color:var(--text-dim)">Converting keeps the files exactly as they are and converts the sound while it plays, with a high-quality converter. It needs about ${mb} MB of memory while the project is open.${up ? ` Converting up to a higher rate adds no detail that the file does not have.` : ''}</p>`
  const answers: [string, string][] = [['convert', `Convert to ${to / 1000} kHz`]]
  if (why === 'import' && projectEmpty && files.length === 1) answers.push(['project', `Set the project to ${files[0][1] / 1000} kHz`])
  return (await choose(why === 'import' ? 'Convert the sample rate?' : 'Convert audio to the new rate?', html, answers)) as 'convert' | 'project' | null
}

export function textPrompt(title: string, value: string, done: (v: string) => void) {
  const d = dialog(title, `<div class="content"><input class="input" value="${esc(value)}" spellcheck="false"></div>`, { footer: '<button class="btn" data-close>Cancel</button><button class="btn primary" data-go>OK</button>' })
  const input = d.querySelector('input')!
  input.focus(); input.select()
  const go = () => { const v = input.value.trim(); closeLayer(); if (v && v !== value) done(v) }
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = go
  input.onkeydown = (e) => { e.stopPropagation(); if (e.key === 'Enter') go(); if (e.key === 'Escape') closeLayer() }
}

/** F9. The moment is logged at once, before any typing; words typed afterwards go in as a second
 *  line that says how long ago the moment was. Closing without typing keeps the first line. */
export function markPrompt() {
  const pressed = performance.now()
  run('mark', { quiet: true })
  const d = dialog('Marked in the log', `<div class="content"><input class="input" placeholder="What did you notice? Enter adds it to the mark" spellcheck="false">
    <div class="sub" style="margin-top:8px">The time and the play position are already recorded. Send the log with Settings, Diagnostics, Create report file.</div></div>`,
    { footer: '<button class="btn" data-close>Close</button><button class="btn primary" data-go>Add note</button>' })
  const input = d.querySelector('input')!
  input.focus()
  const go = () => {
    const v = input.value.trim()
    closeLayer()
    if (v) run(`mark ${q(v)} --ago ${((performance.now() - pressed) / 1000).toFixed(1)}`, { quiet: true })
  }
  d.querySelector<HTMLButtonElement>('[data-go]')!.onclick = go
  input.onkeydown = (e) => { e.stopPropagation(); if (e.key === 'Enter') go(); if (e.key === 'Escape') closeLayer() }
}

// ------------------------------------------------------------------ export
export function openExport() {
  const p = state.project
  // Ranges in beats. Selection is the selected clips' span.
  const sel = state.project.clips.filter((c) => ui.selClips.has(c.id))
  const ranges: Record<string, [number, number] | null> = {
    song: null,
    selection: sel.length ? [Math.min(...sel.map((c) => c.start)) / PPQ, Math.max(...sel.map((c) => c.start + c.length)) / PPQ] : null,
  }
  const sep = state.folder?.includes('/') ? '/' : '\\'
  const file = state.folder ? `${state.folder}${sep}${p.name}.wav` : ''
  const d = dialog('Export', `<div class="content">
      <div class="row"><span>Format</span><span style="color:var(--text-dim)">WAV, stereo, ${p.sample_rate} Hz (the project rate)</span></div>
      <div class="row"><span>Bit depth</span><div class="seg" data-g="bits"><button data-v="16">16-bit</button><button data-v="24" class="on">24-bit</button><button data-v="32f">32-bit float</button></div></div>
      <div class="row"><span>Range</span><div class="seg" data-g="range"><button data-v="song" class="on">Whole song</button><button data-v="selection" ${ranges.selection ? '' : 'disabled title="No clips selected"'}>Selection</button></div></div>
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
const SECTIONS = ['Audio', 'Project', 'Appearance', 'Keyboard', 'Diagnostics']
const ACCENTS = ['#45c08f', '#5aa2ff', '#2dd4bf', '#a78bfa', '#e6b450', '#f47c96']
const prefs = { accent: '#45c08f', reduceMotion: false }

export const SHORTCUTS: [string, string][] = [
  ['Play / stop', 'Space'], ['Return to start', 'Home'],
  ['Undo', 'Ctrl+Z'], ['Redo', 'Ctrl+Shift+Z'], ['Save', 'Ctrl+S'], ['Export', 'Ctrl+Shift+E'], ['Settings', 'Ctrl+,'],
  ['Copy / cut / paste clips', 'Ctrl+C / X / V'], ['Duplicate clips', 'Ctrl+D'], ['Split at playhead', 'Ctrl+E'],
  ['Delete clips (or note)', 'Del'], ['Select all clips', 'Ctrl+A'], ['Nudge clips by grid / bar', '← → / Shift'],
  ['Move clips: drag · copy: Ctrl+drag', 'mouse'], ['Trim: drag a clip edge · no snap: Alt', 'mouse'],
  ['Import audio (or drop files on the window)', 'Ctrl+I'], ['Add track', 'Ctrl+T'], ['Rename track', 'F2'], ['Mute / solo track', 'M / S'], ['Remove track', 'Ctrl+Del'],
  ['Select track above / below', '↑ / ↓'], ['Zoom in / out', '+ / − or Ctrl+wheel'], ['Mixer / piano roll / clip / console', '1 / 2 / 3 / `'],
  ['Show / hide panel', 'B'], ['Next layout', 'Ctrl+Shift+L'], ['Mark this moment in the log', 'F9'],
]

export function openSettings(section = 'Audio') {
  const d = dialog('Settings', `<div class="settings"><nav>${SECTIONS.map((s) => `<button data-s="${s}">${s}</button>`).join('')}</nav><div class="pane"></div></div>`, { wide: true })
  const pane = d.querySelector<HTMLElement>('.pane')!
  let shown = section
  const show = (s: string) => {
    shown = s
    d.querySelectorAll<HTMLButtonElement>('nav button').forEach((b) => b.classList.toggle('on', b.dataset.s === s))
    refresh(s, pane)
    pane.scrollTop = 0
  }
  d.querySelectorAll<HTMLButtonElement>('nav button').forEach((b) => (b.onclick = () => show(b.dataset.s!)))
  show(section)
  // The device list is asked for in full now, and the Audio page follows it for as long as the
  // dialog is open: plug something in and it appears.
  void refreshDevices(true)
  onDevices(() => {
    if (!d.isConnected) return false
    if (shown === 'Audio' && !pane.matches(':focus-within')) { const y = pane.scrollTop; refresh('Audio', pane); pane.scrollTop = y }
    return true
  })
}

function sectionHtml(s: string): string {
  const p = state.project
  switch (s) {
    case 'Audio': return audioHtml()
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
      <div class="row"><span>1. Mark it</span><span>Press <kbd>F9</kbd> the moment it happens, then type what you noticed. The log gets the time and the play position.</span></div>
      <div class="row"><span>2. Report</span><span><button class="btn solid" data-act="report">Create report file</button></span></div>
      <div class="row"><span></span><div class="sub" data-report>One text file on the Desktop: this machine's audio check and the recent logs, with everything done in this window. Send that file. It holds no audio.</div></div>
      <h3>The log</h3>
      <div class="row"><span>File</span><span data-logpath style="color:var(--text-dim);user-select:text;font-family:var(--mono);font-size:12px;overflow-wrap:anywhere">…</span></div>
      <div class="row"><span></span><div class="sub">One file beside the program, written as things happen and started afresh every time Crucible starts. The run before is kept once, as <code>.previous.log</code>, so a crash can still be read after a restart. It holds every command and its result, clicks and keys in this window (never what you type into a field), playback health and every dropout, errors and crashes. Nothing is sent anywhere by itself.</div></div>
      <div class="row"><span>Error codes</span><span style="color:var(--text-dim)">Every failure shows a code like E2004 with what to try.</span></div>`
  }
  return ''
}

function wire(s: string, pane: HTMLElement) {
  if (s === 'Audio') return wireAudio(pane, () => refresh(s, pane))
  const where = pane.querySelector<HTMLElement>('[data-logpath]')
  if (where) exec('log.path').then((r) => { where.textContent = r.replies[0]?.text ?? 'unknown' }).catch(() => { where.textContent = 'unknown' })
  pane.querySelectorAll<HTMLButtonElement>('[data-x]').forEach((b) => (b.onclick = () => {
    prefs.reduceMotion = !prefs.reduceMotion
    b.classList.toggle('on', prefs.reduceMotion)
    document.documentElement.style.setProperty('--t', prefs.reduceMotion ? '0ms' : '120ms')
  }))
  pane.querySelectorAll<HTMLButtonElement>('[data-layout]').forEach((b) => (b.onclick = () => { applyLayout(b.dataset.layout!); refresh(s, pane) }))
  pane.querySelectorAll<HTMLButtonElement>('[data-accent]').forEach((b) => (b.onclick = () => {
    prefs.accent = b.dataset.accent!
    document.documentElement.style.setProperty('--accent', prefs.accent)
    document.documentElement.style.setProperty('--accent-soft', prefs.accent + '1f')
    document.documentElement.style.setProperty('--accent-hover', prefs.accent)
    refreshPalette(); refresh(s, pane)
  }))
  pane.querySelectorAll<HTMLButtonElement>('[data-act]').forEach((b) => (b.onclick = async () => {
    const note = pane.querySelector<HTMLElement>('[data-report]')!
    b.disabled = true
    note.textContent = 'Gathering the machine check and the logs. This takes about 15 seconds.'
    const reply = (await run('report --no-plugins', { quiet: true })).at(-1)
    b.disabled = false
    note.style.userSelect = 'text'
    note.textContent = reply?.ok ? `Written to ${reply.data?.report ?? 'the folder shown in the Console tab'}. Send that one file.` : 'The report could not be made. The Console tab says why.'
  }))
  pane.querySelectorAll<HTMLInputElement | HTMLSelectElement>('[data-cmd]').forEach((el) => {
    const send = async () => {
      const v = el.value.trim()
      const arg = el.dataset.cmd === 'project.signature' ? v.split(/[\s/]+/).join(' ') : q(v)
      if (el.dataset.cmd === 'project.rate') await setRate(+v)
      else await run(`${el.dataset.cmd} ${arg}`)
      refresh(s, pane)
    }
    el.onchange = send
    el.onkeydown = (e) => { if (e.key === 'Enter') (el as HTMLInputElement).blur(); e.stopPropagation() }
  })
}

/** Change the project's rate. Audio at other rates is converted, if the person agrees. */
async function setRate(hz: number) {
  const others = new Map<string, [string, number, number]>()
  for (const c of state.project.clips) if (c.content.kind === 'audio' && c.content.frames && c.content.rate !== hz) others.set(c.content.source, [c.content.source, c.content.rate, c.content.frames / c.content.rate])
  if (!others.size) return run(`project.rate ${hz}`)
  const answer = await askConvert([...others.values()], hz, 'rate')
  if (answer === 'convert') return run(`project.rate ${hz} --convert`)
}

function refresh(s: string, pane: HTMLElement) { pane.innerHTML = sectionHtml(s); wire(s, pane) }
