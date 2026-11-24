// The only way the UI changes anything: send command text, get replies and the new state back.
// In the Tauri window this calls the Rust `exec` command. In a plain browser during development
// it posts to the Vite bridge, which drives `crucible --json shell`. Same engine either way.
import { invoke } from '@tauri-apps/api/core'

/** Ticks per beat, as in crucible-project. Positions in the state are ticks; commands take beats. */
export const PPQ = 960

export type TrackKind = 'audio' | 'instrument'
/** An insert effect on a track. Settings not in `params` are at the effect's default (`fx.kinds`). */
export interface Insert { id: number; kind: string; bypass: boolean; params: Record<string, number>; file?: string }
export interface Track { id: number; name: string; kind: TrackKind; gain_db: number; pan: number; mute: boolean; solo: boolean; effects: Insert[] }
export interface Note { pitch: number; start: number; length: number; velocity: number }
/** Audio: `frames` and `rate` are 0 for a placeholder with no file (the demo song). */
export type Content = { kind: 'audio'; source: string; source_length: number; frames: number; rate: number } | { kind: 'midi'; notes: Note[] }
export interface Clip { id: number; track: number; name: string; start: number; length: number; offset: number; gain_db: number; content: Content }
export interface Project {
  format: number; name: string; sample_rate: number; tempo_bpm: number; time_signature: [number, number]
  master_gain_db: number; tracks: Track[]; next_track_id: number; clips: Clip[]; next_clip_id: number
}
export interface State { project: Project; folder: string | null; dirty: boolean; undo: string | null; redo: string | null; journal: boolean; revision: number }
export interface Reply { ok: boolean; command: string; text?: string; data?: any; code?: string; message?: string; title?: string; help?: string }
export interface Result { replies: Reply[]; state: State }
/** What the engine sends after a command (Engine::sync): the whole state, or only changed clips. */
interface Sync extends State { full: boolean; order: number[] }

export const inTauri = '__TAURI_INTERNALS__' in window

export const transportName = inTauri ? 'window' : 'dev bridge'

/** The state this page holds. Kept here so every reply can be merged into it. */
let held: State | null = null

/** Commands go one at a time, in the order the UI issued them. Two quick clicks must not race:
 *  the window runs commands on a thread pool and the browser may send fetches in parallel. */
let queue: Promise<unknown> = Promise.resolve()

/** Run command text. The engine replies with what changed since the revision this page holds. */
export function exec(line: string): Promise<Result> {
  const next = queue.then(() => execNow(line))
  queue = next.catch(() => {})
  return next
}

async function execNow(line: string): Promise<Result> {
  const since = held?.revision
  let r: { replies: Reply[]; state: Sync }
  if (inTauri) r = await invoke('exec', { line, since: since ?? null })
  else {
    const res = await fetch('/__crucible', { method: 'POST', body: line, headers: since === undefined ? {} : { 'x-crucible-since': String(since) } })
    if (!res.ok) throw new Error(await res.text())
    r = await res.json()
  }
  if (!r?.state?.project) throw new Error(`the engine sent no state for: ${line}`)
  const merged = merge(held, r.state)
  if (merged) {
    held = merged
    return { replies: r.replies, state: merged }
  }
  // Out of step (should not happen): fetch the whole state with a read-only command, and keep
  // this command's replies, because it has already run.
  held = null
  const again = await execNow('project.info')
  return { replies: r.replies, state: again.state }
}

/** Apply a change list to the held state. Null if the list does not fit what is held. */
function merge(prev: State | null, s: Sync): State | null {
  const { full, order, ...state } = s
  if (full) return state
  if (!prev) return null
  const byId = new Map(prev.project.clips.map((c) => [c.id, c]))
  for (const c of s.project.clips) byId.set(c.id, c)
  const clips: Clip[] = []
  for (const id of order) {
    const c = byId.get(id)
    if (!c) return null
    clips.push(c)
  }
  return { ...state, project: { ...state.project, clips } }
}

/** Quote a value for the command line, so names with spaces or quotes survive. */
export function q(v: string | number): string {
  const s = String(v)
  return /^[^\s;"'#]+$/.test(s) ? s : `"${s.replace(/"/g, '""')}"`
}

/** Ticks to a beats argument. Six decimals round-trip every tick exactly. */
export const beats = (ticks: number) => +(ticks / PPQ).toFixed(6)

/** Clip ids as a command argument: #3,#7. */
export const ids = (list: Iterable<number>) => [...list].map((i) => `#${i}`).join(',')
