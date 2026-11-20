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
| Q4 | ASIO licence: accept GPLv3 for the project, or sign Steinberg's proprietary ASIO agreement? | The ASIO SDK is needed to build the ASIO path. Private use is fine either way. It matters once builds are given to other people. Also needs LLVM installed to build. | M0 ASIO step |
| Q5 | Will the project be open source, and under what licence? | Decides whether GPL reference code could ever be used, see D-011 | Nothing yet |
| Q6 | May a free VST3 instrument be installed on the dev machine for automated tests? Surge XT is the suggestion. | Lets a machine test plugin hosting without Omnisphere | Nice to have for M0 |
| Q7 | Standing approval to run `git init` and make local commits as work proceeds? | No repository exists yet, so there are no checkpoints. Local only, no remote. | Nothing, but strongly recommended |
| ~~Q8~~ | ~~What should the product be called?~~ **Answered 2026-11-20: Crucible** (D-023). | | |
| Q9 | Should the built-in EQ, compressor and reverb from `..\music-editor` be included, or should effects be VST3 only? | Scope of M4 | M4 |
| Q10 | Confirm the *Proposed* decisions: D-005, D-011, D-013 to D-017. | So they stop being assumptions | Before M2 |

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
| T7 | `hound` handles WAV files over 4 GB. | Read its docs. Probably false. | Add RF64 or split long recordings. |
| T8 | A pure-Rust FLAC encoder is good enough. | Compare output against the reference encoder. | Bind the reference library. |
| T9 | `rubato` matches soxr for conversion quality. | Measure both. `..\music-editor` already uses soxr. | Use soxr. |
| T10 | A Windows-installed WebView2 is present on all three test machines. | Check each. It ships with Windows 11 and with updated Windows 10. | Bundle the installer bootstrapper. |
| T11 | The Lynx card has a working driver for its machine's Windows version. | Dzyan checks Lynx's site once the model is known. | That machine falls back to exclusive mode. |
| T12 | macOS signing and plugin-loading entitlement details in `03-platform-notes.md`. | Check Apple's docs when the port starts. | Update the note. |
| T13 | The exclusive-mode audio thread needs the MMCSS "Pro Audio" priority to stay glitch-free under load. Not set yet. | Compare `crucible audio.test` and the `t` timing line with and without it, on a loaded machine. | If it makes no difference, leave it out. |
| T14 | Omnisphere and Keyscape behave when their audio arrives in exclusive-mode block sizes (3 ms and up). | M0 step B checklist, steps 3 to 9. | Raise the default buffer. |

## C. Parked ideas

Not in scope. Listed so they are not lost and not built by accident.

- Freeze a track to audio to save CPU.
- One aux send for a shared reverb.
- CLAP plugin support.
- MP3 or other lossy export.
- Out-of-process plugin hosting.
- A touch-first layout for the laptop in tablet mode.
- An MCP server so an AI assistant can drive the DAW. At least one other Rust DAW project does this.

## Platform notes

- Q3 and Q4 both interact with the ports. A Mac user among the engineers raises the priority of
  macOS. The ASIO question is Windows-only and disappears on the other systems.
- T10 is Windows-only. T12 is macOS-only. Every other item is platform-neutral or already
  scoped to the Windows seam.
