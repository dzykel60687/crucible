// Built-in effects in the window: the picker, the editor and the mixer slot menu. Every control
// is drawn from the engine's own parameter table (`fx.kinds`, crucible-dsp's KINDS), so the window
// has no ideas of its own about an effect's settings, and every change is an `fx.*` command (D-022).
import { closeLayer, dialog, menu, textPrompt } from './dialogs'
import { plugins } from './demo'
import { exec, q, type Insert, type Track } from './engine'
import { icon } from './icons'
import { dragNumber, esc, run, state, toast, track } from './store'

type Control = { type: 'number'; min: number; max: number; unit: string } | { type: 'choice'; options: string[] } | { type: 'switch' }
export interface FxParam { name: string; label: string; default: number; help: string; control: Control }
export interface FxKind { name: string; label: string; help: string; needs_file: boolean; params: FxParam[] }

let kinds: FxKind[] = []

/** Fetch the effect table once at start-up. */
export async function loadKinds() {
  const r = await exec('fx.kinds')
  if (r.replies[0]?.ok) kinds = r.replies[0].data
}

const kindOf = (name: string) => kinds.find((k) => k.name === name)
export const fxLabel = (f: Insert) => kindOf(f.kind)?.label ?? f.kind

function show(p: FxParam, v: number): string {
  const c = p.control
  if (c.type === 'choice') return c.options[v] ?? '?'
  if (c.type === 'switch') return v >= 0.5 ? 'On' : 'Off'
  if ((p.name === 'lowcut' && v < 20) || (p.name === 'highcut' && v >= 20000) || (p.name === 'tone' && v >= 20000)) return 'Off'
  return `${+v.toFixed(Math.abs(v) >= 100 ? 0 : Math.abs(v) >= 10 ? 1 : 2)} ${c.unit}`
}

export function openEffectPicker(t: Track) {
  const vst = plugins.filter((p) => p.kind === 'Effect').map((p) => p.name)
  const d = dialog(`Add effect to ${t.name}`, `<div class="content">
      <div class="plugin-list">${kinds.map((k) => `<button data-k="${k.name}"><span class="pi">${icon.plug}</span><span><b>${esc(k.label)}</b><small>${esc(k.help)}</small></span></button>`).join('')}</div>
      <div class="sub" style="margin-top:10px">VST3 effects (${esc(vst.join(', '))}) come with plugin hosting (M3).</div>
    </div>`, { footer: '<button class="btn" data-close>Cancel</button>' })
  d.querySelectorAll<HTMLButtonElement>('[data-k]').forEach((b) => (b.onclick = async () => {
    closeLayer()
    const [r] = await run(`fx.add #${t.id} ${b.dataset.k}`)
    if (r?.ok) openEffectEditor(t.id, r.data.slot)
  }))
}

/** The editor for one effect. Rebuilt after every change, so it always shows the engine's values. */
export function openEffectEditor(trackId: number, slot: number) {
  const t = track(trackId), f = t?.effects[slot - 1], k = f && kindOf(f.kind)
  if (!t || !f || !k) return closeLayer()
  const value = (p: FxParam) => f.params[p.name] ?? p.default
  const set = async (name: string, v: string | number) => {
    await run(`fx.set #${trackId} ${slot} ${name} ${typeof v === 'number' ? +v.toFixed(4) : q(v)}`)
    openEffectEditor(trackId, slot)
  }
  const rows = k.params.map((p) => {
    const c = p.control, v = value(p)
    const control = c.type === 'number'
      ? `<span class="knob-num fxv" data-p="${p.name}" title="Drag up or down · Shift for fine · double-click for the default · click to type">${esc(show(p, v))}</span>`
      : c.type === 'choice'
        ? `<div class="seg">${c.options.map((o, i) => `<button data-p="${p.name}" data-v="${i}" class="${i === v ? 'on' : ''}">${esc(o)}</button>`).join('')}</div>`
        : `<button class="btn ${v >= 0.5 ? 'solid' : ''}" data-p="${p.name}" data-toggle>${v >= 0.5 ? 'On' : 'Off'}</button>`
    return `<div class="row" title="${esc(p.help)}"><span>${esc(p.label)}</span>${control}</div>`
  })
  if (k.needs_file) rows.unshift(`<div class="row"><span>Impulse response</span><span class="fx-file">${f.file ? esc(f.file) : '<i>none yet</i>'} <button class="btn" data-file>Choose file…</button></span></div>`)
  const d = dialog(`${k.label} · ${t.name}, slot ${slot}`, `<div class="content fx-editor"><div class="sub">${esc(k.help)}</div>${rows.join('')}</div>`, {
    footer: `<button class="btn ${f.bypass ? 'solid' : ''}" data-bypass>${f.bypass ? 'Bypassed' : 'Bypass'}</button><button class="btn danger" data-remove>Remove</button><span style="flex:1"></span><button class="btn primary" data-close>Done</button>`,
  })
  for (const p of k.params) {
    const c = p.control
    if (c.type === 'number') {
      const el = d.querySelector<HTMLElement>(`.fxv[data-p="${p.name}"]`)!
      const range = c.max - c.min
      dragNumber(el, () => value(p), {
        min: c.min, max: c.max, perPx: range / 240, step: range >= 100 ? 1 : range >= 10 ? 0.1 : 0.01, reset: p.default,
        preview: (v) => { el.textContent = show(p, v) },
        commit: (v) => set(p.name, v),
        click: () => textPrompt(`${k.label}: ${p.label} (${c.min} to ${c.max} ${c.unit})`, String(+value(p).toFixed(3)), (v) => set(p.name, v.trim())),
      })
    }
  }
  d.querySelectorAll<HTMLButtonElement>('[data-v]').forEach((b) => (b.onclick = () => set(b.dataset.p!, +b.dataset.v!)))
  d.querySelectorAll<HTMLButtonElement>('[data-toggle]').forEach((b) => (b.onclick = () => set(b.dataset.p!, value(k.params.find((p) => p.name === b.dataset.p)!) >= 0.5 ? 'off' : 'on')))
  d.querySelector<HTMLButtonElement>('[data-bypass]')!.onclick = async () => { await run(`fx.bypass #${trackId} ${slot} --state ${f.bypass ? 'off' : 'on'}`); openEffectEditor(trackId, slot) }
  d.querySelector<HTMLButtonElement>('[data-remove]')!.onclick = () => { closeLayer(); run(`fx.remove #${trackId} ${slot}`) }
  d.querySelector<HTMLButtonElement>('[data-file]')?.addEventListener('click', () => chooseImpulse(trackId, slot))
}

/** Import an impulse-response file into the project and give it to the IR Reverb in `slot`. */
function chooseImpulse(trackId: number, slot: number) {
  if (!state.folder) {
    toast('info', 'Save the project first', 'The impulse response is copied into the project folder, so it needs one.')
    return
  }
  textPrompt('Impulse response: WAV or FLAC, mono, stereo or 4-channel true stereo (full path; "Copy as path" works)', '', async (v) => {
    const path = v.trim().replace(/^"(.*)"$/, '$1')
    if (!path) return openEffectEditor(trackId, slot)
    const [imp] = await run(`audio.import ${q(path)}`)
    if (imp?.ok) await run(`fx.file #${trackId} ${slot} ${q(imp.data.source)}`)
    openEffectEditor(trackId, slot)
  })
}

/** Right-click menu for an effect slot in the mixer. */
export function effectMenu(t: Track, slot: number, at: { x: number; y: number }) {
  const f = t.effects[slot - 1], n = t.effects.length
  menu(at, [
    { label: `Edit ${fxLabel(f)}…`, action: () => openEffectEditor(t.id, slot) },
    { label: f.bypass ? 'Turn back on' : 'Bypass', action: () => run(`fx.bypass #${t.id} ${slot} --state ${f.bypass ? 'off' : 'on'}`) },
    { label: 'Move up', action: () => run(`fx.move #${t.id} ${slot} ${slot - 1}`), disabled: slot === 1 },
    { label: 'Move down', action: () => run(`fx.move #${t.id} ${slot} ${slot + 1}`), disabled: slot === n },
    'sep',
    { label: 'Remove', danger: true, action: () => run(`fx.remove #${t.id} ${slot}`) },
  ])
}
