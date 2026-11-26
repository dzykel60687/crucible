// The window's side of the paper trail (D-037). What the person does and sees here goes into the
// engine's session log through the `log.ui` command, so a report from a machine no developer can
// see shows, in order and between the commands, what was clicked and what came up on screen.
//
// Cost: nothing per frame. Lines are batched into one engine call at most every 400 ms and
// capped; the freeze check is one timer tick every half second.
//
// Never recorded: text typed into fields, and mouse movement.
import { exec, q, trailHook } from './engine'

type Kind = 'did' | 'saw' | 'slow' | 'error' | 'info'

const MAX_WAITING = 60
const waiting: { kind: Kind; text: string; at: number }[] = []
let timer = 0, lost = 0

/** Record one line. `did`: an action. `saw`: something shown. `slow`: a freeze. `error`: a bug here. */
export function trail(kind: Kind, text: string) {
  if (waiting.length >= MAX_WAITING) { lost++; return }
  // A command ends at a line break, so a line break inside the text would cut it short.
  waiting.push({ kind, text: text.replace(/\s*[\r\n]+\s*/g, ' / ').slice(0, 600) || '(empty)', at: performance.now() })
  if (kind === 'error') send()
  else if (!timer) timer = window.setTimeout(send, 400)
}

/** Hand over what is waiting, as one line of command text. */
function take(): string {
  clearTimeout(timer)
  timer = 0
  if (!waiting.length) return ''
  const now = performance.now()
  if (lost) { waiting.push({ kind: 'info', text: `${lost} more window events were not recorded: too many at once`, at: now }); lost = 0 }
  // The log stamps a line when it is written. One written late says how long ago it happened.
  return waiting.splice(0).map((w) => `log.ui ${w.kind} ${q(now - w.at > 100 ? `${w.text} (${((now - w.at) / 1000).toFixed(2)} s before this line)` : w.text)}`).join('; ')
}
// Every command takes the waiting lines with it first, so the log keeps cause before effect.
trailHook.take = take

/** Nothing else ran for 400 ms: send what is waiting on its own. */
function send() {
  const line = take()
  // If the engine cannot be reached there is nowhere to write, and nothing to tell the person.
  if (line) exec(line).catch(() => {})
}

/** A short name for what was clicked: the nearest button, field, canvas or named element. */
function name(target: EventTarget | null): string {
  let el = target instanceof Element ? target : null
  for (let n = el; n && n !== document.body; n = n.parentElement) {
    if (n.matches('button, a, input, select, textarea, canvas, [role=button], [data-act], [data-cmd], [id]')) { el = n; break }
  }
  if (!el || el === document.body || el === document.documentElement) return 'the background'
  const tag = el.tagName.toLowerCase()
  const field = tag === 'input' || tag === 'select' || tag === 'textarea'
  const id = el.id ? `#${el.id}` : ''
  const cls = !id && typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/).slice(0, 2).join('.') : ''
  const data = [...el.attributes].filter((a) => a.name.startsWith('data-')).slice(0, 3).map((a) => `${a.name.slice(5)}=${a.value.slice(0, 24)}`).join(',')
  // A field's contents are the person's own text: never recorded.
  const label = (el.getAttribute('aria-label') || el.getAttribute('title') || (field ? '' : el.textContent) || '').trim().replace(/\s+/g, ' ').slice(0, 40)
  return `${tag}${id}${cls}${data ? `[${data}]` : ''}${label ? ` "${label}"` : ''}`
}

const mods = (e: MouseEvent | KeyboardEvent) => (e.ctrlKey || e.metaKey ? 'Ctrl+' : '') + (e.altKey ? 'Alt+' : '') + (e.shiftKey ? 'Shift+' : '')
const at = (e: MouseEvent) => (e.target instanceof HTMLCanvasElement ? ` at ${Math.round(e.offsetX)},${Math.round(e.offsetY)}` : '')
const size = () => `${innerWidth}x${innerHeight} at scale ${devicePixelRatio}`

/** Start recording. Call once, before anything else can go wrong. */
export function startTrail() {
  addEventListener('error', (e) => trail('error', `${e.message} at ${e.filename}:${e.lineno}:${e.colno}${e.error?.stack ? ` | ${String(e.error.stack).split('\n').slice(0, 6).join(' / ')}` : ''}`))
  addEventListener('unhandledrejection', (e) => {
    const r = e.reason
    trail('error', `unhandled promise rejection: ${r?.stack ? String(r.stack).split('\n').slice(0, 6).join(' / ') : String(r)}`)
  })

  // Clicks and drags. Capture phase: recorded even when a handler stops the event.
  let down: { x: number; y: number; t: number; on: string } | null = null
  let unclicked = 0
  addEventListener('pointerdown', (e) => { down = { x: e.clientX, y: e.clientY, t: performance.now(), on: name(e.target) + at(e) } }, { capture: true, passive: true })
  addEventListener('pointerup', (e) => {
    const d = down
    down = null
    if (!d) return
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) >= 5) return trail('did', `${mods(e)}drag from ${d.on} by ${Math.round(e.clientX - d.x)},${Math.round(e.clientY - d.y)} px in ${Math.round(performance.now() - d.t)} ms`)
    // A press that ends where it began should become a click. If none follows (a disabled
    // button, or the element was redrawn under the pointer), say so: "I clicked and nothing
    // happened" is the hardest complaint to diagnose.
    if (e.button === 0) unclicked = window.setTimeout(() => trail('did', `press on ${d.on} did not become a click`), 80)
  }, { capture: true, passive: true })
  addEventListener('click', (e) => { clearTimeout(unclicked); trail('did', `${mods(e)}click ${name(e.target)}${at(e)}`) }, { capture: true, passive: true })
  addEventListener('dblclick', (e) => trail('did', `${mods(e)}double-click ${name(e.target)}${at(e)}`), { capture: true, passive: true })
  addEventListener('contextmenu', (e) => trail('did', `right-click ${name(e.target)}${at(e)}`), { capture: true, passive: true })
  addEventListener('drop', (e) => {
    const files = [...(e.dataTransfer?.files ?? [])]
    if (files.length) trail('did', `dropped ${files.length} file(s) on ${name(e.target)}: ${files.map((f) => f.name.split('.').pop()).join(', ')}`)
  }, { capture: true, passive: true })

  // Keys. In a field only the keys that end the typing are recorded, never the typing.
  addEventListener('keydown', (e) => {
    if (e.repeat || ['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return
    const field = e.target instanceof Element && e.target.matches('input, select, textarea, [contenteditable]')
    if (field && !['Enter', 'Escape', 'Tab'].includes(e.key)) return
    trail('did', `key ${mods(e)}${e.key === ' ' ? 'Space' : e.key}${field ? ` in ${name(e.target)}` : ''}`)
  }, { capture: true, passive: true })

  // The window itself.
  let hiddenAt = 0, resize = 0
  document.addEventListener('visibilitychange', () => { hiddenAt = performance.now(); trail('saw', `window ${document.visibilityState}`) })
  addEventListener('resize', () => { clearTimeout(resize); resize = window.setTimeout(() => trail('saw', `window resized to ${size()}`), 500) })

  // A freeze: a half-second timer that fires late means this page could not run meanwhile.
  // Hidden pages have their timers slowed on purpose, so those are not counted.
  let last = performance.now()
  setInterval(() => {
    const now = performance.now(), late = now - last - 500
    last = now
    if (late > 300 && document.visibilityState === 'visible' && now - hiddenAt > 2000) trail('slow', `the window froze for about ${Math.round(late)} ms`)
  }, 500)

  trail('info', `window started | ${size()} | ${navigator.hardwareConcurrency} logical cores | ${navigator.language} | ${navigator.userAgent}`)
}
