// Waveform levels for audio clips. Imported files use real peaks from the engine (`audio.peaks`,
// fetched once per file and kept for the session); placeholder clips with no file keep the demo
// stand-in, so the demo song still draws.
import { PEAKS_PER_BEAT, peaksFor } from './demo'
import { exec, q, type Clip } from './engine'
import { changed, state } from './store'

/** `data[k] * scale` is the level (0..1) of the k-th slice; `perBeat` slices per beat of content. */
export interface Peaks { data: ArrayLike<number>; perBeat: number; scale: number }

const FRAMES_PER_PEAK = 256 // as in crucible-engine/src/audio.rs
const cache = new Map<string, Uint8Array | 'loading' | 'failed'>()
let cacheFolder: string | null = null

function decode(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

/** Levels for a clip's content, or null while a file's peaks are still loading. */
export function clipPeaks(c: Clip): Peaks | null {
  if (c.content.kind !== 'audio') return null
  const content = c.content
  if (!content.frames) return { data: peaksFor(content.source, content.source_length), perBeat: PEAKS_PER_BEAT, scale: 1 }
  if (state.folder !== cacheFolder) { cache.clear(); cacheFolder = state.folder }
  const hit = cache.get(content.source)
  const p = state.project
  const framesPerBeat = (p.sample_rate * 60) / p.tempo_bpm
  if (hit instanceof Uint8Array) return { data: hit, perBeat: framesPerBeat / FRAMES_PER_PEAK, scale: 1 / 255 }
  if (!hit) {
    cache.set(content.source, 'loading')
    exec(`audio.peaks ${q(content.source)}`)
      .then((r) => {
        const reply = r.replies[0]
        cache.set(content.source, reply?.ok ? decode(reply.data.peaks) : 'failed')
        changed()
      })
      .catch(() => cache.set(content.source, 'failed'))
  }
  return null
}

/** Loudest level in content beats [b0, b1). */
export function levelAt(pk: Peaks, b0: number, b1: number): number {
  const k0 = Math.floor(b0 * pk.perBeat), k1 = Math.max(k0 + 1, Math.floor(b1 * pk.perBeat))
  let v = 0
  for (let j = Math.max(0, k0); j < k1 && j < pk.data.length; j++) v = Math.max(v, pk.data[j])
  return v * pk.scale
}
