# 04. Audio quality

## The honest premise

Exporting audio is arithmetic. A correct engine adds samples exactly, and two correct engines
produce bit-identical files. Reaper's engine is correct. So this DAW cannot sound "better" than
Reaper in its summing. Nobody's can.

Where it can be better is in **defaults and guard rails** that stop the user damaging the audio
by accident, and in putting the translation tools where they are seen.

## Rules the engine must obey

1. **No silent resampling.** Record and play at the project sample rate. If a file or a device
   does not match, convert once, at the edge, with a high-quality converter, and show a visible indicator.
2. **No hidden processing.** No automatic normalising, limiting, dithering or gain change.
   Anything that alters samples is something the user switched on and can see.
3. **Unity is exact.** A track at 0 dB, centre pan, no plugins, passes samples through unchanged.
4. **Recording stores what the driver delivers.** Same bit depth, no dither, no gain. Default 24-bit WAV.
5. **One render path.** Export runs the same code as playback. See `02-architecture.md`.
6. **Exclusive device access for real work.** ASIO, or WASAPI exclusive mode. Windows' shared
   mode resamples and applies a limiter, so it is offered only as a clearly labelled fallback.

## Machine-checkable tests for those rules

These need no ears, so a coding session can run them unattended.

| Test | Pass condition |
|---|---|
| Null test | Import a WAV, export at the same format with everything at unity. Audio data is byte-identical. |
| Determinism | Render the same project twice offline. The two files hash the same. |
| Gain law | A track at -6 dB produces samples scaled by exactly the expected factor. |
| Pan law | Centre, hard left and hard right produce the documented levels. |
| Silence | An empty project renders digital silence, all zeros. |
| No allocation | The audio callback, run under the allocation detector, allocates nothing. |
| Round-trip format | 24-bit in, 24-bit out, no dither: identical. 32-bit float in and out: identical. |

**Status 2026-11-22** (`crucible-engine/src/audio.rs` tests, run by `cargo test`): null test and
round-trip pass for 16-bit (dither off), 24-bit and 32-bit float; determinism passes, including
16-bit dither noise (seeded); gain law, pan law (D-030) and master gain pass with exact values;
silence passes. "No allocation" passes since 2026-11-23: test builds count heap allocations per
thread (`crucible-engine` `alloc_count`), and 30 blocks through `render` make none. These cover offline export only;
nothing has been played through a device yet.

### Added 2026-11-23: playback

| Test | Pass condition | Status |
|---|---|---|
| Playback equals export | The samples the audio callback hands the driver equal a 32-bit float export of the same project, bit for bit | Pass (`play.rs` test, with Reverb and Echo on the tracks) |
| Edits keep effects running | After a gain or pan change during playback the output equals an export of the edited project from that sample on | Pass |
| Device conversion is exact | Every 16-bit value and a sweep of 24-bit values, as import produces them, reach the device unchanged (16, 24-in-32, 32-bit int) | Pass (`convert.rs`). Before this date output scaled by 2^n - 1 and lost the last bit of loud samples |
| Block size does not change the sound | Effects give identical output for blocks of 1, 64, 256, 480 and 4,096 samples, including the moment they go idle after a tail | Pass. Before this date they differed below -150 dBFS near the end of a tail |
| The callback allocates nothing | Counted | Pass |

Shared mode is the exception to all of this: Windows mixes and converts the stream itself. The
transport says so every time it has to fall back to shared mode.

## Where the sound card matters

| Stage | Depends on the interface? |
|---|---|
| Recording a microphone or instrument | Yes. Its converters and preamps set the ceiling. |
| What is heard while mixing | Yes. It drives mix decisions, so it drives translation. |
| Latency while playing an instrument | Yes. Driver quality decides it. |
| The sound of a virtual instrument in the exported file | No. |
| Export to any format | No. Identical on any machine. |

Consequence for the three machines: record and judge on the Lynx machine. Arrange anywhere.
See `07-hardware-and-testing.md`.

## Driver modes on Windows

| | ASIO | WASAPI exclusive | WASAPI shared |
|---|---|---|---|
| Bit-perfect | Yes | Yes | No |
| Latency | Lowest, tuned by the hardware maker | Low on good drivers, variable on cheap ones | Too high for live playing |
| Use here | First choice whenever a native driver exists | Required for the laptop | Labelled fallback only |

ASIO is a driver protocol, not a quality grade. Quality comes from the converters. Exclusive
mode is not lower quality than ASIO. The generic "ASIO4ALL" wrapper used by cheap interfaces is
ordinary Windows driver access presented as ASIO.

## Headroom

Digital audio has a hard ceiling, 0 dBFS (decibels relative to full scale). Above it, samples
are clipped. Headroom is the gap between the loudest peak and that ceiling.

Inside the engine nothing clips, because 32-bit float has effectively unlimited range. The
ceiling exists at exactly two places: the output to the hardware, and a fixed-point export file.

Working targets:

| Point in the chain | Target peak |
|---|---|
| Each track or instrument | About -12 to -18 dBFS |
| The master, before final loudness work | About -6 dBFS |
| An exported file | No higher than -1 dB true peak |
| Live output to a PA | About -6 to -10 dBFS |

What the DAW does about it:

- A **headroom readout** and a **latching clip light** at the two real ceilings, always visible in the top bar.
- A **true-peak meter**, because peaks between samples can exceed the sample values, and lossy
  encoders overshoot.
- An export-time warning when true peak is above -1 dB, with the measured number.
- No automatic limiter. Ever.

## Translation tools

The music must sound good on ordinary headphones, on basic powered speakers, and better live.
No DAW can guarantee that. It comes from checking on several systems. The DAW makes the checks one click:

| Tool | What it is for |
|---|---|
| **Mono button** | Sums left and right. Parts that cancel will vanish on phone speakers, many Bluetooth speakers, subwoofers and summed PA systems. It is a test, not an export format. Exports are always stereo. |
| **Reference A/B** | Loads a commercial track on a hidden track, level-matched, and switches between it and the mix instantly. |
| **Loudness meter** | Integrated, short-term and momentary loudness, using the broadcast standard. |
| **True-peak meter** | See above. |

For live use: keep essential parts near the centre, use panning for width, leave headroom, and
do not pre-limit. The venue's system supplies the loudness.

## Export defaults

Proposed, see decision D-014.

| Setting | Default | Why |
|---|---|---|
| Format | WAV | Lossless and universal |
| Bit depth | 24-bit | Matches the converters |
| Sample rate | The project rate | No conversion unless asked |
| Dither | Off for 24-bit and float, on for 16-bit | Dither only matters when reducing bit depth. Built: TPDF, one LSB, seeded so two exports match; `--dither off` for the null test |
| Also offered | 32-bit float WAV, FLAC | Float cannot clip. FLAC is lossless and smaller. |
| Lossy formats | Not in version 1 | Add MP3 later behind an explicit "lossy" label |
| Normalise | Never automatic | Hidden processing |

The export dialog shows, before the user confirms: the format, whether any sample-rate
conversion will happen, whether dither will be applied, and the measured true peak.

## Note on export determinism (2026-11-21)

"Render twice, get the same hash" holds only for projects without third-party plugins. Some
synths randomise per render, and plugins may behave differently in offline mode
(`12-known-daw-problems.md` R4, R5). Keep the test for Crucible's own code; for projects with
plugins, compare renders by ear and by level, not by hash.

## Platform notes

- All rules and tests in this document are platform-neutral.
- The driver-mode table is Windows-specific. On macOS the built-in audio system is already
  bit-perfect and low latency, so the three-way choice does not exist. On Linux the equivalent
  choice is between PipeWire, JACK and raw ALSA.
- Rule 6 must be restated per platform when each port starts. Record it in `03-platform-notes.md`.
