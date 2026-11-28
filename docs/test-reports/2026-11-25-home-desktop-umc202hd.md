# Test steps: home desktop with the Behringer UMC202HD (0.0.4)

For D-039 and D-040, after Dzyan's first session on this machine with 0.0.2. Written for 0.0.3;
on 2026-11-26 his 0.0.3 install failed when he moved it to Program Files (a per-user installer
cannot write there), so 0.0.4 installs to Program Files again. Same steps.

## Setup

| Item | Value |
|---|---|
| Build | 0.0.4, `Crucible_0.0.4_x64_en-US.msi` |
| Machine | Home desktop |
| Audio | Behringer UMC202HD 192k (ASIO driver installed); MIDI: M-Audio Axiom 61 |
| Before installing | Uninstall any older Crucible from Windows' Apps list. 0.0.4 installs to `C:\Program Files\Crucible` (Windows asks for admin); keep the folder it offers |

## What a machine verified (dev laptop, no Behringer, no ASIO driver)

| # | Check | Result |
|---|---|---|
| 1 | Import keeps the file as it is: an MP3 is copied byte for byte, nothing else is written | Pass: a 16 s MP3 imported in 39 ms |
| 2 | A 44.1 kHz file in a 48 kHz project: refused without consent; with it, placed, plays converted, one second stays one second, pitch and level kept; back at 44.1 kHz the export equals the file exactly | Pass (unit test) |
| 3 | The converter against a perfect sine, 48 to 192 kHz | Within -100 dB |
| 4 | The window asks before converting; Convert places the clip; the clip editor says "converted" | Pass (GUI test, real clicks) |
| 5 | "OUT 1-2 (BEHRINGER UMC 202HD 192k)" finds "UMC ASIO Driver"; two equally likely drivers are not guessed | Pass (unit test) |
| 6 | Ctrl+C with text selected in the Console leaves clips alone | Pass (GUI test) |
| 7 | Settings lists MIDI inputs | Pass (none on this laptop) |
| 8 | The 0.0.4 installer installs for all users, into `Program Files\Crucible`, asking for admin | Pass (MSI tables, 2026-11-26) |
| 9 | All 39 GUI checks, 78 unit tests | Pass (GUI checks re-run on 0.0.4) |

Not verified: anything on the desktop; a real ASIO stream; a real MIDI keyboard; installing 0.0.4.

## Needs Dzyan's ears or hardware

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Install 0.0.4, open Crucible, Settings, Audio | The UMC202HD and the Axiom 61 are listed. Choose the UMC as output, then ASIO: the list shows its ASIO driver | | |
| 2 | Same page, Sample rate line | With ASIO: "set to the project's rate, 48000 Hz", not a warning | | |
| 3 | Close Reaper. Save a project, drag in a 4-minute song | It appears within about two seconds. The audio folder holds the song in its own format and size, nothing bigger | | |
| 4 | Press Space | Sound through the UMC; the top bar says `asio` | | |
| 5 | Drag in a 44.1 kHz song | Crucible asks whether to convert. Convert: it plays at the right speed and pitch | | |
| 6 | Export (Ctrl+Shift+E), then play the file in Windows' Media Player with Reaper closed | It plays | | |
| 7 | Select an error message in the Console, Ctrl+C, paste into Notepad | The text pastes | | |
| 8 | Find the log | `C:\Program Files\Crucible\logs\crucible-app.log` (Settings, Diagnostics shows the path), growing by a line every second | | |

## When a step fails

Press F9 at the moment it goes wrong and type what you noticed. Then Settings, Diagnostics,
Create report file, and send the file it names.
