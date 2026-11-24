// DEMO ONLY. Everything in this file stands in for engine features that do not exist yet
// (record, loop, loudness, devices, plugins) so the layout can be judged by eye. The transport and
// the meters are real since 2026-11-23 (transport.ts); only their values are still kept here. Clips and notes are NOT here any more: they are real engine data (clips.rs).
// Delete this file piece by piece as M2+ lands; anything shown from here carries a "demo" tag.
import { PPQ, type Track } from './engine'

export const PEAKS_PER_BEAT = 48

export const demo = {
  playing: false,
  recording: false,
  loop: false,
  metronome: false,
  loopStart: 16,
  loopEnd: 32,
  pos: 0, // beats
  armed: new Set<number>(),
  levels: new Map<number, { l: number; r: number; pl: number; pr: number }>(), // 0 = master; linear 0..1
  lufs: -Infinity,
  peakDb: -Infinity,
  clipped: false,
  mono: false,
  ab: false,
  xruns: 0,
  inserts: new Map<number, string[]>(),
  instrument: new Map<number, string>(),
  seen: new Set<number>(),
}

export const plugins = [
  { name: 'Omnisphere', vendor: 'Spectrasonics', kind: 'Instrument', note: 'VST3 · reference target' },
  { name: 'Keyscape', vendor: 'Spectrasonics', kind: 'Instrument', note: 'VST3 · reference target' },
  { name: 'Pro-Q 4', vendor: 'FabFilter', kind: 'Effect', note: 'VST3 · example' },
  { name: 'Pro-C 3', vendor: 'FabFilter', kind: 'Effect', note: 'VST3 · example' },
  { name: 'ValhallaRoom', vendor: 'Valhalla DSP', kind: 'Effect', note: 'VST3 · example' },
]

export const devices = {
  drivers: ['WASAPI exclusive', 'WASAPI shared', 'ASIO (not in this build)'],
  outputs: ['Speakers (Realtek(R) Audio)', 'Lynx AES16e', 'Behringer UMC404HD', 'Jabra Evolve2 65'],
  inputs: ['Microphone Array (Realtek)', 'Lynx AES16e', 'Behringer UMC404HD'],
  midi: [{ name: 'Arturia KeyLab 61', on: true }, { name: 'nanoKONTROL2', on: false }],
  buffers: [32, 64, 128, 256, 512, 1024],
}

/** Plugin names shown on tracks, chosen once per track by its name. */
export function seedPlugins(tracks: Track[]) {
  for (const t of tracks) {
    if (demo.seen.has(t.id)) continue
    demo.seen.add(t.id)
    const n = t.name.toLowerCase()
    if (t.kind === 'instrument' && (n.includes('pad') || n.includes('keys') || n.includes('bass'))) demo.instrument.set(t.id, n.includes('pad') ? 'Omnisphere' : 'Keyscape')
    if (n.includes('vox')) demo.inserts.set(t.id, ['Pro-Q 4', 'Pro-C 3'])
    if (n.includes('drum')) demo.inserts.set(t.id, ['Pro-C 3'])
  }
}

// ------------------------------------------------------------------ fake waveforms
// There is no audio import yet, so each audio source gets a made-up but stable waveform.
const peakCache = new Map<string, Float32Array>()

export function peaksFor(source: string, sourceTicks: number): Float32Array {
  const key = `${source}:${sourceTicks}`
  const hit = peakCache.get(key)
  if (hit) return hit
  let seed = 7
  for (const ch of source) seed = (seed * 31 + ch.charCodeAt(0)) % 2147483647
  const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647)
  const n = Math.max(1, Math.round((sourceTicks / PPQ) * PEAKS_PER_BEAT))
  const out = new Float32Array(n)
  const s = source.toLowerCase()
  const style = s.includes('drum') ? 0 : s.includes('vox') || s.includes('voc') ? 1 : 2
  for (let i = 0; i < n; i++) {
    const b = i / PEAKS_PER_BEAT, inBeat = b % 1
    let v: number
    if (style === 0) v = Math.exp(-inBeat * 9) * (0.55 + 0.4 * rnd()) + 0.05 * rnd() + (Math.abs((b % 2) - 1) < 0.02 ? 0.3 : 0)
    else if (style === 1) v = Math.sin(b * 1.7) > -0.3 ? 0.3 + 0.45 * rnd() * (0.6 + 0.4 * Math.sin(b * 5)) : 0.03 * rnd()
    else v = 0.35 + 0.2 * Math.sin(b * 0.6) + 0.1 * rnd()
    out[i] = Math.min(1, Math.max(0, v))
  }
  peakCache.set(key, out)
  return out
}

/** True while something on screen still moves (meters falling back). Lets the draw loop stop. */
export function settling(): boolean {
  if (demo.playing) return true
  for (const v of demo.levels.values()) if (v.l > 0.002 || v.r > 0.002 || v.pl > 0.002 || v.pr > 0.002) return true
  return false
}
