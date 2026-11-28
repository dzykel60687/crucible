// Instruments (D-044): choosing a plugin for an instrument track, its menu, which keyboard plays
// it, and what the engine says about each loaded plugin. Everything here is a command
// (`plugin.*`, `instrument.*`, `track.live`, `track.input`); the window decides nothing itself.
import { closeLayer, dialog, menu, pickFolder, type Item } from './dialogs'
import { exec, q, type Track } from './engine'
import { icon } from './icons'
import { changed, esc, run, state, toast, track } from './store'
import { showLevel, tp } from './transport'

/** One instrument track as `instrument.status` reports it. */
export interface InstStatus {
  track: number; state: 'loaded' | 'missing' | 'failed' | 'none'; why: string | null; live: boolean
  editor?: boolean; peak?: [number | null, number | null] | null
  plugin?: { name: string; vendor: string; has_editor: boolean }
  busy_blocks?: number; failed_blocks?: number
}
interface Keyboard { name: string; messages: number }
interface Plugin { name: string; vendor: string; version: string; category: string; class_id: string; instrument: boolean; path: string }
interface Listing { plugins: Plugin[]; others: number; failed: { path: string; why: string }[]; folders: { folder: string; exists: boolean }[]; scanned: number | null }

/** What the engine last said. View state only. */
export const inst = {
  byTrack: new Map<number, InstStatus>(),
  keyboards: [] as Keyboard[],
  refused: [] as { name: string; why: string }[],
  /** Why the instruments cannot be heard (the audio device would not open), or null. */
  silent: null as string | null,
  /** A keyboard sent something since the look before last: for the light on the keyboard button. */
  hit: false,
}

const lin = (db: number | null | undefined) => (db == null ? 0 : Math.pow(10, db / 20))
let messages = 0, told = ''

/** Ask the engine about the instruments, ten times a second while a track has one. Their levels
 *  move the meters while the song is stopped; a change anyone can see redraws the track list. */
export function watchInstruments() {
  let busy = false
  setInterval(async () => {
    if (busy || document.hidden) return
    if (!state.project.tracks.some((t) => t.instrument)) {
      if (inst.byTrack.size) { inst.byTrack.clear(); inst.keyboards = []; inst.silent = null; changed() }
      return
    }
    busy = true
    try {
      const d = (await exec('instrument.status')).replies[0]?.data
      if (!d) return
      const seen = JSON.stringify([d.instruments.map((i: InstStatus) => [i.track, i.state, i.live, i.editor, i.why]), d.keyboards.map((k: Keyboard) => k.name), d.refused, d.silent])
      inst.byTrack = new Map(d.instruments.map((i: InstStatus) => [i.track, i]))
      inst.keyboards = d.keyboards
      inst.refused = d.refused
      inst.silent = d.silent
      const total = (d.keyboards as Keyboard[]).reduce((n, k) => n + k.messages, 0)
      const hit = total !== messages
      messages = total
      if (!tp.playing) {
        for (const i of d.instruments as InstStatus[]) if (i.peak) showLevel(i.track, lin(i.peak[0]), lin(i.peak[1]))
        if (d.peak) showLevel(0, lin(d.peak[0]), lin(d.peak[1]))
      }
      // The light is set on the buttons themselves: a redraw ten times a second would swallow clicks.
      if (hit !== inst.hit) {
        inst.hit = hit
        document.querySelectorAll('.tbtn.k.on').forEach((b) => b.classList.toggle('hit', hit))
      }
      if (seen !== told) {
        if (d.silent && !told.includes(d.silent)) toast('error', 'The instruments cannot be heard', d.silent)
        told = seen
        changed()
      }
    } catch {
      // The bridge is restarting; the next look tries again.
    } finally {
      busy = false
    }
  }, 100)
}

/** The instrument button on a track: its words, its look and its tooltip. */
export function instButton(t: Track): { label: string; cls: string; title: string } {
  if (!t.instrument) return { label: 'Choose instrument…', cls: 'none', title: 'Choose the plugin that makes this track\'s sound' }
  const s = inst.byTrack.get(t.id)
  if (s?.state === 'missing') return { label: `${t.instrument.name} (not installed)`, cls: 'bad', title: s.why ?? '' }
  if (s?.state === 'failed') return { label: `${t.instrument.name} (did not load)`, cls: 'bad', title: s.why ?? '' }
  return { label: t.instrument.name, cls: s?.editor ? 'open' : '', title: `${t.instrument.name} by ${t.instrument.vendor}. Click for its window and settings.` }
}

/** The keyboard button on a track: whether the MIDI keyboards play it, and what is connected. */
export function keyButton(t: Track): { on: boolean; title: string } {
  const on = inst.byTrack.get(t.id)?.live ?? false
  const from = t.input ? `the keyboard named "${t.input}"` : 'every keyboard'
  const ch = t.input_channel ? `, channel ${t.input_channel}` : ''
  const there = inst.keyboards.length ? inst.keyboards.map((k) => k.name).join(', ') : 'none'
  const no = inst.refused.length ? ` Could not open: ${inst.refused.map((r) => `${r.name} (${r.why})`).join('; ')}.` : ''
  return { on, title: `${on ? `Played by ${from}${ch}` : 'Not played by the keyboards'}. Click: play this track only. Ctrl+click: add or remove this track. Connected: ${there}.${no}` }
}

export function toggleLive(t: Track, add: boolean) {
  const s = inst.byTrack.get(t.id)
  const others = [...inst.byTrack.values()].some((i) => i.live && i.track !== t.id)
  if (add) return run(`track.live #${t.id} ${s?.live ? 'off' : 'on'}`)
  // The only one already: a click turns it off. Otherwise the keyboards move here.
  return run(s?.live && !others ? `track.live #${t.id} off` : `track.live #${t.id} on --only`)
}

/** The instrument button's menu. Straight to the picker when the track has none. */
export function instMenu(t: Track, at: { x: number; y: number }) {
  if (!t.instrument) return openInstrumentPicker(t)
  const s = inst.byTrack.get(t.id)
  const loaded = s?.state === 'loaded'
  const items: Item[] = []
  if (!loaded && s) items.push({ label: s.state === 'missing' ? 'Not installed on this PC' : 'It did not load', small: 'see the Console', action: () => toast('error', t.instrument!.name, s.why ?? ''), danger: true })
  items.push(
    { label: s?.editor ? 'Close its window' : 'Open its window', action: () => run(`instrument.${s?.editor ? 'hide' : 'show'} #${t.id}`), disabled: !loaded || !s?.plugin?.has_editor },
    { label: 'Play a test note', small: 'middle C', action: () => testNote(t.id), disabled: !loaded },
    { label: 'Keyboard and channel…', small: t.input || t.input_channel ? `${t.input ?? 'all'}${t.input_channel ? `, ch ${t.input_channel}` : ''}` : 'all', action: () => openInput(t) },
    'sep',
    { label: 'Change instrument…', action: () => openInstrumentPicker(t) },
    { label: 'Remove instrument', danger: true, action: () => run(`instrument.clear #${t.id}`) },
  )
  menu(at, items)
}

/** Middle C for half a second, as if a key were pressed: hears the plugin without a keyboard. */
async function testNote(id: number) {
  const [r] = await run(`instrument.note #${id} 60`, { quiet: true })
  if (r?.ok) setTimeout(() => void run(`instrument.note #${id} 60 --off`, { quiet: true }), 500)
}

function openInput(t: Track) {
  const names = inst.keyboards.map((k) => k.name)
  const saved = t.input && !names.some((n) => n.toLowerCase().includes(t.input!.toLowerCase())) ? t.input : null
  const picked = t.input ? names.find((n) => n.toLowerCase().includes(t.input!.toLowerCase())) : null
  const d = dialog(`Keyboard for ${t.name}`, `<div class="content">
      <div class="row"><label>Keyboard</label><select class="input" id="kb-name">
        <option value="all">All keyboards</option>
        ${names.map((n) => `<option value="${esc(n)}" ${n === picked ? 'selected' : ''}>${esc(n)}</option>`).join('')}
        ${saved ? `<option value="${esc(saved)}" selected>${esc(saved)} (not connected)</option>` : ''}
      </select></div>
      <div class="row"><label>Channel</label><select class="input" id="kb-ch">
        <option value="0">All channels</option>
        ${Array.from({ length: 16 }, (_, i) => `<option value="${i + 1}" ${t.input_channel === i + 1 ? 'selected' : ''}>${i + 1}</option>`).join('')}
      </select></div>
      <div class="sub">${names.length ? `Connected now: ${esc(names.join(', '))}.` : 'No keyboard is connected. Plug one in: it is found within two seconds while an instrument is loaded.'}${inst.refused.map((r) => ` Could not open ${esc(r.name)}: ${esc(r.why)}`).join('')}</div>
    </div>`, { footer: '<button class="btn" data-close>Cancel</button><button class="btn solid" id="kb-ok">Apply</button>' })
  d.querySelector<HTMLButtonElement>('#kb-ok')!.onclick = () => {
    const name = d.querySelector<HTMLSelectElement>('#kb-name')!.value, ch = d.querySelector<HTMLSelectElement>('#kb-ch')!.value
    closeLayer()
    run(`track.input #${t.id} ${q(name)} --channel ${ch}`)
  }
}

/** Choose a plugin for a track from the ones found on this machine. Scans first if this machine
 *  never has; "Scan again" finds what was installed since; "Add a folder" looks somewhere else too. */
export async function openInstrumentPicker(t: Track) {
  let all = false, filter = ''
  const d = dialog(`Instrument for ${t.name}`, `<div class="content inst-pick">
      <input class="input" id="pl-find" placeholder="Search by name or maker" spellcheck="false">
      <div class="plugin-list" id="pl-list"><div class="sub">Looking…</div></div>
      <div class="sub" id="pl-note"></div>
    </div>`, { footer: '<button class="btn" id="pl-folder">Add a folder…</button><button class="btn" id="pl-scan">Scan again</button><span style="flex:1"></span><button class="btn" data-close>Cancel</button>' })
  const list = d.querySelector<HTMLElement>('#pl-list')!, note = d.querySelector<HTMLElement>('#pl-note')!
  const find = d.querySelector<HTMLInputElement>('#pl-find')!
  let listing: Listing | null = null
  const still = () => d.isConnected

  const draw = () => {
    if (!listing) return
    const f = filter.toLowerCase()
    const rows = listing.plugins.filter((p) => !f || p.name.toLowerCase().includes(f) || p.vendor.toLowerCase().includes(f))
    list.innerHTML = rows.length
      ? rows.map((p) => `<button data-id="${esc(p.class_id)}" title="${esc(p.path)}"><span class="pi">${p.instrument ? icon.keys : icon.plug}</span><span><b>${esc(p.name)}</b><small>${esc(p.vendor)}${p.version ? ` · ${esc(p.version)}` : ''}${p.instrument ? '' : ` · ${esc(p.category || 'not an instrument')}`}</small></span></button>`).join('')
      : `<div class="sub">${listing.plugins.length ? 'Nothing matches.' : 'No instrument plugins were found on this PC.'}</div>`
    const looked = listing.folders.map((x) => `${x.folder}${x.exists ? '' : ' (not there)'}`).join('; ')
    note.innerHTML = `Looked in: ${esc(looked || 'nowhere yet')}.`
      + (listing.failed.length ? ` <span title="${esc(listing.failed.map((x) => `${x.path}: ${x.why}`).join('\n'))}">${listing.failed.length} could not be read (hover for which).</span>` : '')
      + (!all && listing.others ? ` <a href="#" id="pl-all">Show the ${listing.others} that are not instruments too</a>` : '')
    list.querySelectorAll<HTMLButtonElement>('[data-id]').forEach((b) => (b.onclick = () => choose(listing!.plugins.find((p) => p.class_id === b.dataset.id)!)))
    const more = note.querySelector<HTMLElement>('#pl-all')
    if (more) more.onclick = (e) => { e.preventDefault(); all = true; void load() }
  }
  const working = (text: string) => { list.innerHTML = `<div class="sub">${esc(text)}</div>` }
  const load = async (line = `plugin.list${all ? ' --all' : ''}`) => {
    const [r] = await run(line, { quiet: true })
    if (!still()) return
    if (!r?.ok) return working(r?.message ?? 'The plugins could not be listed.')
    // A scan answers with instruments only; ask again when everything is wanted.
    if (all && !line.startsWith('plugin.list')) return load()
    listing = r.data
    draw()
  }
  const choose = async (p: Plugin) => {
    working(`Loading ${p.name}… A big sampled instrument can take a while.`)
    d.querySelectorAll<HTMLButtonElement>('footer .btn').forEach((b) => (b.disabled = true))
    const [r] = await run(`instrument.set #${t.id} ${p.class_id}`)
    if (!r?.ok) { if (still()) { d.querySelectorAll<HTMLButtonElement>('footer .btn').forEach((b) => (b.disabled = false)); draw() } return }
    if (still()) closeLayer()
    // Its own window, as other programs do after adding an instrument.
    if (track(t.id) && r.data?.plugin?.has_editor) await run(`instrument.show #${t.id}`, { quiet: true })
  }
  find.oninput = () => { filter = find.value; draw() }
  find.onkeydown = (e) => {
    if (e.key === 'Escape') closeLayer()
    if (e.key === 'Enter') list.querySelector<HTMLButtonElement>('[data-id]')?.click()
  }
  d.querySelector<HTMLButtonElement>('#pl-scan')!.onclick = () => { working('Looking for plugins… Each new one is asked what it is; a big one takes a few seconds.'); void load('plugin.scan') }
  d.querySelector<HTMLButtonElement>('#pl-folder')!.onclick = async () => {
    const folder = await pickFolder('A folder that holds VST3 plugins')
    if (!folder || !still()) return
    working(`Looking in ${folder}…`)
    void load(`plugin.folder ${q(folder)}`)
  }
  find.focus()
  const [first] = await run('plugin.list', { quiet: true })
  if (!still()) return
  if (first?.ok && first.data.scanned === null) {
    working('Looking for plugins for the first time… Each one is asked what it is; a big one takes a few seconds.')
    await load('plugin.scan')
  } else if (first?.ok) {
    listing = first.data
    draw()
    // Plugins installed since the last look are found by themselves: only new and changed files
    // are asked what they are, so this is quick unless something new is there.
    const ids = (l: Listing | null) => (l?.plugins ?? []).map((p) => p.class_id).join()
    const before = ids(listing)
    note.insertAdjacentHTML('beforeend', ' <span id="pl-check">Checking for new plugins…</span>')
    const [r] = await run('plugin.scan', { quiet: true })
    if (!still()) return
    note.querySelector('#pl-check')?.remove()
    if (r?.ok && !all && ids(r.data) !== before) { listing = r.data; draw(); toast('info', 'New plugins found', 'The list now holds what was installed since the last look.') }
  } else working(first?.message ?? 'The plugins could not be listed.')
}
