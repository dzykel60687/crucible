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

## D-011. No code is copied or ported from GPL or AGPL projects. *Replaced by D-036, 2026-11-23.*

Reason at the time: it kept the licence choice open while Dzyan had not decided whether the
result would be open source. "If he chooses GPL for the whole project, this can be relaxed."
He did (D-036). Nothing was copied or ported while this stood.
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

*Evidence added 2026-11-21, for Dzyan (`12-known-daw-problems.md` P7):* "a plugin's own editor
window" being outside the command set means 30 minutes of tweaking inside Omnisphere is outside
undo, the journal and crash recovery. Proposed fix for M3, not built: the plugin's "state changed"
notifications mark the project dirty, autosave captures plugin state on a timer, and one coarse
`plugin.state` journal entry is written per editor gesture or when the editor closes. The decision
itself stands.

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

*Correction 2026-11-23:* the 391 MB was the wrong measure: committed memory (private bytes),
most of it the GPU process reserving driver memory it does not use. What Task Manager shows,
the private working set summed over all seven processes, is **92 MB** at idle with the demo song
(GPU process 26, browser 27, renderer 22, Crucible 5, the rest small), so the budget is met.
Committed memory varies from 200 to 375 MB between runs with the GPU process. Tried: `--disable-gpu`
saves only 9 MB of real memory and loses GPU compositing; `--in-process-gpu` saves 7 MB and puts
GPU crashes in the browser process. Neither is worth it. Measured with
`tools/measure-memory.ps1`. The budget's unit is now written down in `01-vision-and-priorities.md`.

## D-026. Bundled typefaces: Inter and JetBrains Mono. *Decided by Dzyan 2026-11-21.*

"yes bundle interface types." Inter for the interface, JetBrains Mono for numbers in the console
and readouts. Both SIL Open Font License 1.1, which allows bundling in any app; licences sit beside
the files in `app/src/fonts`. Only the Latin and Latin Extended subsets are bundled (189 KB in
all); other scripts fall back to the system font. Vendored as files, not as an npm package, so
there is nothing to update and nothing extra in the build.

## D-027. Clips and notes live in the engine now; every mouse edit is one command. *Asked for by Dzyan 2026-11-21.*

He asked for intuitive mouse work: dragging, moving, copy and paste, clicking. Faking that in the
UI would have left undo, the crash journal and the CLI blind to it, so clips and notes moved into
the project model ahead of the rest of M2 (transport and audio still wait for M0).
- Model: `Clip { track, start, length, offset, gain_db, content }`, content audio (a source file
  name) or MIDI notes. Positions are integer ticks, 960 per beat, so edits never drift. Format 2;
  format-1 projects open with no clips.
- Commands: `clip.add/remove/move/copy/trim/split/rename/gain/quantize/list/notes`,
  `note.add/remove/edit`, `edit.copy/cut/paste`, `project.demo`. A command naming several clips
  (`#3,#5`) is one undo step. The clipboard lives in the engine so the CLI and the window share
  it; `edit.copy` is journaled so a paste survives a crash (tested).
- Mouse model (`app/src/timeline.ts` header): click selects, Shift adds, Ctrl toggles; drag
  moves in time and across tracks; Ctrl+drag copies; dragging a clip edge trims; dragging empty
  space draws a selection box; a plain click there moves the playhead; double-click empty space
  on an instrument track makes a 1-bar MIDI clip. Snapping follows the grid control above the
  track list; Alt drags free. Drags preview locally and send one command on release.
- Keys: Ctrl+C/X/V, Ctrl+D duplicate, Ctrl+E split at the playhead (export moved to
  Ctrl+Shift+E), Del deletes clips, Ctrl+Del removes the track, arrows nudge.
- Piano roll: drag notes to move, drag the end to resize, velocity lane, Draw tool, double-click
  to delete, quantize. All `note.*` commands.
- Audio waveforms are still made up (no import yet) and say so on screen.

## D-028. Layout themes. *Asked for by Dzyan 2026-11-21.*

"Give user a way to switch between a few different GUI setups ... some people just prefer
different layouts." Five layouts: Studio (default), Compact (laptop), Touch (bigger targets),
Side editor (editors on the right, full-height timeline) and Mixing (mixer takes most of the
screen). Chosen in Settings, Appearance, or with Ctrl+Shift+L; remembered per computer.
The lean rule shaped it: a layout is data, a name on the page that the stylesheet turns into
sizes and grid areas. Every layout runs the same views and code, so a new layout costs a few
lines of CSS and cannot break behaviour. Free-form docking and floating panels were not built:
they multiply states to test and are the clutter this project exists to avoid. A layout preference
is per-machine UI state, not a project command; it becomes a `prefs.*` command when the settings
file exists.

## D-029. Jade green accent and the mark (an anvil since 2026-11-25). *Decided by Dzyan 2026-11-21, 2026-11-25.*

"Instead of orange let's go with green, be a little subtle and seamless about it ... make a
musical/crucible like logo." Accent `#45c08f` (jade), a darker hover, and no orange anywhere:
track colours are cool and muted so the accent and the red record and clip warnings stand out.
The mark was a crucible with a sound wave cut through its body. **Since 2026-11-25 it is an
anvil** (Dzyan: "Logo should also be an anvil instead of the vase"). The first try, side on
with a thin wave, "looks like a T"; the second, from above with notes, did not read either. The
third follows a reference picture he sent: the classic anvil side on (long horn, step block, face
plate with an overhanging heel, deep waist curves, two feet and an arch), its edges drawn as thin
lines, and a sound wave on the side. `app/icon.py` (standard-library Python) draws the app icon
and prints the SVG for `app/src/icons.ts`, from one set of shapes. Accent swatches in Settings
start with the green; the others stay as choices.

## D-030. Pan law: balance, centre is unity. *Proposed by Claude 2026-11-22.*

`04-audio-quality.md` rule 3 says a track at 0 dB and centre pan passes samples unchanged, so
the pan law cannot attenuate the centre. Chosen: balance. Centre multiplies both channels by
exactly 1.0; turning right fades the left channel with a cosine (half right: left at -3 dB) to
silence at hard right, and the mirror for left. Nothing is ever boosted. A mono file feeds both
sides. Tested in `crucible-engine/src/audio.rs` (`gain_and_pan_laws`).
Rejected: constant-power -3 dB centre (breaks rule 3 and the null test), -4.5 and -6 dB centre
laws (same). If Dzyan wants a constant-power option for mono sources later, it is a per-track
setting, never a hidden default.

## D-031. Audio clips keep real time; files are copied in; other rates are refused. *Proposed by Claude 2026-11-22.*

Built overnight because Dzyan asked to test making WAV and MP3 files, mixing and exporting, and
because the roadmap's unattended list allows the offline render path and peak generation.
- An audio clip records its file's length in frames and its rate (project format 3). Its start,
  length and offset stay in ticks like every clip, so editing code is shared. `project.tempo`
  re-expresses audio windows at the new tempo: a clip stays on its beat and keeps its sound and
  duration. `project.rate` is refused while audio at another rate is used.
- `audio.import` copies a WAV byte for byte into `audio/`, or decodes MP3 and FLAC once into a
  32-bit float WAV there. The same file imported twice reuses the copy; a different file with the
  same name gets "name 2". A file at another rate is refused (E5017): no resampler yet, and rule 1
  forbids a silent one. Waveform peaks are computed at import and cached in `audio/peaks/`.
- `clip.place` puts an imported file on a track: an ordinary journaled edit, so crash recovery
  replays it (the copy is already in the folder).
- Overlapping clips on one track are summed, not "last one wins". **Decided by Dzyan
  2026-11-23** (Q12: they "should both play"). Test: `overlapping_clips_on_one_track_both_play`.
- Export renders offline, streaming each file in 4096-frame blocks: memory stays flat (6 MB for
  two 10-minute tracks). `render` takes each clip's samples through a callback, so real-time
  playback can reuse it with buffers a disk thread fills (one render path, rule 5).
Rejected: storing audio windows in frames (two units in every edit command), loading files whole
(321 MB for one 10-minute file, measured), resampling at import with an unreviewed converter.

## D-032. The window gets change lists, not the whole project, after each command. *Proposed by Claude 2026-11-22.*

Measured: on a 2,000-clip, 40,000-note project every edit cost 86 ms in the engine plus about
200 ms through the dev bridge, because the whole 2.7 MB model went back as JSON each time, and
undo snapshots grew memory by about 1 MB per edit (349 MB after 300 edits).
- Each clip is held behind an `Arc` and edited copy-on-write (`Project::clip_mut`). Copying the
  project for an undo step copies pointers. 300 edits: 14 MB.
- The engine keeps the project it last sent and a revision number (seeded from the clock, so a
  restarted engine never reuses one). The front end sends the revision it holds; the engine
  replies with only the clips whose pointer changed, the id order, and the small rest. Anything
  unexpected gets the full state. Edit round trip: 0.5 ms in the engine, about 4 ms through the
  dev bridge.
- The front end sends commands one at a time, in order. The window's `exec` now runs off the
  main thread (`#[tauri::command(async)]`), so a slow command no longer freezes painting, and
  without the queue two quick clicks could have been applied out of order (seen in the browser).

## D-033. Built-in Reverb, IR Reverb, Delay and Echo. *Asked for by Dzyan 2026-11-23; CPU budget Decided, other choices Proposed.*

"I do want some very effective/refined tools (not excessive amounts though) like Reverb, Delay,
Echo ... do deep digging on what exactly makes accurate, authentic reverb ... and try to replicate
that here." Research: `13-reverb-and-delay.md`. Built as its section 9 recommends, in a new crate
`crucible-dsp` (plain arithmetic, no OS code):
- **Reverb**: diffuser (4 steps over 16 channels) into a 16-line FDN with Householder feedback,
  Jot's per-line absorption (a second-order shelf at 3 kHz: the first-order one shortened the 1 kHz
  decay by 12%, measured), 4 slowly modulated lines, seeded early reflections. Room, Hall, Plate.
  Decay is limited by Size (mode-density rule).
- **IR Reverb**: zero-latency partitioned convolution (64 taps direct, then FFT blocks of 64,
  1,024 and 8,192), mono, stereo and 4-channel true-stereo files. Normalise on: trim the silence
  before the sound and match energy; off: the file exactly as recorded (the ReaVerb null test).
  Length below 100% fades the tail; at 100% the IR is untouched.
- **Delay** and **Echo**: one engine; Clean (sample-exact) and Tape (wow, flutter, darker loop,
  soft saturation, feedback past 100% stays bounded). Tempo sync, ping-pong.
- Effects are per-track inserts before gain and pan, stored in the project (format 4) and edited
  only by `fx.*` commands, so they undo, journal and work from the CLI. The window draws each
  editor from the same parameter table the engine validates against. Exports run on for the tails.
- Coefficients and oscillators use Crucible's own `exp`, `ln`, `sin` (`crucible-dsp/src/math.rs`),
  so the algorithmic effects, and now also every gain and pan, render the same bits on every
  machine; the platform maths library may take different paths per CPU.

The open choices in doc 13 section 9, as built (each is his to change):
1. All four effects built. 2. Size changes restart the tail (moot until automation exists).
3. IR files at another rate are refused, like audio (Q13). 4. IR Reverb exports may differ in the
last bit between machines (RustFFT picks SIMD code per CPU); algorithmic effects do not.
5. No impulse responses ship; `dist/test-audio/ir-synthetic-hall.wav` is a generated test file.

Measured (release, laptop, 48 kHz, 128-frame blocks): Reverb 2.0% of one core (doc 13 proposed
1%), Delay 0.08%, Echo 0.28% (proposed 0.2%), IR Reverb with a 3 s true-stereo IR 1.7% (proposed
2%), but its worst block 35% of a 128-frame callback (proposed 25%) because the 8,192 block's FFT
lands in one callback. Offline export does not care. For live playback (M2) that work moves to a
helper thread or is spread over callbacks. Proposed: accept 2% for Reverb.

**Decided by Dzyan 2026-11-23:** "I'm fine with Reverb and other effects using more CPU and Memory
if it makes it higher quality." So the 2% for Reverb stands, and quality decides over CPU from
here on. Nothing was changed blind on that licence: the candidates that trade CPU for quality
(modulating all 16 lines instead of 4, oversampling the tape saturation) wait for his first
listen, because only ears can say whether they help. Also 2026-11-23: effects go idle on the same
sample whatever the block size (`filter.rs` `Idle`), and the convolver rejoins the same block grid
after idling, so playback and export match bit for bit; before, the two differed below -150 dBFS.

## D-034. Live playback renders ahead on a feeder thread. *Asked for by Dzyan 2026-11-23; design Proposed.*

"It needs to work while testing/recording tracks, not just on export." Built before M0 passed on
the music machines, at his request, like D-022 was. It depends on nothing M0 tests (no plugins);
the device layer under it is the exclusive-mode stream verified on 2026-11-20.

Design (`crucible-engine/src/play.rs`):
- A **feeder thread** runs the same `audio::render` an export runs, about 40 ms ahead of the
  listener (or three device buffers, if that is more), into a lock-free queue. It reads the clip
  files and runs every effect. It runs at audio priority (MMCSS on Windows).
- The **audio callback** only copies from the queue to the device. Nothing in it allocates, locks
  or waits; a test counts its allocations.
- An edit builds a new mixer on the command's thread and sends it over. The feeder moves the
  running effects into it (`Mixer::inherit`): an unchanged effect moves whole, a changed one is
  built fresh and adopts the old one's delay lines and filter history (`Effect::adopt`), so
  reverb and echo tails survive knob moves. A changed room size or impulse response starts empty.
- Commands: `transport.play`, `transport.stop`, `transport.seek`, `transport.status`, and
  `audio.output` (driver, device, buffer; kept per machine beside the logs, never in a project).

Why not render inside the callback, as `02-architecture.md` first said: the callback would then
have to read disks through per-clip ring buffers, and IR Reverb's largest FFT (35% of a 128-frame
callback, D-033) would need its own helper thread. Rendering ahead removes both problems and
keeps one render path, so **what is heard is what an export writes, bit for bit** (tested).

Cost: an edit is heard about 40 ms plus the device buffer later. That is fine for mixing and
wrong for playing an instrument, so **live-played tracks (instruments in M3, input monitoring)
will be rendered inside the callback** and summed with the feeder's output. REAPER does the same
and calls it anticipative FX processing. The device's own latency is unaffected either way.

Rejected: loading every audio file into memory (a 30-track song would hold gigabytes); rendering
everything in the callback now (above).

Measured on the dev laptop (release build, demo song: 4 tracks, Reverb, Echo, IR Reverb): through
the Realtek output in exclusive mode at a 144-frame (3 ms) buffer, 36.6 s, 0 dropouts, mixing 6% of
one core; shared mode the same. Opening the device takes place on Play and it is released on Stop.

Not built: loop playback, record, metronome, scrubbing sound, a device picker in Settings (the
`audio.output` command does it for now), holding the device open between stop and play.

## D-035. ASIO is compiled into every build. *Decided by Dzyan 2026-11-23.*

"We definitely want ASIO working regardless." He approved downloading the Steinberg ASIO SDK,
LLVM and WiX the same day. The ASIO path is cpal's `asio` feature, on by default in
`crucible-audio-io`. Since D-036 the SDK's host files are in the repo (`third-party/asiosdk`), so the only
thing a build machine needs besides Rust is LLVM's `libclang.dll`, kept beside the repo in
`..\crucible-sdk` (`.cargo/config.toml` points at both). Machines that run Crucible need nothing extra.

Driver choice stays D-018's order: ASIO if a driver is installed, else exclusive, else shared with
a note. One output function covers all three (`output.rs`).

**Licence, settled 2026-11-23 (D-036):** Crucible is GPLv3, so the SDK is used under its GPLv3
option. No agreement with Steinberg is needed, for private or shared builds.

Not verified: any ASIO stream. The dev laptop has no ASIO driver, so only "ASIO: 0 devices" and
the no-driver error were seen. The Lynx machine is the first real test.

Found on the way and fixed: device output scaled floats by 2^23 - 1 while import divides by 2^23,
so a 24-bit file played at unity gain lost its last bit on loud samples. Output now uses the same
power of two as import and export, and a test checks every 16-bit value and a sweep of 24-bit
values come back exactly.

## D-036. Crucible is free software under the GNU GPL, version 3 or later. *Decided by Dzyan 2026-11-23.*

"GPLv3, this can be totally open software, I don't care about trying to 'own' it." Answers Q4
and Q5.

- **Licence:** `GPL-3.0-or-later` in `Cargo.toml` and `app/package.json`. The GPLv3 text is
  `LICENSE` at the repo root and is installed beside the program as `LICENSE.txt`. "Or later" is
  Claude's reading of "GPLv3" (it is the FSF's own default wording); it can be changed to
  version 3 only at any time before the first public release.
- **ASIO:** the SDK is used under its GPLv3 option (the SDK's `LICENSE.txt` offers "the Steinberg
  ASIO License, or alternatively ... the General Public License (GPL) Version 3"). Nothing to
  sign. "ASIO" is still Steinberg's trademark: Crucible uses the word only to name the driver
  type and does not use the logo.
- **Dependencies checked the same day** (`cargo metadata` for the Windows build): 286 third-party
  crates, 261 of them linked into the programs. Every licence is one the FSF lists as compatible
  with GPLv3: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, Unicode-3.0, or a choice that includes
  one of those. The bundled fonts are OFL and ship as separate files. The VST3 SDK is MIT.
- **Closed plugins:** hosting them (Omnisphere, Keyscape) is unchanged. Other GPL hosts do the
  same, and the licence binds people who receive Crucible, not its author.
- **Replaces D-011.** Code from GPLv3-compatible projects (GPLv3, GPLv2-or-later, LGPL) may now
  be ported when that is the better engineering. Each use is named in `10-references.md` with
  its source and licence, and the file keeps the original notice. Still not allowed: AGPL code
  (Zrythm, JUCE 8), GPLv2-only code, code with no licence. Check the project's licence at the
  time, not from memory. Dependencies outside the permissive list are still raised first.
  Nothing has been ported, and the lean rule argues for keeping it rare.

**Published 2026-11-23**, at his word the same evening: "Go ahead and push it along with
installer to public repo." Done first:

1. `THIRD-PARTY-NOTICES.txt`: every third-party crate linked into the programs (261), the ASIO
   SDK, the fonts and the JavaScript API package, with their licence texts. Generated by
   `tools/third-party-notices.ps1`, committed, and installed beside the program. Re-run the
   script after any dependency change.
2. The ASIO SDK's host part is in the repo: `third-party/asiosdk`, 11 files, unchanged, under
   the SDK's GPLv3 option, and the build uses them. The repo at a release tag is therefore the
   whole source of that installer (GPLv3 section 6).
   Left out on purpose: `combase.cpp`, `combase.h`, `dllentry.cpp` and `wxdebug.h`. They sit in
   the SDK but are Microsoft sample code ("All Rights Reserved") without Steinberg's licence
   notice. They are for writing drivers; `asio-sys` used to compile them because it compiles
   every file in the folder. They are no longer compiled at all.
3. Checked before publishing: every file and every commit for employer names, e-mail addresses,
   the machine name, keys and tokens. None found.
4. Then: commit, push, release `v0.0.1` (marked pre-release) with the installer attached, and
   the repository's visibility set to public.

Older commits say `license = "MIT"` in `Cargo.toml` and have no licence file. They stay readable
in the history; Dzyan is their only author.

## D-037. The session log is a full record of a session, and the window makes the report. *Asked for by Dzyan 2026-11-23; the design is Claude's, Proposed.*

His words: testing on another PC, "I need to be able to provide a log file of basically
everything I was doing, what was happening behind the scenes, and send it to you", and from it
alone "you should be able to parse ... what needs to be done". Before this the log held command
text, coded errors and crashes, and nothing else: not what a command answered, not which device
playback opened, no dropouts, nothing from the window.

What a log holds now (`docs/11-error-codes.md` lists every line type):

1. **Every command with its outcome**: how long it took and what it answered. One helper
   (`crucible-engine/src/trail.rs`) used by the command line and the window alike.
2. **Playback**: the device, driver, format and buffer opened; a health line every ten seconds;
   each dropout when it happens, with its cause and the position in the song; late wake-ups;
   each edit taken over during playback; the totals at the end. The audio thread still only
   counts (real-time rule); the lines are written from the command thread when the window asks
   for the play position, twenty times a second.
3. **The window**: clicks, double-clicks, right-clicks, drags, keys, dropped files, every
   message shown, resizes, freezes, its own errors (code E9003), focus changes. They travel as
   `log.ui` commands, because the window has no file access (D-022), and are written before the
   command they caused. Never recorded: text typed into fields, mouse movement.
4. **The person's own notes**: F9 in the window, `mark` anywhere. The moment is logged at the
   key press; the words follow with "noticed N s before this line".
5. **Which build**: the commit in every header and in `crucible version`, beside the fingerprint.
6. **A watchdog**: a warning when a command has run for 5, 30, 120 and 600 seconds, so a hang
   names its command.

The report: Settings, Diagnostics, Create report file (or `report` in the Console tab) writes one
file to the Desktop. The window first copies the project and the play state into its log
(`log.snapshot`), then starts `crucible.exe report`, which owns the machine check. One
implementation, two ways in.

Costs, measured or bounded: nothing per frame in the window (lines are batched, at most one
engine call per 400 ms, plus a two-per-second timer for freeze detection); about 40 log lines
per minute of playback; a log of a long session stays under a megabyte. A machine that cannot
keep up writes one dropout line a second at most.

Rejected:
- A second, structured log (JSON lines) beside the text one. Two files to keep in step; the text
  lines already have fixed shapes and fixed tags.
- A network upload. Crucible makes no connections; the person sends the file.
- Logging from the audio thread or the mixing thread. A slow disk would cause the very dropout
  being logged.
- Recording what is typed into fields, or every mouse move. Privacy and volume.

**Where the log is, decided by Dzyan 2026-11-25:** one file "in the root application directory",
live while the program runs, and "if the app restarts, it should just write over that same log
file". Built that way: `crucible-app.log` beside the program, started afresh each run; the 200
dated files in a hidden folder are gone. Two things added to his words, both Claude's: the run
before is kept once as `.previous.log` (otherwise restarting after a crash erases the log of the
crash), and where Windows will not let the program write beside itself (an install under Program
Files) the file goes to `%LOCALAPPDATA%\Crucible`. A per-user installer would put program and
log together in a folder he can write; that is his to ask for.

**Log everything while testing, decided by Dzyan 2026-11-25** ("Can't we just have it log everything
while the program runs? ... this is while we're testing it"). Until Crucible is released for real
the log records everything it can (`everything()` in `crucible-diagnostics`; `CRUCIBLE_LOG=normal`
gives the quieter log): a line every second while the window is open (`ALIVE`: what the engine is
doing, playback that second, memory, processor), every command's whole answer, every step of
opening a device, every device notice from Windows, what the mixing thread does, every press, key,
wheel burst, field focus and popup in the window, and how fast it draws. Library detail that only
repeats this (thousands of format checks) is left out. About 1 MB per hour idle.

**Where the log is, part two (2026-11-25):** "Log file should always be churning and available in
the root directory of application." Installed under Program Files, the program cannot write its own
folder, so the log had gone to `%LOCALAPPDATA%\Crucible`. From 0.0.3 the installer installs per
person, in `%LOCALAPPDATA%\Programs\Crucible`, needing no administrator rights, and the log sits
beside the program there. `app/src-tauri/wix/main.wxs` is Tauri's own WiX template with that
change; an older per-machine install must be removed by hand first (Windows Installer will not
upgrade across the two).

Verified by machine on 2026-11-23 (`docs/test-reports/2026-11-23-devlaptop-logging-and-report.md`).
Not verified: a real problem diagnosed from a log sent from another PC.

## D-038. The window shows only what works, and the devices it shows are real. *Asked for by Dzyan 2026-11-25; the details are Claude's, Proposed.*

His first real use found Stop dead and the mixer faders "shaking up and down" during playback,
and he asked for the demo content out, devices reported accurately, a way round programs
"stealing" the sound driver or at least detection of it, and volume sliders by the tracks.

**Why Stop died and the faders shook.** Both came from one fault. While playing, the window
rebuilt its whole page every time the dropout count changed. A rebuilt fader spent one frame at
the top before being placed, and a rebuilt button is a new element: a click whose press and
release straddle the rebuild is thrown away by the browser. On his PC the count was changing all
the time, so the faders flickered and Stop never registered. Fixed at three levels:

1. Nothing is rebuilt for a number. Dropouts, position and levels are written into the existing
   elements; the play button's icon is replaced only when its state changes.
2. The page is never rebuilt while a mouse button is held. The rebuild waits for the release
   (`store.ts`). This protects every button and every drag, not just Stop.
3. A fader's place comes from a CSS variable, so it is right in its first frame.

**Why the count was changing all the time** is not known for certain: he tested on another PC
and its log was not sent. The likely cause was found and fixed: the playback queue was sized from
the buffer the driver reported in its first callback, and shared-mode and some ASIO drivers ask
for more later; every such request then ran the queue dry. The queue now sizes itself from what
the driver really takes (`play.rs`, `Shared::want`), and the log says when that differs.

**What left the window** until the engine can do it: the demo song at start-up; record, loop and
metronome buttons; arm buttons; the loudness readout, mono and A/B buttons; made-up plugin names
and the plugin picker; the MIDI and Plugins pages of Settings; made-up waveforms. A control that
does nothing reads as a broken program. `project.demo` remains as a command: tests use it.

**Devices** (`crucible-audio-io/src/wasapi_endpoints.rs`, `crucible-engine/src/devices.rs`,
`app/src/devices.ts`):
- `audio.devices` lists every output and input Windows has, the default, and the rate, bit depth
  and channels Windows runs each at. `--probe` adds the rates each accepts in exclusive mode and
  the ASIO drivers. Settings, Audio shows all of it and sets the output, driver and buffer.
- Plugging in and unplugging: Windows tells the engine (an endpoint notification callback); the
  window asks the engine every two seconds whether anything was reported and lists again only
  then. A change shows as a message and a log line.
- The top bar names the output that will be used and its rate; while playing, what the driver
  really gave.

**"Stealing" the driver.** What can and cannot be done:
- Exclusive mode and most ASIO drivers give one program the device. That is what makes them
  bit-exact and low-latency, and it is why a browser goes silent beside REAPER on ASIO. A program
  cannot share a device another program holds this way: the driver refuses.
- Crucible's own behaviour: it holds the device only between Play and Stop. The **Shared** driver
  in Settings lets every other program keep playing, at the cost of Windows mixing and, when
  rates differ, converting the sound. D-018's order (ASIO, exclusive, shared) is unchanged; whether
  Shared should be the default is his call (it trades priority 2, transparent sound, for
  convenience).
- Detection: before opening an output Crucible lists the programs that have sound on it. If they
  will be silenced it says so when playback starts, and "device in use" errors name them.
  Limit: a program holding a device through ASIO is invisible, because ASIO goes round Windows.

**Volume by the tracks.** Each track header has a slider: the mixer's fader lying down, the same
travel and the same command. Faders at the same level now sit at the same height on every strip.
Named "Volume" in the window; the command stays `track.gain`.

**The GUI is tested by machine before he sees it** (`tools/gui-test.ps1`): the real window,
clicked through its debugging port with real mouse events held about 90 ms. Every button must do
something; every popup must open on screen with nothing squashed; nothing may move during
playback, also while dropouts are forced by freezing the mixing thread; Stop must stop on the
first click. Earlier tests clicked instantly and only while stopped, which is how both faults got
through. Every GUI bug found from now on gets a check there.

Not verified: plugging a real device in and out; the "in use by" names (nothing else was playing
on this laptop during the tests); anything on the PC where the faults were seen.

## D-039. Imported files stay as they are; other formats and rates play from memory, converted only when agreed. *Asked for by Dzyan 2026-11-25.*

His first session at home with a Behringer UMC202HD: importing a 4-minute song took 7 to 8 seconds,
the project's audio folder filled with 80 to 200 MB WAV files ("a big no-no, shouldn't they be kept
in Memory? Need to keep them highest quality"), files at another rate were refused, and he wants
the option to convert instead.

Why it was slow and big: import decoded every MP3 and FLAC into a 32-bit float WAV in the project
folder, five to ten times the size of the file, on his project drive. Writing and reading that back
was the 7 to 8 seconds.

Now (`crucible-engine/src/audio.rs`):
- **The file is kept as it is.** Import copies it byte for byte, in its own format, into the
  project's `audio` folder: an MP3 stays an MP3. The project folder stays self-contained, so it
  still opens the same on every machine (priority 5).
- **What plays from where.** A WAV at the project's rate streams from disk, as before. Anything else
  (MP3, FLAC, a file at another rate) is decoded, and converted to the project's rate if needed,
  once, into memory, and plays from there. Exactly what plays is what exports.
- **Memory:** about 23 MB per stereo minute at 48 kHz, 92 MB at 192 kHz. The import message, the
  log and the convert question say how much.
- **Conversion is asked for, never silent.** A file at another rate is refused (E5017) unless the
  person agrees: the window asks "Convert to 48 kHz?" (or offers to set an empty project to the
  file's rate); commands take `--convert`. `project.rate` does the same for audio already placed.
  A converted clip says so in the clip editor.
- **The converter:** rubato's FFT resampler (MIT or Apache-2.0) in 64-bit floating point, one
  channel at a time. Tested: a converted sine is within -100 dB of a perfect one; length is the
  exact ratio. Like IR Reverb, its last bit may differ between processors (FFT code picks
  instructions by CPU).

Pushback recorded with him: converting a 48 kHz file up to 192 kHz adds no detail and costs four
times the memory. The reason the window pushed him toward 192 kHz was a wrong message in
Settings (it said Windows would convert, which is not true with ASIO); fixed. Keep the project at
the songs' rate; ASIO sets the interface to it.

Not verified: his 4-minute songs on his drive (this laptop: a 16 s MP3 imports in 39 ms).

## D-040. ASIO finds the interface by its other name; MIDI keyboards are listed. *Found by Dzyan 2026-11-25.*

- His UMC202HD is "OUT 1-2 (BEHRINGER UMC 202HD 192k)" to Windows and has another name to its
  ASIO driver, so choosing it in Settings and then ASIO failed with E2002. Outputs and drivers are
  now matched by their distinctive words (`closest_name` in `crucible-audio-io`; "UMC" matches
  "UMC ASIO Driver"); with ASIO chosen and only one ASIO driver installed, that one is used. The
  Settings list shows ASIO drivers when ASIO is chosen. Automatic now picks ASIO whenever the chosen
  output has an ASIO driver.
- The Settings rate message depends on the driver: with ASIO, the driver is set to the project's
  rate and Windows' own setting does not apply.
- MIDI inputs (his M-Audio Axiom 61) are listed in Settings and noticed when plugged in or out.
  Playing an instrument from them comes with plugin hosting.
- Ctrl+C, Ctrl+X and Ctrl+A act on clips only when no text is selected, so text can be copied out
  of the Console.

## D-041. Crucible never goes online; a newer installer is the update. *Decided by Dzyan 2026-11-25.*

He first asked for the installer to work as an updater too ("we will likely be updating many many
times"). Claude built an updater inside the program that asked GitHub for new releases. Before it
shipped he decided: "i don't want this app or installer trying to go online, needs to be self
contained." So:

- **Neither the program nor its installer makes any network connection.** The in-program updater
  was removed before any release had it. The installer no longer downloads Microsoft's WebView2
  runtime when it is missing (`webviewInstallMode: skip` in `tauri.conf.json`): Windows 11 and
  up-to-date Windows 10 already carry it. On a PC without it the window cannot open; the fix is
  Microsoft's WebView2 runtime installer, run by hand.
- **Updating = running the newer installer.** It replaces the older version in place (Windows
  Installer major upgrade: same upgrade code, higher version number; `MajorUpgrade` in
  `app/src-tauri/wix/main.wxs`). Projects, settings and per-machine audio choices are kept. The
  one exception is 0.0.2 to 0.0.3, which moves from a per-machine to a per-user install: uninstall
  the old one once.
- Rejected: the in-program updater (network); a "check for updates" button (network); embedding
  WebView2's offline installer (about 130 MB more per installer, and a download when building).

Not verified by machine: an in-place upgrade from one installed version to the next (Claude does
not install on the build laptop). It is the installer's standard upgrade path, configured as above.

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
