# 08. Decision log

**Decided** means Dzyan made the call. Do not re-argue it. If new evidence appears, append it
under the entry and raise it with him.

**Proposed** means Claude recommends it and Dzyan has not yet confirmed. It can be built on,
but say so when reporting, and ask at the next natural point.

All entries dated 2026-11-19 unless noted.

---

## D-001. Build a DAW even though good ones exist. *Decided.*

Reason: a learning project in Rust and a tool with a clean interface for a small group. Not a
cost saving. Dzyan was explicit that "it already exists" is not a reason to stop.

## D-002. The engine is written in Rust. *Decided.*

Reason: Dzyan wants a practical Rust project. Rust also suits real-time audio.
Cost accepted: Rust plugin hosting on Windows is thin and unproven. The proven route would have
been C++ with JUCE or Tracktion Engine.

## D-003. Windows first, perfected there. Never tied to Windows. *Decided.*

macOS and Linux follow later. Dzyan will not test them for a while. Every doc carries platform
notes and the code keeps OS-specific parts behind seams. Contract: `03-platform-notes.md`.

## D-004. Reference plugins are Omnisphere and Keyscape, plus mainstream VST3. *Decided.*

Consequence: MIDI tracks, MIDI recording and a piano roll are mandatory, because both are
instruments. Not every plugin has to work.
Fact: Spectrasonics ships no Linux build. Linux will use other instruments.

## D-005. VST3 is the only plugin format in version 1. *Proposed.*

Reason: it covers the reference plugins on Windows and macOS. The SDK is MIT-licensed since 3.8.
Deferred: CLAP, which is cheap to add. Not planned: VST2, AU, AAX, LV2.

## D-006. The interface is Tauri 2 with a TypeScript front end. *Decided.*

Ruled out by Dzyan: Electron.
Fallback if the M1 stress test fails: egui.

Evidence gathered before the decision:

| Option | Why not |
|---|---|
| egui | Best for custom drawing and has the most early DAW precedents, but looks are entirely self-built, it is pre-1.0 and breaks on upgrades. Kept as the fallback. |
| Slint | Stable and has paid support, but its own vendor lists missing desktop features: shortcuts, popup windows, multi-window, drag and drop. |
| iced | One release in twelve months. Calls itself experimental. Its own FAQ concedes the documentation is poor. |
| GPUI | The official package has not been released in a year. The community builds on forks. |
| Qt from Rust | Tiny community. A 2025 survey could not build it on Windows. Would make the front end C++. |
| Dioxus | Pre-1.0, no named shipped desktop app found, and it uses the same web engine as Tauri anyway. |

Why Tauri: top of every community measure by a wide margin, stable since 2022, Windows is its
most reliable platform, and web styling gives a modern look with the least effort.

Costs accepted:
- A bridge between the interface and the engine. Meters, playhead and waveforms need deliberate design.
- Two languages.
- Linux is its weakest platform. Linux is also the lowest priority.
- The timeline must be canvas-drawn.

Cautions on record:
- No mature DAW exists on any Rust toolkit, Tauri included.
- Only one public write-up shows a VST3 plugin window working from a Tauri app.
- The author of the Meadowlark DAW rejected egui, iced, Slint and web technology in 2023 and
  wrote his own toolkit.
- Tauri 3 is in alpha. Stay on the stable 2.x line.

## D-007. The engine is headless and the interface is replaceable. *Decided.*

The engine is a library driven by commands. It does not depend on Tauri. This is what makes the
egui fallback in D-006 real, and what lets a machine test the engine.

## D-008. Plugin editors are floating windows, never embedded. *Decided.*

Reason: embedding is the fragile path on every platform and in every toolkit examined.

## D-009. Both ASIO and WASAPI exclusive mode are supported from the start. *Decided.*

Reason: the Lynx machine uses ASIO and the laptop has no ASIO driver. Shared mode is offered
only as a labelled fallback, because it resamples and limits.

## D-010. Projects are portable folders. Device settings are per machine. *Decided.*

Reason: the same project is worked on across three machines with different audio hardware
and different MIDI keyboards.

## D-011. No code is copied or ported from GPL or AGPL projects. *Proposed.*

Reason: keeps the licence choice open. Dzyan has not decided whether the result will be open
source. If he chooses GPL for the whole project, this can be relaxed.
Clean references: Steinberg's own MIT-licensed hosting examples, and the MIT `vst3-host` crate.

## D-012. Order of work: Omnisphere test first, Tauri stress test second, then the engine. *Decided.*

Reason: test the two riskiest unknowns with throwaway code before building on them.
Dzyan agreed to the spike-first plan and to the stress prototype as the GUI go or no-go.

## D-013. Audio is processed as 32-bit float. *Proposed.*

Reason: it is what VST3 plugins universally accept, and its range is far beyond any converter.
A 64-bit mix bus gives no audible benefit. The null test in `04-audio-quality.md` is the proof
that nothing is lost.
Reversible: yes, if a null test ever fails for a reason 64-bit would fix.

## D-014. Export defaults: 24-bit WAV at the project rate, no automatic processing. *Proposed.*

Full table in `04-audio-quality.md`. No lossy formats in version 1. No automatic limiter or
normaliser, ever.

## D-015. Front-end stack: TypeScript, React, Vite, Tailwind, Radix primitives, Canvas 2D. *Proposed.*

Reason: the most widely used combination, so the most examples and the best AI fluency. Radix
and Tailwind give a modern look quickly. Canvas 2D first, WebGL only if M1 demands it.
Reversible: cheaply, until M2 builds real screens.
Alternative worth knowing: Svelte is lighter. A Rust-written front end is possible but less mature.

*Revised 2026-11-20, still Proposed, after D-024 (lean rule):* the first GUI was built with plain
TypeScript, Vite and Canvas 2D, and no React, Tailwind or Radix. The whole UI is about 1,800 lines
with one runtime dependency (`@tauri-apps/api`). The screen is one fixed layout redrawn from
engine state, and canvases do the heavy drawing, so a component framework would add weight
without solving anything here. Revisit only if dialogs and forms multiply past what plain DOM
code handles cleanly.

## D-016. Plugins run in-process in version 1. *Proposed.*

Reason: out-of-process hosting costs latency and a great deal of complexity. The defence against
a plugin crash is frequent autosave and a crash journal. Plugin scanning does run in a separate
process. Revisit after M3.

## D-017. Stereo only. No sends or buses in version 1. *Proposed.*

Reason: priority 1. Routing complexity is a large part of what makes other DAWs intimidating.

## D-018. The DAW accepts whatever audio drivers are installed. *Decided 2026-11-20.*

Dzyan: "whatever Windows drivers are installed or get installed with soundcard hookup should be
accepted." So no per-model support. Order of preference per device: its ASIO driver if one is
installed, else WASAPI exclusive mode, else WASAPI shared mode with a visible warning.
Supersedes the old question about exact Lynx and Behringer models.

## D-019. Plugin scanning probes each plugin in a child copy of our own executable. *Proposed 2026-11-20.*

`crucible.exe probe-plugin <bundle>` loads one plugin and prints one marked JSON line. The parent
kills it after a timeout (default 60 s, because big sample instruments are slow to instantiate).
Rejected: vst3-host's own isolated probe, because it needs a separate `vst3-host-probe.exe`
shipped next to the app. Ours needs nothing extra and is covered by tests that simulate a
crashing, a hanging and a noisy plugin.

## D-020. WASAPI exclusive mode uses the `wasapi` crate, behind our own API. *Proposed 2026-11-20.*

`wasapi` 0.27 is MIT-licensed and maintained by the author of the `rubato` resampler and
CamillaDSP. cpal cannot do exclusive mode (T2). The portable API is `exclusive_capabilities`,
`exclusive_stream_test` and `open_exclusive_output` in `crucible-audio-io`. Format preference: 32-bit
float, then 24-bit in 32, packed 24-bit, 32-bit int, 16-bit. Conversion from the engine's float
clamps and does not dither, which is acceptable for monitoring only.
Verified on the dev laptop's Realtek output: 3 ms buffer, steady timing, zero dropouts.

## D-021. Diagnostics: coded errors, write-through session logs, crash capture. *Proposed 2026-11-20.*

Asked for by Dzyan: "make sure our app is very good at providing error codes and logs that are
easy for you and others to diagnose."

- Every user-visible problem carries a stable code (`E2004`, `W2101`) with a one-line meaning
  and what to try. Codes are grouped by area and map to process exit codes. The list lives in
  `crates/crucible-diagnostics/src/codes.rs`; `docs/11-error-codes.md` is generated from it and a test
  keeps them in step.
- One log file per run in the per-user log folder, written line by line rather than at exit,
  so a crash cannot lose it. The header records build fingerprint, command line, computer, OS,
  CPU and memory. Library messages (vst3-host, wasapi, midir) are captured through the `log`
  facade; vst3-host's step-by-step loading trace is switched on while a plugin loads.
- Native crashes are caught on Windows: the crashing module is named and a minidump written.
- `crucible report` bundles the machine check and recent logs into one file to send.
- Real-time rule kept: the audio thread only bumps counters. Library log records raised on a
  marked audio thread are dropped and counted. The main thread turns counter changes into
  timestamped warnings once a second.

New dependencies, all permissively licensed: `sysinfo` (MIT) for machine facts on all three
systems, `chrono` (MIT or Apache-2.0) for local time, `log` (MIT or Apache-2.0), `windows-sys`
(MIT or Apache-2.0, Windows only) for the crash handler.
Rejected: `tracing` (more than needed today, can replace the bridge later); external crash
reporters such as Sentry or crashpad (send data off the machine, or heavy C++ dependencies).

Verified on the dev laptop with a fake plugin built to crash with an access violation:
the scan survived and named it; `m0-play` logged each loading step, then the crash and module,
and wrote a 180 KB dump.

## D-022. Every user action is a named command; GUI, console and CLI are front ends to the same set. *Decided by Dzyan 2026-11-20.*

Asked by Dzyan: should the DAW be built "CLI first, GUI on top", like Windows or a firewall
that has a shell underneath?

Yes, with one correction: **command-API first, not CLI first.** The engine already takes
typed commands (`02-architecture.md`, "The bridge"). This makes that command set the public,
named surface:

- One registry of commands in the engine: a dotted name (`track.add`, `clip.move`,
  `transport.play`, `project.export`), typed arguments, help text, and whether it is undoable.
- The GUI calls commands by name through the bridge. A button never does work the command
  set cannot do.
- An in-app console runs the same commands as text, and shows the command behind each GUI action.
  Exchange's old management console worked this way with PowerShell cmdlets.
- `crucible` runs the same commands headless against a project file (batch export, tests, scripts).
  Later, optionally, it can attach to the running app over a local pipe restricted to the current user.
- The undo history and crash-recovery journal are lists of these commands, so a journal doubles
  as a replayable script and a bug reproduction.

Not commands: the audio thread (it receives compiled snapshots, never text), a plugin's own
editor window, and the in-between steps of a drag (only the final value is a command).

Rejected: the GUI shelling out to `crucible.exe` and parsing text. It is slow, loses typed errors,
and duplicates state. Also rejected: string commands inside the engine. Text is parsed only at
the console and CLI edge, into the same typed Rust values the GUI sends.

Cost: every feature needs a command definition before its button. That costs a little upfront
and pays back in tests, scripting and diagnosis.

Implemented 2026-11-20 at Dzyan's request, ahead of the rest of M2: the command framework
(`crucible-engine/src/command.rs`), the engine's project and track commands with undo, redo and the
crash journal (`crucible-engine/src/lib.rs`), the project store (`crucible-project`), and the `crucible`
CLI with one-shot, `-c`, `run` script, `shell` and `--json` modes. Not built: transport, audio
graph and plugins in the engine. Those wait for M0 to pass. The attach-to-running-app pipe waits
for the desktop app.

Implementation choices:
- A command is a row: name, typed arguments, help, effect, and a plain function. No macros, traits
  or dynamic dispatch beyond the function pointer.
- Undo stores whole copies of the project model, which is small plain data. No inverse commands
  to get wrong.
- History and the journal store a canonical text form with tracks as stable ids (`#3`), so a
  replay means the same thing after renames and moves.
- Track names may repeat; a name that matches more than one track is refused (E5004) and the
  position or id is used instead.

## D-023. The product is called Crucible. *Decided by Dzyan 2026-11-20.*

User-facing text, the `crucible` command-line binary and the per-user folder
(`%LOCALAPPDATA%\Crucible`) use the name.

*Amended by Dzyan 2026-11-20:* he found the leftover `daw` names confusing, so the internals were
renamed too: the repository folder is `crucible` (was `rust-daw`) and the crates are `crucible-*`
(were `daw-*`). The error type keeps the name `DawError`: "DAW" there is the kind of program, not the
old name.
Not checked: trademarks. That only matters if Crucible is ever distributed beyond the small group.

---

## D-024. Build philosophy: small, fast, robust; the GUI must not cost the engine. *Decided by Dzyan 2026-11-20.*

"Think John Carmack style of programming ... We want that philosophy here." Written up as the six
rules and the budget table in `01-vision-and-priorities.md`, and the motion and effects budget in
`05-gui-design-brief.md`. Claude is asked to push back on GUI requests that cost frames or engine
time and to offer a middle ground.

## D-025. The first window is a full clickable mockup on the real engine. *Asked for by Dzyan 2026-11-20.*

He asked for the GUI now, "all the theoretical tabs and buttons ... with smooth, but minimal
animations", to evaluate the look. Built as `app/` (`crucible-app.exe`), ahead of M0 passing.
- Everything the engine can already do is real and goes through commands: tracks, gain, pan,
  mute, solo, rename, move, remove, tempo, time signature, sample rate, master gain, new, open,
  save, undo, redo and history. The console tab runs any command.
- Everything else is a stand-in in `app/src/demo.ts` with a "demo" tag on screen: transport,
  clips, piano-roll notes, meters, loudness, devices, plugins, settings values, export.
- Only what `05-gui-design-brief.md` lists is on screen, nothing extra: top bar, track headers,
  timeline, and Mixer, Piano roll, Clip editor and Console tabs. Settings is one page. Menus are
  one level deep. Claude added the in-app console from D-022 and history in the project menu.
- Motion: 120 ms transform and opacity transitions on state changes only. The draw loop stops
  when nothing moves. Work per playing frame measured at 0.38 ms in Chromium (budget 8 ms), after
  batching waveform drawing into one path. Before that it was 2.1 ms.
- Font: the system font (Segoe UI Variable on Windows) for now. The brief says to bundle one; the
  candidate, Inter, is OFL-licensed, which needs Dzyan's yes under the licence rule.
- The M1 stress items (5,000 clips, 32 meters from Rust, plugin window focus) are still open.
- Measured on the dev laptop, release build: window up and demo song built 0.69 s after launch
  (budget 1 s). `crucible-app.exe` is 9.4 MB. **Memory misses the 150 MB budget:** 391 MB private
  across 7 processes, the app at 29 MB and the rest WebView2's own browser, GPU and renderer
  processes. This is the baseline cost of a web view, not of Crucible's code. To decide at M1: count
  the web view separately in the budget, trim it (WebView2 flags, single renderer), or take it as a
  reason for the egui fallback (D-006).

---

## Facts that decisions rest on, with dates

Verified on 2026-11-19 unless noted. Sources in `10-references.md`.

- The VST3 SDK is MIT-licensed from version 3.8, released October 2025.
- The ASIO SDK is available under GPLv3 or Steinberg's proprietary licence.
- Reaper is closed source. Ardour, Zrythm and Tracktion Engine are open and GPL.
- VST3 is cross-platform: Windows, macOS, and Linux since 2017. Each plugin is compiled per system.
- Spectrasonics supports Windows and macOS only.
- The `vst3-host` crate is MIT. Its docs say third-party plugins are exercised mainly on macOS.
- Tauri's latest stable was 2.12.7 on 2026-11-18. Tauri uses WebView2 on Windows.
- cpal 0.18.2 opens WASAPI in shared mode only (read in its source, 2026-11-20).
- vst3-host 0.11.1 splits a driver block larger than the plugin's maximum block into chunks
  itself, so a driver that grants a bigger buffer than requested is safe (read in its source).
- The dev laptop's Realtek output accepts 16-bit and 24-in-32 at 48 kHz exclusively, not float,
  with a 3 ms minimum period (measured 2026-11-20).
- vst3-host 0.11.1 does not run a Windows message loop. The host app must pump messages on the
  thread that opened the editor (read in its source, 2026-11-20).
- vst3-host's `play_with_backend` locks the plugin mutex inside the audio callback. Its
  `RealtimePluginRunner` is lock-free but owns the plugin, so the editor cannot reach it. The
  real engine needs its own split between processor and editor (read in its source, 2026-11-20).
