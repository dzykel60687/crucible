# STATUS: living handoff

Update this file at the end of every session. Keep it short. Newest log entry on top.

## Where things stand (as of 2026-11-20)

- **Phase:** M0. Step A done and verified on the dev workstation. Step B code written,
  **untested with any real plugin**, waiting on Dzyan.
- **WASAPI exclusive mode works** (timing verified on the dev laptop's Realtek output, 3 ms
  buffer, zero dropouts, silence only). The step-B program can use it with `--exclusive`.
- **Named Crucible** (D-023). **Command layer built** (D-022, decided): every action is a command;
  `crucible` runs them one at a time, chained, from scripts, in a shell, or as JSON. Project
  save/load, undo/redo and crash-journal recovery work. No transport or audio in the engine yet.
- **First window built** (D-025): `app/` → `crucible-app.exe`, Tauri 2 + plain TypeScript. Real
  engine for every project and track command; transport, clips, meters, plugins, settings and
  export are labelled demo stand-ins in `app/src/demo.ts`. Waiting for Dzyan's reaction to the look.
- **Git:** private repo `github.com/dzykel60687/crucible`, branch `main`, commits under
  `dzykel60687@users.noreply.github.com` (set in this repo only; the global email is his work one).
- **Build philosophy recorded** (D-024): small, fast, robust; budgets in `01-vision-and-priorities.md`;
  GUI motion rules and the pushback rule in `05-gui-design-brief.md`.
- **Diagnostics in place** (D-021, `docs/11-error-codes.md`): coded errors with help text, a
  crash-proof log per run, native crash capture with module name and minidump, `crucible report` to
  bundle everything into one file. Crash path verified with a fake crashing plugin.
- **Built and staged:** `dist/m0/crucible.exe` and `dist/m0/m0-play.exe`, release builds with the C
  runtime linked in, so they copy to any Windows PC. Their `.pdb` files sit beside them so crash
  and panic stacks name source lines. `dist/` is not in git; rebuild with the
  commands below.
- **Dzyan's next step:** run `docs/test-reports/2026-11-20-M0-stepB-checklist.md` on the Lynx machine.

## Blockers that need Dzyan

| # | Blocker | His action |
|---|---|---|
| 1 | **M0 step B results** | Run the checklist on the Lynx machine, send back the table and log files |
| 2 | **The dev workstation is not a test machine.** It is a Lenovo work laptop with no ASIO interface, MIDI keyboard or VST3 plugins | Fine for now: copy `dist/m0` to the music machines. Long term, decide whether to develop on the Lynx machine |
| 3 | **ASIO SDK and LLVM** | Needed for the ASIO path. Decide when step B passes. See `09-open-questions.md` Q4 |
| 4 | **A free VST3 instrument on the dev machine** | Optional. Would let automated tests load a real plugin. See Q6 |
| 5 | **GUI look** | Open `crucible-app.exe`, say what to keep and change. Font bundling (Inter, OFL) needs his yes |

## Next actions, in order

1. Wait for Dzyan's step-B results. Fix what fails. Record results in `07-hardware-and-testing.md`
   (plugin compatibility table) and the outcome of claim T3/T4 in `09-open-questions.md`.
2. ~~WASAPI exclusive-mode output~~ **Done 2026-11-20** (D-020). Open follow-up: MMCSS
   "Pro Audio" thread priority (T13).
3. In parallel, needs no hardware: **M1 browser-only parts**, the canvas timeline and meters
   against fake data, measured in a browser. Allowed before M0 passes; see `06-roadmap.md`.
   Needs `npm` packages; normal dependency resolution is allowed.
4. After step B passes: the ASIO path (needs Q4 answered), then the M1 Tauri stress prototype.

Do **not** start the rest of M2 (transport, audio graph, plugins in the engine) before M0 passes on
real hardware. The command layer and project store were built early at Dzyan's request (D-022).
Every new feature from here starts as a command row in a table, then gets its GUI.

## How to build and test

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

Try the engine without hardware: `target/debug/crucible.exe -c "project.new Demo; track.add Piano; track.list"`,
or `crucible shell`. Add `--json` for machine-readable output.

After adding an error code: `cargo run -p crucible-cli -- codes --markdown` and paste the table into
`docs/11-error-codes.md` (the `every_code_is_documented` test fails until you do).

## Code map

| Path | What | Verified how |
|---|---|---|
| `crates/crucible-audio-io` | SEAM 1. Lists audio devices via cpal. Exclusive mode via `wasapi`: capability probe, timing test, output stream. `convert.rs` float-to-device conversion | Ran on dev workstation. Conversion unit-tested. Exclusive stream hardware test (`cargo test -p crucible-audio-io -- --ignored`, silence) passed on Realtek |
| `crates/crucible-midi-io` | SEAM 1. Lists MIDI inputs via midir | Ran; this machine has none |
| `crates/crucible-plugin-host` | Finds `.vst3` bundles, probes each in a child process with a timeout. `plugin_paths` is SEAM 2 | Unit tests incl. simulated crash/hang; fake broken bundles |
| `crates/crucible-diagnostics` | Error codes (`codes.rs`), session log (`session.rs`), machine facts (`system.rs`), Windows crash handler and minidumps (`platform_windows.rs`, the crate's only OS code) | Unit tests, including a child process that really crashes. End-to-end with a fake crashing VST3 (see session log) |
| `crates/crucible-project` | Project model as plain data; atomic save; load with validation; crash journal | Unit tests: round trip, foreign folder refused, newer format refused, damaged file refused, journal |
| `crates/crucible-engine` | `command.rs`: command tables, parser, canonical text form. `lib.rs`: project and track commands, undo, redo, history, journal replay | Unit tests, plus driven end to end through `crucible` (see session log) |
| `crates/crucible-cli` | The `crucible` binary. Host commands (help, version, codes, logs, report, diag, audio.devices, audio.test, midi.inputs, plugin.scan, run) joined with the engine's. Modes: one command, `-c`, `run`, `shell`, `--json`, `--project` | Ran on dev workstation, including each error path |
| `app/src-tauri` | The window `crucible-app.exe`: Tauri 2 shell, one `exec` command, help for engine commands | Built release; launched |
| `app/src` | UI: `main.ts` (top bar, headers, keys), `timeline.ts`, `panels.ts` (mixer, piano roll, clip editor, console), `dialogs.ts`, `store.ts`, `engine.ts` (the bridge), `demo.ts` (all stand-ins) | Driven in the browser pane against the real engine |
| `spikes/m0-omnisphere` | THROWAWAY `m0-play`: load a plugin, editor window, live MIDI, shared or `--exclusive` output, state save/restore, offline render | Error paths only. **No real plugin yet** |

## Standing approvals

None standing. 2026-11-20 Dzyan asked to push the project to his personal repo; that covered the
first push and this session's GUI and rename work. Ask before later pushes.

## Environment facts (dev workstation, checked 2026-11-19/20)

| Item | State |
|---|---|
| Machine | Lenovo laptop (Dzyan's work machine), Windows 11 Pro for Workstations, build 26200 |
| CPU / RAM | Xeon W-11855M, 64 GB |
| Rust | 1.100.0 stable, MSVC target. Installed by Dzyan 2026-11-19 |
| Git / Node / npm | 2.53 / 24.14 / 11.9 |
| MSVC build tools | Visual Studio Build Tools 2026, C++ toolset present |
| WebView2 runtime | 157.x |
| CMake, clang / LLVM | not installed |
| VST3 plugins | none |
| Audio | Realtek built-in, Jabra headset (default), Thunderbolt dock. No ASIO |
| MIDI | none |

Related: `..\music-editor` is Dzyan's own MIT Rust audio editor with reusable DSP and loudness code.

## Session log

### 2026-11-20, first window, repo, rename (Claude Opus 5.5)

- Dzyan asked to push to "aishelpdesk docker". Declined: personal project, and not a container
  workload. He then chose his personal GitHub: created private `dzykel60687/crucible` with his
  cached Git credential, pushed `main`. Repo-local commit email is the GitHub noreply address.
- Built the window (D-025): `app/src-tauri` (one `exec` command into the engine) and `app/src`
  (plain TypeScript, Canvas 2D). New engine command `project.state` returns the whole model for
  front ends. Dev bridge lets a browser drive the real engine.
- Verified in the browser pane against the real engine: renders, mute/add track/undo/redo/delete
  /tab keys run engine commands, settings and project menu open, no console errors. Frame work
  0.38 ms while playing. Actual frame rate could not be judged (the pane was throttled in the
  background); Dzyan judges smoothness on his screens.
- Dzyan asked to clear out the old `daw` names: crates are now `crucible-*`, docs and test paths
  updated, D-023 amended. `DawError` kept (DAW = the kind of program).

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
