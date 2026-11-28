// UI state: the engine's state (the truth, replaced after every command) plus view-only choices
// such as which track is selected. Views redraw when either changes.
import { exec, PPQ, type Reply, type State } from './engine'
import { trail } from './trail'

export type Tab = 'mixer' | 'piano' | 'clip' | 'console'

export const ui = {
  selTrack: null as number | null,
  /** Selected clips; selClip is the one the editors show (the last clicked). */
  selClips: new Set<number>(),
  selClip: null as number | null,
  /** Grid for clip edits, in beats. 0 = off. Hold Alt while dragging to ignore it. */
  snap: 1,
  /** Where the keyboard's Delete and arrows act. */
  focus: 'timeline' as 'timeline' | 'piano',
  tab: 'mixer' as Tab,
  panelOpen: true,
  pxPerBeat: 28,
  scrollX: 0,
  scrollY: 0,
  log: [] as { kind: 'cmd' | 'ok' | 'err' | 'help'; text: string }[],
}

export let state: State = {
  project: { format: 1, name: '…', sample_rate: 48000, tempo_bpm: 120, time_signature: [4, 4], master_gain_db: 0, tracks: [], next_track_id: 1, clips: [], next_clip_id: 1 },
  folder: null, dirty: false, undo: null, redo: null, journal: false, revision: 0,
}

const listeners: (() => void)[] = []
export const onChange = (f: () => void) => listeners.push(f)
let queued = false
/** Coalesce redraw requests into one per frame. */
export function changed() {
  if (queued) return
  queued = true
  requestAnimationFrame(() => { queued = false; for (const f of listeners) f() })
}

// While a mouse button is held, the page must not be rebuilt: the element under the pointer
// would be replaced, and the browser then drops the click (a button "does nothing") or the drag
// loses its target. Listeners ask `pointerHeld()` and call `owe()` instead of rebuilding; the
// rebuild happens as soon as the button is released. Found 2026-11-25: Stop did not respond
// while the page was being redrawn many times a second.
let held = false, owed = false
export const pointerHeld = () => held
export const owe = () => { owed = true }
const released = () => {
  held = false
  if (owed) { owed = false; setTimeout(changed, 0) }
}
addEventListener('pointerdown', () => { held = true }, true)
addEventListener('pointerup', released, true)
addEventListener('pointercancel', released, true)
addEventListener('blur', released)

/** Fader travel: position 0..1 for a level in dB, and back. One curve for every volume control,
 *  so the mixer fader and the slider in the track header agree. 0 dB sits at 0.89. */
const FADER_EXP = 1.6
export const dbToPos = (db: number) => (db <= -100 ? 0 : Math.pow(Math.min(1, Math.max(0, (db + 60) / 72)), 1 / FADER_EXP))
export const posToDb = (p: number) => (p <= 0.01 ? -144 : Math.round((72 * Math.pow(p, FADER_EXP) - 60) * 10) / 10)

/** Render now, without waiting for a frame. Dev and test use only: a hidden page gets no frames. */
export function flush() {
  queued = false
  for (const f of listeners) f()
}

/** Cheap path for scroll, zoom and drags: move and repaint the canvas only, no DOM rebuild. */
let redrawQueued = false
export const view = { redraw: () => {} }
export function redraw() {
  if (redrawQueued) return
  redrawQueued = true
  requestAnimationFrame(() => { redrawQueued = false; view.redraw() })
}

function logLine(kind: 'cmd' | 'ok' | 'err' | 'help', text: string) {
  ui.log.push({ kind, text })
  if (ui.log.length > 500) ui.log.splice(0, ui.log.length - 500)
}

/** Run command text through the engine. Every GUI action ends up here, and appears in the console. */
/** `handles`: error codes the caller deals with itself (with a question, say): no error message. */
export async function run(line: string, opts: { quiet?: boolean; handles?: string[] } = {}): Promise<Reply[]> {
  logLine('cmd', line)
  let replies: Reply[]
  try {
    const r = await exec(line)
    state = r.state
    replies = r.replies
  } catch (e) {
    toast('error', 'Engine unreachable', String(e), 'E9099')
    logLine('err', String(e))
    changed()
    return []
  }
  for (const r of replies) {
    if (r.ok) { if (r.text && !opts.quiet) logLine('ok', r.text) }
    else {
      logLine('err', `${r.code}: ${r.message}`)
      if (r.help) logLine('help', r.help)
      if (!opts.handles?.includes(r.code ?? '')) toast('error', r.title ?? 'Command failed', r.message ?? '', r.code)
    }
  }
  changed()
  return replies
}

export function bpb(): number {
  const [n, d] = state.project.time_signature
  return n * 4 / d
}

/** Where the last clip ends, in beats. */
export function lastSound(): number {
  let end = 0
  for (const c of state.project.clips) end = Math.max(end, c.start + c.length)
  return end / PPQ
}
/** Where the song ends, in beats (D-043): the last clip's end, or further out if the end was
 *  moved there. Playback stops here, plus whatever the effects still ring out. */
export const songEnd = () => Math.max(lastSound(), (state.project.end ?? 0) / PPQ)

export function track(id: number | null) {
  return state.project.tracks.find((t) => t.id === id)
}

export function clip(id: number | null) {
  return state.project.clips.find((c) => c.id === id)
}

/** Replace the clip selection. The last id becomes the one the editors show. */
export function selectClips(ids: Iterable<number>) {
  ui.selClips = new Set(ids)
  const last = [...ui.selClips].at(-1)
  ui.selClip = last ?? null
  // The selected track is the topmost one in the selection, because paste puts the clipboard's
  // top row there: box-select Pad and Vox, copy, paste, and they land on Pad and Vox again.
  const order = new Map(state.project.tracks.map((t, i) => [t.id, i]))
  const top = [...ui.selClips].map((i) => clip(i)?.track).filter((t) => t !== undefined).sort((a, b) => order.get(a)! - order.get(b)!)[0]
  if (top !== undefined) ui.selTrack = top
}

/** Mute and solo buttons toggle. The commands set a state (`--state on|off`, default on), so
 *  send the opposite of what the engine holds now. Sending the bare command only ever turned
 *  them on: a muted track could not be unmuted from the window (found 2026-11-22). */
export function toggleMute(id: number) {
  const t = track(id)
  if (t) run(`track.mute #${id} --state ${t.mute ? 'off' : 'on'}`)
}
export function toggleSolo(id: number) {
  const t = track(id)
  if (t) run(`track.solo #${id} --state ${t.solo ? 'off' : 'on'}`)
}

/** Ids a command reported creating (copy, paste, split), so they can be selected. */
export function created(replies: { data?: any }[]): number[] {
  return replies.flatMap((r) => (Array.isArray(r.data?.created) ? r.data.created : []))
}

// ------------------------------------------------------------------ toasts
export function toast(kind: 'error' | 'info', title: string, text = '', code?: string) {
  trail('saw', `${kind === 'error' ? 'error message' : 'message'}: ${code ? code + ' ' : ''}${title}${text ? ' | ' + text : ''}`)
  const host = document.getElementById('toasts')!
  const el = document.createElement('div')
  el.className = `toast ${kind}`
  el.innerHTML = `<b>${code ? `<span class="code">${esc(code)}</span>` : ''}${esc(title)}</b>${text ? `<p>${esc(text)}</p>` : ''}`
  host.appendChild(el)
  while (host.children.length > 4) host.firstElementChild!.remove()
  setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 150) }, kind === 'error' ? 6000 : 3000)
}

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!)
}

export const fmtDb = (db: number) => (db <= -144 ? '-inf' : `${db > 0 ? '+' : ''}${db.toFixed(1)}`)
export const fmtPan = (p: number) => (Math.abs(p) < 0.005 ? 'C' : `${p < 0 ? 'L' : 'R'}${Math.round(Math.abs(p) * 100)}`)

/** Vertical drag on an element changes a number; the command runs once, on release. */
/** A click without movement calls `click` instead (e.g. to type a value). */
export function dragNumber(el: HTMLElement, get: () => number, opts: { min: number; max: number; perPx: number; step: number; preview: (v: number) => void; commit: (v: number) => void; reset?: number; click?: () => void }) {
  el.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return
    e.preventDefault()
    e.stopPropagation()
    try { el.setPointerCapture(e.pointerId) } catch { /* synthetic event: the drag still works over the element */ }
    const start = get(), y0 = e.clientY
    let v = start, moved = false
    const move = (m: PointerEvent) => {
      if (Math.abs(y0 - m.clientY) > 2) moved = true
      const fine = m.shiftKey ? 0.1 : 1
      v = Math.round(Math.min(opts.max, Math.max(opts.min, start + (y0 - m.clientY) * opts.perPx * fine)) / opts.step) * opts.step
      opts.preview(v)
    }
    const up = () => {
      el.removeEventListener('pointermove', move)
      el.removeEventListener('pointerup', up)
      el.removeEventListener('pointercancel', up)
      if (!moved) opts.click?.()
      else if (Math.abs(v - start) > 1e-9) opts.commit(v)
    }
    el.addEventListener('pointermove', move)
    el.addEventListener('pointerup', up)
    el.addEventListener('pointercancel', up)
  })
  if (opts.reset !== undefined) el.addEventListener('dblclick', (e) => { e.stopPropagation(); opts.commit(opts.reset!) })
}
