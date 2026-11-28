# 09. Open questions

Section A is for Dzyan. Section B is technical claims nobody has verified yet. Section C is
feature ideas parked so they do not leak into the code.

When something is answered, move the answer into `08-decisions.md` or the relevant doc and
strike it here with the date.

## A. Questions only Dzyan can answer

| # | Question | Why it matters | Blocks |
|---|---|---|---|
| ~~Q1~~ | ~~Exact Lynx and Behringer models?~~ Answered 2026-11-20: not needed, accept whatever drivers are installed. See D-018. | | |
| Q2 | Where does development happen: this workstation, the Lynx machine, or the laptop with the Behringer? | Sets the speed of every hardware test. See `07-hardware-and-testing.md` | M0 step B |
| Q3 | Are the other engineers on Windows or Mac? | Decides how soon the macOS port matters | Planning only |
| ~~Q4~~ | ~~ASIO licence: GPLv3, or sign Steinberg's agreement?~~ **Answered 2026-11-23: GPLv3** (D-036). The SDK is used under its GPLv3 option; nothing to sign. | | |
| ~~Q5~~ | ~~Will the project be open source, and under what licence?~~ **Answered 2026-11-23: yes, GPLv3** (D-036): "this can be totally open software". Making the repo public is still a separate ask. | | |
| Q6 | May a free VST3 instrument be installed on the dev machine for automated tests? Surge XT is the suggestion. | Lets a machine test plugin hosting without Omnisphere | Nice to have for M0 |
| Q7 | Standing approval to commit (and push to `dzykel60687/crucible`) as work proceeds? | Repo created and first pushes made 2026-11-20 at his request. Per-push asks remain until he says otherwise. | Nothing, but saves round trips |
| ~~Q11~~ | ~~Bundle the Inter typeface (OFL)?~~ **Answered 2026-11-21: yes** (D-026). | | |
| ~~Q8~~ | ~~What should the product be called?~~ **Answered 2026-11-20: Crucible** (D-023). | | |
| Q9 | Should the built-in EQ, compressor and reverb from `..\music-editor` be included, or should effects be VST3 only? **Partly answered 2026-11-23:** Dzyan wants a few very refined built-in tools, "not excessive amounts": Reverb, Delay, Echo, with reverb that is accurate and authentic (he named ReaVerb as the bar). Research: `docs/13-reverb-and-delay.md`. EQ and compressor still open. | Scope of M4 | M4 |
| Q10 | Confirm the *Proposed* decisions: D-005, D-013 to D-017, D-030 to D-034. (D-011 was replaced by D-036; the overlapping-clips part of D-031 is decided.) | So they stop being assumptions | Before M2 |
| ~~Q12~~ | ~~Overlapping clips on one track: sum them, or let the later one win?~~ **Answered 2026-11-23: both play** (summed, as built). | | |
| ~~Q13~~ | ~~Files at another sample rate: keep refusing, or convert at import?~~ **Answered 2026-11-25: offer to convert** (D-039). | | |
| Q14 | The ruler counts bars; he wants positions in minutes and seconds (2026-11-26, said of the top bar). Ruler in time, in bars, or a switch between them? | Bars mean nothing for a song not made to a click; they mean everything for MIDI at a tempo | Nothing |
| Q15 | Ctrl+drag drags the view on empty space only, because on a clip it copies (D-043). Make it drag the view everywhere and move copy to another key? | He asked for Ctrl+drag; the middle button already drags from anywhere | Nothing |
| Q16 | Hold converted audio at the file's own rate and convert while playing (D-039, 2026-11-26)? About four times less memory at 192 kHz | His two songs held 1.1 GB | Nothing |
| Q17 | ~~Start instrument and VST3 hosting before M0?~~ **Answered 2026-11-27: build it** ("why can't you build the baseline MIDI and VST stuff yet?"). Built as D-044 | | |

## B. Unverified technical claims

Each of these is written into the design from memory or from one source. Check it the first
time the code touches it. Do not build far on any of them unchecked.

| # | Claim | How to check | If false |
|---|---|---|---|
| T1 | `cpal` can open ASIO devices on Windows through its build feature. | Build it with the ASIO SDK and list devices on a machine with an ASIO driver. | Use a dedicated ASIO binding behind the same trait. |
| T2 | **Verified true 2026-11-20.** cpal 0.18.2 hard-codes WASAPI shared mode. Exclusive mode needs its own module. | Done: read `src/host/wasapi/device.rs`. | |
| T3 | `vst3-host` loads third-party plugins on Windows and can open their editors. **Partly checked 2026-11-20:** it fails cleanly on broken bundles. No real plugin tried yet. | M0 step B checklist. | Fall back to the `vst3` bindings and write the host layer. |
| T4 | Omnisphere and Keyscape work in a host that follows the VST3 spec plainly, with no special handling. | M0. | Record the quirks. Compare against how open-source hosts handle them, without copying code. |
| T5 | Tauri allows native windows to be created and serviced on its main thread alongside the web view. | M1. One public write-up says yes. | Host plugin windows from a helper process. |
| T6 | Tauri 2 can send raw binary payloads and typed commands with generated TypeScript types. | Read the current Tauri 2 docs. | Fall back to JSON with hand-written types. Meters may need batching. |
| T7 | **Answered false 2026-11-21:** `hound` stops at 4 GiB, about 4.1 hours of 24-bit stereo at 48 kHz (`12-known-daw-problems.md` F3). | | Write RF64 that falls back to plain WAV below 4 GB, behind one small writer function. |
| T8 | A pure-Rust FLAC encoder is good enough. | Compare output against the reference encoder. | Bind the reference library. |
| T9 | `rubato` matches soxr for conversion quality. | Measure both. `..\music-editor` already uses soxr. | Use soxr. |
| T10 | A Windows-installed WebView2 is present on all three test machines. | Check each. It ships with Windows 11 and with updated Windows 10. | Bundle the installer bootstrapper. |
| T11 | The Lynx card has a working driver for its machine's Windows version. | Dzyan checks Lynx's site once the model is known. | That machine falls back to exclusive mode. |
| T12 | macOS signing and plugin-loading entitlement details in `03-platform-notes.md`. | Check Apple's docs when the port starts. | Update the note. |
| T13 | The exclusive-mode audio thread needs the MMCSS "Pro Audio" priority to stay glitch-free under load. **Set 2026-11-21** (JUCE and Microsoft's sample both do it); `CRUCIBLE_NO_MMCSS=1` turns it off for comparison. No difference on the idle dev laptop. | Checklist step 2b on a loaded music machine. | Keep it either way; it costs nothing. |
| T14 | Omnisphere and Keyscape behave when their audio arrives in exclusive-mode block sizes (3 ms and up). | M0 step B checklist, steps 3 to 9. | Raise the default buffer. |

## C. Parked ideas

Not in scope. Listed so they are not lost and not built by accident.

- Freeze a track to audio to save CPU.
- One aux send for a shared reverb.
- CLAP plugin support.
- MP3 or other lossy export.
- Out-of-process plugin hosting.
- ~~A touch-first layout for the laptop in tablet mode.~~ Built 2026-11-21 as the Touch layout (D-028).
- An MCP server so an AI assistant can drive the DAW. At least one other Rust DAW project does this.

## Platform notes

- Q3 interacts with the ports. A Mac user among the engineers raises the priority of macOS.
  ASIO is Windows-only; its licence question (Q4) is settled by D-036.
- T10 is Windows-only. T12 is macOS-only. Every other item is platform-neutral or already
  scoped to the Windows seam.
