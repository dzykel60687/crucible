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

/** The output playback will use: the chosen one if it is connected, else the system default. */
export function chosenOutput(d: DeviceList | null = devices): Device | null {
  if (!d) return null
  const want = d.choice.device?.toLowerCase()
  return (want ? d.outputs.find((o) => o.name.toLowerCase().includes(want)) : null) ?? d.outputs.find((o) => o.default) ?? d.outputs[0] ?? null
}

const DRIVERS: [string, string, string][] = [
  ['auto', 'Automatic', 'ASIO if a driver is installed, else Exclusive, else Shared.'],
  ['asio', 'ASIO', "The interface's own driver: the lowest latency. Whether other programs can play at the same time is up to that driver."],
  ['exclusive', 'Exclusive', 'Crucible takes the device while it plays: bit-exact and low latency. Other programs on this device are silent until you press Stop.'],
  ['shared', 'Shared', 'Other programs keep playing. Windows mixes the sound and converts it if the rates differ: not bit-exact, a little more latency.'],
]
const BUFFERS = [0, 64, 128, 256, 512, 1024]

const format = (x: Device) => (x.rate ? `${x.rate} Hz${x.bits ? `, ${x.bits}-bit` : ''}${x.channels ? `, ${x.channels} ch` : ''} in Windows` : 'rate unknown')
const row = (x: Device) => {
  const ex = x.exclusive?.formats.length ? ` · exclusive: ${[...new Set(x.exclusive.formats.flatMap((f) => f.sample_rates))].sort((a, b) => a - b).join(', ')} Hz` : x.exclusive ? ' · no exclusive mode' : ''
  const used = x.used_by.length ? `<br><small style="color:var(--solo)">In use by ${esc(x.used_by.join(', '))}</small>` : ''
  return `<div><span class="grow">${esc(x.name)}<br><small>${esc(format(x) + ex)}</small>${used}</span>${x.default ? '<span class="tag live">default</span>' : ''}</div>`
}

/** The Audio page of Settings. */
export function audioHtml(): string {
  const d = devices
  if (!d) return '<h3>Audio</h3><div class="sub">Asking Windows for its audio devices…</div>'
  const out = chosenOutput(d)
  const driver = d.choice.driver ?? 'auto'
  const rate = d.project_rate
  let verdict = ''
  if (out) {
    const direct = out.exclusive?.formats.some((f) => f.sample_rates.includes(rate))
    if (out.rate && out.rate !== rate) {
      verdict = `<span style="color:var(--solo)">Windows runs this device at ${out.rate} Hz, the project is ${rate} Hz.</span> ${direct ? 'Exclusive mode plays the project rate directly; in Shared mode Windows converts it.' : 'Windows will convert the sound. To avoid that, set the device to ' + rate + ' Hz in Windows Sound settings.'}`
    } else if (out.rate) {
      verdict = `<span style="color:var(--ok)">Device and project both run at ${rate} Hz.</span> Nothing is converted.`
    }
  }
  return `<h3>Output</h3>
    <label class="row"><span>Device</span><select class="input" data-out>
      <option value="default" ${d.choice.device ? '' : 'selected'}>System default${d.outputs.find((o) => o.default) ? ` (${esc(d.outputs.find((o) => o.default)!.name)})` : ''}</option>
      ${d.outputs.map((o) => `<option value="${esc(o.name)}" ${d.choice.device && out === o ? 'selected' : ''}>${esc(o.name)}</option>`).join('')}
    </select></label>
    <div class="row"><span>Driver</span><div class="seg" data-drv>${DRIVERS.map(([v, label]) => `<button data-v="${v}" class="${v === driver ? 'on' : ''}" ${v === 'asio' && d.probed && !d.asio.length ? 'disabled title="No ASIO driver is installed"' : ''}>${label}</button>`).join('')}</div></div>
    <div class="row"><span></span><div class="sub">${DRIVERS.find(([v]) => v === driver)![2]}</div></div>
    <div class="row"><span>Buffer</span><div class="seg" data-buf>${BUFFERS.map((b) => `<button data-v="${b}" class="${(d.choice.buffer ?? 0) === b ? 'on' : ''}">${b || "Driver's choice"}</button>`).join('')}</div></div>
    <div class="row"><span></span><div class="sub">${d.choice.buffer ? `${d.choice.buffer} samples at ${rate / 1000} kHz = ${(d.choice.buffer / rate * 1000).toFixed(1)} ms. Smaller is quicker to respond and harder on the computer.` : 'The driver picks its smallest safe size. The top bar shows what it chose while playing.'}</div></div>
    ${verdict ? `<div class="row"><span>Sample rate</span><div class="sub" style="margin:0">${verdict}</div></div>` : ''}
    ${d.playing ? '<div class="row"><span></span><div class="sub">Playback keeps its current device until you stop and start it again.</div></div>' : ''}
    <h3>Outputs connected now</h3><div class="list">${d.outputs.map(row).join('') || '<div><span class="grow">None. Plug one in: it appears here by itself.</span></div>'}</div>
    <h3>Inputs connected now</h3><div class="list">${d.inputs.map(row).join('') || '<div><span class="grow">None.</span></div>'}</div>
    <div class="sub">Recording is not built yet. Inputs are listed so you can see that they are detected.</div>
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
