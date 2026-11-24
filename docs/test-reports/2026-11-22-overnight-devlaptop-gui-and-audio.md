# Test report: overnight GUI and audio pass (dev laptop)

Dzyan asked, 2026-11-22: "play with and actually test the functionality of the app ... create
mp3/wave files, play with mixing, exporting, interact with the GUI, make sure things work as
expected and there isn't awkward latency bugs".

## Setup

| Item | Value |
|---|---|
| Date | 2026-11-22, overnight, unattended |
| Build tested | working tree after commit `5845a0b`, uncommitted |
| Machine | Dev workstation (Lenovo laptop). Not a music machine |
| Windows version | Windows 11 Pro for Workstations, build 26200 |
| Audio device | none used: nothing was played through a device |
| Front end | `npm run dev` in the browser pane (dev bridge, debug engine), plus the release window launched and closed |
| Test audio | `dist/test-audio/`, made by `make_test_audio.py` (standard-library Python); `lead.mp3` made by Windows' own MP3 encoder (`to-mp3.ps1`, Media Foundation, nothing downloaded) |

## What a machine verified

| # | Area | Result |
|---|---|---|
| 1 | Engine stress, 48 tracks, 2,000 clips, 40,000 notes | Before: 86 ms per edit round trip in the engine, about 280 ms through the bridge, memory +1 MB per edit. After D-032: 0.5 ms and 4 ms; 14 MB after 300 edits |
| 2 | Timeline draw at that size | 1.35 ms per frame, 0.55 ms zoomed out |
| 3 | Track headers: mute, solo, add audio and instrument tracks | Pass after fix (see bugs 1) |
| 4 | Mixer: fader drag, pan knob drag, double-click resets, M and S | Pass after fixes (bugs 1, 3) |
| 5 | Timeline: drag move (snapped, one undo step), drag to another track, illegal drop refused with a toast, edge trim (both edges), box select, Ctrl+drag copy, Alt fine move, double-click creates a MIDI clip, Ctrl+E split, Ctrl+C/V/D, Del, Ctrl+Z | Pass; real mouse input for move, cross-track, trim and box select, synthetic events for the rest |
| 6 | Paste after box-selecting two tracks | Fail, then pass after fix (bug 4) |
| 7 | Piano roll: open by double-click, drag note (pitch and time), resize end, velocity lane, draw tool, double-click delete, quantize | Pass. Usability fixed (bug 5) |
| 8 | Console: commands, typo suggestion, help, chained statements, history, Tab completion | Pass |
| 9 | Settings, Export and plugin dialogs open and close; Esc closes; five layouts cycle with Ctrl+Shift+L | Pass |
| 10 | Save and open through the dialogs, including folder names with spaces, `#` and `;` | Pass |
| 11 | Audio import: 24-bit, 16-bit, 32-bit float WAV, mono and stereo, MP3; same file twice reuses the copy; other name clash gets "name 2" | Pass |
| 12 | Refusals: no project folder (E5015), 44.1 kHz file in a 48 kHz project (E5017), audio on an instrument track, missing file at export (E5018, names the file), `project.rate` change with audio in use | Pass |
| 13 | Mix and export: gain, pan, mute, solo, master; 16-bit dithered, 24-bit, 32-bit float; range by beats; clipping warning W5020 with the measured peak | Pass |
| 14 | 04-audio-quality tests: null, round-trip, determinism (incl. dither), gain law, pan law, silence | Pass (`cargo test`) |
| 15 | Split clip renders the same samples; tempo change keeps audio duration; placing audio survives a crash (journal) | Pass (`cargo test`) |
| 16 | 10-minute stereo file: import, waveform, export with two tracks | 48 ms, 435 ms (once; cached on disk), 1.43 s. Peak memory 6 MB (was 321 MB before streaming) |
| 17 | GUI import (Ctrl+I), real waveforms in timeline and clip editor, Export dialog writes the file and shows the result | Pass |
| 18 | Release window: starts, runs the engine through the new async command path, closes; the log now ends with "finished" | Pass (bug 7) |
| 19 | Independent check of the mix: `dist/test-audio/verify_mix.py` rebuilds the demo mix from the stems in Python (gains, D-030 pan, master) and compares it with the 32-bit float export | Largest difference 8.5e-8 (-141 dBFS, float rounding). 16-bit dither noise -96.3 dBFS RMS, the theoretical value for TPDF |

## Bugs found and fixed

1. **Mute and solo could not be turned off from the window.** The buttons and M/S keys sent
   `track.mute #id`, which sets mute on. Now they send `--state on|off` from the current state.
2. **Two quick clicks could reach the engine out of order** (seen: a double-click's commands
   applied as 59, 59, 0, 59). Commands are now queued in order in `engine.ts`.
3. **The pan knob could resend a stale value on a plain click.** Fixed; fader and knob also
   ignore right-clicks and handle a cancelled drag.
4. **Paste after a box selection went to the bottom track**, so a MIDI clip landed on an audio
   track and was refused. The selected track is now the topmost of the selection.
5. **Piano roll rows were 4.7 px tall** on the laptop panel. Now at least 11 px, wheel scrolls pitch.
6. **Repeated Ctrl+I stacked files on one track at the same spot.** A file now gets a new track
   when audio is already there.
7. **The window's log had no end line**, because Tauri ends the process itself. Finished on its
   Exit event now.
8. **Loading an older project file kept its old format number**, so saving it would have let an
   older build open data it does not understand. Load now stamps the current format.
9. **The whole project went to the window after every command** (the latency in row 1), and
   **the window ran commands on its main thread**, so a slow one froze painting. See D-032.

## Not fixed, noted

- MP3 files from encoders without a gapless tag start late: 25 ms measured with Windows' encoder.
  Recorded as `12-known-daw-problems.md` F7. Use WAV or FLAC for anything that must line up.
- Overlapping clips on one track are summed (Q12).
- No resampler: files at another rate are refused (Q13).
- The page in the browser pane was hidden all night (screen locked), so screenshots were stale and
  frame rate could not be judged; checks were made on state, logs and canvas pixels instead.
- Peak loading and export run in the command queue: a long export holds other commands until it
  ends (1.4 s for 10 minutes). Fine for now; a progress bar comes with longer songs.

## Needs Dzyan's ears or hands

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Play `dist\test-audio\exports\overnight-mix-24bit.wav` in any player | Drums, bass, pad chords and a lead, 32 s, no clicks, chords a little left, lead a little right | | |
| 2 | Compare `overnight-mix-16bit-dithered.wav` with the 24-bit one | Sound the same; dither is below hearing | | |
| 3 | Open `dist\app\crucible-app.exe`, Ctrl+O, `...\dist\test-audio\demo-project` | Five tracks with real waveforms; the MP3 track muted | | |
| 4 | Drag a WAV or MP3 from Explorer onto a track in the window | It lands at the pointer, on that track (or a new one if the spot is taken) | | |
| 5 | Ctrl+Shift+E, Export | A WAV appears where the dialog says; a toast gives the peak | | |
| 6 | Try mute/solo on and off, the pan knob, box select + copy + paste, the piano roll wheel | Feels right; say what does not | | |
