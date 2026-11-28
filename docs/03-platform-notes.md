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
   - `crates/crucible-test-synth`, a test fixture never shipped: its module entry points and `platform_windows.rs` (its window)
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
| Per-user install (D-037, 0.0.3) | MSI from Tauri's WiX template, changed to install for the person in `%LOCALAPPDATA%\Programs\Crucible`, no administrator rights, no download (WebView2 must already be there, D-041); registry key paths and RemoveFolder entries as Windows Installer requires there (`app/src-tauri/wix/main.wxs`) | `.app` in `~/Applications` or `/Applications`; logs go to `~/Library/Application Support/Crucible` (bundles are read-only) | AppImage anywhere the person can write, or a user `.deb`; log beside it | Template only; nothing in code |
| Hidden helper process | The window runs `crucible.exe report` with CREATE_NO_WINDOW, or a console window would flash (`quiet_command`) | Nothing needed | Nothing needed | `crucible-diagnostics/src/platform_windows.rs` and `platform_other.rs` |
| Where a report is written | The Desktop (`dirs::desktop_dir`; may be inside OneDrive), else Documents, else the log folder | `~/Desktop` | `~/Desktop` or the XDG desktop folder | `crucible-diagnostics/src/session.rs`, `report_dir`. Portable |
| Device list and plug detection (D-038) | MMDevice API through the `wasapi` crate: endpoints, the format Windows runs each at (`PKEY_AudioEngine_DeviceFormat`), and an endpoint notification callback for plug, unplug, default and property changes | CoreAudio `kAudioHardwarePropertyDevices` with property listeners; `kAudioDevicePropertyNominalSampleRate`. To do | PipeWire registry events, or udev with ALSA hardware parameters. To do | `crucible-audio-io/src/wasapi_endpoints.rs`; `endpoints()` and `DeviceWatch` in `lib.rs`. Off Windows: names through cpal, no rates, no events |
| Who else is using a device (D-038) | Audio sessions on the endpoint (`IAudioSessionManager2`), process names from `QueryFullProcessImageNameW`. A program on ASIO is invisible | Not applicable: CoreAudio mixes every program | `pw-dump` shows clients linked to a node. To do | Same file, `users` |
| Per-machine install (0.0.4 on; D-037 part three) | The same template, per machine again: `C:\Program Files\Crucible`, asks for admin. It makes a `logs` folder there that Users may write (`MsiLockPermissionsEx`, so Windows Installer 5.0, Windows 7 on); the program files stay protected | `.app` in `/Applications`; logs in `~/Library/Application Support/Crucible` | A system package installs read-only; logs in `~/.local/share/Crucible` | `app/src-tauri/wix/main.wxs`; `log_dirs` in `crucible-diagnostics/src/session.rs` (portable) |
| Folder and file pickers (D-042) | Explorer's own dialogs (`IFileDialog`) through the `rfd` crate, on a thread of their own for COM, in front of the window | `NSOpenPanel` through the same crate | The XDG desktop portal through the same crate (its default features) | `pick` in `app/src-tauri/src/main.rs`. No per-OS code of ours |
| An unsaved project's folder (D-042) | `%LOCALAPPDATA%\Crucible\unsaved\<process>-<n>`, a lock file held while in use (`File::try_lock`, std) | `~/Library/Application Support/Crucible/unsaved` | `~/.local/share/Crucible/unsaved` | `crucible-engine/src/scratch.rs`. Portable |
| GUI test: the real picker | `tools/close-window.ps1` finds the dialog by its title (user32 `FindWindow`) and closes it | AppleScript. To do | `xdotool` or a portal call. To do | Test tool only, not shipped |
| Where the log is (D-037) | Beside the program; installed under Program Files, in the `logs` folder the installer makes there; `%LOCALAPPDATA%\Crucible` if neither can be written | Signed app bundles are read-only: `~/Library/Application Support/Crucible` | Beside the program, else `~/.local/share/Crucible` | `crucible-diagnostics/src/session.rs`, `log_dirs`. Portable |
| ASIO drivers (D-035) | cpal's `asio` feature, on by default. The SDK's host files are in the repo (`third-party/asiosdk`, GPLv3 option); a build machine also needs LLVM's `libclang.dll`, by default in `..\crucible-sdk` (`.cargo/config.toml`); machines that run Crucible need nothing | No ASIO: CoreAudio covers it | No ASIO: JACK / PipeWire cover it | `crucible-audio-io` feature `asio`; the feature does nothing off Windows |
| One output function for every driver | `open_output`: ASIO and shared mode through cpal, exclusive through the `wasapi` module; order ASIO, exclusive, shared (D-018) | The cpal path alone (CoreAudio) | The cpal path alone (ALSA / PipeWire / JACK) | `crucible-audio-io/src/output.rs`. Portable |
| Audio priority for helper threads | MMCSS "Pro Audio" (`AudioPriority::join`), used by the playback feeder | `thread_policy_set` time-constraint policy. To do | `SCHED_FIFO` through rtkit. To do | `crucible-audio-io/src/lib.rs`; does nothing off Windows yet |
| Per-machine audio output choice | `%LOCALAPPDATA%\Crucible\audio-output.json` (beside the log folder) | `~/Library/Application Support/Crucible/` | `~/.local/share/Crucible/` | `crucible-engine/src/play.rs`, path from `crucible_diagnostics::log_dir`. Portable |
| Plugin list (D-044) | `%LOCALAPPDATA%\Crucible\plugins.json`; `CRUCIBLE_VST3_PATH` replaces the folders searched and `CRUCIBLE_PLUGIN_CATALOG` the file (tests) | `~/Library/Application Support/Crucible/` | `~/.local/share/Crucible/` | `crucible-plugin-host/src/catalog.rs`. Portable |
| A plugin's window above the main one (D-044) | The main window becomes its owner (`SetWindowLongPtrW(GWLP_HWNDPARENT)`), found among this thread's windows by vst3-host's class name `VST3PluginWindow` | `NSWindow.addChildWindow`, or a floating panel level. To do | `WM_TRANSIENT_FOR` on the X11 window. To do | `crucible-plugin-host/src/editor_window.rs` (seam 2); the main window's handle reaches the engine through `raw-window-handle` in `app/src-tauri/src/main.rs` |
| Plugin calls on the main thread (D-044) | Tauri's `run_on_main_thread`; the window program's command threads hand load, settings and windows over and wait | Same; AppKit requires it as well | Same | `crucible-engine/src/rack.rs` (`on_main`) and `serve_plugins` in `app/src-tauri/src/main.rs`. No per-OS code |
| MIDI keyboards played live (D-044) | `midir` over WinMM. A port can be open in one program at a time: with another program on the keyboard, Crucible says it could not open it. Windows MIDI Services (shared ports) is a `midir` feature not yet on | CoreMIDI: ports are shared between programs | ALSA sequencer: ports are shared | `crucible-midi-io::connect` |
| Scan child process (D-044) | The window program runs itself with `--probe-plugin <bundle>` (a GUI program, so no console flashes); the command-line tool runs itself with `probe-plugin` | Same | Same | `app/src-tauri/src/main.rs`, `crates/crucible-cli/src/main.rs` |
| Test instrument (D-044, never shipped) | A `.vst3` folder with `Contents/x86_64-win/<name>.vst3` (the DLL); `InitDll`/`ExitDll`; its window a Win32 `STATIC` child | `Contents/MacOS/<name>`, `bundleEntry`/`bundleExit`; no window yet | `Contents/x86_64-linux/<name>.so`, `ModuleEntry`/`ModuleExit`; no window yet | `crates/crucible-test-synth` |

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
