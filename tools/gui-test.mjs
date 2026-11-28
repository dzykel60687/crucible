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
import { existsSync, readFileSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// The window's process id, the folder the folder picker "chooses" in this run (gui-test.ps1 puts
// it, and the file to import, in the window's environment: see `pick` in app/src-tauri/src/main.rs),
// and the window's session log.
const [port, freezeScript, cdFile, appPid, saveRoot, logFile] = process.argv.slice(2)
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

/** Watch one action: what changed on the page, and which commands went to the engine.
 *  The commands are read from the session log, which gets a line for each as it starts. Until
 *  2026-11-26 this replaced the page's `invoke` to count them, but Tauri makes that property
 *  read-only, so the replacement never took and no command was ever seen: "Ctrl+C leaves the
 *  clips alone" passed without testing anything. */
await js(`(() => {
  window.__t = { mutations: 0 }
  new MutationObserver((ms) => { window.__t.mutations += ms.length }).observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true })
})()`)
let logFrom = 0
const logSize = () => { try { return statSync(logFile).size } catch { return 0 } }
const reset = async () => { logFrom = logSize(); await js(`window.__t.mutations = 0`) }
const seen = async () => {
  let text = ''
  try { text = readFileSync(logFile).subarray(logFrom).toString('utf8') } catch { /* no log: no commands seen, and the checks that need them fail */ }
  const commands = [...text.matchAll(/ INFO +> (.*)$/gm)].map((m) => m[1].trim()).filter((c) => !/^(transport\.status|log\.ui|audio\.changes|project\.info|project\.state|project\.sync|instrument\.status)/.test(c))
  return { mutations: await js(`window.__t.mutations`), commands }
}

// ---------------------------------------------------------------- the test
await sleep(1500)
// The window remembers the folder last saved in. The test saves in a folder of its own, so it
// puts back what the person had (step 19).
const placeBefore = await js(`localStorage.getItem('crucible.place')`)
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
await reset()
await click('#header-rows .tbtn.s')
check('the test sees the commands the window sends (read from the session log)', (await seen()).commands.some((c) => c.startsWith('track.solo')), (await seen()).commands.join(' | ') || `none in ${logFile}`)
await click('#header-rows .tbtn.s')

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

/// 10 to 16. Found by Dzyan on 2026-11-26, his first long session with real songs (D-042, D-043).
const typeInConsole = async (text) => {
  await tab('console')
  await click('.console input')
  await send('Input.insertText', { text })
  await key('Enter', 'Enter', 13, 0, '\r')
  await sleep(600)
  await tab('mixer')
}
/** What the window itself holds: asked through the page, so it is the page's own state. */
const pageState = () => js(`window.__TAURI_INTERNALS__.invoke('exec', { line: 'project.info', since: null }).then((r) => ({ folder: r.state.folder, dirty: r.state.dirty, name: r.state.project.name, end: r.state.project.end ?? 0, tracks: r.state.project.tracks, clips: r.state.project.clips.length }))`)
/** Where the timeline's view is: scroll in pixels, pixels per beat, and where the song's end is drawn. */
const view = async () => { const [x, ppb, end] = (await js(`document.getElementById('timeline').dataset.view ?? ''`)).split(' ').map(Number); return { x, ppb, end } }
const wheel = async (x, y, deltaY, modifiers = 0) => { await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY, modifiers }); await sleep(120) }
const dragWith = async (x0, y0, x1, y1, modifiers = 0) => {
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x0, y: y0, modifiers })
  await mouse('mousePressed', x0, y0, { modifiers })
  for (let i = 1; i <= 8; i++) { await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: x0 + (x1 - x0) * i / 8, y: y0 + (y1 - y0) * i / 8, button: 'left', buttons: 1, modifiers }); await sleep(15) }
  await mouse('mouseReleased', x1, y1, { modifiers })
  await sleep(250)
}

if (cdFile && saveRoot) {
  // 10. Work first, save later: a project never saved takes a file, from the "Add audio" button
  // on an empty audio track, chosen in the system's file picker (the test answers for the picker).
  await typeInConsole('project.new --discard; track.add Gtr --kind audio')
  const tl = await rectOf('#timeline')
  const rowH = await js(`parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--row-h')) || 72`)
  await reset()
  await clickAt(tl.x + 14 + 62, tl.y + 30 + rowH / 2)
  await sleep(1300)
  check('"Add audio" on an empty audio track opens the file picker, with no path to type', (await seen()).commands.some((c) => c.startsWith('ui.pick_files')), (await seen()).commands.join(' | ').slice(0, 160))
  const asked = await js(`document.querySelector('#layer .dialog h2')?.textContent ?? ''`)
  check('importing a file at another rate asks whether to convert', /Convert/.test(asked), asked)
  let problems = await popupProblems()
  check('...and that question is sound', problems.length === 0, problems.join('; '))
  const answers = await js(`[...document.querySelectorAll('#layer footer button')].map((b) => b.textContent)`)
  const convert = answers.findIndex((b) => b.startsWith('Convert'))
  if (convert >= 0) await click('#layer footer button', convert)
  await sleep(1500)
  const clips = (await project()).clips
  check('Convert places the clip, converted', clips.length === 1 && clips[0].content.rate === 44100, `${clips.length} clips${clips[0] ? `, file at ${clips[0].content.rate} Hz` : ''}`)
  let st = await pageState()
  check('...in a project that was never saved: nothing asked for a folder', st.folder === null && st.dirty === true, `folder ${st.folder}`)

  // 11. The position is minutes, seconds and tenths; playback stops at the end of the song, and
  // Play from there starts over.
  const position = () => js(`document.querySelector('.position .v').textContent`)
  check('the position reads minutes:seconds.tenths', /^\d+:\d\d\.\d$/.test(await position()), await position())
  await click('#t-play')
  await sleep(700)
  const playing = await status()
  check('Play with the playhead past the end starts from the beginning', playing.playing === true && playing.seconds < 1.5, `${playing.playing ? playing.seconds.toFixed(2) + ' s' : 'not playing'}`)
  // The window finds out by itself, from its own asking; a device can take a second to start.
  for (let i = 0; i < 60 && await js(`document.getElementById('t-play').classList.contains('on')`); i++) await sleep(150)
  check('playback stops by itself at the end of the song', !(await js(`document.getElementById('t-play').classList.contains('on')`)) && (await status()).playing === false, await position())
  check('...with the playhead at the end (the file is 2.0 s long)', (await position()) === '0:02.0', await position())
  if ((await status()).playing) await exec('transport.stop')

  // 12. The end of the song can be dragged further out by its tab on the ruler.
  let v = await view()
  await dragWith(tl.x + v.end - v.x, tl.y + 22, tl.x + v.end - v.x + 220, tl.y + 22)
  st = await pageState()
  check('dragging the end tab on the ruler moves the song\'s end out', st.end > 4 * 960, `end at ${(st.end / 960).toFixed(2)} beats`)

  // 13. Getting around: wheel, Ctrl+wheel out to the whole song, Ctrl+drag, the scroll bar.
  const body = { x: tl.x + tl.w / 2, y: tl.y + 30 + rowH + 80 }
  await typeInConsole('project.end 2000')
  const before = await view()
  await wheel(body.x, body.y, 360)
  v = await view()
  check('the wheel moves through time when the tracks all fit', v.x > before.x + 100, `scroll ${before.x} to ${v.x} px`)
  for (let i = 0; i < 8; i++) await wheel(body.x, body.y, 600, 2)
  v = await view()
  check('Ctrl+wheel zooms out until the whole song is on screen', v.x === 0 && v.end > 0 && v.end <= tl.w, `the end is drawn at ${v.end} px of ${Math.round(tl.w)}, ${v.ppb} px per beat`)
  for (let i = 0; i < 3; i++) await wheel(tl.x + 40, body.y, -600, 2)
  const zoomed = await view()
  await dragWith(body.x, body.y, body.x - 160, body.y, 2)
  v = await view()
  check('Ctrl+drag on empty space drags the view', Math.abs(v.x - zoomed.x - 160) <= 4, `scroll ${zoomed.x} to ${v.x} px`)
  await dragWith(tl.x + 30, tl.y + tl.h - 5, tl.x + 30 + tl.w / 3, tl.y + tl.h - 5)
  const barred = await view()
  check('the scroll bar along the bottom moves through the song', barred.x > v.x + 200, `scroll ${v.x} to ${barred.x} px`)
  await typeInConsole('project.end 0')

  // 14. Save: a name, and a folder chosen with Browse. The audio goes along.
  await key('s', 'KeyS', 83, 2)
  await sleep(400)
  check('Ctrl+S on a project never saved asks where, with a Browse button', /Save project as/.test(await js(`document.querySelector('#layer .dialog h2')?.textContent ?? ''`)) && await js(`!!document.querySelector('#layer [data-browse]')`))
  await escape()
  check('Escape closes it, with the cursor in the name box', !(await layerOpen()))
  await key('s', 'KeyS', 83, 2)
  await sleep(400)
  problems = await popupProblems()
  check('...and that dialog is sound', problems.length === 0, problems.join('; '))
  await send('Input.insertText', { text: 'Test Song' })
  await click('#layer [data-browse]')
  await sleep(500)
  check('Browse fills in the folder chosen', (await js(`document.querySelector('#layer [data-in]')?.value ?? ''`)) === saveRoot, await js(`document.querySelector('#layer [data-in]')?.value ?? ''`))
  await click('#layer [data-go]')
  await sleep(1500)
  st = await pageState()
  check('Save makes a folder named after the project there', st.folder === `${saveRoot}\\Test Song` && st.name === 'Test Song' && st.dirty === false, `${st.name} in ${st.folder}${st.dirty ? ', still unsaved' : ''}`)
  check('...and takes the audio along', existsSync(`${saveRoot}\\Test Song\\project.json`) && existsSync(`${saveRoot}\\Test Song\\audio\\cd-44k1.wav`))

  // 15. Effects from the track itself, and an editor with sliders, typed values and ranges.
  const track1 = (await pageState()).tracks[0]
  await click('#header-rows .tbtn.fx')
  check('FX on a track with no effects opens the effect picker', /Add effect/.test(await js(`document.querySelector('#layer .dialog h2')?.textContent ?? ''`)))
  await click('#layer [data-k="echo"]')
  await sleep(700)
  const ed = await js(`(() => { const e = document.querySelector('#layer .fx-editor'), d = document.querySelector('#layer .dialog'); if (!e) return null
    return { sliders: e.querySelectorAll('input[type=range]').length, fields: e.querySelectorAll('.fxin').length, lists: e.querySelectorAll('select').length,
      ranges: [...e.querySelectorAll('.fxnum small')].every((s) => / to /.test(s.textContent)), sideways: Math.max(e.scrollWidth - e.clientWidth, d.scrollWidth - d.clientWidth),
      cut: [...e.querySelectorAll('.row > *')].filter((c) => c.getBoundingClientRect().right > e.getBoundingClientRect().right + 1).length } })()`)
  check('the effect editor gives every number a slider, a value to type and its range', !!ed && ed.sliders > 0 && ed.sliders === ed.fields && ed.ranges, JSON.stringify(ed))
  check('...and nothing in it runs off the side (Echo\'s Sync did)', !!ed && ed.sideways <= 0 && ed.cut === 0 && ed.lists >= 1, JSON.stringify(ed))
  problems = await popupProblems()
  check('...and it is sound', problems.length === 0, problems.join('; '))
  const numbers = (await exec('fx.kinds'))[0].data.find((k) => k.name === 'echo').params.filter((p) => p.control.type === 'number')
  const [typed, slid] = [numbers.find((p) => p.name === 'feedback') ?? numbers[0], numbers.find((p) => p.name === 'mix') ?? numbers.at(-1)]
  const param = async (name) => (await pageState()).tracks.find((t) => t.id === track1.id).effects[0]?.params[name]
  const want = Math.round((typed.control.min + typed.control.max) / 2) + 3
  await click(`#layer .fxin[data-p="${typed.name}"]`)
  await send('Input.insertText', { text: String(want) })
  await key('Enter', 'Enter', 13, 0, '\r')
  await sleep(500)
  check('typing a value sets it, in place, with no second popup', (await param(typed.name)) === want && (await js(`document.querySelectorAll('#layer .dialog').length`)) === 1, `${typed.label}: ${await param(typed.name)}, wanted ${want}`)
  const rail = await rectOf(`#layer input[type=range][data-p="${slid.name}"]`)
  await clickAt(rail.x + rail.w * 0.85, rail.cy)
  await sleep(500)
  const set = await param(slid.name)
  check('the slider sets it too, and the value beside it follows', set > slid.control.min + (slid.control.max - slid.control.min) * 0.7 && Number(await js(`document.querySelector('#layer .fxin[data-p="${slid.name}"]').value`)) === set, `${slid.label}: ${set}`)
  await click('#layer footer [data-close]')
  await click('#header-rows .tbtn.fx')
  check('FX on a track with effects lists them', /Echo/.test(await js(`document.querySelector('#layer .menu')?.textContent ?? ''`)))
  await escape()
}

// 17. Settings lists MIDI inputs (a keyboard must show up when plugged in).
await click('#gear')
await sleep(1500)
check('Settings, Audio has a MIDI inputs list', await js(`[...document.querySelectorAll('.settings h3')].some((h) => /MIDI/.test(h.textContent))`))
await escape()

// 20. Instruments (D-044). The launcher gave this window a plugin folder of its own holding the
// test instrument (crates/crucible-test-synth: a real VST3 plugin, loaded like any other) and a
// plugin list of its own, so this is the first scan on this "machine".
{
  const said = (title) => new Promise((done) => {
    let out = ''
    const p = spawn('pwsh', ['-NoProfile', '-File', fileURLToPath(new URL('./close-window.ps1', import.meta.url)), title])
    p.stdout.on('data', (d) => { out += d })
    p.on('close', () => done(out))
  })
  const inst = async () => (await exec('instrument.status'))[0].data
  const mine = async () => (await inst()).instruments.at(-1)
  const until = async (ok, tries = 60) => { let v; for (let i = 0; i < tries; i++) { v = await mine(); if (ok(v)) break; await sleep(250) } return v }
  const last = (sel) => js(`document.querySelectorAll(${JSON.stringify(sel)}).length - 1`)
  const menuItem = async (text) => click('#layer .menu button', await js(`[...document.querySelectorAll('#layer .menu button')].findIndex((b) => b.textContent.includes(${JSON.stringify(text)}))`))

  await reset()
  await click('[data-add="instrument"]')
  let names = []
  for (let i = 0; i < 80 && !names.length; i++) { await sleep(250); names = await js(`[...document.querySelectorAll('#layer .inst-pick [data-id] b')].map((b) => b.textContent)`) }
  check('adding an instrument track opens the picker, which finds the plugins installed', names.includes('Crucible Test Tone'), names.join(', ') || await js(`document.querySelector('#layer .inst-pick')?.textContent.slice(0, 160) ?? 'no picker'`))
  let problems = await popupProblems()
  check('...and the picker is sound, with Scan again and Add a folder', problems.length === 0 && await js(`['#pl-scan', '#pl-folder'].every((s) => document.querySelector(s))`), problems.join('; '))
  await click('#pl-find')
  await send('Input.insertText', { text: 'zzz' })
  await sleep(250)
  const none = await js(`document.querySelectorAll('#layer .inst-pick [data-id]').length`)
  await key('a', 'KeyA', 65, 2)
  await key('Backspace', 'Backspace', 8)
  await send('Input.insertText', { text: 'tone' })
  await sleep(250)
  check('...and its search narrows the list', none === 0 && await js(`document.querySelectorAll('#layer .inst-pick [data-id]').length`) === 1, `${none} for "zzz"`)
  await click('#layer .inst-pick [data-id]')
  let me = await until((v) => v?.state === 'loaded' && v.editor)
  check('choosing a plugin loads it on the track', me?.state === 'loaded' && me.plugin?.name === 'Crucible Test Tone', JSON.stringify(me ?? null).slice(0, 220))
  check('...the keyboards play it', me?.live === true)
  check('...and its own window opens', me?.editor === true)
  const s = await seen()
  check('...all through the engine\'s commands', ['track.add', 'plugin.scan', 'instrument.set', 'instrument.show'].every((c) => s.commands.some((x) => x.startsWith(c))), s.commands.join(' | ').slice(0, 300))
  await sleep(400)
  check('the track shows its instrument and a lit keyboard button', await js(`(() => { const row = [...document.querySelectorAll('#header-rows .track')].at(-1); return row.querySelector('.tinst')?.textContent === 'Crucible Test Tone' && !!row.querySelector('.tbtn.k.on') })()`), await js(`[...document.querySelectorAll('#header-rows .track')].at(-1).querySelector('.row1').textContent`))

  // The plugin's window is a real one: found by its title and closed the way its own X closes it.
  const closed = await said('Crucible Test Tone - VST3')
  me = await until((v) => v && !v.editor, 30)
  check('the plugin\'s window is a real window, and closing it is noticed', /closed/.test(closed) && me?.editor === false, closed.trim() || 'no such window')
  check('...and it floats above the main window (it has it as its owner)', /had an owner/.test(closed), closed.trim())

  // A key at velocity 1 (56 dB below full scale: nothing to hear) answers through the device.
  if (me) {
    await inst()
    await exec(`instrument.note #${me.track} 60 --velocity 1`)
    await sleep(500)
    const st = await inst()
    await exec(`instrument.note #${me.track} 60 --off`)
    const pk = st.instruments.at(-1).peak
    check('a key makes the instrument sound through the audio device', st.output.open && pk && pk[0] !== null && pk[0] < -40 && pk[0] > -70, JSON.stringify({ output: st.output, peak: pk, silent: st.silent }).slice(0, 260))
    await sleep(300)
    const pk2 = (await inst()).instruments.at(-1).peak
    check('...and stops when the key comes up', pk2 && pk2[0] === null, JSON.stringify(pk2))
  }

  // An export takes the instrument away from the live sound, tells it this is a render, and
  // gives it back afterwards: the file holds the clip's note, and a key still sounds after.
  if (me) {
    const out = `${saveRoot}\\instrument.wav`.replaceAll('\\', '/')
    // Soloed, so the file holds this track alone: one key at velocity 100 is 0.25 * 100 / 127 of
    // full scale, -14.1 dBFS.
    const made = await exec(`clip.add #${me.track} 0 2; note.add ${(await project()).next_clip_id} 64 0 1; track.solo #${me.track} on; project.export "${out}" --bits 32f --end 2`)
    const exported = made.at(-1)
    await exec(`track.solo #${me.track} off`)
    check('an export holds the notes of a track whose instrument is loaded, at the level the plugin made', exported?.ok && Math.abs(exported.data.peak_dbfs + 14.1) < 0.2, (exported?.text ?? JSON.stringify(made.at(-1) ?? null)).slice(0, 200))
    await inst()
    await exec(`instrument.note #${me.track} 60 --velocity 1`)
    await sleep(500)
    const after = await inst()
    await exec(`instrument.note #${me.track} 60 --off`)
    const pk = after.instruments.at(-1).peak
    check('...and the instrument plays live again afterwards', pk && pk[0] !== null && pk[0] < -40, JSON.stringify({ me: after.instruments.at(-1), out: after.output, silent: after.silent }).slice(0, 400))
    const mineNow = after.instruments.at(-1)
    check('hardly any of its blocks were lost to the plugin being busy', mineNow.blocks > 200 && mineNow.busy_blocks + mineNow.failed_blocks <= Math.max(4, mineNow.blocks / 200), `${mineNow.busy_blocks} busy and ${mineNow.failed_blocks} failed of ${mineNow.blocks}`)
  }

  await click('#header-rows .tinst', await last('#header-rows .tinst'))
  const items = await js(`document.querySelector('#layer .menu')?.textContent ?? ''`)
  check('the instrument button offers its window, a test note, the keyboard and removal', ['Open its window', 'test note', 'Keyboard and channel', 'Change instrument', 'Remove instrument'].every((t) => items.includes(t)), items.slice(0, 200))
  await menuItem('Keyboard and channel')
  problems = await popupProblems()
  check('the keyboard question is sound and offers all keyboards and all channels', problems.length === 0 && await js(`document.querySelector('#kb-name')?.options[0].textContent === 'All keyboards' && document.querySelector('#kb-ch')?.options.length === 17`), problems.join('; '))
  await escape()

  await click('#header-rows .tbtn.k', await last('#header-rows .tbtn.k'))
  me = await until((v) => v && !v.live, 20)
  const lit = await js(`!![...document.querySelectorAll('#header-rows .track')].at(-1).querySelector('.tbtn.k.on')`)
  await click('#header-rows .tbtn.k', await last('#header-rows .tbtn.k'))
  const back = await until((v) => v?.live, 20)
  check('the keyboard button takes the keyboards off the track and gives them back', me?.live === false && !lit && back?.live === true)

  await click('#header-rows .tinst', await last('#header-rows .tinst'))
  await menuItem('Remove instrument')
  me = await until((v) => v?.state === 'none', 20)
  await sleep(300)
  const asks = await js(`[...document.querySelectorAll('#header-rows .track')].at(-1).querySelector('.tinst')?.textContent`)
  await click('#timeline')
  await key('z', 'KeyZ', 90, 2)
  const again = await until((v) => v?.state === 'loaded', 40)
  check('removing the instrument and undoing it brings it back, loaded', me?.state === 'none' && /Choose instrument/.test(asks ?? '') && again?.state === 'loaded', `${me?.state} | ${asks} | ${again?.state}`)
}

// 8. Nothing went wrong inside the page itself.
check('no errors in the window\'s own code', pageErrors.length === 0, pageErrors.slice(0, 2).join(' | ').slice(0, 200))

// 18. Browse shows the system's own folder picker. Everywhere else in this test the picker is
// answered for (see the top); here the real one is asked for, found by its title, and closed.
{
  const title = `Crucible GUI test ${process.pid}`
  const reply = exec(`ui.pick_folder --ask --title "${title}"`)
  let said = ''
  const closer = spawn('pwsh', ['-NoProfile', '-File', fileURLToPath(new URL('./close-window.ps1', import.meta.url)), title])
  closer.stdout.on('data', (d) => { said += d })
  closer.stderr.on('data', (d) => { said += d })
  const answer = await Promise.race([reply, sleep(20000).then(() => null)])
  check('Browse shows the system\'s own folder picker (closed again by the test)', /closed/.test(said) && answer?.[0]?.ok === true && answer[0].data?.folder == null, said.trim().slice(0, 160) || 'the picker never answered')
}

// 19. Closing the window with unsaved changes asks first; "Don't save" then closes it (D-042).
// Last, because the window is gone afterwards. The close is asked for the way the title bar's X
// asks: from outside the page.
await js(`(() => { const was = ${JSON.stringify(placeBefore ?? null)}; if (was === null || /crucible-gui-test-/.test(was)) localStorage.removeItem('crucible.place'); else localStorage.setItem('crucible.place', was) })()`)
if (appPid) {
  const alive = () => { try { process.kill(Number(appPid), 0); return true } catch { return false } }
  await typeInConsole('track.add Unsaved')
  spawn('pwsh', ['-NoProfile', '-Command', `[void](Get-Process -Id ${Number(appPid)}).CloseMainWindow()`])
  let asked = ''
  for (let i = 0; i < 40 && !/Save changes/.test(asked); i++) { await sleep(150); asked = await js(`document.querySelector('#layer .dialog h2')?.textContent ?? ''`).catch(() => '') }
  check('closing the window with unsaved changes asks first', /Save changes/.test(asked) && alive(), asked || (alive() ? 'no question' : 'the window just closed'))
  if (/Save changes/.test(asked)) {
    const problems = await popupProblems()
    check('...and that question is sound', problems.length === 0, problems.join('; '))
    const answers = await js(`[...document.querySelectorAll('#layer footer button')].map((b) => b.textContent)`)
    check('...with Save, Don\'t save and Cancel', ['Cancel', 'Save'].every((a) => answers.includes(a)) && answers.some((a) => a.startsWith('Don')), answers.join(' | '))
    await click('#layer footer button', answers.findIndex((a) => a.startsWith('Don')))
    for (let i = 0; i < 50 && alive(); i++) await sleep(150)
    check('"Don\'t save" closes the window', !alive())
  }
}

const failed = results.filter((r) => !r.ok)
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed`)
ws.close()
process.exit(failed.length ? 1 : 0)
