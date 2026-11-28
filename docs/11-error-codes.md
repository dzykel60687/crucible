# 11. Error codes, logs and crash reports

How the programs report problems, where the evidence ends up, and what every code means.
The goal: when something goes wrong on a machine no developer can see, one file sent back
says what happened, where, on which build and on what system.

## Sending a problem from another PC

This is the whole procedure. It needs no command line and no developer at the machine.

1. **When it happens, press F9** in the window and type what you noticed ("click on the snare",
   "the playhead froze"). The log gets the time and the play position at once; the words are added
   when you press Enter. In `crucible shell`, type `mark heard a click`.
2. **Settings, Diagnostics, Create report file.** One text file appears on the Desktop:
   `crucible-report-<date_time>.txt`. From a command line: `crucible report`.
3. **Send that file** and say in a sentence or two what went wrong. If the report lists a crash
   dump (`.dmp`), send that too.

The report is enough to tell: which build ran (version, commit, fingerprint), on what machine and
audio hardware, every command with its result and duration, what was clicked and what the window
showed, how playback was coping at every moment, every dropout with its cause, every error with
its code, and the project as it stood (tracks, clips, effects and settings, no audio).

**Privacy:** a log names the computer, the user folder, the audio devices and the projects and
files that were opened. It never holds audio, and never what was typed into a field. Nothing is
sent anywhere by itself.

## Where things go

| What | Where | Notes |
|---|---|---|
| Session log | **Installed** (0.0.4 on): `C:\Program Files\Crucible\logs\crucible-app.log` (and `crucible.log` for the tool). **Run from a folder it can write** (a build, or the 0.0.3 per-user install): `<program>.log` beside the program | Written line by line as things happen, started afresh on every run. The run before is kept once as `<program>.previous.log`. The program tries its own folder first, then the `logs` folder the installer makes, then `%LOCALAPPDATA%\Crucible`. Settings, Diagnostics shows the path |
| Crash dump | Beside the log, `<program>.dmp` (and `.previous.dmp`) | Only when the program crashes |
| Problem report | From the window: the Desktop. From `crucible report`: the folder it was run from (`--out <folder>` to choose); the Desktop if that folder cannot be written | Machine check, audio output choice, and each program's log of this run and the run before, in one file. **This is the file to send** |
| Audio output choice | `%LOCALAPPDATA%\Crucible\audio-output.json` | Per machine. Copied into the report |

`crucible logs` lists the log files with the first problem in each.

## What a log contains

1. **Header:** program and version, **source** (the commit it was built from), build fingerprint
   (matches `crucible version`; tells two builds apart), exe path and date, full command line,
   working folder, start time with time zone, computer name and model, OS and kernel version, CPU
   and memory. The window adds its Tauri and web view versions, then its size, display scale and
   core count.
2. **Timeline:** every line has clock time to the millisecond and seconds since start, then a
   level or a tag:

   | Line | Meaning |
   |---|---|
   | `ALIVE idle \| Song: 5 tracks ... \| memory 37 MB, processor 2%` | Once a second while the window is open (test builds): what the engine is doing; while playing, how that second went (queue low point, mixing load, late wake-ups, dropouts, peak) |
   | `INFO  > track.gain Piano -3` | A command, typed or sent by the window |
   | `INFO  < ok in 0.4 ms: Piano gain -3.0 dB` | Its outcome: how long it took and what it answered (first 12 lines). `(slow)` from half a second |
   | `INFO  < FAILED E5017 in 0.9 ms` | It failed. The `ERROR` line beside it has the detail |
   | `UI    did: click button#t-play "Play / stop (Space)"` | What the person did: clicks, double-clicks, right-clicks, drags (start and distance), keys, dropped files, the window coming to the front or going to the back |
   | `UI    did: click ... (0.15 s before this line)` | The log stamps a line when it is written; a window line written late says how long before |
   | `UI    saw: error message: E2004 ...` | What the window showed: every message, resizes, hidden or visible |
   | `UI    slow: the window froze for about 480 ms` | The window could not run for that long |
   | `MARK  "click on the snare" \| playing, 12.34 s into the song ...` | The person's own note (F9, or `mark`). With `noticed 4.2 s before this line` when the words were typed after the moment |
   | `AUDIO Windows reports: device added {0.0.0...}` | Every notice Windows sends about audio devices, as it arrives (test builds) |
   | `AUDIO exclusive mode, Speakers: 24-bit int in 32: accepted` | Every step of opening a device: each format asked for and the answer, the buffer offered and granted (test builds) |
   | `AUDIO into memory: song.mp3 (MP3, 44100 Hz, converted to 48000 Hz, 216.2 s) \| 79 MB \| 640 ms` | A file decoded or converted into memory (D-039) |
   | `AUDIO device change: Connected: USB Interface (output)` | A device was plugged in, unplugged, made the default, or had its rate changed |
   | `AUDIO other programs with sound on this output: chrome.exe` | Written before the output is opened |
   | `AUDIO the driver asks for up to 4800 frames at a time, not the 480 it reported; ...` | The driver takes more per request than it said. The queue has grown to fit |
   | `AUDIO start \| Speakers [exclusive, 24-bit, 48000 Hz, 144-frame buffer = 3.0 ms] ...` | Playback began: device, driver, format, buffer, queue, what is being played |
   | `AUDIO at 20.0 s \| dropouts 0, late wake-ups 0, mixing 5.5% of one core, ...` | Health, every ten seconds while playing |
   | `AUDIO the driver's thread woke late 1 time(s) near 0.02 s ...` | A near miss, on record when it happens (one at the start of playback is normal) |
   | `AUDIO edit taken over near 13.20 s \| new mixer built in 2.1 ms` | An edit during playback reached the sound |
   | `AUDIO stop (transport.stop) \| ...` | Playback ended, why, and the totals |
   | `WARN  W2101 ... \| near 12.31 s of the song: the queue ran dry 1 time(s): mixing fell behind` | A dropout, logged when it happens (one line a second at most), with its cause: mixing fell behind, the driver reported an overrun, or the driver never asked for sound |
   | `WARN  W1005 ... still running after 5 s: project.export ...` | A command is taking long (again at 30, 120 and 600 s). If the window hangs, the last such line names the command that never came back |
   | `ERROR`, `PANIC`, `CRASH` | A coded failure, a bug in Crucible (with a stack), a native crash (below) |
   | `---- snapshot (copied below) ----` | Written when a report is made: play state, output choice, the last 60 edits, and the project as JSON |

3. **Library messages:** vst3-host, wasapi and midir log through Rust's `log` facade. Their
   warnings and errors land in the same file, tagged `[vst3_host::...]`. While a plugin loads,
   m0-play turns on `DEBUG`, so vst3-host's numbered loading steps are recorded. If a plugin
   crashes during loading, the last step in the log shows how far it got.
4. **Ending:** `finished: OK (exit code 0)` or `finished: FAILED with E2004 (exit code 10)`.
   **A log with no `finished:` line means the program crashed, was killed or lost power.**

**Test builds log everything** (Dzyan, 2026-11-25): the header says `logging : everything`. Set the
environment variable `CRUCIBLE_LOG=normal` for the quieter log a released version would write.

Not recorded: text typed into fields, plain mouse movement, and the position
polls the window makes twenty times a second while playing (their numbers reach the log through
the `AUDIO` lines instead).

Real-time rule: the audio thread never writes to the log. It only counts (dropouts, late
wake-ups); the lines above are written from the command thread when the window next asks for the
play position, so a dropout is on record within about 50 ms of happening. Library messages raised
on the audio thread are dropped and counted (`... records from the audio thread were dropped`).

**The source line's one limit:** it names the commit that was checked out when the program was
built, and adds "plus uncommitted changes" when it saw any. It is checked again only when the
commit changes, so a test build made from uncommitted work can name the commit alone. The build
fingerprint always tells two builds apart. Release builds are made right after their commit, so
for them the commit is exact.

## Reading a crash

A native crash (most likely a plugin) is written as:

```
21:42:07.123 CRASH E9001 The program crashed | native exception 0xC0000005: access violation (bad memory access) (reading address 0x10)
  crashed in : C:\Program Files\Common Files\VST3\Omnisphere.vst3\Contents\x86_64-win\Omnisphere.vst3 + 0x1a2b3c
  thread     : main (id 1234)
  dump file  : C:\Users\...\Crucible\logs\m0-play-play-omnisphere-2026-11-21_21-41-50.dmp
```

- `crashed in` is the key line. A plugin file means the plugin crashed. A `crucible` or `m0-play` exe means
  our bug. A Windows DLL such as `ntdll.dll` usually means memory was corrupted earlier, often
  by a plugin, so read the lines before the crash.
- The exception names cover the common cases: access violation, unhandled C++ exception
  (`0xE06D7363`), illegal instruction (code built for a newer CPU), stack overflow, heap corruption.
- The `.dmp` file opens in Visual Studio or WinDbg for a full stack. Neither is installed on the dev
  workstation. The text lines are usually enough to decide whether the plugin or Crucible is at fault.
- A plugin that crashes during **scanning** (`crucible plugin.scan`, `crucible diag`) only kills its own probe
  process. The scan reports `CRASHED E4004` with the same `crashed in` line and carries on.
- A bug in our Rust code is a `PANIC E9002` line with the source file, line and a stack.

Limits:
- A plugin may install its own crash handler and replace ours. Then the log just ends, with no
  `finished:` line.
- A "fast fail" (`0xC0000409`) skips crash handlers by design. Rust's own aborts use it, but
  those are caught by the panic hook first.

## Exit codes

| Exit code | Meaning |
|---|---|
| 0 | OK |
| 2 | Bad command line (`E1001`) |
| 3 | File problem (`E1xxx`) |
| 10 | Audio device problem (`E2xxx`) |
| 11 | MIDI problem (`E3xxx`) |
| 12 | Plugin problem (`E4xxx`) |
| 13 | Project or command problem (`E5xxx`) |
| 70 | Internal bug (`E9002`) |
| 1 | Anything else |
| `0xC0000005` and similar | Crashed. Windows reports the exception code as the exit code |

## Rules for code that reports problems

- Every user-visible failure gets a code from `crates/crucible-diagnostics/src/codes.rs`. Add a new
  code rather than reusing a vague one. Never change a published code's meaning or reuse its number.
- Keep the raw evidence in the message: the OS error number (HRESULT), the path, the device or
  plugin name. The code says what kind of problem. The message says exactly which one.
- After adding a code, regenerate the table below with `crucible codes --markdown`. A unit test fails
  if a code is missing from this file.

## Platform notes

- Log folder: from `dirs::data_local_dir()`. macOS: `~/Library/Application Support/Crucible/logs`.
  Linux: `~/.local/share/Crucible/logs`. No code change needed.
- Machine facts: the `sysinfo` crate, which works on all three systems.
- Crash capture: **Windows only today** (`platform_windows.rs`: SetUnhandledExceptionFilter,
  MiniDumpWriteDump). macOS and Linux need a signal handler (`platform_other.rs` is a stub).
  Ledger row "Crash capture" in `03-platform-notes.md`.
- HRESULT decoding is WASAPI-specific and lives in `crucible-audio-io/src/wasapi_exclusive.rs`. Each
  backend maps its own errors to the same E2xxx codes.
- Some help texts name Windows settings. Add macOS and Linux wording when the ports start.
- The window starts `crucible.exe report` as a hidden helper. Windows needs the CREATE_NO_WINDOW
  flag for that (`platform_windows.rs`, `quiet_command`); macOS and Linux need nothing. Ledger row
  "Hidden helper process".
- The report folder is the Desktop from `dirs::desktop_dir()` on all three systems. On Windows a
  Desktop redirected into OneDrive is still the Desktop: the report is then synced with it.
  `report --out <folder>` writes elsewhere.
- The source line needs `git` on the build machine, on every system.
- "Beside the program" is `std::env::current_exe()`'s folder on every system. macOS application
  bundles are read-only once signed, so the log will go to the per-user folder there.

## All codes

Generated by `crucible codes --markdown`. `E` = error, `W` = warning. First digit: 1 general,
2 audio, 3 MIDI, 4 plugins, 5 projects and commands, 9 crashes and bugs.

| Code | Meaning | What to try |
|---|---|---|
| `E1001` | The command line was not understood | Run the program with no arguments, or with `help`, to see the commands and options. |
| `E1002` | A file could not be read | Check that the path exists and that no other program has the file locked. |
| `E1003` | A file could not be written | Check free disk space, that the folder exists and that you may write there. OneDrive folders can lock files while syncing. |
| `W1004` | The log file could not be created | The program still runs but records nothing. Check that the folder shown by `crucible logs` is writable. |
| `W1005` | A command is taking a long time | Wait if it is an export or a large import. If the window stays stuck, close it and send a report: the log names the command that never finished. |
| `E1006` | The report could not be made: crucible.exe was not found | The window uses the crucible.exe command-line tool to gather the report. It is installed in the tools folder beside the window program. Reinstall Crucible, or run `crucible.exe report` yourself. |
| `E2001` | No audio output device was found | Connect or enable an output in the system sound settings, then run `crucible audio.devices`. |
| `E2002` | No audio device matches the name given | Run `crucible audio.devices` and use part of a device name exactly as listed. |
| `E2003` | The device name matches more than one device | Use more of the name, so that only one device matches. |
| `E2004` | Another program is using the audio device exclusively | Close other audio programs (other DAWs, standalone plugins, browsers, Teams, Zoom), then retry. |
| `E2005` | Exclusive mode is switched off for this device | Windows: Sound settings > the device > Properties/Advanced > tick 'Allow applications to take exclusive control of this device'. |
| `E2006` | The device does not accept this sample rate or format | Try --rate 48000 or --rate 44100. `crucible audio.devices` lists what each device accepts in exclusive mode. |
| `E2007` | The driver rejected the buffer size | Try a larger buffer, for example --block 256 or --block 512. |
| `E2008` | The audio stream could not start | Retry once. If it repeats, run `crucible audio.test` and send the log. |
| `E2009` | Audio stopped unexpectedly | The device was unplugged, changed or reset by its driver. Sleep, or changing the device's format in Windows, also does this. Reconnect and restart. |
| `E2010` | The system audio service is not running | Windows: start the 'Windows Audio' service in services.msc, or reboot. |
| `E2011` | Windows stopped the audio stream because the audio thread used too much CPU | Use a larger buffer, close CPU-heavy programs, and use the High performance power plan. |
| `E2012` | Exclusive mode does not exist on this system | Only Windows has WASAPI exclusive mode. Other systems use their normal low-latency audio path. |
| `E2013` | This machine's audio output choice could not be saved | The choice is kept beside the logs, in the per-user Crucible folder. Check that the folder can be written; the message names the file. |
| `E2099` | The audio driver reported an error | The log line includes the driver's own code (HRESULT on Windows). Send the log. |
| `W2101` | An audible dropout: the sound was interrupted | Use a larger buffer (`audio.output --buffer 256` or 512; `--block` in m0-play), close CPU-heavy programs, use the High performance power plan and keep a laptop on mains power. |
| `W2102` | The audio thread woke up late: a risk of dropouts | Harmless if rare. If it is frequent, follow the advice for W2101. |
| `E3001` | The MIDI system could not be opened | Restart the program. If it repeats, reconnect the keyboard and send the log. |
| `E3002` | A MIDI input could not be opened | On Windows a MIDI input can be open in only one program at a time. Close other music programs (DAWs, standalone Omnisphere, browser MIDI pages, the keyboard's own editor). |
| `W3003` | No MIDI inputs were found | Connect the keyboard before starting the program and check that `crucible midi.inputs` lists it. Unplugging and replugging it can help. |
| `E4001` | No installed plugin matches that name | Run `m0-play list` or `crucible plugin.scan` to see what is installed, or give the full path to the .vst3 bundle. |
| `E4002` | The plugin name matches more than one plugin | Use more of the name, or give the full path to the .vst3 bundle. |
| `E4003` | The plugin could not be loaded | Check that the plugin works in another host. Spectrasonics plugins need their STEAM folder present and authorised. The log lists each loading step. |
| `E4004` | The plugin crashed while being scanned | The plugin is broken, incompatible or needs an update. Other plugins are not affected. |
| `E4005` | The plugin took too long to load and was stopped | Large libraries can be slow the first time. Retry with a longer --timeout, for example 180. |
| `W4006` | Saved plugin settings could not be restored | The plugin starts with its default sound. The saved file may come from a different plugin version. |
| `E4007` | Plugin settings could not be saved | Check that the folder is writable. The plugin's own error is in the log. |
| `E4008` | The plugin window could not be opened | Sound and MIDI may still work. Send the log. |
| `W4009` | The plugin window reported an error while running | Usually harmless. Send the log if the window misbehaves. |
| `W4010` | The plugin refused offline rendering mode | The render still runs, in real-time mode. Usually harmless. |
| `W4011` | The render is digital silence | Check that a patch is loaded. Large sample libraries may need a longer --preroll, for example 10. |
| `E4012` | The render contains invalid samples (NaN or infinity) | A plugin bug. Note the patch and send the log. |
| `E4013` | The plugin failed while processing audio | Note the patch and send the log. |
| `E4014` | The plugin host could not start | Send the log. |
| `E5001` | Unknown command | Run `crucible help` for the list of commands. |
| `E5002` | A command argument is missing or wrong | Run `crucible help <command>` to see its arguments. |
| `E5003` | No track matches | Refer to a track by its name, its position (1, 2, ...) or its id (#3). `track.list` shows all three. |
| `E5004` | More than one track has that name | Use the track's position or id instead. `track.list` shows both. |
| `E5005` | A value is outside the allowed range | The message gives the allowed range. |
| `E5006` | The project has unsaved changes | Save with `project.save`, or repeat the command with --discard to drop the changes. |
| `E5007` | The project could not be opened | Check that the folder holds a project.json written by Crucible. The message says what was wrong with it. |
| `E5008` | The project was saved by a newer version of Crucible | Update Crucible on this machine to the version used on the other machine. |
| `E5009` | The project could not be saved | Check free disk space and that you may write to the folder. OneDrive folders can lock files while syncing. The previous save is untouched. |
| `W5010` | Crash recovery could not replay every unsaved change | The project opened with the changes up to the one named. The full journal was kept beside the project. |
| `E5011` | No clip matches | Refer to clips by id: #4, or several as #4,#7. `clip.list` shows them. |
| `E5012` | That clip edit is not possible | The message says why, for example a clip would start before bar 1, end before it starts, or land on a track of the wrong kind. |
| `E5013` | Nothing has been copied | Copy or cut clips first with `edit.copy` or `edit.cut`. |
| `E5014` | No note matches | Notes are found by pitch and start, in beats from the clip's start. `clip.notes <clip>` lists them. |
| `E5015` | The project needs a folder first | Imported audio is copied into the project's folder, so save the project once (`project.save <folder>`), then import. |
| `E5016` | The audio file could not be read | Crucible reads WAV (16, 24 or 32-bit integer, or 32-bit float), MP3 and FLAC, mono or stereo. The message gives the file and the reason. |
| `E5017` | The audio is at another sample rate than the project | The message names both rates. Convert it (the window asks; with commands, add --convert): the file is kept as it is and converted while it plays. Or set the project to the file's rate with project.rate. |
| `E5018` | An audio file the project uses is missing | The file should be in the project's `audio` folder. Copy it back there; the message names it. |
| `E5019` | The export could not be written | Check the folder exists and is writable, and that the file is not open in another program. |
| `W5020` | The export has samples above full scale | Fixed-point files cannot hold them, so they were clipped. Lower the master gain by at least the reported amount, or export 32-bit float. |
| `E5021` | No effect in that slot | Effects are numbered from 1 on each track, top to bottom. `fx.list <track>` shows them. |
| `E5022` | That effect setting is not possible | The message says why: an unknown effect or setting, a value out of range, or an impulse-response file Crucible cannot use. `fx.kinds` lists every effect and setting. |
| `E5023` | Audio could not be converted to the project's sample rate | The message says why. Send a report (Settings, Diagnostics, Create report file), and use the file at its own rate meanwhile. |
| `E9001` | The program crashed | The log names the file the crash happened in. A plugin file there means the plugin crashed. Send the log and the .dmp file next to it. |
| `E9002` | Internal error: a bug in Crucible | Send the log. It contains the source location of the bug. |
| `E9003` | Internal error in the window: a bug in Crucible | The window may behave oddly from here on. Save under a new name, restart Crucible and send a report (Settings, Diagnostics, Create report file). |
| `E9099` | Unexpected error | Send the log. |
