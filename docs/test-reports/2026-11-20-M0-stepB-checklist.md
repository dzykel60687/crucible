# Test checklist: M0 step B, Omnisphere and Keyscape

Written by Claude Opus 5.5 on 2026-11-20. **Nothing here has been run with a real plugin.**
The dev workstation has no plugins, no MIDI keyboard and no ASIO interface.

## Verified by machine before this test

- Both programs build cleanly in release mode with no linter warnings.
- They need no Visual C++ runtime: only standard Windows system files.
- `crucible diag` lists audio devices, exclusive-mode capabilities, MIDI inputs and VST3 folders,
  and writes a log file.
- The plugin scanner survives a plugin that crashes, kills one that hangs, and finds its
  result among plugin noise. Simulated in automated tests.
- A broken `.vst3` bundle is reported as FAILED, not a crash.
- **Exclusive mode works on the dev laptop's built-in Realtek output:** 3 ms buffer, 24-bit,
  zero dropouts over 5 seconds, with silence as the signal. The exclusive-mode output stream
  delivered the right amount of audio in an automated hardware test.
- Sample conversion from float to 16-bit, 24-bit and float device formats is unit-tested.
- **Diagnostics (added later on 2026-11-20):** every run writes a log as it goes, so a crash
  cannot lose it. A fake plugin built to crash was used to check the crash path. During scanning, the
  scan carried on and named the plugin file. During `m0-play play`, the log recorded each loading step,
  then the crash with the plugin's file name, and a crash dump was written. Device-in-use, unknown
  device, unknown plugin and bad options each print their error code and what to try.

## Needs you

Everything below. Do it on the **Lynx machine first**.

## Before you start

1. Copy the folder `crucible\dist\m0` to the music machine, for example to `C:\m0`.
2. In Windows sound settings, set the **default output** to the Lynx card (or the Behringer).
3. Close other audio programs. Exclusive mode cannot take a device another program is using.
4. Plug in the MIDI keyboard **before** starting the program.
5. Open PowerShell in that folder:

```powershell
cd C:\m0
```

No ASIO yet. Exclusive mode is the low-latency path in this build.

## Steps

Mark each one Pass or Fail and note what happened. Logs are kept automatically; you don't
need to save anything by hand.

While `m0-play` runs, type `m` and a few words, then Enter, to mark the moment in the log, for
example `m heard a click`. Do this the moment you hear a problem: the log times every line, so a
marker shows what was happening then.

| # | Step | Command or action | Expected |
|---|---|---|---|
| 1 | Machine report | `.\crucible.exe diag` | Lists audio devices, the EXCLUSIVE MODE section, your MIDI keyboard, and finds Omnisphere and Keyscape with **OK**. Prints a log file path. |
| 2 | Driver timing, silent | `.\crucible.exe audio.test --seconds 30` | Verdict PASS. Note the buffer size in ms. If it fails, try step 2 again with `--device` and part of the interface's name. |
| 3 | Load and open, low latency | `.\m0-play.exe play omnisphere --exclusive` | Within a minute: "loaded", "audio started", "exclusive output: ... buffer ... ms", "MIDI input connected", "editor window opened". The Omnisphere window appears. |
| 4 | Sound without keyboard | Type `c` then Enter | A chord plays for 1.5 seconds. |
| 5 | Sound from keyboard | Play the keyboard | Notes sound. Sustain pedal works. Pitch bend works. |
| 6 | Feel | Play fast passages | Does it feel immediate? Note the buffer ms from step 3. |
| 7 | Editor works | Use the patch browser, including typing in its search box | Typing goes into Omnisphere. Patches load. |
| 8 | Patch switching | Switch patches while holding notes, then type `t` | Note any dropouts or clicks. `t` prints dropouts counted so far. |
| 9 | Endurance | Play and switch patches for **60 minutes**, then type `t` | No crash. Report the timing line. |
| 10 | Save on quit | Pick a distinctive patch, then close the Omnisphere window. If the program keeps running, type `q` then Enter, and note that the close wasn't detected. | Console says "state saved". |
| 11 | Recall | Run step 3 again | The same patch you left comes back. |
| 12 | Offline render | `.\m0-play.exe render omnisphere` | Writes `m0-render-Omnisphere.wav`. Reports a peak level, not "silence". |
| 13 | Listen to render | Play the WAV | A C-major chord with your saved patch, released near the end. No clicks or garbage. |
| 14 | Shared mode, for comparison | `.\m0-play.exe play omnisphere` (no `--exclusive`) | Plays, with noticeably more delay. Confirms both paths work. |
| 15 | Keyscape | Repeat steps 3 to 13 with `keyscape` instead of `omnisphere` | Same results. |
| 16 | Other machines | Repeat steps 1, 2, 3, 4, 5 and 12 on the Behringer machine and the laptop | Loads and plays on both. On the laptop, plug in the Behringer for listening. |

## If the name matches more than one plugin

Give the full path instead. `crucible diag` prints every bundle's path.

```powershell
.\m0-play.exe play "C:\Program Files\Common Files\VST3\Omnisphere.vst3" --exclusive
```

## What to send back

1. The filled-in table, even just "1-11 pass, 12 silent".
2. Run this once at the end, in the same folder. It runs the machine check again and gathers it,
   plus the 8 most recent logs, into **one file** in the current folder:

   ```powershell
   .\crucible.exe report
   ```

   Send the `crucible-report-....txt` it names. If it also names a `.dmp` crash dump, send that too.
3. Anything you heard: clicks, dropouts, delay, wrong sound.

If you ran more than 8 programs since the problem, send the specific log instead. `.\crucible.exe logs`
lists them all with the first error in each. They live in `%LOCALAPPDATA%\Crucible\logs`.

## Reading what the programs print

- Every problem has a code such as `E2004`, a one-line meaning, and **what to try**.
  `.\crucible.exe codes` lists them all.
- `WARNING` means it carried on. `ERROR` means that step failed.
- `CRASH E9001` means the program crashed. The `crashed in` line names the file responsible:
  a plugin file means the plugin crashed.
- `W2101` during play means an audible dropout happened that second. It is printed live, so you
  can match it to what you heard.

## Options worth trying if something misbehaves

| Problem | Try |
|---|---|
| `E2005` exclusive mode is switched off | Sound settings, the device, Advanced, tick "Allow applications to take exclusive control" |
| `E2004` device is in use | Close other audio programs, including browsers playing sound |
| `E3002` MIDI input could not be opened | Close other music programs. Windows lets only one program open a MIDI input |
| Crackles or dropouts in exclusive mode | Add `--block 256`, then `--block 512` |
| Interface rejects 48 kHz | Add `--rate 44100` |
| Render is silent | `.\m0-play.exe render omnisphere --preroll 10` |
| Render sounds wrong | Delete `m0-state-Omnisphere.bin` and render the default sound |

## Platform notes

This checklist and both programs are Windows-only for now. The message pump, exclusive mode,
static runtime setting and native crash capture are Windows-specific and recorded in
`docs/03-platform-notes.md`. Logs, codes and `crucible report` are portable.
