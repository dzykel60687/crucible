// DEMO ONLY. Everything in this file stands in for engine features that do not exist yet
// (transport, clips, notes, meters, devices, plugins) so the layout can be judged by eye.
// It is the only UI file that simulates behaviour. Delete it piece by piece as M2+ lands;
// anything shown from here carries a "demo" tag in the UI.
import type { Track } from './engine'

export interface Note { pitch: number; start: number; len: number; vel: number }
export interface Clip { id: number; track: number; start: number; len: number; name: string; kind: 'audio' | 'midi'; peaks?: Float32Array; notes?: Note[] }

export const PEAKS_PER_BEAT = 48

export const demo = {
  clips: [] as Clip[],
  seeded: new Set<number>(),
  nextClip: 1,
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

// Deterministic noise so the demo looks the same every run.
let seed = 7
const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647)

function peaks(beats: number, style: 'drums' | 'bass' | 'pad' | 'vox'): Float32Array {
  const n = Math.round(beats * PEAKS_PER_BEAT)
  const out = new Float32Array(n)
  for (let i = 0; i < n; i++) {
    const b = i / PEAKS_PER_BEAT
    const inBeat = b % 1
    let v: number
    if (style === 'drums') v = Math.exp(-inBeat * 9) * (0.55 + 0.4 * rnd()) + 0.05 * rnd() + (Math.abs((b % 2) - 1) < 0.02 ? 0.3 : 0)
    else if (style === 'bass') v = (0.45 + 0.25 * Math.exp(-((b * 2) % 1) * 4)) * (0.85 + 0.15 * rnd())
    else if (style === 'pad') v = 0.35 + 0.2 * Math.sin(b * 0.6) + 0.05 * rnd()
    else v = (Math.sin(b * 1.7) > -0.3 ? 0.3 + 0.45 * rnd() * (0.6 + 0.4 * Math.sin(b * 5)) : 0.03 * rnd())
    out[i] = Math.min(1, Math.max(0, v))
  }
  return out
}

function chordNotes(beats: number): Note[] {
  const prog = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]]
  const notes: Note[] = []
  for (let bar = 0; bar < beats / 4; bar++) {
    const ch = prog[bar % prog.length]
    for (const p of ch) notes.push({ pitch: p, start: bar * 4, len: 3.75, vel: 80 + Math.round(rnd() * 20) })
    for (let k = 0; k < 4; k++) notes.push({ pitch: ch[k % 3] + 12, start: bar * 4 + k + 0.5, len: 0.45, vel: 60 + Math.round(rnd() * 40) })
  }
  return notes
}

function bassNotes(beats: number): Note[] {
  const roots = [45, 41, 36, 43]
  const notes: Note[] = []
  for (let b = 0; b < beats; b += 0.5) {
    if (rnd() < 0.18) continue
    notes.push({ pitch: roots[Math.floor(b / 4) % 4] + (rnd() < 0.2 ? 12 : 0), start: b, len: 0.4, vel: 70 + Math.round(rnd() * 40) })
  }
  return notes
}

/** Give each demo-named track some clips the first time it is seen. New tracks stay empty. */
export function seedClips(tracks: Track[], firstRun: boolean) {
  for (const t of tracks) {
    if (demo.seeded.has(t.id)) continue
    demo.seeded.add(t.id)
    if (!firstRun) continue
    const n = t.name.toLowerCase()
    const add = (start: number, len: number, name: string, kind: Clip['kind'], extra: Partial<Clip>) =>
      demo.clips.push({ id: demo.nextClip++, track: t.id, start, len, name, kind, ...extra })
    if (n.includes('drum')) { add(0, 32, 'Drums verse', 'audio', { peaks: peaks(32, 'drums') }); add(32, 32, 'Drums chorus', 'audio', { peaks: peaks(32, 'drums') }) }
    else if (n.includes('bass')) { add(8, 24, 'Bass line', 'midi', { notes: bassNotes(24) }); add(32, 32, 'Bass chorus', 'midi', { notes: bassNotes(32) }) }
    else if (n.includes('keys') || n.includes('piano')) { add(0, 32, 'Keyscape chords', 'midi', { notes: chordNotes(32) }); add(40, 24, 'Chords B', 'midi', { notes: chordNotes(24) }) }
    else if (n.includes('pad')) add(16, 48, 'Omnisphere pad', 'midi', { notes: chordNotes(48).filter((x) => x.len > 1) })
    else if (n.includes('vox') || n.includes('vocal')) { add(16, 16, 'Vox take 3', 'audio', { peaks: peaks(16, 'vox') }); add(36, 24, 'Vox take 5', 'audio', { peaks: peaks(24, 'vox') }) }
    if (t.kind === 'instrument') demo.instrument.set(t.id, n.includes('pad') ? 'Omnisphere' : 'Keyscape')
    if (n.includes('vox')) demo.inserts.set(t.id, ['Pro-Q 4', 'Pro-C 3'])
    if (n.includes('drum')) demo.inserts.set(t.id, ['Pro-C 3'])
  }
}

const dbToLin = (db: number) => Math.pow(10, db / 20)

/** Advance the fake transport and fake meters by dt seconds. */
export function tick(dt: number, tempo: number, tracks: Track[], masterDb: number) {
  if (demo.playing) {
    demo.pos += dt * tempo / 60
    if (demo.loop && demo.pos >= demo.loopEnd) demo.pos = demo.loopStart + (demo.pos - demo.loopEnd)
  }
  const anySolo = tracks.some((t) => t.solo)
  let sumL = 0, sumR = 0
  const fall = Math.exp(-dt * 9)
  for (const t of tracks) {
    let target = 0
    if (demo.playing && !t.mute && (!anySolo || t.solo)) {
      const c = demo.clips.find((c) => c.track === t.id && demo.pos >= c.start && demo.pos < c.start + c.len)
      if (c) {
        if (c.peaks) target = c.peaks[Math.min(c.peaks.length - 1, Math.floor((demo.pos - c.start) * PEAKS_PER_BEAT))]
        else target = 0.5 + 0.25 * Math.abs(Math.sin(demo.pos * Math.PI)) * (0.7 + 0.3 * Math.random())
        target *= dbToLin(t.gain_db) * 0.8
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
