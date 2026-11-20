// UI state: the engine's state (the truth, replaced after every command) plus view-only choices
// such as which track is selected. Views redraw when either changes.
import { exec, type Reply, type State } from './engine'

export type Tab = 'mixer' | 'piano' | 'clip' | 'console'

export const ui = {
  selTrack: null as number | null,
  selClip: null as number | null,
  tab: 'mixer' as Tab,
  panelOpen: true,
  pxPerBeat: 28,
  scrollX: 0,
  scrollY: 0,
  log: [] as { kind: 'cmd' | 'ok' | 'err' | 'help'; text: string }[],
}

export let state: State = {
  project: { format: 1, name: '…', sample_rate: 48000, tempo_bpm: 120, time_signature: [4, 4], master_gain_db: 0, tracks: [], next_track_id: 1 },
  folder: null, dirty: false, undo: null, redo: null, journal: false,
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
export async function run(line: string, opts: { quiet?: boolean } = {}): Promise<Reply[]> {
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
      toast('error', r.title ?? 'Command failed', r.message ?? '', r.code)
    }
  }
  changed()
  return replies
}

export function bpb(): number {
  const [n, d] = state.project.time_signature
  return n * 4 / d
}

export function track(id: number | null) {
  return state.project.tracks.find((t) => t.id === id)
}

// ------------------------------------------------------------------ toasts
export function toast(kind: 'error' | 'info', title: string, text = '', code?: string) {
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
    el.setPointerCapture(e.pointerId)
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
