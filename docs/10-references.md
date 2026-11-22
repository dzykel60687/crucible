# 10. References

Gathered during the design session on 2026-11-19. Web pages change. Re-check anything a
decision depends on before relying on it.

## Licensing

- Steinberg moves VST3 to MIT: https://www.soundonsound.com/news/steinberg-adopt-mit-license-vst3
- VST3 licence page: https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/VST3+License
- VST3 SDK source, includes hosting examples: https://github.com/steinbergmedia/vst3sdk
- VST3 Linux support since 3.6.7: https://steinbergmedia.github.io/vst3_dev_portal/pages/Versions/Version+3.6.7.html

## Rust plugin hosting

- `vst3-host` crate, MIT: https://github.com/HelgeSverre/rust-vst3-host
- Its platform-support doc: https://github.com/HelgeSverre/rust-vst3-host/blob/main/docs/reference/platform-support.md
- "Creating a DAW in Rust: VST Plugins", a Tauri app hosting an Arturia instrument, October 2026:
  https://whoisryosuke.com/blog/2026/creating-a-daw-vst-plugins/
  Key lessons: plugin windows must be created on the main thread, and serviced from the main event loop.
- nexus, a Rust VST3 and CLAP host: https://github.com/poucet/nexus
- Futureboard, a pre-alpha Rust DAW with out-of-process VST3 hosting: https://github.com/futureboard/Futureboard

## Other Rust DAW projects, all early stage

- Meadowlark, paused: https://billydm.github.io/blog/why-im-taking-a-break-from-meadowlark
- Meadowlark's author on DAW GUI needs: https://billydm.github.io/blog/daw-frontend-development-struggles
- YADAW, egui: https://github.com/mlm-games/yadaw
- Generic DAW, iced: https://github.com/generic-daw/generic-daw
- SoundCraft, egui: https://github.com/storytold/soundcraft/wiki
- ProWave, egui: https://github.com/fabox-os/Pro-Wave/wiki

## Open-source DAWs to read for approach only (GPL, do not copy code)

- Ardour. Zrythm. Tracktion Engine. JUCE's plugin host example.

## Tauri

- Governance: https://v2.tauri.app/about/governance/
- Linux graphics troubleshooting: https://v2.tauri.app/develop/debug/linux-graphics/
- Window builder docs, including the Windows deadlock warning:
  https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindowBuilder.html
- Experimental replacement web engine: https://v2.tauri.app/blog/tauri-verso-integration/
- First-hand reports:
  - Six months with Tauri: https://hackernoon.com/six-months-with-tauri-the-benefits-and-the-bill
  - Firezone: https://www.firezone.dev/blog/using-tauri
  - Hopp: https://www.gethopp.app/blog/tauri-vs-electron
  - A team that went back to Electron over Linux: https://dev.to/moboudra/i-was-wrong-about-electron-1e9g
  - Windows drag-and-drop war stories: https://dev.to/devrayat000/building-a-transparent-drag-and-drop-shelf-for-windows-with-tauri-the-engineering-war-stories-3c8c

## GUI toolkit comparison

Community numbers as measured on 2026-11-19:

| Toolkit | GitHub stars | Downloads, last 90 days | Maturity | Releases, last 12 months |
|---|---|---|---|---|
| Tauri | 113k | 19.4M | Stable since 2022, 2.12 | 21 |
| egui | 32k | 10.4M | Pre-1.0 | 10 |
| Dioxus | 40k | 1.4M | Pre-1.0 | 9 patches |
| iced | 32k | 0.85M | Pre-1.0, "experimental" | 1 |
| Slint | 25k | 1.15M | Stable since 2023 | 9 |
| GPUI | not comparable | 0.3M | Pre-1.0 | 0 official |
| Qt via cxx-qt | 1.6k | 0.13M | Pre-1.0 | 5 |

- 2025 survey of Rust GUI libraries: https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html
- blessed.rs: https://blessed.rs/crates
- lib.rs GUI category: https://lib.rs/gui
- egui in production for a year: https://nominal.io/blog/nominal-connect-shipping-realtime-desktop-software-with-rust-bevy-and-egui
- Slint's own desktop gap list: https://www.slint.dev/blog/making-slint-desktop-ready
- Slint used for audio plugin interfaces: https://slint.dev/success/wesaudio-daw
- iced FAQ: https://book.iced.rs/faq.html

## Hardware and plugins

- Spectrasonics Omnisphere: https://www.spectrasonics.net/products/omnisphere/overview.php
- ASUS TN3604YA support and drivers: https://www.asus.com/us/supportonly/tn3604ya/helpdesk_download/
- Behringer driver guide: https://www.sweetwater.com/sweetcare/articles/behringer-audio-interface-setup-guide/
- Native Linux instruments thread: https://www.kvraudio.com/forum/viewtopic.php?p=9270258
- u-he Linux builds: https://www.kvraudio.com/forum/viewtopic.php?p=8453654
- yabridge, for Windows plugins on Linux: https://www.github.com/robbert-vdh/yabridge

## Crates in use, with licences (checked when added)

| Crate | Version | Licence | Used for |
|---|---|---|---|
| `cpal` | 0.18 | Apache-2.0 | Device listing, WASAPI shared mode |
| `wasapi` | 0.27 | MIT | WASAPI exclusive mode (D-020) |
| `midir` | 0.11 | MIT | MIDI inputs |
| `vst3-host` | 0.11 | MIT | VST3 loading, playback path in the spike |
| `hound` | 3.5 | Apache-2.0 | WAV writing in the spike |
| `sysinfo` | 0.40 | MIT | Machine facts in log headers (D-021) |
| `chrono` | 0.4 | MIT or Apache-2.0 | Local time in logs (D-021) |
| `log` | 0.4 | MIT or Apache-2.0 | Captures library messages into the session log (D-021) |
| `windows-sys` | 0.61 | MIT or Apache-2.0 | Windows crash handler and minidumps (D-021) |
| `dirs`, `serde`, `serde_json`, `anyhow` | | MIT or Apache-2.0 | Folders, JSON, error plumbing |

## Dzyan's own related code

- `..\music-editor`: MIT, Rust. A browser-based audio editor for a NAS music library.
  Reusable parts: `src/dsp` (EQ, dynamics, reverb, delay), `src/loudness.rs`, `src/render.rs`.
  Its README has a "What it does to quality" section in the same spirit as `04-audio-quality.md`.

## Bundled fonts (D-026)

- Inter, SIL Open Font License 1.1, https://github.com/rsms/inter (files taken from the
  `@fontsource-variable/inter` 5.3.0 npm package, Latin and Latin Extended subsets).
- JetBrains Mono, SIL Open Font License 1.1, https://github.com/JetBrains/JetBrainsMono
  (from `@fontsource-variable/jetbrains-mono` 5.3.0, same subsets).

## Platform notes

Most sources above describe Windows or macOS behaviour. The Linux-specific ones are the Tauri
Linux graphics page, the team that reverted to Electron, the native Linux instruments thread,
the u-he builds thread and yabridge. Re-read those when the Linux port starts.
