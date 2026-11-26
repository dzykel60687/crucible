// GUI smoke test: drives the REAL window program the way a hand does, and fails loudly on the
// basic things: a button that does nothing, a popup that opens empty or off screen, controls
// that jump about, Stop not stopping.
//
// Run it through tools\gui-test.ps1, which starts the window with its debugging port open.
// Mouse and keys go in through that port as real input events (a press, about 90 ms, a
// release), not as script calls, so a click is lost here exactly when a person's would be.
//
// PLATFORM: any (Node 22 or later). The window's debugging port is WebView2's on Windows; the
// same protocol is offered by WebKitGTK and WKWebView inspectors when the ports start.
import { spawn } from 'node:child_process'

const [port, freezeScript, cdFile] = process.argv.slice(2)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const results = []
const check = (name, ok, detail = '') => { results.push({ name, ok: !!ok, detail }); console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`) }

// ---------------------------------------------------------------- the debugging port
let page
for (let i = 0; i < 60 && !page; i++) {
  try { page = (await (await fetch(`http://127.0.0.1:${port}/json`)).json()).find((t) => t.type === 'page') } catch { /* not up yet */ }
  if (!page) await sleep(250)
}
if (!page) { console.log('FAIL  the window did not open its debugging port'); process.exit(2) }
const ws = new WebSocket(page.webSocketDebuggerUrl)
await new Promise((r) => ws.addEventListener('open', r))
let id = 0
const waits = new Map()
const pageErrors = []
ws.addEventListener('message', (m) => {
  const d = JSON.parse(m.data)
  if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id) }
  if (d.method === 'Runtime.exceptionThrown') pageErrors.push(d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text)
})
const send = (method, params = {}) => new Promise((r) => { const i = ++id; waits.set(i, r); ws.send(JSON.stringify({ id: i, method, params })) })
await send('Runtime.enable')
const js = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
  if (r.result?.exceptionDetails) throw new Error(`in the page: ${r.result.exceptionDetails.exception?.description ?? r.result.exceptionDetails.text}\n${expression.slice(0, 200)}`)
  return r.result?.result?.value
}
const exec = (line) => js(`window.__TAURI_INTERNALS__.invoke('exec', { line: ${JSON.stringify(line)}, since: null }).then((r) => r.replies.map((x) => ({ ok: x.ok, text: x.text ?? x.message ?? '', data: x.data ?? null, code: x.code })))`)
const status = async () => (await exec('transport.status'))[0].data
const project = async () => (await js(`window.__TAURI_INTERNALS__.invoke('exec', { line: 'project.info', since: null }).then((r) => r.state.project)`))

// ---------------------------------------------------------------- a hand
const rectOf = (sel, nth = 0) => js(`(() => { const e = document.querySelectorAll(${JSON.stringify(sel)})[${nth}]; if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height, cx: r.x + r.width / 2, cy: r.y + r.height / 2 } })()`)
const mouse = (type, x, y, extra = {}) => send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1, ...extra })
async function clickAt(x, y, hold = 90, count = 1) {
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
  await mouse('mousePressed', x, y, { clickCount: count })
  await sleep(hold)
  await mouse('mouseReleased', x, y, { clickCount: count })
}
async function click(sel, nth = 0, hold = 90) {
  const r = await rectOf(sel, nth)
  if (!r) throw new Error(`nothing matches ${sel} [${nth}]`)
  await clickAt(r.cx, r.cy, hold)
  await sleep(160)
}
async function drag(x0, y0, x1, y1) {
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x0, y: y0 })
  await mouse('mousePressed', x0, y0)
  for (let i = 1; i <= 8; i++) { await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x0 + (x1 - x0) * i / 8, y: y0 + (y1 - y0) * i / 8, button: 'left', buttons: 1 }); await sleep(15) }
  await mouse('mouseReleased', x1, y1)
  await sleep(200)
}
async function key(k, code, vk, mods = 0, text) {
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: k, code, windowsVirtualKeyCode: vk, modifiers: mods, ...(text ? { text } : {}) })
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: vk, modifiers: mods })
  await sleep(160)
}
const escape = () => key('Escape', 'Escape', 27)
const layerOpen = () => js(`document.getElementById('layer').children.length > 0`)

/** Everything a popup must be: on screen, a real size, and nothing with text squashed flat. */
const popupProblems = () => js(`(() => {
  const out = []
  for (const el of document.querySelectorAll('#layer .dialog, #layer .menu')) {
    const r = el.getBoundingClientRect()
    if (r.width < 80 || r.height < 30) out.push('tiny: ' + Math.round(r.width) + 'x' + Math.round(r.height))
    if (r.left < 0 || r.top < 0 || r.right > innerWidth + 1 || r.bottom > innerHeight + 1) out.push('partly off screen')
    for (const c of el.querySelectorAll('*')) {
      if (!c.children.length && c.textContent.trim() && c.tagName !== 'OPTION' && c.getClientRects().length) {
        const cr = c.getBoundingClientRect()
        if (cr.height < 4 || cr.width < 4) out.push('squashed: "' + c.textContent.trim().slice(0, 30) + '"')
      }
      if (c.classList && c.classList.contains('list') && c.children.length && c.getBoundingClientRect().height < 10) out.push('a list is collapsed')
    }
  }
  return out
})()`)

// The port answers before the page has loaded: wait for the window to have built itself.
for (let i = 0; i < 80; i++) {
  if (await js(`!!(window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke && document.getElementById('t-play'))`).catch(() => false)) break
  await sleep(125)
}

/** Watch the page for one action: what changed, and which commands went to the engine. */
await js(`(() => {
  window.__t = { mutations: 0, commands: [] }
  new MutationObserver((ms) => { window.__t.mutations += ms.length }).observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true })
  const real = window.__TAURI_INTERNALS__.invoke
  window.__TAURI_INTERNALS__.invoke = (cmd, args, opts) => {
    if (cmd === 'exec' && args && !/^(transport\\.status|log\\.ui|audio\\.changes)/.test(args.line)) window.__t.commands.push(args.line)
    return real(cmd, args, opts)
  }
})()`)
const reset = () => js(`(() => { window.__t.mutations = 0; window.__t.commands.length = 0 })()`)
const seen = () => js(`({ mutations: window.__t.mutations, commands: [...window.__t.commands] })`)

// ---------------------------------------------------------------- the test
await sleep(1500)
check('the window starts empty: no demo song', (await project()).tracks.length === 0)
check('the top bar names a real output device', /kHz|No audio output/.test(await js(`document.getElementById('dev-text').textContent`)), await js(`document.getElementById('dev-text').textContent`))
// The test project (five tracks, nine clips, no sound) is made by typing its command into the
// Console tab, like a person would: the window then knows about it, and the Console is tested.
const tab = async (name) => click('#panel-tabs button', await js(`[...document.querySelectorAll('#panel-tabs button')].findIndex((b) => b.textContent.trim().toLowerCase().startsWith('${name}'))`))
await tab('console')
await click('.console input')
await send('Input.insertText', { text: 'project.demo' })
await key('Enter', 'Enter', 13, 0, '\r')
await sleep(500)
check('a command typed in the Console runs and the window shows the result', await js(`document.querySelectorAll('#header-rows .track').length`) === 5, `${await js(`document.querySelectorAll('#header-rows .track').length`)} tracks on screen`)
await tab('mixer')

// 1. Mute and solo say what the engine holds.
const first = (await project()).tracks[0]
await click('#header-rows .tbtn.m')
check('Mute in the track header mutes the track', (await project()).tracks.find((t) => t.id === first.id).mute === true && await js(`document.querySelector('#header-rows .tbtn.m').classList.contains('on')`))
await click('#header-rows .tbtn.m')
check('...and a second click unmutes it', (await project()).tracks.find((t) => t.id === first.id).mute === false)

// 2. The volume slider by the track and the mixer fader are one control.
const hf = await rectOf('#header-rows .hfader')
await drag(hf.cx + 10, hf.cy, hf.cx - 25, hf.cy)
const afterSlider = (await project()).tracks[0].gain_db
check('dragging the slider in the track header changes the volume', afterSlider < -1, `${afterSlider} dB`)
const capShare = await js(`(() => { const f = document.querySelector('.strip .fader'); return 1 - Number(getComputedStyle(f).getPropertyValue('--p')) })()`)
const sliderShare = await js(`Number(getComputedStyle(document.querySelector('#header-rows .hfader')).getPropertyValue('--p'))`)
check('the mixer fader moved with it', Math.abs(capShare - sliderShare) < 0.01, `mixer ${capShare.toFixed(3)}, header ${sliderShare.toFixed(3)}`)
const fd = await rectOf('.strip .fader')
await drag(fd.cx, fd.cy, fd.cx, fd.cy - 20)
const afterFader = (await project()).tracks[0].gain_db
check('dragging the mixer fader changes the same volume', afterFader > afterSlider, `${afterSlider} to ${afterFader} dB`)
await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: hf.cx, y: hf.cy })
await mouse('mousePressed', hf.cx, hf.cy, { clickCount: 1 }); await mouse('mouseReleased', hf.cx, hf.cy, { clickCount: 1 })
await mouse('mousePressed', hf.cx, hf.cy, { clickCount: 2 }); await mouse('mouseReleased', hf.cx, hf.cy, { clickCount: 2 })
await sleep(300)
check('double-click on the slider goes back to 0 dB', (await project()).tracks[0].gain_db === 0, `${(await project()).tracks[0].gain_db} dB`)
const zeroLine = await js(`[...document.querySelectorAll('.strip')].filter((s) => /0\\.0 dB/.test(s.querySelector('.db').textContent)).map((s) => Math.round(s.querySelector('.cap').getBoundingClientRect().top))`)
check('faders at 0 dB sit at the same height on every strip', new Set(zeroLine).size <= 1, zeroLine.join(', '))

// 3. Popups: every Settings page, the project menu, export, the mark box.
await click('#gear')
check('the gear opens Settings', await layerOpen())
const pages = await js(`[...document.querySelectorAll('.settings nav button')].map((b) => b.textContent)`)
for (let i = 0; i < pages.length; i++) {
  await click('.settings nav button', i)
  await sleep(i === 0 ? 1500 : 150)   // the Audio page waits for the device list
  const problems = await popupProblems()
  const filled = await js(`document.querySelector('.settings .pane').textContent.trim().length`)
  check(`Settings page "${pages[i]}" is filled in and sound`, filled > 40 && problems.length === 0, problems.slice(0, 3).join('; ') || `${filled} characters`)
}
await click('.settings nav button', 0)
const outputs = await js(`document.querySelectorAll('.settings .pane .list')[0]?.children.length ?? 0`)
check('Settings lists the audio outputs that are connected', outputs >= 1, `${outputs} listed`)
await click('.dialog [data-close]')
check('the X closes Settings', !(await layerOpen()))
for (const [what, open, sel] of [['the project menu', () => click('#proj'), '.menu'], ['Export', () => key('E', 'KeyE', 69, 2 | 8), '.dialog'], ['the mark box (F9)', () => key('F9', 'F9', 120), '.dialog']]) {
  await open()
  const there = await js(`!!document.querySelector('#layer ${sel}')`)
  const problems = there ? await popupProblems() : ['did not open']
  check(`${what} opens and is sound`, there && problems.length === 0, problems.slice(0, 3).join('; '))
  await escape()
  check(`Escape closes ${what}`, !(await layerOpen()))
}

// 4. Play, a steady screen, Stop.
const layout = (ms) => js(`(async () => { const seen = new Set(); const t0 = performance.now(); let n = 0
  while (performance.now() - t0 < ${ms}) {
    seen.add([...document.querySelectorAll('.strip .cap, #header-rows .thumb, #topbar button, .strip')].map((e) => { const r = e.getBoundingClientRect(); return Math.round(r.left) + ':' + Math.round(r.top) }).join(' '))
    n++; await new Promise((r) => requestAnimationFrame(r)) }
  return { frames: n, layouts: seen.size } })()`)
await click('#t-play')
await sleep(800)
let st = await status()
check('Play starts playback', st.playing === true, st.playing ? `${st.driver}, ${st.device.device}` : 'not playing')
let lay = await layout(2500)
check('nothing on screen jumps about while playing', lay.layouts === 1, `${lay.layouts} different layouts in ${lay.frames} frames`)
await click('#t-play', 0, 110)
await sleep(400)
st = await status()
check('Stop stops playback, first click', st.playing === false)

// 5. The same with dropouts coming all the time: the worst case for the screen.
if (freezeScript) {
  await click('#t-play')
  await sleep(1000)
  const again = await status()
  check('Play starts playback a second time', again.playing === true)
  const before = again.dropouts ?? 0
  let said = ''
  const child = spawn('pwsh', ['-NoProfile', '-File', freezeScript, 'crucible-app', '9', '70', '250'])
  child.stdout.on('data', (d) => { said += d })
  child.stderr.on('data', (d) => { said += d })
  await sleep(2500)
  await reset()
  lay = await layout(3000)
  const during = await status()
  const rebuilt = await js(`window.__t.mutations`)
  check('dropouts were forced (the mixing thread frozen 4 times a second)', during.dropouts > before + 3, `${during.dropouts - before} dropouts${said.trim() ? '; the helper said: ' + said.trim().slice(0, 160) : ''}`)
  check('...and still nothing jumps about', lay.layouts === 1, `${lay.layouts} layouts in ${lay.frames} frames`)
  check('...the dropout count is shown in the top bar', /dropout/.test(await js(`document.getElementById('xruns').textContent`)), await js(`document.getElementById('xruns').textContent`))
  await click('#t-play', 0, 110)
  await sleep(400)
  check('...and Stop still stops, first click', (await status()).playing === false, `page changes during 3 s of dropouts: ${rebuilt}`)
  child.kill()
  if ((await status()).playing) await exec('transport.stop')
}

// 7. Every button does something when clicked the way a person clicks.
const buttons = await js(`[...document.querySelectorAll('#topbar button, #topbar .device, #topbar .field.edit, #header-rows button, #headers .snap-btn, #panel-tabs button, .strip button')].map((b, i) => { b.dataset.t = i; return { i, label: (b.id ? '#' + b.id : '') + ' ' + (b.title || b.textContent.trim() || b.className).slice(0, 40) } })`)
const dead = []
for (const b of buttons) {
  if (/t-play/.test(b.label)) continue   // played with below
  const r = await rectOf(`[data-t="${b.i}"]`)
  if (!r || r.w === 0) continue   // redrawn away by an earlier click, or not on show
  await reset()
  await clickAt(r.cx, r.cy)
  await sleep(220)
  const s = await seen()
  if (s.mutations === 0 && s.commands.length === 0) dead.push(b.label.trim())
  const problems = await popupProblems()
  if (problems.length) check(`popup after "${b.label.trim()}" is sound`, false, problems.slice(0, 3).join('; '))
  if (await layerOpen()) await escape()
  if (await layerOpen()) { check(`Escape closes the popup after "${b.label.trim()}"`, false); await js(`document.getElementById('layer').replaceChildren()`) }
}
check(`every button responds to a click (${buttons.length} tried)`, dead.length === 0, dead.join(' | '))

// 9. Ctrl+C on selected text copies the text, not clips (found by Dzyan 2026-11-25).
await tab('console')
await js(`(() => { const d = [...document.querySelectorAll('.console .out div')].pop(); const r = document.createRange(); r.selectNodeContents(d); getSelection().removeAllRanges(); getSelection().addRange(r) })()`)
await reset()
await key('c', 'KeyC', 67, 2)
check('Ctrl+C with text selected leaves the clips alone', !(await seen()).commands.some((c) => c.startsWith('edit.copy')), (await seen()).commands.join(' | '))
await js(`getSelection().removeAllRanges()`)
await tab('mixer')

// 10. A file at another rate: the window asks, and Convert places it (found by Dzyan 2026-11-25).
if (cdFile) {
  // Typed into the Console, so the window itself knows the project has a folder now.
  const folder = `${process.env.TEMP}\\crucible-gui-test-${process.pid}`
  await tab('console')
  await click('.console input')
  await send('Input.insertText', { text: `project.new --discard; track.add Gtr --kind audio; project.save "${folder}"` })
  await key('Enter', 'Enter', 13, 0, '\r')
  await sleep(800)
  await tab('mixer')
  await key('i', 'KeyI', 73, 2)
  await sleep(300)
  await send('Input.insertText', { text: cdFile })
  await key('Enter', 'Enter', 13, 0, '\r')
  await sleep(1200)
  const asked = await js(`document.querySelector('#layer .dialog h2')?.textContent ?? ''`)
  check('importing a file at another rate asks whether to convert', /Convert/.test(asked), asked)
  const problems = await popupProblems()
  check('...and that question is sound', problems.length === 0, problems.join('; '))
  const buttons = await js(`[...document.querySelectorAll('#layer footer button')].map((b) => b.textContent)`)
  const i = buttons.findIndex((b) => b.startsWith('Convert'))
  if (i >= 0) await click('#layer footer button', i)
  await sleep(1500)
  const clips = (await project()).clips
  check('Convert places the clip, converted', clips.length === 1 && clips[0].content.rate === 44100, `${clips.length} clips${clips[0] ? `, file at ${clips[0].content.rate} Hz` : ''}`)
}

// 11. Settings lists MIDI inputs (a keyboard must show up when plugged in).
await click('#gear')
await sleep(1500)
check('Settings, Audio has a MIDI inputs list', await js(`[...document.querySelectorAll('.settings h3')].some((h) => /MIDI/.test(h.textContent))`))
await escape()

// 8. Nothing went wrong inside the page itself.
check('no errors in the window\'s own code', pageErrors.length === 0, pageErrors.slice(0, 2).join(' | ').slice(0, 200))

const failed = results.filter((r) => !r.ok)
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed`)
ws.close()
process.exit(failed.length ? 1 : 0)
