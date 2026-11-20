# 01. Vision and priorities

## Why this exists

Dzyan finds Reaper capable but too complex. Most DAWs were built over thirty to forty years and
carry every option ever added: dense menus, tiny toolbars, hundreds of preferences. The goal is
a DAW with only the essentials and an interface that looks and feels modern.

This is not a money-saving project. Reaper costs sixty dollars and free DAWs exist. It is a
learning project in Rust that should end as a tool a few people genuinely prefer to use.

## Who it is for

- Dzyan, on three Windows machines.
- His family.
- A few fellow sound engineers. Whether they use Windows or Mac is not yet known.

It is not a mass-market product. It does not need to support every plugin or every workflow.
It does need to be trustworthy: engineers forgive a missing feature, not a lost take.

## Priorities, in order

1. **Clean, modern GUI.** One calm window, an uncluttered top bar, nothing that looks twenty
   years old. See `05-gui-design-brief.md`.
2. **Transparent sound.** The engine adds nothing and removes nothing. See `04-audio-quality.md`.
3. **Low latency.** Playing Omnisphere and Keyscape from a MIDI keyboard must feel immediate.
4. **Lossless export.** No quality loss on the way out unless the user picks a lossy format.
5. **Portable projects.** The same project folder opens the same way on all three machines.
6. **Windows first, never Windows-only.** See `03-platform-notes.md`.

When two conflict, the lower number wins. Example: a feature that clutters the top bar loses
to priority 1 even if it is useful.

## How it is built: small, fast, robust

Dzyan's direction (2026-11-20, D-024): build it the way id Software built Doom and Quake. Small,
fast and dependable, with nothing in it that is not pulling its weight.

1. **Plain data, plain functions.** The project is a struct of values. Commands are rows in a
   table. No class hierarchies, frameworks or layers that only pass calls along.
2. **Few dependencies.** Each crate or npm package needs a reason recorded in `08-decisions.md`
   or `10-references.md`. A few lines of our own code beat a large library for a small job.
3. **Measured budgets.** The table below is tracked in `STATUS.md`. Breaking one is a bug.
4. **Fail loudly, fail safe.** Input is validated at the edge. A failed command changes nothing.
   Saves are atomic. Edits are journaled. Every error has a code (`11-error-codes.md`).
5. **One way to do each thing.** Every action is a command (D-022). The GUI is a view and a
   remote control, never a second implementation.
6. **No noise.** No feature without a use, no setting for something that should just work, and no
   decoration that costs frames.

Budgets, first draft, measured on the weakest machine (the laptop):

| What | Budget |
|---|---|
| Audio thread | No allocation, lock or I/O. Zero overruns in a 60-minute session at the chosen buffer |
| Edit command | Under 1 ms for a 200-track project, excluding disk |
| Startup | Main window usable in under 1 second, plugins loading afterwards |
| Idle | Near 0% CPU when nothing plays and nothing moves |
| GUI frame | Under 8 ms of the 16 ms frame at 60 fps, with a 5,000-clip timeline |
| Memory | Under 150 MB for the app itself, plugins excluded |
| Size | `crucible.exe` under 5 MB (1.6 MB on 2026-11-20). Desktop installer under 20 MB |

## Reference targets

- **Spectrasonics Omnisphere and Keyscape** must load, show their editors, play from MIDI,
  save and recall their state, and render to file. These are the acceptance test for plugin hosting.
- **Mainstream VST3 plugins** should work. Obscure ones are allowed to fail.
- The finished music must translate: good on ordinary headphones, good on basic powered
  speakers, better live. The DAW helps with tools, not magic. See `04-audio-quality.md`.

## In scope for version 1

- Audio tracks: record, play, trim, split, move, fade.
- MIDI tracks driving VST3 instruments: record, play, a basic piano roll.
- VST3 effects as inserts on tracks and on the master.
- Per-track gain, pan, mute, solo. A master bus.
- Metronome and tempo.
- Undo and redo.
- Project save, autosave and crash recovery.
- Export to WAV and FLAC, with safe defaults.
- Loudness and true-peak meters, a mono check button, a reference-track A/B switch.
- Device settings stored per machine.

## Explicitly out of scope for version 1

Each of these is a deliberate "no", not an oversight. Adding one needs Dzyan's decision.

- Video, notation, scripting, macros, custom toolbars, themes beyond dark and light.
- Surround and immersive formats. Stereo only.
- VST2, AU, AAX, LV2 plugin formats. CLAP may come later because it is cheap to add.
- Complex routing: sends, buses, sidechains. A single aux send may be revisited.
- Time-stretching and pitch correction.
- Control surfaces, ReWire, network sync.
- Per-plugin sandboxing as a user feature. Crash safety is handled a different way, see `02-architecture.md`.
- A plugin or preset marketplace of any kind.

## What "done" looks like for a first usable release

Dzyan records a song using Omnisphere, Keyscape and a microphone on the Lynx machine, opens the
same project on the laptop to arrange, exports a WAV that null-tests clean against expectations,
and nothing crashed or lost data along the way.

## Platform notes

Nothing in the vision is Windows-specific. Two facts shape the later ports:

- Omnisphere and Keyscape ship for Windows and macOS only. Linux users will use other native
  instruments. So the Linux port hosts Linux-native VST3 plugins and is not tested against Spectrasonics.
- macOS has low-latency audio built in, so the driver question in priority 3 mostly disappears there.
