# Test report: logs that tell the whole session, and the report file (dev laptop)

D-037. The goal: from a PC no developer can see, one file is enough to work out what happened.

## Setup

| Item | Value |
|---|---|
| Date | 2026-11-23 |
| Build tested | 0.0.2, working tree after `62b5c38`, uncommitted |
| Machine | Dev workstation (Lenovo laptop). Not a music machine |
| Audio devices | Realtek speakers and a Jabra headset. No ASIO driver |
| How the window was driven | The real `crucible-app.exe`, through WebView2's debugging port: real mouse and key events, no test code inside the program |

## What a machine verified

| # | Check | Result |
|---|---|---|
| 1 | Every command is followed by its outcome, its duration and its answer; a failure by `< FAILED` and a coded `ERROR` line | Pass (window and command line) |
| 2 | Clicks, keys and a drag in the window appear as `UI did:` lines, before the command they caused | Pass. Before a fix the click was written after its command |
| 3 | 6 of 6 clicks made 20 to 600 ms after another action registered and were logged | Pass |
| 4 | F9 logs the moment at once (`MARK (no note)`), and the typed words follow with "noticed 0.5 s before this line" | Pass |
| 5 | A mark made while playing carries the position in the song and playback's health | Pass |
| 6 | Playback writes: the output asked for, the device opened (driver, format, buffer), a health line every 10 s, the totals at stop | Pass (exclusive mode, Realtek and Jabra) |
| 7 | **Dropouts**: the mixing thread was frozen for 400 ms and 250 ms during playback at -60 dB. Each produced one `W2101` line with the position, the cause ("the queue ran dry 1 time(s): mixing fell behind") and "queue 0% full" | Pass |
| 8 | 28 reverbs on a debug build (the slowest the program can be made) | 0 dropouts, mixing 49% of one core at worst |
| 9 | A deliberate error in the window's own code | Logged as `ERROR E9003` with the message, file, line and stack |
| 10 | The window's `report` command | One file: header with version, commit and fingerprint, how to read a log, the audio output choice, the machine check (devices, exclusive-mode formats, MIDI), then the 8 newest logs with the window's own log among them, and in it the snapshot: play state, output choice, the session's edits and the project as JSON. 114 KB. 2.5 to 13.6 s |
| 11 | A command running for more than 5 s | `WARN W1005 ... still running after 5 s: report ...` |
| 12 | Closing the window while it plays | Playback is stopped, its totals are logged, the log ends with `finished: OK` |
| 13 | A long log cut to fit a report keeps every `MARK`, `WARN`, `ERROR`, `PANIC` and `CRASH` line from the cut part | Pass (unit test) |
| 14 | Text typed into a field is not in the log | Pass: only `key Enter in input.input` |
| 15 | All tests, the linter, the TypeScript check | 73 pass, clean, clean |

Not verified: the Create report file **button** (it writes to the Desktop, which on this laptop
is inside OneDrive, so the same command was run with `--out` instead); installing 0.0.2; any
ASIO line (no driver here); a real problem diagnosed from a log sent from another PC.

## Needs Dzyan's ears or hardware

Do this once on each other PC, after installing 0.0.2. It is the real test.

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Open Crucible, open or build a project, play it | Sound | | |
| 2 | While it plays, press **F9**, type a few words, press Enter | A small "Marked in the log" box; it closes on Enter | | |
| 3 | Settings (Ctrl+,), Diagnostics, **Create report file** | After about 15 seconds the text under the button names a file on the Desktop | | |
| 4 | Send that file | Claude can say from it alone: which build, which audio device and driver, what you did, and where your mark is | | |
| 5 | Lynx machine only: check the report says `asio` in the `AUDIO start` line | It names the Lynx driver | | |

If the window will not even start, run `crucible.exe report` from the `tools` folder beside the
program (`C:\Program Files\Crucible\tools`): the file lands on the Desktop.
