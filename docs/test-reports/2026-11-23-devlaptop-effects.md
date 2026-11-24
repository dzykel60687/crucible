# Test report: built-in effects (dev laptop)

Reverb, IR Reverb, Delay and Echo (D-033, design in `13-reverb-and-delay.md`).

## Setup

| Item | Value |
|---|---|
| Date | 2026-11-23 |
| Build tested | working tree after `5845a0b`, uncommitted |
| Machine | Dev workstation (Lenovo laptop). Not a music machine |
| Audio device | none: effects run in the offline export only, until playback exists |
| Test files | `dist/test-audio`: the stems, `ir-synthetic-hall.wav` (`make_test_ir.py`), the demo project |

## What a machine verified

| # | Check | Result |
|---|---|---|
| 1 | Mix 0 leaves the input byte-identical; Mix 100 has no dry signal | Pass, every effect |
| 2 | No memory allocated while processing, for every effect including IR Reverb's largest FFT level | Pass |
| 3 | Finite output with full-scale noise; exactly silent after the tail; Tape echo at 110% stays under +6 dBFS | Pass |
| 4 | Identical output for host blocks of 1, 64, 480 and 4,096 samples | Pass (Reverb, Echo, IR Reverb) |
| 5 | Reverb decay per band, all three characters, 48 and 96 kHz: 500 Hz and 1 kHz within 12% of Decay; 8 kHz within 25% of Decay x Damping | Pass |
| 6 | Reverb stereo tail: left/right correlation under 0.3; wet level steady across Decay 0.3 to 8 s and characters | Pass |
| 7 | IR Reverb against a 64-bit direct convolution | Pass, error below -100 dB of peak |
| 8 | IR Reverb latency: an impulse at sample k comes out at exactly k, buffers 1 to 512 | Pass |
| 9 | True-stereo IR: each of the four paths routes only where it should | Pass |
| 10 | Delay: a quarter note at 120 BPM lands on sample 22,050 / 24,000 / 48,000 at 44.1 / 48 / 96 kHz; repeats fall by the feedback within 0.15 dB; ping-pong alternates | Pass |
| 11 | Commands: add, set, bypass, move, remove, file, undo, save and reopen; refusals coded (E5021, E5022) | Pass |
| 12 | Window: "+ effect" lists the four, the editor drags send one command, double-click resets, bypass dims the slot | Pass (browser, synthetic input) |
| 13 | CPU, release, 48 kHz, 128-frame blocks | Reverb 2.0% of a core, Delay 0.08%, Echo 0.28%, IR Reverb 3 s true stereo 1.7% (worst block 35% of a callback) |
| 14 | Demo mix with effects | `exports/overnight-mix-with-effects-24bit.wav`, 36.6 s including a 4.6 s tail, rendered in 1.1 s |

## Needs Dzyan's ears

| # | Step | Expected | Pass / Fail | What actually happened |
|---|---|---|---|---|
| 1 | Play `dist\test-audio\exports\overnight-mix-with-effects-24bit.wav`, then the dry `overnight-mix-24bit.wav` | A small room on the drums, a tape echo on the lead, a hall on the chords. Nothing metallic, no clicks, the echo tail fades out cleanly at the end | | |
| 2 | In the window, open the demo project, click a slot in the mixer (Drums: Reverb), switch Room / Hall / Plate, export each (Ctrl+Shift+E) | Each sounds like its name. Say which one sounds wrong and how | | |
| 3 | Same with Echo on the lead: Wear 0, 30, 100 | 0 clean, 30 like a tape echo, 100 like a worn one | | |
| 4 | If you have real impulse responses (a hall or plate WAV), add IR Reverb, Choose file, export | Sounds like the space the IR came from | | |
| 5 | Optional, the ReaVerb null test: same IR in ReaVerb (normalise off, attenuate off, wet 0 dB, dry off, ZL on) and in Crucible (Normalise off, Mix 100), same dry file, send me both exports | A machine compares them: they should match to within float rounding | | |
