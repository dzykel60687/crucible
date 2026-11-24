# 03. Platform notes: the "never tied to Windows" contract

Dzyan's requirement: perfect it on Windows first, but always leave room for macOS and Linux.
He will not test either for a while. So the code and docs must stay honest about what is
Windows-specific without anyone running the other systems.

This file is the single ledger. Every other doc has a short "Platform notes" section that
points back here.

## The rules

1. Operating-system code lives only in these places:
   - `crates/crucible-audio-io` (seam 1)
   - `crates/crucible-midi-io` (seam 1)
   - `crates/crucible-plugin-host`, in the `editor_window`, `module_loader` and `plugin_paths` modules (seam 2)
   - `crates/crucible-diagnostics`, in `platform_windows.rs` and `platform_other.rs` only (native crash capture)
   - `apps/desktop/src-tauri`, for installer and window-chrome details only
2. Outside those places there is no `#[cfg(target_os = ...)]`, no `windows` crate import, and no
   Win32, WASAPI or ASIO call. A search for those strings outside the seams should return nothing.
3. Each seam exposes a trait or a small set of functions. The Windows implementation is one
   module. A new platform means adding a module, not editing callers.
4. Every platform-specific source file starts with a comment like this:

   ```rust
   // PLATFORM: Windows only (WASAPI exclusive mode).
   // macOS equivalent: CoreAudio. Linux equivalent: PipeWire, JACK or ALSA.
   // Callers use the AudioBackend trait and never see this module.
   ```
5. Every new Windows-specific thing gets a row in the ledger below, in the same session it is written.
6. The portable crates should compile for Linux and macOS targets even though nobody runs them
   yet. A compile check is cheap and catches leaks early. Add it once the workspace exists.

## The ledger

| Area | Windows (now) | macOS (later) | Linux (later) | Where it lives |
|---|---|---|---|---|
| Low-latency audio | ASIO, and WASAPI exclusive mode | CoreAudio, built in, already low latency | PipeWire or JACK, ALSA as fallback | `crucible-audio-io` |
| Ordinary audio fallback | WASAPI shared mode, labelled as not for serious work | Not needed | PulseAudio through PipeWire | `crucible-audio-io` |
| MIDI devices | Windows MIDI through `midir` | CoreMIDI | ALSA sequencer | `crucible-midi-io` |
| VST3 plugin folders | `C:\Program Files\Common Files\VST3` | `/Library/Audio/Plug-Ins/VST3` and the per-user copy | `~/.vst3`, `/usr/lib/vst3`, `/usr/local/lib/vst3` | `crucible-plugin-host::plugin_paths` |
| VST3 module loading | A `.vst3` bundle folder or single DLL | A bundle with a Mach-O binary | A bundle with a `.so` | `crucible-plugin-host::module_loader` |
| Plugin editor window | A Win32 window handle | An `NSView` | An X11 window. Plugins expect X11 even on Wayland. | `crucible-plugin-host::editor_window` |
| Web view engine | WebView2, Chromium-based | WKWebView, WebKit | WebKitGTK, the weakest of the three | Tauri |
| Per-user settings folder | `%APPDATA%` | `~/Library/Application Support` | `~/.config` | Resolved by a library call, never hard-coded |
| Installer | `.msi` or `.exe` | `.dmg`, needs Apple signing to share | AppImage or a package | Tauri bundler |
| Reference instruments | Omnisphere, Keyscape | Omnisphere, Keyscape | Not available. Use native instruments. | Test matrix |
| Per-user VST3 folder | `%LOCALAPPDATA%\Programs\Common\VST3` | `~/Library/Audio/Plug-Ins/VST3` | `~/.vst3` | `crucible-plugin-host::plugin_paths` |
| Editor window event pump | Win32 `PeekMessage` loop on the main thread. vst3-host does not do it | AppKit run loop | X11 polling, done inside vst3-host | `spikes/m0-omnisphere/src/pump.rs` today. Tauri's event loop in the real app |
| Static C runtime | `.cargo/config.toml` links the MSVC runtime statically so test builds copy to any PC | Not applicable | Not applicable | `.cargo/config.toml` |
| WASAPI exclusive mode | Implemented with the `wasapi` crate: capability probe, timing test, output stream. Audio thread joins the MMCSS "Pro Audio" class (avrt.dll); `CRUCIBLE_NO_MMCSS=1` skips it | Not applicable: CoreAudio is already exclusive-grade | ALSA `hw:` devices, or JACK / PipeWire with a small quantum | `crucible-audio-io/src/wasapi_exclusive.rs`, behind `open_exclusive_output` |
| Diagnostic log folder | `%LOCALAPPDATA%\Crucible\logs` | `~/Library/Application Support/Crucible/logs` | `~/.local/share/Crucible/logs` | `crucible-diagnostics::log_dir`, resolved by the `dirs` crate. No per-OS code |
| Crash capture | `SetUnhandledExceptionFilter` names the crashing module; `MiniDumpWriteDump` (dbghelp.dll, part of Windows) writes a `.dmp` | Not done. Needs a signal handler (SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGABRT) and `dladdr` | Not done. Same as macOS | `crucible-diagnostics/src/platform_windows.rs`; stub in `platform_other.rs`. Details in `11-error-codes.md` |
| Exit-status decoding | NTSTATUS codes such as `0xC0000005` named in plain English | Signals named instead | Signals named instead | `crucible-diagnostics::describe_exit` |
| Audio error decoding | WASAPI HRESULTs mapped to E2xxx codes | CoreAudio `OSStatus` will map to the same codes | ALSA/PipeWire errors will map to the same codes | `crucible-audio-io/src/wasapi_exclusive.rs`, `explain()` |
| Machine facts in log headers | `sysinfo` crate | `sysinfo` crate | `sysinfo` crate | `crucible-diagnostics/src/system.rs`. Portable |
| UI typeface | Bundled Inter and JetBrains Mono (D-026), identical everywhere | Same files | Same files | `app/src/fonts`, `@font-face` in `app/src/style.css` |
| Canvas drawing | Chromium Canvas 2D. `roundRect` used; WebKit has it since Safari 16 | WebKit Canvas 2D | WebKitGTK Canvas 2D; check `roundRect` and performance first | `app/src/timeline.ts`, `panels.ts` |
| Memory measurement tool | `tools/measure-memory.ps1`: private working set over the WebView2 process tree | `footprint` or Activity Monitor per process | Sum `Pss` from `/proc/<pid>/smaps_rollup` | Test tool only, not shipped |
| Installer | MSI built by Tauri's bundler with WiX 3 (`npx tauri build --bundles msi`) | `.dmg` / `.app` from the same bundler | `.deb` / AppImage from the same bundler | `app/src-tauri/tauri.conf.json` `bundle` section |
| ASIO drivers (D-035) | cpal's `asio` feature, on by default. The SDK's host files are in the repo (`third-party/asiosdk`, GPLv3 option); a build machine also needs LLVM's `libclang.dll`, by default in `..\crucible-sdk` (`.cargo/config.toml`); machines that run Crucible need nothing | No ASIO: CoreAudio covers it | No ASIO: JACK / PipeWire cover it | `crucible-audio-io` feature `asio`; the feature does nothing off Windows |
| One output function for every driver | `open_output`: ASIO and shared mode through cpal, exclusive through the `wasapi` module; order ASIO, exclusive, shared (D-018) | The cpal path alone (CoreAudio) | The cpal path alone (ALSA / PipeWire / JACK) | `crucible-audio-io/src/output.rs`. Portable |
| Audio priority for helper threads | MMCSS "Pro Audio" (`AudioPriority::join`), used by the playback feeder | `thread_policy_set` time-constraint policy. To do | `SCHED_FIFO` through rtkit. To do | `crucible-audio-io/src/lib.rs`; does nothing off Windows yet |
| Per-machine audio output choice | `%LOCALAPPDATA%\Crucible\audio-output.json` (beside the log folder) | `~/Library/Application Support/Crucible/` | `~/.local/share/Crucible/` | `crucible-engine/src/play.rs`, path from `crucible_diagnostics::log_dir`. Portable |

## Known future costs, recorded so they are not a surprise

- **macOS:** sharing a build with other people requires a paid Apple developer account for
  signing and notarisation. Loading third-party plugins into a signed app also needs a specific
  entitlement. **(unverified detail, check when the port starts)**
- **macOS:** many Mac musicians expect Audio Unit plugins. VST3 covers Omnisphere and most
  mainstream plugins, so Audio Unit support is not planned.
- **Linux:** the web view is slower and less polished than on Windows. One team publicly moved
  back to Electron because of it. Tauri has an experimental replacement engine that is not
  production-ready. Linux is the lowest-priority platform, so this cost is accepted. If it becomes
  blocking, the headless engine allows a different front end on Linux only.
- **Linux:** plugin editors need X11. Floating plugin windows, already a design rule, avoid the
  worst of the Wayland problems.
- **All three:** the interface must be checked on each web engine. Windows uses Chromium's
  engine, Mac and Linux use WebKit. Avoid Chromium-only CSS and JavaScript features in the UI.

## Things that are easy to get wrong

- Hard-coding `\` in paths. Use path-joining functions, and forward slashes inside project files.
- Assuming file names are case-insensitive. They are not on Linux.
- Assuming a drive letter exists.
- Using Windows-only fonts in the UI. Bundle the fonts.
- Using a keyboard shortcut that only makes sense on Windows. Define shortcuts with a
  "primary modifier" that maps to Ctrl on Windows and Linux and Cmd on Mac.
- Storing an ASIO device name in a project. Device choices are per machine.
