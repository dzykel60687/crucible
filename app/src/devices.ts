// The machine's audio devices in the window: the list in Settings, a notice when something is
// plugged in or unplugged, and the choice of output, driver and buffer. Everything comes from
// the engine's `audio.devices` and `audio.output` commands; nothing here talks to a driver.
import { exec, q } from './engine'
import { changed, esc, run, toast } from './store'

export interface Device {
  name: string; input: boolean; default: boolean
  /** What Windows runs the device at when programs share it. */
  rate: number | null; channels: number | null; bits: number | null
  /** Other programs with sound on it right now. */
  used_by: string[]
  /** What it accepts when one program takes it over. Only after a probe. */
  exclusive?: { formats: { format: string; sample_rates: number[] }[]; min_period_ms: number | null; error: string | null }
}
export interface DeviceList {
  outputs: Device[]; inputs: Device[]; asio: string[]; probed: boolean; changes: string[]; generation: number
  midi: string[]; midi_error: string | null
  choice: { driver: string | null; device: string | null; buffer: number | null }
  project_rate: number; playing: boolean
}

/** The devices as last listed. Null until the first answer. */
export let devices: DeviceList | null = null
let generation = -1
const listeners = new Set<() => boolean>()
/** Called after every new list. Return false to stop being called. */
export const onDevices = (f: () => boolean) => { listeners.add(f) }

/** List the devices again. `probe` also asks what each accepts in exclusive mode (slower). */
export async function refreshDevices(probe = false): Promise<DeviceList | null> {
  let reply
  try { reply = (await exec(`audio.devices${probe ? ' --probe' : ''}`)).replies[0] } catch { return devices }
  if (!reply?.ok) return devices
  const d = reply.data as DeviceList
  // A plain list carries no exclusive-mode details: keep the ones from the last probe.
  if (!d.probed && devices) {
    for (const x of [...d.outputs, ...d.inputs]) x.exclusive = [...devices.outputs, ...devices.inputs].find((o) => o.name === x.name && o.input === x.input)?.exclusive
    d.probed = devices.probed
  }
  devices = d
  generation = d.generation
  for (const c of d.changes) toast('info', 'Audio devices', c)
  changed()
  for (const f of [...listeners]) if (!f()) listeners.delete(f)
  return d
}

/** Start noticing devices being plugged in and out. Windows tells the engine; the window asks
 *  the engine every two seconds whether anything was reported, and lists again only then. */
export function watchDevices() {
  void refreshDevices()
  let pending = 0
  setInterval(async () => {
    if (document.visibilityState !== 'visible' || pending) return
    try {
      const g = (await exec('audio.changes')).replies[0]?.data?.generation
      if (typeof g !== 'number' || g === generation) return
      // One plug or unplug is reported as several changes: wait for them to finish.
      pending = window.setTimeout(() => { pending = 0; void refreshDevices(devices?.probed ?? false) }, 700)
    } catch { /* the bridge is restarting */ }
  }, 2000)
}

/** The one name in `names` that best fits `wanted`: the same interface is called "OUT 1-2
 *  (BEHRINGER UMC 202HD 192k)" by Windows and "UMC ASIO Driver" by its ASIO driver. Mirrors
 *  `closest_name` in crucible-audio-io, which decides what really opens. */
const GENERIC = new Set(['out', 'outs', 'output', 'outputs', 'in', 'input', 'inputs', 'audio', 'usb', 'driver', 'drivers', 'asio', 'wasapi', 'speaker', 'speakers', 'headphones', 'line', 'device', 'high', 'definition', 'the', 'and', 'for', 'digital', 'analog', 'stereo', 'main', 'playback', 'interface'])
const words = (s: string) => s.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length >= 3 && !GENERIC.has(w) && !/^\d+$/.test(w))
export function closestName(wanted: string, names: string[]): string | null {
  const w = wanted.toLowerCase()
  const contains = names.filter((n) => n.toLowerCase().includes(w) || w.includes(n.toLowerCase()))
  if (contains.length === 1) return contains[0]
  const want = words(wanted)
  const scored = names.map((n) => [words(n).filter((x) => want.includes(x)).length, n] as const).filter(([k]) => k > 0).sort((a, b) => b[0] - a[0])
  return scored.length === 1 || (scored.length > 1 && scored[0][0] > scored[1][0]) ? scored[0][1] : null
}

/** The Windows output playback will use: the chosen one if it is connected, else the default. */
export function chosenOutput(d: DeviceList | null = devices): Device | null {
  if (!d) return null
  const names = d.outputs.map((o) => o.name)
  const pick = d.choice.device ? closestName(d.choice.device, names) : null
  return d.outputs.find((o) => o.name === pick) ?? d.outputs.find((o) => o.default) ?? d.outputs[0] ?? null
}

/** The ASIO driver that belongs to the chosen output, if any. */
export function chosenAsio(d: DeviceList | null = devices): string | null {
  if (!d?.asio.length) return null
  if (!d.choice.device) return d.asio[0]
  return closestName(d.choice.device, d.asio) ?? (d.asio.length === 1 && d.choice.driver === 'asio' ? d.asio[0] : null)
}

const DRIVERS: [string, string, string][] = [
  ['auto', 'Automatic', "ASIO if the chosen output has an ASIO driver, else Exclusive, else Shared."],
  ['asio', 'ASIO', "The interface's own driver: the lowest latency, and it sets the interface to the project's rate. Whether other programs can play at the same time is up to that driver."],
  ['exclusive', 'Exclusive', 'Crucible takes the device while it plays: bit-exact and low latency. Other programs on this device are silent until you press Stop.'],
  ['shared', 'Shared', 'Other programs keep playing. Windows mixes the sound and converts it if the rates differ: not bit-exact, a little more latency.'],
]
const BUFFERS = [0, 64, 128, 256, 512, 1024]

const format = (x: Device) => (x.rate ? `${x.rate} Hz${x.bits ? `, ${x.bits}-bit` : ''}${x.channels ? `, ${x.channels} ch` : ''} in Windows` : 'rate unknown')
const exclusiveRates = (x: Device) => [...new Set(x.exclusive?.formats.flatMap((f) => f.sample_rates) ?? [])].sort((a, b) => a - b)
const row = (x: Device) => {
  const ex = x.exclusive?.formats.length ? ` · exclusive: ${exclusiveRates(x).join(', ')} Hz` : x.exclusive ? ' · no exclusive mode' : ''
  const used = x.used_by.length ? `<br><small style="color:var(--solo)">In use by ${esc(x.used_by.join(', '))}</small>` : ''
  return `<div><span class="grow">${esc(x.name)}<br><small>${esc(format(x) + ex)}</small>${used}</span>${x.default ? '<span class="tag live">default</span>' : ''}</div>`
}
const good = (s: string) => `<span style="color:var(--ok)">${s}</span>`
const warn = (s: string) => `<span style="color:var(--solo)">${s}</span>`

/** What will happen to the sample rate with this driver and output, in plain words. */
function rateVerdict(d: DeviceList, out: Device | null, driver: string): string {
  const rate = d.project_rate
  if (driver === 'auto') driver = chosenAsio(d) ? 'asio' : 'exclusive'
  if (driver === 'asio') {
    const asio = chosenAsio(d)
    if (!asio) return warn('No ASIO driver belongs to this output.') + ' Choose an ASIO driver above, or another driver.'
    return good(`${esc(asio)} is set to the project's rate, ${rate} Hz, when playback starts.`) + ` Windows' own setting for the device${out?.rate ? ` (${out.rate} Hz)` : ''} does not apply to ASIO. Nothing is converted.`
  }
  if (!out) return ''
  if (driver === 'exclusive') {
    const rates = exclusiveRates(out)
    if (!out.exclusive) return 'Checking which rates this device takes in exclusive mode…'
    if (rates.includes(rate)) return good(`Exclusive mode plays ${rate} Hz directly.`) + ' Nothing is converted.'
    return warn(`In exclusive mode this device takes only ${rates.join(', ') || 'no tested rate'} Hz, not the project's ${rate} Hz.`) + ` Use its ASIO driver, or set the project to ${rates.join(' or ') || 'a rate it takes'} under Settings, Project.`
  }
  if (!out.rate || out.rate === rate) return good(`Windows runs this device at the project's rate, ${rate} Hz.`) + ' Windows still mixes it with other programs.'
  return warn(`In Shared mode Windows converts the project's ${rate} Hz to the device's ${out.rate} Hz.`) + ' Not bit-exact. ASIO or Exclusive avoid it.'
}

/** The Audio page of Settings. */
export function audioHtml(): string {
  const d = devices
  if (!d) return '<h3>Audio</h3><div class="sub">Asking Windows for its audio devices…</div>'
  const out = chosenOutput(d)
  const driver = d.choice.driver ?? 'auto'
  const rate = d.project_rate
  const verdict = rateVerdict(d, out, driver)
  // With ASIO the list is the ASIO drivers: those are the names ASIO knows.
  const asioList = driver === 'asio' && d.asio.length > 0
  const options = asioList
    ? d.asio.map((a) => `<option value="${esc(a)}" ${chosenAsio(d) === a ? 'selected' : ''}>${esc(a)}</option>`).join('')
    : `<option value="default" ${d.choice.device ? '' : 'selected'}>System default${d.outputs.find((o) => o.default) ? ` (${esc(d.outputs.find((o) => o.default)!.name)})` : ''}</option>` +
      d.outputs.map((o) => `<option value="${esc(o.name)}" ${d.choice.device && out === o ? 'selected' : ''}>${esc(o.name)}</option>`).join('')
  return `<h3>Output</h3>
    <label class="row"><span>${asioList ? 'ASIO driver' : 'Device'}</span><select class="input" data-out>${options}</select></label>
    <div class="row"><span>Driver</span><div class="seg" data-drv>${DRIVERS.map(([v, label]) => `<button data-v="${v}" class="${v === driver ? 'on' : ''}" ${v === 'asio' && d.probed && !d.asio.length ? 'disabled title="No ASIO driver is installed"' : ''}>${label}</button>`).join('')}</div></div>
    <div class="row"><span></span><div class="sub">${DRIVERS.find(([v]) => v === driver)![2]}</div></div>
    <div class="row"><span>Buffer</span><div class="seg" data-buf>${BUFFERS.map((b) => `<button data-v="${b}" class="${(d.choice.buffer ?? 0) === b ? 'on' : ''}">${b || "Driver's choice"}</button>`).join('')}</div></div>
    <div class="row"><span></span><div class="sub">${d.choice.buffer ? `${d.choice.buffer} samples at ${rate / 1000} kHz = ${(d.choice.buffer / rate * 1000).toFixed(1)} ms. Smaller is quicker to respond and harder on the computer.` : 'The driver picks its smallest safe size. The top bar shows what it chose while playing.'}</div></div>
    ${verdict ? `<div class="row"><span>Sample rate</span><div class="sub" style="margin:0">${verdict}</div></div>` : ''}
    ${d.playing ? '<div class="row"><span></span><div class="sub">Playback keeps its current device until you stop and start it again.</div></div>' : ''}
    <h3>Outputs connected now</h3><div class="list">${d.outputs.map(row).join('') || '<div><span class="grow">None. Plug one in: it appears here by itself.</span></div>'}</div>
    <h3>Inputs connected now</h3><div class="list">${d.inputs.map(row).join('') || '<div><span class="grow">None.</span></div>'}</div>
    <div class="sub">Recording is not built yet. Inputs are listed so you can see that they are detected.</div>
    <h3>MIDI inputs connected now</h3><div class="list">${d.midi_error ? `<div><span class="grow">${warn('Could not be listed')}<br><small>${esc(d.midi_error)}</small></span></div>` : d.midi.map((m) => `<div><span class="grow">${esc(m)}</span></div>`).join('') || '<div><span class="grow">None. Plug a keyboard in: it appears here by itself.</span></div>'}</div>
    <div class="sub">Every keyboard here plays the instrument track whose keyboard button is lit. A track can be told to listen to one keyboard or one channel: click its instrument, then "Keyboard and channel". Keyboards are opened only while an instrument is loaded, so other programs can use them the rest of the time.</div>
    <h3>ASIO drivers</h3><div class="sub">${d.probed ? (d.asio.length ? esc(d.asio.join(', ')) : 'None installed on this computer.') : 'Checking…'}</div>
    <div class="sub">Crucible cannot see a program that holds a device through ASIO: ASIO goes around Windows.</div>`
}

/** Make the Audio page's controls run their commands. `again` redraws the page. */
export function wireAudio(pane: HTMLElement, again: () => void) {
  const set = async (arg: string) => { await run(`audio.output ${arg}`, { quiet: true }); await refreshDevices(true); again() }
  const out = pane.querySelector<HTMLSelectElement>('[data-out]')
  if (out) out.onchange = () => set(`--device ${q(out.value)}`)
  pane.querySelectorAll<HTMLButtonElement>('[data-drv] button').forEach((b) => (b.onclick = () => set(`--driver ${b.dataset.v}`)))
  pane.querySelectorAll<HTMLButtonElement>('[data-buf] button').forEach((b) => (b.onclick = () => set(`--buffer ${b.dataset.v}`)))
}
