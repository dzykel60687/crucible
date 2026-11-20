# 06. Roadmap

Each milestone has pass or fail criteria. A milestone is not done until Dzyan has confirmed the
parts that need hardware or ears. Time estimates are Claude's rough guesses, not commitments.

The order is deliberate: the two riskiest unknowns are tested first with throwaway code, before
anything is built on top of them.

## M0. The Omnisphere test program

**Question it answers:** can a Rust program host Omnisphere on Windows with low latency?
If not, the project as specified is not viable in Rust and Dzyan needs to know that early.

Throwaway code in `spikes/m0-omnisphere`. No Tauri. A plain window or console program.

**Step A, machine-verifiable on the dev workstation:**

- [x] Lists audio output devices. ASIO devices appear once the ASIO build feature is enabled (Q4).
- [x] Lists MIDI input devices.
- [x] Scans the standard VST3 folders and prints each plugin's name, vendor, class ID and type.
- [x] Scanning runs in a separate process, so a plugin that crashes on load does not kill the program.

Done 2026-11-20 as `daw diag`, now `crucible diag`. Verified on the dev workstation, which has no plugins, so the
"prints each plugin" item is proven only by tests and broken fake bundles.

**Step B code:** `spikes/m0-omnisphere` (`m0-play`). Untested. Checklist:
`docs/test-reports/2026-11-20-M0-stepB-checklist.md`. Known limit: shared-mode audio only, so the
latency item is tested through `--exclusive` (WASAPI exclusive mode). ASIO waits on Q4.

**Step B, needs Dzyan and his hardware:**

- [ ] Loads Omnisphere. Loads Keyscape.
- [ ] Opens each plugin's editor in a floating window. The editor is fully usable, including typing in the patch browser.
- [ ] Notes from a MIDI keyboard produce sound through the audio device.
- [ ] Latency feels immediate at a buffer size the machine sustains without dropouts.
- [ ] Sixty minutes of playing and preset switching with no crash and no dropout.
- [ ] Plugin state saves to a file. After a restart it restores and the same patch sounds.
- [ ] Renders ten seconds of a held chord to a WAV file offline.
- [ ] Repeated on all three machines: Lynx through ASIO, Behringer through its driver, laptop through exclusive mode.

**Exit:** all of step B passes on the Lynx machine and at least loads and plays on the other two.
**If it fails:** record exactly what failed in `08-decisions.md` and stop for a decision. Options
then are a lower-level host layer, an out-of-process host, or a C++ plugin-hosting sidecar.

Rough size: days to write, then as long as the test loop with Dzyan takes.

## M1. The Tauri stress prototype

**Question it answers:** can a web-technology interface stay smooth doing DAW things, and do
plugin windows behave inside a Tauri app?

- [ ] A Tauri 2 app starts and shows the main-screen mockup from `05-gui-design-brief.md`.
- [ ] A canvas timeline with five thousand synthetic clips scrolls and zooms at sixty frames per second.
- [ ] Thirty-two meters update sixty times a second, driven from the Rust side over the bridge.
- [ ] A playhead moves smoothly, interpolated in the UI from engine timestamps.
- [ ] The M0 plugin host is embedded: a plugin editor floats above the main window.
- [ ] Keyboard focus moves correctly between the main window and the plugin window. Space bar still plays and stops.
- [ ] Measured on the Lynx machine and on the laptop.
- [ ] Dzyan has looked at the mockup and said what he likes and dislikes.

**Exit:** all pass, and Dzyan approves the visual direction.
**If it fails:** a few days of optimisation. If still failing, switch the front end to egui.
The headless engine design means nothing else is lost.

The canvas and meter parts can be built and measured in an ordinary browser with fake data
before M0 has passed.

## M2. Engine core

- [ ] Workspace and crates as laid out in `02-architecture.md`.
- [ ] Transport: play, stop, locate, loop.
- [ ] Audio tracks: import, play, record, with latency-compensated recording.
- [ ] Mixing: gain, pan, mute, solo, master.
- [ ] Metronome and tempo.
- [ ] Offline render to WAV through the same render path as playback.
- [ ] Project save, load, autosave and crash journal. *Save, load and crash journal done 2026-11-20 (D-022); autosave not yet.*
- [ ] All machine-checkable tests in `04-audio-quality.md` pass.
- [ ] The command-line harness can do everything above with no UI. *Command layer and `crucible` CLI built 2026-11-20; grows with each feature.*

## M3. MIDI and instruments

- [ ] MIDI tracks, recording from all inputs, playback.
- [ ] VST3 instruments in the render graph, with state saved in the project.
- [ ] A basic piano roll: add, move, resize, delete notes, velocity.
- [ ] Plugin delay compensation.
- [ ] Missing-plugin placeholders that preserve state.
- [ ] A project made on one machine opens correctly on the other two.

## M4. Editing and effects

- [ ] Clip editing: split, trim, move, fade, crossfade.
- [ ] Undo and redo across everything. *Built into the command layer from the start (2026-11-20); covers every edit command.*
- [ ] VST3 effects as inserts on tracks and master.
- [ ] Built-in essentials reused from `..\music-editor`, if Dzyan wants them.

## M5. Quality and translation tools

- [ ] Loudness and true-peak meters, headroom readout, latching clip light.
- [ ] Mono button. Reference A/B.
- [ ] Export dialog with the defaults and warnings in `04-audio-quality.md`. WAV and FLAC.

## M6. Hardening

- [ ] Crash recovery proven by killing the app mid-session.
- [ ] A long-session soak test on each machine.
- [ ] An installer.
- [ ] A plugin compatibility table.
- [ ] First hand-off to another engineer.

## Later, in no fixed order

- macOS port. Linux port. See `03-platform-notes.md`.
- CLAP plugin format.
- Freeze track to audio.
- One aux send.
- Lossy export.
- Out-of-process plugin hosting.

## What can be done without Dzyan present

For unattended sessions. Anything not on this list waits for him.

- Workspace scaffolding and anything that only needs to compile.
- M0 step A.
- The engine's offline render path and every test in `04-audio-quality.md`.
- The project file format, with save and load tests.
- Waveform peak generation.
- The front-end canvas work, measured in a browser with fake data.
- Writing his test checklists.

## Platform notes

- M0 through M6 are Windows work. Each milestone's code still follows the seam rules.
- From M2 on, add a compile check of the portable crates for macOS and Linux targets.
- The macOS port is the natural second platform because the reference plugins run there.
- Do not start a port until M6 is done on Windows. That is Dzyan's stated order.
