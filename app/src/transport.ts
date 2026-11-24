// The real transport (D-034). Play, stop and seek are engine commands; the play position and the
// meters come from polling `transport.status`. Between polls the playhead is carried forward by
// the screen's clock, so it moves every frame without a command per frame.
//
// Still stand-ins in demo.ts: record, loop, metronome, mono, A/B and loudness (LUFS).
import { demo } from './demo'
import { exec } from './engine'
import { changed, run, state, toast } from './store'

type Status = {
  playing: boolean; beat: number; dropouts: number; error: string | null; stopped_because?: string | null
  peak: [number | null, number | null]
  tracks: { id: number; peak: [number | null, number | null] }[]
  device: { buffer_frames: number; buffer_ms: number; device: string }; driver: string
}

const POLL_MS = 50
/** The device in use while playing, for the top bar. */
export let device: Status['device'] & { driver: string } | null = null

let anchorBeat = 0, anchorAt = 0
/** Where this module last put the playhead. If demo.pos differs, the user moved it: seek. */
let expected = 0
let seeking = 0, polling = false, toldError = false
/** Peaks from the last poll, waiting to be shown: track id (0 = master) to linear left, right. */
const fresh = new Map<number, [number, number]>()

const lin = (db: number | null) => (db === null ? 0 : Math.pow(10, db / 20))
const beatArg = (b: number) => +Math.max(0, b).toFixed(6)

export async function togglePlay() {
  if (demo.playing) {
    demo.playing = false
    demo.recording = false
    device = null
    changed()
    await run('transport.stop', { quiet: true })
    return
  }
  const from = demo.pos
  const r = await run(`transport.play --from ${beatArg(from)}`)
  if (!r.length || !r[r.length - 1].ok) return
  demo.playing = true
  demo.peakDb = -Infinity
  demo.clipped = false
  demo.xruns = 0
  toldError = false
  anchorBeat = expected = demo.pos = from
  anchorAt = performance.now()
  changed()
  void poll()
}

async function poll() {
  if (polling) return
  polling = true
  while (demo.playing) {
    const asked = seeking
    try {
      const d = (await exec('transport.status')).replies[0]?.data as Status | undefined
      if (!d?.playing) {
        // The engine stopped by itself: another project was opened, or the device went away.
        demo.playing = false
        device = null
        if (d?.stopped_because) toast('info', 'Playback stopped', d.stopped_because)
        changed()
        break
      }
      if (asked === seeking) {
        // Follow the audio clock gently: a hard jump only when far off (a seek, a stall).
        const now = performance.now()
        const shown = anchorBeat + ((now - anchorAt) / 1000) * state.project.tempo_bpm / 60
        const off = d.beat - shown
        anchorBeat = Math.abs(off) > 0.25 ? d.beat : shown + off * 0.15
        anchorAt = now
      }
      fresh.set(0, [lin(d.peak[0]), lin(d.peak[1])])
      for (const t of d.tracks) fresh.set(t.id, [lin(t.peak[0]), lin(t.peak[1])])
      if (d.dropouts !== demo.xruns || !device) {
        demo.xruns = d.dropouts
        device = { ...d.device, driver: d.driver }
        changed()
      }
      if (d.error && !toldError) {
        toldError = true
        toast('error', 'Playback problem', d.error)
      }
    } catch {
      // The bridge is restarting; the next poll tries again.
    }
    await new Promise((r) => setTimeout(r, POLL_MS))
  }
  polling = false
}

/** Once per drawn frame: move the playhead and let the meters follow the real levels. */
export function advance(dt: number) {
  if (demo.playing) {
    if (demo.pos !== expected) {
      // The user clicked or dragged the playhead.
      const to = demo.pos
      seeking++
      anchorBeat = to
      anchorAt = performance.now()
      void exec(`transport.seek ${beatArg(to)}`)
    } else {
      demo.pos = anchorBeat + ((performance.now() - anchorAt) / 1000) * state.project.tempo_bpm / 60
    }
    expected = demo.pos
  }
  const fall = Math.exp(-dt * 9), hold = Math.exp(-dt * 1.5)
  const idsSeen = new Set<number>([0, ...state.project.tracks.map((t) => t.id)])
  for (const id of idsSeen) {
    const [l, r] = demo.playing ? fresh.get(id) ?? [0, 0] : [0, 0]
    const lv = demo.levels.get(id) ?? { l: 0, r: 0, pl: 0, pr: 0 }
    lv.l = Math.max(l, lv.l * fall); lv.r = Math.max(r, lv.r * fall)
    lv.pl = Math.max(lv.l, lv.pl * hold); lv.pr = Math.max(lv.r, lv.pr * hold)
    demo.levels.set(id, lv)
  }
  fresh.clear()
  const m = demo.levels.get(0)!
  const peak = Math.max(m.pl, m.pr)
  if (demo.playing && peak > 0) {
    demo.peakDb = Math.max(demo.peakDb, 20 * Math.log10(peak))
    if (peak >= 1) demo.clipped = true
  }
}
