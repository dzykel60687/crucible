// The transport. Play, stop and seek are engine commands (D-034); the play position and the
// meters come from asking `transport.status` twenty times a second while playing. Between two
// answers the playhead is carried forward by the screen's clock, so it moves every frame without
// a command per frame.
import { exec } from './engine'
import { changed, run, state, toast } from './store'

/** What the transport shows. View state only: the engine holds the truth. */
export const tp = {
  playing: false,
  /** Play position in beats. Setting it while playing makes the engine jump there. */
  pos: 0,
  /** Meter levels by track id (0 = master), linear 0..1: now, and the held peak, left and right. */
  levels: new Map<number, { l: number; r: number; pl: number; pr: number }>(),
  /** Loudest master sample since play started, in dBFS, and whether it reached full scale. */
  peakDb: -Infinity,
  clipped: false,
  dropouts: 0,
}

type Status = {
  playing: boolean; beat: number; dropouts: number; error: string | null; stopped_because?: string | null
  peak: [number | null, number | null]
  tracks: { id: number; peak: [number | null, number | null] }[]
  device: { buffer_frames: number; buffer_ms: number; device: string; sample_rate: number; format: string }; driver: string; notes: string[]
}

const POLL_MS = 50
/** The device in use while playing, for the top bar. */
export let device: (Status['device'] & { driver: string }) | null = null

let anchorBeat = 0, anchorAt = 0
/** Where this module last put the playhead. If tp.pos differs, the user moved it: seek. */
let expected = 0
let seeking = 0, polling = false, toldError = false
/** Peaks from the last answer, waiting to be shown: track id (0 = master) to linear left, right. */
const fresh = new Map<number, [number, number]>()

const lin = (db: number | null) => (db === null ? 0 : Math.pow(10, db / 20))
const beatArg = (b: number) => +Math.max(0, b).toFixed(6)

export async function togglePlay() {
  if (tp.playing) {
    tp.playing = false
    device = null
    changed()
    await run('transport.stop', { quiet: true })
    return
  }
  const from = tp.pos
  const r = await run(`transport.play --from ${beatArg(from)}`)
  const reply = r.at(-1)
  if (!reply?.ok) return
  tp.playing = true
  tp.peakDb = -Infinity
  tp.clipped = false
  tp.dropouts = 0
  toldError = false
  anchorBeat = expected = tp.pos = from
  anchorAt = performance.now()
  // The engine says when it could not use the driver that was asked for (taken by another
  // program, format refused) and what it used instead. The person should see that.
  const notes = (reply.text ?? '').split('\n').filter((l) => l.startsWith('Note: ')).map((l) => l.slice(6))
  if (notes.length) toast('info', 'Audio output', notes.join(' '))
  changed()
  void poll()
}

async function poll() {
  if (polling) return
  polling = true
  while (tp.playing) {
    const asked = seeking
    try {
      const d = (await exec('transport.status')).replies[0]?.data as Status | undefined
      if (!d?.playing) {
        // The engine stopped by itself: another project was opened, or the device went away.
        tp.playing = false
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
      // Numbers only. The top bar reads them every frame; nothing here rebuilds the page. A
      // rebuild on every dropout made the faders jump and swallowed clicks (found 2026-11-25).
      tp.dropouts = d.dropouts
      if (!device) {
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
  if (tp.playing) {
    if (tp.pos !== expected) {
      // The user clicked or dragged the playhead.
      const to = tp.pos
      seeking++
      anchorBeat = to
      anchorAt = performance.now()
      void exec(`transport.seek ${beatArg(to)}`)
    } else {
      tp.pos = anchorBeat + ((performance.now() - anchorAt) / 1000) * state.project.tempo_bpm / 60
    }
    expected = tp.pos
  }
  const fall = Math.exp(-dt * 9), hold = Math.exp(-dt * 1.5)
  const idsSeen = new Set<number>([0, ...state.project.tracks.map((t) => t.id)])
  for (const id of idsSeen) {
    const [l, r] = tp.playing ? fresh.get(id) ?? [0, 0] : [0, 0]
    const lv = tp.levels.get(id) ?? { l: 0, r: 0, pl: 0, pr: 0 }
    lv.l = Math.max(l, lv.l * fall); lv.r = Math.max(r, lv.r * fall)
    lv.pl = Math.max(lv.l, lv.pl * hold); lv.pr = Math.max(lv.r, lv.pr * hold)
    tp.levels.set(id, lv)
  }
  fresh.clear()
  const m = tp.levels.get(0)!
  const peak = Math.max(m.pl, m.pr)
  if (tp.playing && peak > 0) {
    tp.peakDb = Math.max(tp.peakDb, 20 * Math.log10(peak))
    if (peak >= 1) tp.clipped = true
  }
}

/** True while something on screen still moves (meters falling back). Lets the draw loop stop. */
export function settling(): boolean {
  if (tp.playing) return true
  for (const v of tp.levels.values()) if (v.l > 0.002 || v.r > 0.002 || v.pl > 0.002 || v.pr > 0.002) return true
  return false
}
