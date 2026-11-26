# Test report: Stop and fader faults, real devices, one log file (dev laptop)

D-038 and the 2026-11-25 addition to D-037.

## Setup

| Item | Value |
|---|---|
| Date | 2026-11-25 |
| Build tested | 0.0.2, working tree after `62b5c38`, uncommitted; `dist\app\crucible-app.exe` |
| Machine | Dev workstation (Lenovo laptop), Windows 11, display scale 100% |
| Audio devices | Realtek speakers, two Jabra headsets. No ASIO driver |
| How | `pwsh -File tools\gui-test.ps1`: the real window, real mouse events held about 90 ms |

## What a machine verified

| # | Check | Result |
|---|---|---|
| 1 | The faults reproduced on the old build: with dropouts forced, the mixer was rebuilt on every count (faders seen in 2 places) and a click on Stop was lost | Reproduced |
| 2 | New build, same forced dropouts (17 in 3 s): one layout in 181 frames, Stop stops on the first click, the count shows in the top bar | Pass |
| 3 | Every button responds to a click (40 tried) | Pass |
| 4 | Settings: all five pages filled in, nothing squashed or off screen; project menu, Export, the mark box open and close with Escape | Pass, after a fix: the device lists had collapsed to 2 px |
| 5 | The slider by a track and the mixer fader move the same volume and each other; double-click returns to 0 dB | Pass |
| 6 | Faders at 0 dB sit at one height on every strip | Pass, after a fix: the master was 3 px lower |
| 7 | The window starts empty; a command typed in the Console runs | Pass |
| 8 | `audio.devices`: three outputs and three inputs with the rate, bits and channels Windows runs them at; exclusive-mode rates with `--probe`; answers in about 10 ms | Pass |
| 9 | A driver that asks for 8,192 frames at a time: the queue grows and the next request is served in full | Pass (unit test) |
| 10 | The log is `crucible-app.log` beside the program, the run before is `crucible-app.previous.log`; the report from the window includes it | Pass |
| 11 | All tests, the linter, the TypeScript check | 75 pass, clean, clean |

Not verified: plugging a device in or out (nothing was plugged during the tests); "in use by"
with another program really playing; display scales other than 100%; an install under Program
Files (the log should then be in `%LOCALAPPDATA%\Crucible`); anything on the PC where the faults
were first seen.

## Needs Dzyan's ears or hardware

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | On the PC where Stop failed: run 0.0.2, play for 30 seconds, press Stop | The faders stay still; Stop stops at once; the top bar shows no dropouts, or a count that is not climbing | | |
| 2 | Same PC: Settings, Diagnostics, Create report file; send the file | It shows which driver and buffer that PC uses and whether dropouts were counted | | |
| 3 | Settings, Audio, with a USB device or headphones unplugged, then plug them in | A message "Connected: ..." within about three seconds; the device appears in the list. Unplug: "Disconnected: ..." | | |
| 4 | Start music in a browser, then press Play in Crucible | A message names the browser and says it is silenced while Crucible plays (Exclusive). With the Shared driver chosen in Settings, both play | | |
| 5 | Change the output's sample rate in Windows Sound settings | Settings, Audio shows the new rate and says whether it matches the project | | |
| 6 | Drag the slider by a track | The mixer fader for that track moves with it, and the level changes | | |
| 7 | Find the log | `crucible-app.log` in the folder the program was started from (installed: Settings, Diagnostics shows where) | | |

## When a step fails

Press F9 at the moment it goes wrong and type what you noticed. Then Settings, Diagnostics,
Create report file, and send the file it names.
