// DEMO ONLY. Everything in this file stands in for engine features that do not exist yet
// (transport, audio waveforms, meters, loudness, devices, plugins) so the layout can be judged by
// eye. Clips and notes are NOT here any more: they are real engine data (clips.rs).
// Delete this file piece by piece as M2+ lands; anything shown from here carries a "demo" tag.
import { PPQ, type Clip, type Track } from './engine'

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

const dbToLin = (db: number) => Math.pow(10, db / 20)

/** Advance the fake transport and fake meters by dt seconds, from the real clips. */
export function tick(dt: number, tempo: number, tracks: Track[], clips: Clip[], masterDb: number) {
  if (demo.playing) {
    demo.pos += dt * tempo / 60
    if (demo.loop && demo.pos >= demo.loopEnd) demo.pos = demo.loopStart + (demo.pos - demo.loopEnd)
  }
  const t0 = demo.pos * PPQ
  const anySolo = tracks.some((t) => t.solo)
  let sumL = 0, sumR = 0
  const fall = Math.exp(-dt * 9)
  for (const t of tracks) {
    let target = 0
    if (demo.playing && !t.mute && (!anySolo || t.solo)) {
      const c = clips.find((c) => c.track === t.id && t0 >= c.start && t0 < c.start + c.length)
      if (c) {
        const inContent = t0 - c.start + c.offset
        if (c.content.kind === 'audio') {
          const p = peaksFor(c.content.source, c.content.source_length)
          target = p[Math.min(p.length - 1, Math.floor((inContent / PPQ) * PEAKS_PER_BEAT))] ?? 0
        } else {
          const sounding = c.content.notes.filter((n) => inContent >= n.start && inContent < n.start + n.length)
          target = sounding.length ? Math.min(1, 0.35 + 0.08 * sounding.length) * (Math.max(...sounding.map((n) => n.velocity)) / 127) : 0
        }
        target *= dbToLin(t.gain_db + c.gain_db) * 0.8
      }
    }
    const lv = demo.levels.get(t.id) ?? { l: 0, r: 0, pl: 0, pr: 0 }
    const l = target * Math.min(1, 1 - t.pan), r = target * Math.min(1, 1 + t.pan)
    lv.l = Math.max(l, lv.l * fall); lv.r = Math.max(r, lv.r * fall)
    lv.pl = Math.max(lv.l, lv.pl * Math.exp(-dt * 1.5)); lv.pr = Math.max(lv.r, lv.pr * Math.exp(-dt * 1.5))
    demo.levels.set(t.id, lv)
    sumL += l * 0.55; sumR += r * 0.55
  }
  const m = dbToLin(masterDb)
  sumL *= m; sumR *= m
  if (demo.mono) sumL = sumR = (sumL + sumR) / 2
  const mv = demo.levels.get(0) ?? { l: 0, r: 0, pl: 0, pr: 0 }
  mv.l = Math.max(sumL, mv.l * fall); mv.r = Math.max(sumR, mv.r * fall)
  mv.pl = Math.max(mv.l, mv.pl * Math.exp(-dt * 1.5)); mv.pr = Math.max(mv.r, mv.pr * Math.exp(-dt * 1.5))
  demo.levels.set(0, mv)
  const peak = Math.max(mv.pl, mv.pr)
  if (demo.playing) {
    demo.peakDb = Math.max(demo.peakDb, 20 * Math.log10(Math.max(peak, 1e-6)))
    if (peak >= 1) demo.clipped = true
    const inst = 20 * Math.log10(Math.max((mv.l + mv.r) / 2, 1e-6)) - 0.7
    if (inst > -70) demo.lufs = Number.isFinite(demo.lufs) ? demo.lufs * 0.97 + inst * 0.03 : inst
  }
}

/** True while something on screen still moves (meters falling back). Lets the draw loop stop. */
export function settling(): boolean {
  if (demo.playing) return true
  for (const v of demo.levels.values()) if (v.l > 0.002 || v.r > 0.002 || v.pl > 0.002 || v.pr > 0.002) return true
  return false
}
