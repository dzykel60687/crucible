# 11. Error codes, logs and crash reports

How the programs report problems, where the evidence ends up, and what every code means.
The goal: when something goes wrong on a machine no developer can see, one file sent back
says what happened, where, on which build and on what system.

## Where things go

| What | Where | Notes |
|---|---|---|
| Session log | `%LOCALAPPDATA%\Crucible\logs\<program>-<command>-<date_time>.txt` | One per run of anything that touches hardware. Written line by line, so it survives a crash. Newest 200 kept |
| Crash dump | Same folder, same name, `.dmp` | Only when the program crashes. Newest 10 kept |
| Problem report | The folder `crucible report` was run from, `crucible-report-<date_time>.txt` | Machine check plus the 8 newest logs in one file. **This is the file to send** |

`crucible logs` prints the folder and lists recent logs with the first problem in each.

## What a log contains

1. **Header:** program and version, build fingerprint (matches `crucible version`; tells test builds
   apart), exe path and date, full command line, working folder, start time with time zone,
   computer name and model, OS and kernel version, CPU and memory.
2. **Timeline:** every line has clock time and seconds since start, then a level:
   `INFO`, `WARN`, `ERROR`, `DEBUG`, `PANIC`.
3. **Library messages:** vst3-host, wasapi and midir log through Rust's `log` facade. Their
   warnings and errors land in the same file, tagged `[vst3_host::...]`. While a plugin loads,
   m0-play turns on `DEBUG`, so vst3-host's numbered loading steps are recorded. If a plugin
   crashes during loading, the last step in the log shows how far it got.
4. **Ending:** `finished: OK (exit code 0)` or `finished: FAILED with E2004 (exit code 10)`.
   **A log with no `finished:` line means the program crashed or was killed.**

Real-time rule: the audio thread never writes to the log. Library messages raised on it are
dropped and counted (`... records from the audio thread were dropped`). Timing problems are
measured with counters on the audio thread and reported by the main thread once a second.

## Reading a crash

A native crash (most likely a plugin) is written as:

```
21:42:07.123 CRASH E9001 The program crashed | native exception 0xC0000005: access violation (bad memory access) (reading address 0x10)
  crashed in : C:\Program Files\Common Files\VST3\Omnisphere.vst3\Contents\x86_64-win\Omnisphere.vst3 + 0x1a2b3c
  thread     : main (id 1234)
  dump file  : C:\Users\...\Crucible\logs\m0-play-play-omnisphere-2026-11-21_21-41-50.dmp
```

- `crashed in` is the key line. A plugin file means the plugin crashed. A `rust-daw` exe means
  our bug. A Windows DLL such as `ntdll.dll` usually means memory was corrupted earlier, often
  by a plugin, so read the lines before the crash.
- The exception names cover the common cases: access violation, unhandled C++ exception
  (`0xE06D7363`), illegal instruction (code built for a newer CPU), stack overflow, heap corruption.
- The `.dmp` file opens in Visual Studio or WinDbg for a full stack. Neither is installed on the dev
  workstation. The text lines are usually enough to decide whether the plugin or rust-daw is at fault.
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

- Every user-visible failure gets a code from `crates/daw-diagnostics/src/codes.rs`. Add a new
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
- HRESULT decoding is WASAPI-specific and lives in `daw-audio-io/src/wasapi_exclusive.rs`. Each
  backend maps its own errors to the same E2xxx codes.
- Some help texts name Windows settings. Add macOS and Linux wording when the ports start.

## All codes

Generated by `crucible codes --markdown`. `E` = error, `W` = warning. First digit: 1 general,
2 audio, 3 MIDI, 4 plugins, 5 projects and commands, 9 crashes and bugs.

| Code | Meaning | What to try |
|---|---|---|
| `E1001` | The command line was not understood | Run the program with no arguments, or with `help`, to see the commands and options. |
| `E1002` | A file could not be read | Check that the path exists and that no other program has the file locked. |
| `E1003` | A file could not be written | Check free disk space, that the folder exists and that you may write there. OneDrive folders can lock files while syncing. |
| `W1004` | The log file could not be created | The program still runs but records nothing. Check that the folder shown by `crucible logs` is writable. |
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
| `E2099` | The audio driver reported an error | The log line includes the driver's own code (HRESULT on Windows). Send the log. |
| `W2101` | The audio driver missed a deadline: an audible dropout | Use a larger buffer (--block 256 or 512), close CPU-heavy programs, use the High performance power plan and keep a laptop on mains power. |
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
| `E9001` | The program crashed | The log names the file the crash happened in. A plugin file there means the plugin crashed. Send the log and the .dmp file next to it. |
| `E9002` | Internal error: a bug in Crucible | Send the log. It contains the source location of the bug. |
| `E9099` | Unexpected error | Send the log. |
