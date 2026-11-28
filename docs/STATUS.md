# STATUS: living handoff

Update this file at the end of every session. Keep it short. Newest log entry on top.

## Where things stand (as of 2026-11-27)

- **Phase:** M0's question (does plugin hosting work with real plugins on Windows?) is now
  answered by the window itself: instruments were built ahead of it at Dzyan's request (D-044).
  **No real plugin has been loaded yet.** His home desktop (UMC202HD, Keyscape, free instruments
  to come) is where it is first tried; nothing is tailored to Omnisphere or Keyscape (his word).
- **Instruments and MIDI keyboards (D-044, 2026-11-27, in 0.0.4, not committed):** the picker
  finds the VST3 instruments installed (scans by itself, then looks for new ones each time it
  opens; "Add a folder" for others); an instrument track loads one, opens its window floating
  above Crucible's, and the keyboards play it live, mixed in the audio callback; a keyboard
  button per track moves the keyboards between tracks; a track can listen to one keyboard or
  channel; clips' notes play through it and into exports; its settings are saved in the project
  (`plugin-state/`, file format 6) and survive undo; a plugin missing on a PC is marked and its
  settings kept. Tested against a real VST3 plugin built for the tests (`crucible-test-synth`).
  Limits: one lock between a plugin's sound and its settings (a big instrument's save or window
  can cost it a few ms of silence, counted in the log), loading freezes the window briefly, the
  device stays held while an instrument is loaded, no MIDI recording, no VST3 effects, no delay
  compensation.
- **WASAPI exclusive mode works** (timing verified on the dev laptop's Realtek output, 3 ms
  buffer, zero dropouts, silence only). The step-B program can use it with `--exclusive`.
- **Named Crucible** (D-023). **Command layer built** (D-022, decided): every action is a command;
  `crucible` runs them one at a time, chained, from scripts, in a shell, or as JSON. Project
  save/load, undo/redo and crash-journal recovery work.
- **First window built** (D-025): `app/` → `crucible-app.exe`, Tauri 2 + plain TypeScript.
  Dzyan: "GUI looks pretty good." Real engine for project, track, **clip and note** commands
  (D-027: clips moved into the model early so mouse editing is undoable and journaled). Five
  **layout themes** (D-028). **Bundled fonts** Inter and JetBrains Mono (D-026).
- **The window shows only what works** (D-038, 2026-11-25): no demo song, no stand-in buttons
  (record, loop, metronome, loudness, mono, A/B, arm, plugin picker are gone until built). It
  starts empty. Each track header has a volume slider, the same control as the mixer fader.
- **Audio devices are real** (D-038): Settings, Audio lists the outputs and inputs Windows has,
  the rate each runs at, which other programs are using them, and sets output, driver and buffer.
  Plugging in and out is noticed. **Not yet tried with a real device being plugged in.**
- **`tools/gui-test.ps1` tests the real window by machine**: 34 checks, all passing on the dev
  laptop. Run it after every change to the window.
- **Audio files and offline export work** (D-031, built overnight 2026-11-22 at Dzyan's request):
  `audio.import` (WAV copied, MP3/FLAC decoded once), `clip.place`, real waveforms (`audio.peaks`,
  cached in `cache/peaks/`), `project.export` (16/24/32f WAV, gain, balance pan D-030, mute, solo,
  master), all from the window too (Ctrl+I, drop files on the window, Ctrl+Shift+E). The
  04-audio-quality machine tests pass.
- **Live playback works, effects included** (D-034, 2026-11-23, at Dzyan's request before M0
  passed): `transport.play/stop/seek/status`, the window's Play, playhead and meters. A feeder
  thread renders about 40 ms ahead; the callback only copies. Playback equals the export bit for
  bit (tested); edits during playback keep reverb and echo tails. Ran with 0 dropouts on the dev
  laptop (exclusive, 3 ms buffer). **Not yet heard by anyone.** No record, loop or metronome.
- **ASIO is compiled in** (D-035, decided): cpal's `asio` feature. The SDK's host files are in the repo
  (`third-party/asiosdk`); LLVM's `libclang.dll` sits beside the repo in `..\crucible-sdk`. One `open_output` picks ASIO, else exclusive, else shared (D-018).
  **No ASIO stream has ever run**: this laptop has no ASIO driver. The Lynx is the first test.
- **Released: 0.0.3** on GitHub (`v0.0.3`, pre-release, 2026-11-25): per-user installer
  (`%LOCALAPPDATA%\Programs\Crucible`, no admin, the log beside the program), unsigned, never goes
  online (D-041). Never installed on any PC by Claude. **Dzyan rejected the per-user install
  2026-11-26** (he moved it to Program Files and it failed: a per-user installer cannot write there).
- **0.0.4, built, not yet committed or released (2026-11-26):** from his second session, with two
  whole songs at 192 kHz, and a failed install.
  - Installer: per machine again, `C:\Program Files\Crucible`, asks for admin; the log is in a
    `logs` folder there that Users may write (D-037, part three). A 0.0.3 install is removed by
    hand first.
  - Work first, save later (D-042): an unsaved project takes files; New, Open and closing ask
    Save / Don't save / Cancel; Save as has a name, a folder and Browse; Import and the "Add
    audio" button on an empty audio track open Explorer's file picker; **Save as now takes the
    audio along** (it never did); saving and importing no longer stop playback.
  - Timeline (D-043): wheel and scroll bar through time, Ctrl+wheel out to the whole song,
    Ctrl+drag (empty space) or middle button drags the view; the song has an end where playback
    stops, with a tab to drag it out (`project.end`, file format 5); position reads `m:ss.t`.
  - Effects (D-043): an FX button on each track; the editor has a slider, a typed value and the
    range for every number, and nothing runs off the side.
  - Memory (D-039): sound no clip uses is let go (his two songs held 1.1 GB after removal).
  - The GUI test's command watching never worked (Tauri's `invoke` is read-only); it now reads
    the session log. 69 checks, 81 unit tests pass. Carries the new logo and green.
- **0.0.3 (2026-11-25, from his home-desktop test with a Behringer UMC202HD):** imported files stay
  as they are, MP3/FLAC and other rates play from memory (D-039); conversion offered instead of
  refused; ASIO finds the interface under its driver's name (D-040); MIDI keyboards listed; Ctrl+C
  copies selected text; the log records everything while testing, a line every second; the logo
  is an anvil (side on, after his reference picture); nothing goes online (D-041).
- **Since 0.0.3, on `main` (2026-11-26):** the logo is his pick, the anvil with spectrum sparks
  and a note (D-029); the accent is Excel's green `#21a366`. In the 0.0.4 installer.
- **Built-in effects** (D-033, 2026-11-23): Reverb (Room/Hall/Plate), IR Reverb (zero-latency
  convolution with his IR files), Delay and Echo, as track inserts edited by `fx.*` commands and
  from the mixer slots. Machine-tested (`crucible-dsp`); not yet heard. CPU budget: quality
  wins over CPU (decided).
- **Edits are fast on big projects** (D-032): change lists instead of the whole model, copy-on-write
  clips, commands queued in order and run off the window's main thread.
- **Licence: GPL-3.0-or-later** (D-036, decided 2026-11-23): "totally open software". `LICENSE` at
  the root, installed as `LICENSE.txt`. All 269 third-party crates in the Windows build are GPLv3-compatible (counted 2026-11-26); their
  notices are in `THIRD-PARTY-NOTICES.txt` (`tools/third-party-notices.ps1` regenerates it). The
  ASIO SDK is used under its GPLv3 option, so nothing needs signing.
- **Git:** **public** repo (since 2026-11-23, at his word) `github.com/dzykel60687/crucible`, branch `main`, commits under
  `dzykel60687@users.noreply.github.com` (set in this repo only, not globally).
- **Build philosophy recorded** (D-024): small, fast, robust; budgets in `01-vision-and-priorities.md`;
  GUI motion rules and the pushback rule in `05-gui-design-brief.md`.
- **Diagnostics in place** (D-021, `docs/11-error-codes.md`): coded errors with help text, a
  crash-proof log per run, native crash capture with module name and minidump. Crash path verified
  with a fake crashing plugin.
- **The log is a full record of a session** (D-037): every command with its outcome and timing,
  playback health and every dropout with its cause, the devices and their changes, what was
  clicked and shown in the window, his own marks (F9), the commit of the build. It is **one file**,
  `crucible-app.log`, started afresh each run, the run before kept as `.previous.log`: in
  `C:\Program Files\Crucible\logs` for the installed program (0.0.4 on), beside the program for a build. **Settings, Diagnostics, Create report file** writes one file to the Desktop.
  Not yet used to diagnose a real problem from another PC.
- **Built and staged:** `dist/m0/crucible.exe` and `dist/m0/m0-play.exe`, release builds with the C
  runtime linked in, so they copy to any Windows PC. Their `.pdb` files sit beside them so crash
  and panic stacks name source lines. `dist/` is not in git; rebuild with the
  commands below.
- **Dzyan's next step:** run `docs/test-reports/2026-11-20-M0-stepB-checklist.md` on the Lynx machine.

## Blockers that need Dzyan

| # | Blocker | His action |
|---|---|---|
| 1 | **Install 0.0.4 on the home desktop** | Commit, push and release 0.0.4: yes or no. Then uninstall any old Crucible (Apps list) and install, keeping the folder it offers |
| 2 | **Home desktop: 2026-11-26 changes, ASIO on the UMC202HD, then instruments with Keyscape, free instruments and the Axiom** | Close Reaper. `docs/test-reports/2026-11-26-second-session-changes.md`, `2026-11-25-home-desktop-umc202hd.md`, then `2026-11-27-instruments-and-keyboard.md`; then Settings, Diagnostics, Create report file, and send it |
| 2a | **Three choices** (Q14 to Q16) | Ruler in minutes or bars; Ctrl+drag everywhere or empty space only; convert audio while playing (4x less memory) |
| 3 | **The export that "plays no sound"** | Does `Crucible.wav` play in Reaper, or with Reaper closed? Its peak was +1.15 dBFS, so the file is not silent |
| 4 | **Listen: playback and effects** | `docs/test-reports/2026-11-23-devlaptop-live-playback-and-asio.md` steps 1 to 5 |
| 5 | **Lynx machine: ASIO, and Omnisphere when it is there** | Same report steps 6 and 7; then the instruments sheet there. The old `m0-play` checklist is optional now |
| 6 | **Default driver: keep ASIO, exclusive, shared; or Shared first?** | Shared never silences other programs but is not bit-exact (D-038) |
| 7 | **"GPLv3 or later", or version 3 only?** | Claude chose "or later" (D-036) |
| 8 | **Proposed decisions** | D-030 to D-034, D-037, D-038; Q7 standing approval to commit and push |
| 9 | **Rewrite the git history?** | An old wording he may want gone (asked 2026-11-23). Needs a force-push. Yes or no |
| 10 | **Remove 0.0.1 and 0.0.2 from the releases page?** | They have the Stop fault and the import bloat. Yes or no |

Also open, lower: develop on the Lynx machine long term (Q2); a free VST3 instrument on the dev
machine (Q6); GUI feel of mouse editing, the five layouts and the piano roll.

## Next actions, in order

1. Wait for Dzyan's results with real plugins and the Axiom (blockers 1 and 2). Fix what fails.
   Record results in `07-hardware-and-testing.md` (the plugin table) and the outcome of claims
   T3/T4 in `09-open-questions.md`. If he hears the lock's gaps, start the processor/editor split
   (D-044).
1a. Instruments, next in M3: record what the keyboard plays into a MIDI clip; plugin delay
   compensation; then VST3 effects as inserts (M4).
2. Playback follow-ups that need no hardware: loop playback; send effect-slider drags as they move
   (today one command on release) with undo grouping; hold the device open between stop and play.
   From 2026-11-26, each waiting on his answer or a later milestone: hold converted audio at the
   file's own rate and convert while playing (Q16, about four times less memory at 192 kHz); a
   ruler in minutes and seconds (Q14); crash recovery for a project never saved (D-042; a must
   once recording exists); draw the song's end where the effects' tails end, not at the last clip.
3. After his first listen: the quality-for-CPU candidates in D-033 (modulate all 16 reverb lines,
   oversample the tape saturation), judged by ear.
4. Record (audio input, latency-compensated) and the metronome: M2's remaining transport work.
5. Input monitoring renders inside the callback, as instruments now do (D-044).

6. For every later release, in this order: bump `version` in `Cargo.toml`,
   `app/src-tauri/tauri.conf.json`, `app/package.json` and `app/package-lock.json`; re-run
   `tools/third-party-notices.ps1` if dependencies changed; scan for employer and personal data;
   **commit**; build (so the log headers name the commit); run `tools/gui-test.ps1` on the build;
   tag `v<version>`, push; create the GitHub release with the `.msi`; check the anonymous download.
   Crucible never checks for updates itself (D-041): a newer installer replaces the older version.
7. Logging follow-ups (D-037): record and MIDI input will need their own `AUDIO`-style lines;
   `crucible report` should say the power plan and whether the laptop is on battery.
8. GUI test follow-ups (D-038): checks for the timeline (clip drag, trim, box select), the piano
   roll, the effect editor and the five layouts; a run at 125% and 150% display scale.
9. Controls that left the window come back with their engine side: record and arm, loop,
   metronome, loudness, mono, reference A/B, instrument and plugin pickers, MIDI settings.

Instruments are in the engine (D-044) though no real plugin has been tried. Every new feature
starts as a command row in a table, then gets its GUI.

## How to build and test

**Build machine needs** (D-035), besides Rust and Node.js: LLVM's `libclang.dll`, by default in
`..\crucible-sdk\llvm\LLVM\bin` beside the repo (not in git; any LLVM install works). The ASIO SDK's host files
are in the repo, `third-party/asiosdk`. `.cargo/config.toml` points at both; the `CPAL_ASIO_DIR`
and `LIBCLANG_PATH` environment variables override. Without libclang the build fails in
`asio-sys`. The downloaded zips are kept in `..\crucible-sdk\downloads`.

Installer (WiX is fetched by Tauri on first use; per machine, except 0.0.3 which was per user): `cargo build --release -p crucible-cli`, then
`cd app && npx tauri build --config src-tauri/tauri.bundle.json` writes
`target/release/bundle/msi/Crucible_<version>_x64_en-US.msi`. It installs `crucible-app.exe`, and the
`crucible.exe` command-line tool in a `tools` folder, to Program Files, adds a Start menu entry and an uninstaller, and
installs WebView2 only if Windows lacks it. Bump `version` in `app/src-tauri/tauri.conf.json` for
every installer you hand out, or Windows will not upgrade the old one in place. Unsigned for now:
SmartScreen says "unknown publisher" (More info, Run anyway).


```bash
cargo build --release          # binaries land in target/release
cargo test                     # unit tests, including scanner crash/timeout simulation
cargo clippy --all-targets     # must stay warning-free
```

Stage for Dzyan: copy `crucible.exe`, `crucible.pdb`, `m0-play.exe` and `m0_play.pdb` from
`target/release` into `dist/m0/`.

The window (`app/`, needs `npm install` once):

```bash
cd app && npx tauri dev                 # live-reloading window
cd app && npx tauri build --no-bundle   # target/release/crucible-app.exe
cd app && npm run dev                   # UI in a browser at :1420, driving target/debug/crucible.exe
```

In a browser, the dev-only Vite bridge (`app/vite.config.ts`) runs `crucible --json shell`, so the UI
talks to the real engine. `window.__crucible.perf` holds measured frame cost in dev builds.

Test audio: `dist/test-audio/make_test_audio.py` (stems, a 44.1 kHz file, a 10-minute file) and
`to-mp3.ps1` (Windows' own MP3 encoder). `build-demo-project.crucible` there rebuilds the demo
project and its exports: `..\m0\crucible.exe run build-demo-project.crucible`.

Play from the command line: `crucible --project <folder> -c "transport.play --wait"` (add
`--driver shared` to leave the device free for other programs). `audio.output` shows or sets this
machine's driver, device and buffer.

Try the engine without hardware: `target/debug/crucible.exe -c "project.new Demo; track.add Piano; track.list"`,
or `crucible shell`. Add `--json` for machine-readable output.

After adding an error code: `cargo run -p crucible-cli -- codes --markdown` and paste the table into
`docs/11-error-codes.md` (the `every_code_is_documented` test fails until you do).

## Code map

| Path | What | Verified how |
|---|---|---|
| `crates/crucible-audio-io` | SEAM 1. `wasapi_endpoints.rs`: the device list with rates and users, plug detection. Lists audio devices in full via cpal (`audio.probe`). Exclusive mode via `wasapi`: capability probe, timing test, output stream. `output.rs`: one `open_output` for ASIO and shared mode (cpal) and exclusive. `convert.rs` float-to-device conversion (bit-exact with import) | Ran on dev workstation. Conversion unit-tested. Exclusive and shared streams played the demo song with 0 dropouts. **ASIO stream never run** (no driver here) |
| `crates/crucible-midi-io` | SEAM 1. Lists MIDI inputs and opens them (`connect`) via midir | Ran; this machine has none |
| `crates/crucible-plugin-host` | Finds `.vst3` bundles, probes each in a child process with a timeout. `catalog.rs`: the plugin list kept between runs, scans only what is new. `instance.rs`: one loaded plugin as a control half and an audio half (D-044). `plugin_paths` and `editor_window` are SEAM 2 | Unit tests incl. simulated crash/hang; the test instrument loaded, played, saved, restored, made busy |
| `crates/crucible-test-synth` | A real VST3 instrument for tests only (sine per key, one setting, sustain pedal, a window). Never shipped | Through the host tests and the GUI test |
| `crates/crucible-diagnostics` | Error codes (`codes.rs`), session log (`session.rs`), the build's commit (`build.rs`), machine facts (`system.rs`), Windows crash handler and minidumps (`platform_windows.rs`, the crate's only OS code) | Unit tests, including a child process that really crashes. End-to-end with a fake crashing VST3 (see session log) |
| `crates/crucible-project` | Project model as plain data; atomic save; load with validation; crash journal | Unit tests: round trip, foreign folder refused, newer format refused, damaged file refused, journal |
| `crates/crucible-engine` | `command.rs`: command tables, parser, canonical text form. `lib.rs`: project and track commands, undo, redo, history, journal replay, `sync` change lists. `clips.rs`: clip, note, clipboard commands. `audio.rs`: import, place, peaks, mixer, `render`, export. `fx.rs`: effect commands. `play.rs`: live playback (feeder thread, queue, callback), `transport.*`, `audio.output`, the `AUDIO` log lines. `trail.rs`: command outcomes in the log, `mark`, `log.ui`, `log.snapshot` (D-037). `devices.rs`: `audio.devices`, `audio.changes` (D-038). `rack.rs`: instruments and keyboards (D-044): `plugin.*`, `instrument.*`, `track.live`, `track.input`; `play.rs` `Stage` mixes them in the device callback | Unit tests (42), CLI, browser, real devices |
| `crates/crucible-dsp` | Built-in effects: `reverb.rs` (FDN), `convolve.rs` (IR Reverb), `delay.rs` (Delay/Echo), `filter.rs`, `math.rs` (deterministic exp/ln/sin), parameter tables in `lib.rs` | 15 unit tests (decay per band, convolution against direct, timing, no allocation, determinism, block-size independence including going idle, tail hand-over); CPU bench with `--ignored` |
| `crates/crucible-cli` | The `crucible` binary. Host commands (help, version, codes, logs, report, diag, audio.devices, audio.test, midi.inputs, plugin.scan, run) joined with the engine's. Modes: one command, `-c`, `run`, `shell`, `--json`, `--project` | Ran on dev workstation, including each error path |
| `app/src-tauri` | The window `crucible-app.exe`: Tauri 2 shell, one `exec` command, help for engine commands, the `report` command (starts `crucible.exe report`), a watchdog for slow commands, window events into the log | Built release; driven through WebView2's debugging port with real clicks and keys |
| `app/src` | UI: `main.ts` (top bar, headers, keys, import), `timeline.ts`, `panels.ts` (mixer, piano roll, clip editor, console), `dialogs.ts` (export, settings), `effects.ts` (effect picker and editor), `store.ts`, `engine.ts` (the bridge, change-list merge, command queue), `devices.ts` (the device list, plug notices, the Audio page of Settings), `trail.ts` (clicks, keys, messages and errors into the log, D-037), `transport.ts` (play, stop, seek, playhead and meters from `transport.status`), `wave.ts` (waveforms), `layout.ts`, `demo.ts` (stand-ins) | Driven in the browser pane against the real engine |
| `spikes/m0-omnisphere` | THROWAWAY `m0-play`: load a plugin, editor window, live MIDI, shared or `--exclusive` output, state save/restore, offline render | Error paths only. **No real plugin yet** |

## Standing approvals

None standing for commits or pushes. One-off, 2026-11-26: commit and push the logo and the green
("Go ahead and push afterwards"; no release). One-off, 2026-11-25: commit, push and release `v0.0.2`
("go and push this and installer up to Git"). One-off, 2026-11-23: commit, push, make the repo public and
publish release `v0.0.1` with the installer ("Go ahead and push it along with installer to public
repo"). Downloads approved 2026-11-23: the ASIO SDK, LLVM and WiX
(done, see `10-references.md`); anything else still needs his yes. 2026-11-20 Dzyan asked to push the project to his personal repo; that covered the
first push and that session's GUI and rename work. 2026-11-22 "go ahead and push for now" covered
the 2026-11-21 batch (mouse editing, layouts, fonts, green, research). Ask before later pushes.

## Environment facts (dev workstation, checked 2026-11-19/20)

| Item | State |
|---|---|
| Machine | Lenovo laptop (Dzyan's work machine), Windows 11 Pro for Workstations, build 26200 |
| CPU / RAM | Xeon W-11855M, 64 GB |
| Rust | 1.100.0 stable, MSVC target. Installed by Dzyan 2026-11-19 |
| Git / Node / npm | 2.53 / 24.14 / 11.9 |
| MSVC build tools | Visual Studio Build Tools 2026, C++ toolset present |
| WebView2 runtime | 157.x |
| CMake | not installed |
| LLVM 23.1.6 | unpacked (not installed) in `..\crucible-sdk\llvm`, 2026-11-23 |
| ASIO SDK 2.3.4 | host part in the repo (`third-party/asiosdk`); the full SDK is unpacked in `..\crucible-sdk\asio`, unused by the build |
| WiX 3.14.1 | in `%LOCALAPPDATA%\tauri`, fetched by Tauri 2026-11-23 |
| VST3 plugins | none installed (the test instrument is built in `target`, laid out in a temp folder by the tests) |
| Audio | Realtek built-in, Jabra headset (default), Thunderbolt dock. No ASIO |
| MIDI | none |

Related: `..\music-editor` is Dzyan's own MIT Rust audio editor with reusable DSP and loudness code.

## Session log

### 2026-11-27, instruments and MIDI keyboards before M0 (Claude Opus 5.5 and Fable 5.2)

He asked why the basic MIDI and VST parts could not be built yet: plug in the keyboard and use it
on any track; browse for and add VST instruments, found on the PC by themselves. Mid-turn: the
test PC is the home desktop with the UMC and only Keyscape; he will try free instruments; do not
tailor it to Omnisphere or Keyscape. Built as D-044 (Q17 answered).

- **Test instrument** `crates/crucible-test-synth` (real VST3, `vst3` crate), so hosting is tested
  by machine on a laptop with no plugins.
- **Host**: `catalog.rs` (scan, kept list, new-only rescans, extra folders), `instance.rs`
  (control half on the main thread, audio half that tries 0.15 ms for the plugin then gives the
  block up, keeping its keys: a first version dropped them, found while chasing one flaky GUI
  run), `editor_window.rs` (owner window).
- **Engine**: `rack.rs` (plugins follow the project after every command; settings kept across
  removal for undo; save writes `plugin-state/`; keyboards opened while an instrument is loaded;
  `track.live`, `track.input`), project format 6 (`Track::instrument`, `input`, `input_channel`).
  `play.rs` reworked: the device and its callback (`Rig`, `Stage`) outlive Play and Stop while an
  instrument is loaded; the callback adds the instruments, mixed there from their own mixer,
  to the feeder's queue; mixers and queues are handed over and freed without the audio thread
  waiting or freeing. `audio.rs`: `mixer_for(Part)`, instrument strips, export pauses them and
  switches plugins to offline.
- **Shells**: the window gives the engine Tauri's main thread, its window handle, a scan child
  (`crucible-app --probe-plugin`) and a 30 Hz look-in for plugin windows; `plugin.scan` runs
  outside the engine lock. The CLI's own `plugin.scan` was replaced by the engine's.
- **Window**: `instruments.ts` (picker with search, Scan again, Add a folder; instrument menu;
  keyboard and channel dialog; status ten times a second, meters while stopped); each instrument
  track shows its plugin and a keyboard button.
- Verified by machine: 88 unit tests, clippy clean, 88 GUI checks (19 new) on the release build,
  four clean runs in a row; the CLI scans, loads and exports with the test instrument.
  One GUI run in six had a silent key after an export, before the lost-keys fix; not seen since;
  cause not proven.
- Not verified: Keyscape, any free plugin, the Axiom, anything audible.
- Not committed: 0.0.4 still waits for his yes; it now carries this too.

### 2026-11-26, his second session: installer, saving, getting around, effects, memory: 0.0.4 (Claude Opus 5.5, then Fable 5.2)

He installed 0.0.3 (it failed in Program Files), then worked with a 4-minute and a 9-minute MP3
at 192 kHz in a build of `main`, and sent thirteen messages of findings while this was built.

- **Installer** (D-037 part three): per machine again (`InstallScope="perMachine"`,
  `ProgramFiles64Folder\Crucible`, files as their own key paths), plus a `logs` folder Users may
  write (`MsiLockPermissionsEx`, InstallerVersion 500); `log_dirs` looks there. Version 0.0.4.
- **Engine**: `scratch.rs` (an unsaved project's folder, D-042); `project.save` copies the audio
  when the folder is new and re-points the sound and waveforms held in memory; `project.end` and
  the song's end in playback (`transport.status` stops there and says `ended`), file format 5;
  `release_unused` lets go of sound no clip uses; only New and Open stop playback now.
- **Window shell**: `ui.pick_folder` and `ui.pick_files` (the `rfd` crate), answered outside the
  engine lock; the page may close its own window (`core:window:allow-destroy`) after asking.
- **Window**: `saveAs`, `openProject`, `newProject`, `confirmUnsaved` in `dialogs.ts`; the effect
  editor rewritten in `effects.ts`; timeline scroll bar, pan, zoom-to-fit, end tab, "Add audio"
  button in `timeline.ts`; FX button and `m:ss.t` position in `main.ts`.
- **GUI test**: its command watching had never worked (it replaced Tauri's `invoke`, which is
  read-only), so "Ctrl+C leaves the clips alone" had tested nothing. It reads the session log
  now, with a check that it sees commands. The pickers are answered through two environment
  variables the test sets; one check asks for the real picker and closes it
  (`tools/close-window.ps1`). 30 new checks, 69 in all.
- Verified by machine: 81 unit tests, 69 GUI checks, clippy clean; the MSI's tables; the
  permission text on a test folder; screenshots of the new dialogs looked at.
- Not verified: installing it (Claude does not install on the build laptop), so the `logs`
  folder's rights on a real install; anything audible; how the new gestures feel.
- Told him plainly: there is no VST button because instruments are not hosted yet (M0 gate);
  MIDI device and channel assignment is not built either; the 1.1 GB was the design, not a leak.
- Not committed or released: waiting for his yes.

### 2026-11-26, the new logo and Excel green (Claude Opus 5.5)

- **He picked number 3 as the logo** (the anvil with spectrum sparks and a note cut into it) and
  saved 5, 6, 13, 15 and 17 as alternatives (D-029); the rest were dropped from
  `docs/logo-options/`. `app/icon.py` now draws number 3: the app icons, and the top-bar logo
  (34 x 26 px). Release build: 39 GUI checks pass. Committed and pushed at his word; no release,
  so 0.0.3 as installed still has the old logo and green until the next installer.

- Of the 12 options he kept only "Spectrum sparks"; asked for it with a music symbol on the anvil,
  ten more, and a green "closer to Excel green". Gallery rebuilt: 1 current, 2 his pick, 3 his
  pick with two beamed notes cut into the body, 4 to 13 new (waveform, radial spectrum, piano
  keys, forging a wave, headphones, EQ curve, solid tile, one-line ring-out, VU meter, staff).
  The rejected round-1 marks were dropped from the folder.
- Accent is now `#21a366` (Excel's green), hover `#33c481` (D-029): style.css, the Settings
  swatches, `app/icon.py` and the regenerated app icons. Release build: 39 GUI checks pass.
  Not committed (with the options folder).
- Then round 3 at his ask ("experiment with passthrough and behind ... or different style of
  anvil"): 14 to 25, a wave through the anvil, a waveform, keys, spectrum, record or striped sun
  behind it, an anvil cut by waves, a bold geometric anvil (three ways), an outline anvil, an
  8-bit anvil.

### 2026-11-25 (evening), his home-desktop test: storage, conversion, ASIO name, MIDI, log (Claude Opus 5.5)

- Dzyan tested 0.0.2 at home (Behringer UMC202HD, M-Audio Axiom 61, Reaper alongside) and sent:
  log everything while testing; log in the program's folder; offer to convert to 192 kHz instead
  of refusing; a 4-minute import takes 7 to 8 s; imports become 200 MB WAVs ("shouldn't they be
  kept in Memory?"); an exported WAV "plays no sound"; E2002 with ASIO; Ctrl+C blocks copying
  text; the MIDI keyboard is not detected; where are VST3 and instruments.
- Done: D-039 (files kept as they are, memory for MP3/FLAC/other rates, rubato conversion when
  agreed, `--convert`, window questions), D-040 (ASIO name matching, rate message per driver,
  MIDI list, Ctrl+C), log everything (`ALIVE` every second, device-opening steps, Windows device
  notices, window presses, wheel, focus, popups, drawing), per-user installer (WiX template).
  Version 0.0.3. 78 unit tests, 39 GUI checks pass.
- Found while there: the window's Settings told him Windows would convert at 192 kHz even with
  ASIO, which is false; that is what pushed him toward a 192 kHz project.
- Not verified: anything on his desktop. The silent export: its log line says peak +1.15 dBFS, so
  the samples are there; most likely Reaper held the interface through ASIO, so Windows' player
  could not use it. Asked him to check.
- VST3 and instruments: not in the window yet; they follow M0 (the plugin-hosting test with
  `m0-play`).
- The logo is an anvil now (his word, same evening; D-029). The first two tries (side on, then from
  above) did not read well to him.
- He asked for the installer to work as an updater; Claude built an in-program updater (GitHub,
  signed); he then decided nothing may go online (D-041). The updater was removed before release,
  and the installer no longer downloads WebView2. A newer installer upgrades in place.
- The logo was redrawn a third time after his reference picture: side on, the classic anvil with
  its edge lines (horn, step block, face plate, waist, arch), and a wave on the side.
- Released as `v0.0.3` (commit `4325ec2`), at his word ("push it yourself after you're done").
  Anonymous download hash matched. Then, at his ask, 12 logo options in `docs/logo-options/`
  (`options.py` draws them; `index.html` shows each as icon, top-bar size and on light). Not committed.

### 2026-11-25, Stop and fader faults, real devices, one log file, GUI test (Claude Fable 5.2)

- Dzyan, after his first real use on another PC: Stop did not work and the mixer faders shook
  during playback; "Can you not test GUI changes ... These are the kind of basic things I want
  you to catch"; remove the demo content; show devices and their sample rate accurately; defeat
  or at least detect programs taking the sound driver; volume sliders by the tracks; one log file
  in the program's folder, written live, overwritten on restart. D-038, and D-037's addition.
- Reproduced both faults in the real window by forcing dropouts: the page was rebuilt on every
  dropout count. Fixed at the root (no rebuild for a number; no rebuild while a mouse button is
  held; fader place from CSS). Likely cause of the dropout count on his PC fixed too: the queue
  now sizes itself from what the driver really asks for.
- `tools/gui-test.ps1`: 34 checks on the real window with human-length clicks. It found, before
  he could: the device lists in Settings collapsed to nothing; the master fader 3 px out of line.
- Out: demo song, stand-in buttons, fake plugins and devices (`demo.ts` deleted). In: real device
  list, plug detection, "in use by", header volume sliders, `audio.devices`, `audio.changes`,
  `log.path`; the old full listing is now `audio.probe`.
- Log: `crucible-app.log` beside the program. `%LOCALAPPDATA%\Crucible\logs` is no longer
  written; its old files are left alone.
- Committed, pushed and released as `v0.0.2` the same day at his word. Report:
  `docs/test-reports/2026-11-25-devlaptop-gui-devices-log.md`.

### 2026-11-23 (night), logs that tell the whole session (Claude Fable 5.2)

- Dzyan: design "very solid log and output files": from another PC he must be able to send one
  file showing "basically everything I was doing, what was happening behind the scenes", enough
  to work out what to fix. "Is that in place?" It was not: the log had command text, errors and
  crashes only. D-037 records the design; `docs/11-error-codes.md` the line types and the
  three-step procedure (F9, Create report file, send).
- Built: command outcomes and timing (`trail.rs`), `mark`, `log.ui`, `log.snapshot`, playback
  lines and dropout causes (`play.rs`), the window's trail (`app/src/trail.ts`), F9, the report
  button, the window's `report` command and watchdog, the commit in log headers (`build.rs`),
  `report --out`, marks and problems kept when a long log is cut, three new codes (W1005, E1006,
  E9003). Version 0.0.2.
- Found on the way: the window's log never said which device playback opened or whether it
  dropped out; `crucible report` run from Program Files had nowhere to write but the log folder;
  the one-shot playback health line said "silent" whatever was playing (fixed before release).
- Tested: the real window through WebView2's debugging port (real clicks, keys, F9, a deliberate
  error, the report, closing while playing); dropouts injected by freezing the mixing thread.
  Report: `docs/test-reports/2026-11-23-devlaptop-logging-and-report.md`.
- This laptop's Desktop is inside OneDrive, so a report made with the button here is synced.
  `report --out <folder>` in the Console tab writes elsewhere.
- Not committed. `dist/` holds the 0.0.2 build; its headers say "commit 62b5c38, plus
  uncommitted changes", which is true. Rebuild after the commit for the release.

### 2026-11-23 (evening), licence and overlapping clips (Claude Fable 5.2)

- Dzyan: "GPLv3, this can be totally open software, I don't care about trying to 'own' it"
  (D-036; answers Q4 and Q5; replaces D-011). Overlapping clips "should both play" (Q12; as
  built; new test). "Yes to #10" (the GUI-feel checklist line): nothing to act on; those
  features are built and wait for him to try them.
- Added `LICENSE` (GPLv3 text from Git for Windows' own copy, nothing downloaded), set
  `GPL-3.0-or-later` in `Cargo.toml` and `app/package.json`, README section, `LICENSE.txt` in the
  installer. Rewrote the licence rule in `CLAUDE.md`.
- Checked every dependency licence against GPLv3: all compatible.
- Fixed: `audio.devices` and `diag` still printed "ASIO is not compiled into this build"; the
  note now follows the build. `dist/m0/README.txt` said the same and is corrected.
- Later the same evening, Dzyan: "Go ahead and push it along with installer to public repo."
  Before publishing: `THIRD-PARTY-NOTICES.txt` and its generator; the ASIO SDK's host files
  vendored into `third-party/asiosdk` and the build switched to them (four Microsoft sample
  files in the SDK carry no Steinberg licence, so they are left out and no longer compiled);
  every file and commit checked for employer names, e-mail addresses, the machine name and
  secrets (none). Then: commit, push, release `v0.0.1` (pre-release) with the installer, repo
  made public.
- Rebuilt and restaged `dist/`.

### 2026-11-23 (later), downloads, ASIO, live playback, installer (Claude Opus 5.5, then Fable 5.2)

- Dzyan: download the ASIO SDK, LLVM and WiX; "we definitely want ASIO working regardless" (D-035);
  fine with effects using more CPU and memory for quality (D-033 budget decided); effects must
  work while playing and recording, not only on export (D-034); how big is the installed program;
  keep the reply checklist very compact (`CLAUDE.md`).
- Downloaded to `..\crucible-sdk`: ASIO SDK 2.3.4 from Steinberg, LLVM 23.1.6 from the LLVM
  project's GitHub release (hash matched winget's manifest; unpacked with `msiexec /a`, not
  installed). WiX came through Tauri. Nothing was installed system-wide.
- ASIO: feature on by default, `output.rs` with one `open_output` for all three drivers. The
  laptop has no ASIO driver, so no ASIO stream has run.
- Live playback: `play.rs` (feeder thread, lock-free queue, callback), `transport.*` and
  `audio.output` commands, `Effect::adopt` and `Mixer::inherit` so edits keep tails, the window's
  Play, playhead, meters and seek (`transport.ts`; the fake transport in `demo.ts` is gone).
- Bugs found by the new tests and fixed: effects decided when to go idle per block, so small and
  large blocks differed below -150 dBFS; the convolver's block grid shifted after idling; device
  output lost the last bit of loud 24-bit samples (scale 2^23 - 1 instead of 2^23).
- MSI built: 6.0 MB, installs 16 MB. Demo exports regenerated. `dist/` restaged.
- Verified by machine only; report `docs/test-reports/2026-11-23-devlaptop-live-playback-and-asio.md`.
  Everything since `5845a0b` is uncommitted.

### 2026-11-23, refinements, installer prep, reverb research (Claude Opus 5.5)

- Dzyan: iron out known inefficiencies and memory issues; make an MSI; end every reply with his
  to-do list (now in `CLAUDE.md`); explain the Steinberg SDK again; wants refined Reverb, Delay and
  Echo, "very good" reverb (Q9 partly answered).
- **Memory budget is met.** The 391 MB in D-025 was committed memory, inflated by the GPU process.
  Task Manager's measure, summed over all WebView2 processes, is 92 MB (`tools/measure-memory.ps1`).
- Bulk edits measured at 5,000 clips / 100,000 notes: single edits 2 to 3 ms; moving all 5,000
  clips 0.3 s (every moved clip resends its notes). Recorded, not reworked: rare, and the fix
  (notes behind their own pointer) can wait for a real project that needs it.
- "No allocation" test added and passing: all 04-audio-quality machine tests now pass.
- Installer configured (MSI via Tauri's bundler, WiX on the build machine only). Building it waits
  for his yes to download WiX (blocker 8). Nothing is needed on users' machines beyond WebView2,
  which Windows 11 has.
- Reverb and delay research done (`docs/13-reverb-and-delay.md`, background agent) and built at
  his request (D-033): new crate `crucible-dsp`, `fx.*` commands, per-track mixer, export tails,
  mixer slots and an editor drawn from the parameter table. 14 DSP tests plus an engine test.
  Bugs the tests caught on the way: the IR was always faded (13 dB off), a first-order damping
  shelf shortened mid decay by 12%, idle detection cut echoes, trimming moved IR timing. CPU cut
  by removing integer divisions from delay-line reads (2.9% to 2.0% for Reverb).
- Found while building: the installer config would have copied the release `crucible.exe` over
  the debug one on every build. Installer-only settings now live in `tauri.bundle.json`.
- Demo exports regenerated, now including `overnight-mix-with-effects-24bit.wav`.

### 2026-11-22, overnight test pass, audio import and export (Claude Opus 5.5)

- Dzyan: "go ahead and push for now", then asked for an unattended test of the app (WAV and MP3
  files, mixing, exporting, the GUI, latency). The 2026-11-21 batch was committed and pushed
  (`5845a0b`). Everything below is **uncommitted**: ask before committing or pushing.
- Full report: `docs/test-reports/2026-11-22-overnight-devlaptop-gui-and-audio.md`. Nine bugs
  found and fixed, among them: mute and solo could not be turned off from the window; commands
  could reach the engine out of order; every edit sent the whole project (86 ms engine, about
  280 ms through the bridge at 2,000 clips; now 0.5 and 4 ms, D-032); paste after box select
  went to the wrong track; piano-roll rows 4.7 px tall; the window's log had no end line.
- Built, because the roadmap's unattended list allows the offline render path and peaks, and
  testing "mixing, exporting" needed them: audio import/place/peaks/export (D-031, D-030),
  streaming (6 MB peak memory with two 10-minute files), MP3 and FLAC import via `symphonia`.
  Project format is now 3. New codes E5015 to E5019, W5020.
- Found and recorded: MP3s without a gapless tag import 25 ms late (`12-known-daw-problems.md` F7).
- Verified by machine only. Not heard: Dzyan listens to `dist/test-audio/exports/`.
- Staged: `dist/m0`, `dist/app`, `dist/test-audio` (stems, MP3, a 10-minute file, a demo project,
  three exports, the scripts that made them).

### 2026-11-21, mouse editing, layouts, fonts (Claude Opus 5.5)

- Dzyan renamed the folder to `crucible` himself. Launch config updated.
- Fonts bundled (D-026). Clips and notes added to the project model, format 2, with 20 commands
  (D-027). Timeline and piano roll rewritten around them. Layout themes (D-028).
- Verified in the browser pane against the real engine: drag-move snaps and is one undo step,
  Ctrl+drag copies, edge trim, box select, Ctrl+C/V paste at the playhead, Ctrl+E split, Del and
  undo, piano-roll note drag/draw/delete, all five layouts render. Frame work 0.34 ms. Engine tests
  12, including copy/paste surviving a crash. The test tool could not hold Ctrl during a real
  drag, so Ctrl+drag was checked with synthetic events.
- Research on problems other DAWs keep solving: `docs/12-known-daw-problems.md` (background agent).
- Research done (`docs/12-known-daw-problems.md`, 58 problems, sources opened). Acted on now:
  MMCSS "Pro Audio" on exclusive audio threads (T13), checklist steps for sustain, mod wheel,
  pitch bend and stuck notes, 02 threads table corrected (all VST3 lifecycle calls on the main
  thread), evidence appended to D-022 (plugin-window edits) and 04 (determinism), T7 answered.
  The top-15 list in that doc is the to-do list for M2 and M3.
- Dzyan asked for green instead of orange and a musical crucible logo: D-029.
- Committed and pushed 2026-11-22 at his request.

### 2026-11-20, first window, repo, rename (Claude Opus 5.5)

- Dzyan asked to push to a work container registry. Declined: personal project, and not a
  container workload. He then chose his personal GitHub: created private `dzykel60687/crucible` with his
  cached Git credential, pushed `main`. Repo-local commit email is the GitHub noreply address.
- Built the window (D-025): `app/src-tauri` (one `exec` command into the engine) and `app/src`
  (plain TypeScript, Canvas 2D). New engine command `project.state` returns the whole model for
  front ends. Dev bridge lets a browser drive the real engine.
- Verified in the browser pane against the real engine: renders, mute/add track/undo/redo/delete
  /tab keys run engine commands, settings and project menu open, no console errors. Frame work
  0.38 ms while playing. Actual frame rate could not be judged (the pane was throttled in the
  background); Dzyan judges smoothness on his screens.
- Dzyan asked to clear out the old `daw` names: crates are now `crucible-*`, docs and test paths
  updated, D-023 amended. `DawError` kept (DAW = the kind of program). Renaming the folder
  `rust-daw` → `crucible` failed because the folder was in use; Dzyan renames it when nothing has it
  open. Docs already use `crucible`. Also update `Development\.claude\launch.json` (crucible-ui).

### 2026-11-20, command layer and name (Claude Opus 5.5)

- Dzyan named the product **Crucible** (D-023), asked for the command layer now (D-022, now
  Decided), and set the build philosophy: small, fast, robust, "John Carmack style", and to be
  caught when GUI wishes cost the engine (D-024). Written into `01`, `05`, `CLAUDE.md` rule 11.
- Built `crucible-project` (model, atomic save, validated load, journal), `crucible-engine` (command tables,
  parser, canonical form, 21 commands, snapshot undo/redo, journal replay), and rewrote the CLI
  as `crucible` with one-shot, `-c`, `run`, `shell`, `--json`, `--project`. The early command names
  became `audio.devices`, `audio.test`, `midi.inputs`, `plugin.scan`. New error area E5xxx, exit 13.
- Log folder moved to `%LOCALAPPDATA%\Crucible\logs`. Release builds now keep line tables in the PDB.
- Verified here by driving `crucible`: chained edits and save, `--project` auto-save, JSON output,
  mid-chain error stops with E5003 and exit 13, typo suggests the right name, and crash recovery
  (shell exited without saving; reopening replayed 3 journaled edits; undo worked after).
- Not done: autosave, the in-app console (needs the GUI), attach-to-running-app pipe.

### 2026-11-20, diagnostics session (Claude Opus 5.5)

- Dzyan asked for error codes and logs that are easy to diagnose, and what ASIO and the
  Steinberg SDK are. Answered ASIO in chat; nothing changed about Q4.
- New crate `crucible-diagnostics` (D-021). Codes E1xxx-E9xxx and W-warnings with help text,
  grouped exit codes, `docs/11-error-codes.md` generated by `crucible codes --markdown` and checked by a test.
- Logs are now written line by line to the per-user log folder (now `%LOCALAPPDATA%\Crucible\logs`). **Before this, `m0-play`
  wrote its log only on a clean exit, so a plugin crash would have left no log at all.**
- Crash handler: names the crashing module, writes a `.dmp`. The probe child prints the same to
  stderr, so `crucible plugin.scan` shows which file crashed.
- vst3-host's loading steps are logged while a plugin loads (via the `log` facade bridge).
- `m0-play` now: prints dropouts (W2101) live once a second, logs late wake-ups, notices a
  stream that died (E2009 etc.), writes a heartbeat with memory use every minute, has an
  `m <text>` marker command.
- WASAPI HRESULTs map to codes; the number is always kept. A driver that stops signalling is now
  checked, so an unplugged device ends with E2009 instead of silent timeouts.
- Verified here: bad option (E1001, exit 2), unknown device (E2002, exit 10), device in use by a
  second program (E2004), unknown plugin (E4001, exit 12), and a fake VST3 built to crash with an
  access violation. Its source is in the session scratchpad and is not kept in the repo. It crashed
  the scanner's child (`CRASHED E4004`, module named) and `m0-play` (log showed loading steps up to
  `GetPluginFactory`, then `CRASH E9001` with the module, plus a 180 KB dump).
- Checklist updated: send `crucible report` output instead of collecting log files by hand.
- Release builds restaged in `dist/m0`. Nothing committed (no repo).
- Dzyan asked about a CLI/shell under the app. Recorded as D-022 (Proposed): command-API first,
  with the GUI, an in-app console and the CLI as front ends. Applies from M2. Needs his yes.

### 2026-11-20, first coding session (Claude Opus 5.5)

- Rust confirmed installed. Created the Cargo workspace with the six crates from `02-architecture.md`.
- **M0 step A done:** `crucible diag` lists devices, MIDI and plugins and writes a log. Verified here.
- Plugin scanning runs each plugin in a child process of `crucible.exe` itself, not vst3-host's
  separate probe binary. Decision D-019.
- **Verified T2:** cpal 0.18.2 opens WASAPI in shared mode only. Exclusive mode needs its own module.
- Dzyan said not to worry about driver models: the DAW accepts whatever drivers are installed. D-018.
- **M0 step B code written** in `spikes/m0-omnisphere`. Uses vst3-host's playback path, which locks
  the plugin inside the audio callback. Acceptable for the spike only; noted in the checklist.
- Found: vst3-host does not pump Windows messages. The spike runs its own Win32 pump on the main thread.
- Added static C runtime linking for Windows so test builds copy to other PCs.
- Added **WASAPI exclusive mode** in `crucible-audio-io` using the `wasapi` crate (D-020): capability
  report in `audio.devices`/`diag`, `audio.test`, and an output stream the spike uses with
  `--exclusive`. Verified timing on the Realtek output here (silence only). Not yet MMCSS-boosted.
- Wrote Dzyan's step-B checklist, including exclusive-mode steps. No git repo created, nothing committed.

### 2026-11-19, design session (Claude Fable 5.2)

- Researched feasibility, VST3 licensing, Rust plugin hosting and GUI toolkits. Sources in `10-references.md`.
- Dzyan chose Tauri. Electron ruled out. egui is the fallback.
- Created this folder and the first-draft design docs.
