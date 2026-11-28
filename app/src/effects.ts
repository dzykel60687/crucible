// Built-in effects in the window: the picker, the editor and the mixer slot menu. Every control
// is drawn from the engine's own parameter table (`fx.kinds`, crucible-dsp's KINDS), so the window
// has no ideas of its own about an effect's settings, and every change is an `fx.*` command (D-022).
import { closeLayer, dialog, menu, pickFiles } from './dialogs'
import { exec, q, type Insert, type Track } from './engine'
import { icon } from './icons'
import { esc, run, track } from './store'

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
  const d = dialog(`Add effect to ${t.name}`, `<div class="content">
      <div class="plugin-list">${kinds.map((k) => `<button data-k="${k.name}"><span class="pi">${icon.plug}</span><span><b>${esc(k.label)}</b><small>${esc(k.help)}</small></span></button>`).join('')}</div>
      <div class="sub" style="margin-top:10px">These are Crucible's own effects. VST3 instruments are on instrument tracks; VST3 effects are not built yet.</div>
    </div>`, { footer: '<button class="btn" data-close>Cancel</button>' })
  d.querySelectorAll<HTMLButtonElement>('[data-k]').forEach((b) => (b.onclick = async () => {
    closeLayer()
    const [r] = await run(`fx.add #${t.id} ${b.dataset.k}`)
    if (r?.ok) openEffectEditor(t.id, r.data.slot)
  }))
}

/** The editor for one effect (reworked 2026-11-26 at Dzyan's word). Every number is a slider with
 *  its value beside it, to drag or to type, and its range shown; nothing opens a second popup.
 *  A choice with a few options is a row of buttons, with many a list; nothing scrolls sideways.
 *  A number changes in place, so the control in hand is never replaced; the other controls
 *  redraw the editor from the engine's values. */
export function openEffectEditor(trackId: number, slot: number) {
  const cur = () => track(trackId)?.effects[slot - 1]
  const t = track(trackId), f = cur(), k = f && kindOf(f.kind)
  if (!t || !f || !k) return closeLayer()
  const value = (p: FxParam) => cur()?.params[p.name] ?? p.default
  const set = async (name: string, v: string | number) => {
    await run(`fx.set #${trackId} ${slot} ${name} ${typeof v === 'number' ? +v.toFixed(4) : q(v)}`)
  }
  const redo = async (name: string, v: string | number) => { await set(name, v); openEffectEditor(trackId, slot) }
  const stepOf = (c: { min: number; max: number }) => (c.max - c.min >= 100 ? 1 : c.max - c.min >= 10 ? 0.1 : 0.01)
  const plain = (c: { min: number; max: number }, v: number) => String(+v.toFixed(stepOf(c) >= 1 ? 0 : stepOf(c) >= 0.1 ? 1 : 2))
  const rangeText = (p: FxParam, v: number) => {
    const c = p.control
    return c.type === 'number' ? `${c.min} to ${c.max} ${c.unit}${show(p, v) === 'Off' ? ' · off' : ''}` : ''
  }
  const rows = k.params.map((p) => {
    const c = p.control, v = value(p)
    const control = c.type === 'number'
      ? `<div class="fxnum"><input type="range" data-p="${p.name}" min="${c.min}" max="${c.max}" step="${stepOf(c)}" value="${v}" title="Drag · arrow keys · double-click for the default (${plain(c, p.default)})"><input class="input fxin" data-p="${p.name}" value="${plain(c, v)}" inputmode="decimal" spellcheck="false" title="Click to type a value"><small data-r="${p.name}">${esc(rangeText(p, v))}</small></div>`
      : c.type === 'choice'
        ? c.options.length <= 4
          ? `<div class="seg">${c.options.map((o, i) => `<button data-p="${p.name}" data-v="${i}" class="${i === v ? 'on' : ''}">${esc(o)}</button>`).join('')}</div>`
          : `<select class="input" data-p="${p.name}" data-choice>${c.options.map((o, i) => `<option value="${i}" ${i === v ? 'selected' : ''}>${esc(o)}</option>`).join('')}</select>`
        : `<span><button class="switch ${v >= 0.5 ? 'on' : ''}" data-p="${p.name}" data-toggle title="${v >= 0.5 ? 'On' : 'Off'}"></button></span>`
    return `<div class="row" title="${esc(p.help)}"><span>${esc(p.label)}</span>${control}</div>`
  })
  if (k.needs_file) rows.unshift(`<div class="row"><span>Impulse response</span><span class="fx-file"><span>${f.file ? esc(f.file) : '<i>none yet</i>'}</span><button class="btn solid" data-file>Browse…</button></span></div>`)
  const d = dialog(`${k.label} · ${t.name}`, `<div class="content fx-editor"><div class="sub">${esc(k.help)}</div>${rows.join('')}</div>`, {
    footer: `<button class="btn ${f.bypass ? 'solid' : ''}" data-bypass>${f.bypass ? 'Bypassed' : 'Bypass'}</button><button class="btn danger" data-remove>Remove</button><span style="flex:1"></span><button class="btn primary" data-close>Done</button>`,
  })
  d.classList.add('fx')
  for (const p of k.params) {
    const c = p.control
    if (c.type !== 'number') continue
    const slider = d.querySelector<HTMLInputElement>(`input[type=range][data-p="${p.name}"]`)!
    const field = d.querySelector<HTMLInputElement>(`.fxin[data-p="${p.name}"]`)!
    const note = d.querySelector<HTMLElement>(`[data-r="${p.name}"]`)!
    /** Show the engine's value: after a change, and when what was typed was refused. */
    const put = () => { const v = value(p); slider.value = String(v); field.value = plain(c, v); note.textContent = rangeText(p, v) }
    const commit = async (v: string | number) => { await set(p.name, v); if (d.isConnected) put() }
    slider.oninput = () => { field.value = plain(c, +slider.value) }
    slider.onchange = () => commit(+slider.value)
    slider.ondblclick = () => commit(p.default)
    field.onfocus = () => field.select()
    field.onchange = () => { if (field.value.trim()) commit(field.value.trim()); else put() }
    field.onkeydown = (e) => { e.stopPropagation(); if (e.key === 'Enter') field.blur(); if (e.key === 'Escape') { put(); field.blur() } }
  }
  d.querySelectorAll<HTMLButtonElement>('[data-v]').forEach((b) => (b.onclick = () => redo(b.dataset.p!, +b.dataset.v!)))
  d.querySelectorAll<HTMLSelectElement>('[data-choice]').forEach((s) => (s.onchange = () => redo(s.dataset.p!, +s.value)))
  d.querySelectorAll<HTMLButtonElement>('[data-toggle]').forEach((b) => (b.onclick = () => redo(b.dataset.p!, value(k.params.find((p) => p.name === b.dataset.p)!) >= 0.5 ? 'off' : 'on')))
  d.querySelector<HTMLButtonElement>('[data-bypass]')!.onclick = async () => { await run(`fx.bypass #${trackId} ${slot} --state ${cur()?.bypass ? 'off' : 'on'}`); openEffectEditor(trackId, slot) }
  d.querySelector<HTMLButtonElement>('[data-remove]')!.onclick = () => { closeLayer(); run(`fx.remove #${trackId} ${slot}`) }
  d.querySelector<HTMLButtonElement>('[data-file]')?.addEventListener('click', () => chooseImpulse(trackId, slot))
}

/** Choose an impulse-response file with the system's picker, bring it into the project and give
 *  it to the IR Reverb in `slot`. An unsaved project takes it too (D-042). */
async function chooseImpulse(trackId: number, slot: number) {
  const [path] = await pickFiles('Impulse response: WAV or FLAC, mono, stereo or 4-channel true stereo')
  if (path) {
    const [imp] = await run(`audio.import ${q(path)}`)
    if (imp?.ok) await run(`fx.file #${trackId} ${slot} ${q(imp.data.source)}`)
  }
  openEffectEditor(trackId, slot)
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
