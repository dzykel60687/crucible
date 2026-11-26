# Test report: live playback, ASIO build, installer (dev laptop)

Live playback with effects (D-034), ASIO compiled in (D-035), the first MSI.

## Setup

| Item | Value |
|---|---|
| Date | 2026-11-23 |
| Build tested | working tree after `5845a0b`, uncommitted |
| Machine | Dev workstation (Lenovo laptop). Not a music machine |
| Audio devices | Realtek speakers and a Jabra headset. **No ASIO driver** |
| Test project | a copy of `dist/test-audio/demo-project` with the master at -60 dB, so nothing was audible |

## What a machine verified

| # | Check | Result |
|---|---|---|
| 1 | The samples handed to the driver equal a 32-bit float export, bit for bit (tracks with Reverb and Echo) | Pass |
| 2 | A gain and pan change during playback: output equals the old export before it and the new export after it, exactly (so the effects ran on untouched) | Pass |
| 3 | An effect's own setting changed during playback: its tail carries on | Pass (Reverb, Echo, Delay, IR Reverb; with the same settings the hand-over is bit-identical and allocates nothing) |
| 4 | A seek jumps to the right frame and is not counted as a dropout | Pass |
| 5 | The audio callback allocates nothing | Pass |
| 6 | Effects give the same output for blocks of 1, 64, 256, 480 and 4,096 samples, including going idle | Pass, after a fix: they used to differ below -150 dBFS near the end of a tail |
| 7 | Device conversion: every 16-bit value and a 24-bit sweep reach the device unchanged | Pass, after a fix: loud samples used to lose their last bit |
| 8 | Demo song through Realtek, **exclusive mode**, 144-frame (3.0 ms) buffer, 24-bit: 36.6 s | 0 dropouts, 0 late wake-ups, mixing 6% of one core |
| 9 | Same through the Jabra, **shared mode**, and through the automatic choice (exclusive, 16-bit) | 0 dropouts |
| 10 | `--driver asio` with no ASIO driver installed | Clear error E2001, "no ASIO driver is installed on this machine" |
| 11 | `audio.devices` | Lists host "ASIO (0 devices)": the ASIO code is in the build |
| 12 | Window (browser against the real engine): Play, playhead follows the audio clock, real meters, click-to-seek, 7 edits during playback (Reverb mix, IR Reverb mix, gain, solo, bypass), Stop | Pass, 0 dropouts, no console errors |
| 13 | MSI builds; unpacked (not installed) to check its contents | 6.0 MB installer (`dist\installer`); installs `crucible-app.exe` (12.1 MB) and `tools\crucible.exe` (4.1 MB): 16 MB |

Not verified: anything audible; any ASIO stream; installing the MSI (it needs administrator rights);
the real window (only the browser view was driven).

## Needs Dzyan's ears or hardware

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Open `dist\app\crucible-app.exe`, Ctrl+O, choose `dist\test-audio\demo-project`, press Space | The song plays. The playhead moves with the sound, the mixer meters move, the top bar shows the driver and buffer | | |
| 2 | While it plays, click a mixer effect slot and move Mix, Decay, Feedback | The change is heard within a blink (about 40 ms). No click, and the reverb or echo tail does not cut out | | |
| 3 | While it plays, click in the timeline ruler | Playback jumps there at once, no noise | | |
| 4 | While it plays, mute, solo, move a fader, drag a clip | Heard straight away, no dropouts counter in the top bar | | |
| 5 | Stop, export (Ctrl+Shift+E), and compare the file with what you heard | The same | | |
| 6 | **Lynx machine:** in the Console tab type `audio.output`, then play | "ASIO drivers installed" names the Lynx driver; the top bar says `asio`; sound comes out; 0 dropouts | | |
| 7 | Lynx: `audio.output --buffer 64` (then 128, 256), stop and play again | Each plays cleanly. Note the smallest that never drops out | | |
| 8 | Laptop without ASIO: play | Top bar says `exclusive`; other programs go quiet while it plays and come back on Stop | | |
| 9 | Install `dist\installer\Crucible_0.0.1_x64_en-US.msi` on another PC | Start menu entry "Crucible", it opens, Apps list shows it and can remove it. SmartScreen warns "unknown publisher": More info, Run anyway | | |

If something fails: press F9 when it happens, then Settings, Diagnostics, Create report file,
and send that file (needs build 0.0.2 or later; on 0.0.1 send `crucible report` output).
