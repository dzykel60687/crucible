// The only way the UI changes anything: send command text, get replies and the new state back.
// In the Tauri window this calls the Rust `exec` command. In a plain browser during development
// it posts to the Vite bridge, which drives `crucible --json shell`. Same engine either way.
import { invoke } from '@tauri-apps/api/core'

/** Ticks per beat, as in crucible-project. Positions in the state are ticks; commands take beats. */
export const PPQ = 960

export type TrackKind = 'audio' | 'instrument'
export interface Track { id: number; name: string; kind: TrackKind; gain_db: number; pan: number; mute: boolean; solo: boolean }
export interface Note { pitch: number; start: number; length: number; velocity: number }
export type Content = { kind: 'audio'; source: string; source_length: number } | { kind: 'midi'; notes: Note[] }
export interface Clip { id: number; track: number; name: string; start: number; length: number; offset: number; gain_db: number; content: Content }
export interface Project {
  format: number; name: string; sample_rate: number; tempo_bpm: number; time_signature: [number, number]
  master_gain_db: number; tracks: Track[]; next_track_id: number; clips: Clip[]; next_clip_id: number
}
export interface State { project: Project; folder: string | null; dirty: boolean; undo: string | null; redo: string | null; journal: boolean }
export interface Reply { ok: boolean; command: string; text?: string; data?: any; code?: string; message?: string; title?: string; help?: string }
export interface Result { replies: Reply[]; state: State }

const inTauri = '__TAURI_INTERNALS__' in window

export const transportName = inTauri ? 'window' : 'dev bridge'

export async function exec(line: string): Promise<Result> {
  if (inTauri) return invoke<Result>('exec', { line })
  const res = await fetch('/__crucible', { method: 'POST', body: line })
  if (!res.ok) throw new Error(await res.text())
  return res.json()
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
