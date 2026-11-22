# 12. Known DAW problems

Status: research notes, gathered 2026-11-21. Not decisions. Anything here that changes a decision
goes to `08-decisions.md` with Dzyan's yes first (rule 5).

## What this is and how to use it

Established hosts keep hitting the same problems. Their code comments, issue trackers and commit
logs show where. This file lists those problems so Crucible can design for them up front instead
of rediscovering them one bug report at a time.

- Before starting a milestone, read every entry whose **Crucible** line names that milestone.
- Each entry has five parts: **Problem**, **Evidence**, **What they do**, **Crucible**, **Severity**.
- **Severity** is for our users only: a Lynx PCI card on ASIO, a Behringer USB interface, a Realtek
  laptop in WASAPI exclusive mode, Omnisphere and Keyscape. **High**: a likely lost take, lost work,
  a crash or an unusable instrument. **Med**: audible trouble or a confusing failure. **Low**: rare
  or cosmetic for this group.
- "Already covered by X" means an existing doc or decision handles it; the entry only adds evidence.
- **(unverified)** marks a claim not confirmed in a source opened for this research.
- Every link was opened for this research. Source-file links point at the default branch; line
  positions move, so search for the words described.
- Licence: Ardour, JUCE, LMMS, Audacity and Carla are GPL or commercial. They were read to understand
  problems only. No code was copied. Techniques are described in our own words.
- "Local" means read in the cargo registry on the dev workstation: `vst3-host` 0.11.1, `hound` 3.5.1,
  `wasapi` 0.27.0. "Checked" means Crucible's own code was read to confirm the statement.

---

## 1. Audio drivers and devices

### A1. Device rate and project rate disagree

- **Problem:** the device runs at another rate than the project. Some paths resample silently, some play at the wrong pitch. The rate can also change under a running app: the user changes it in the driver panel, or the interface follows an external clock. Some interfaces change channel count with the rate.
- **Evidence:** Audacity refuses to join tracks of different rates and makes the user resample first ([PR #4739](https://github.com/audacity/audacity/pull/4739)). JUCE registers the ASIO driver's "sample rate changed" callback and resets the device ([juce_ASIO_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_ASIO_windows.cpp)). A MOTU changes channel count with rate; not re-reading it caused heap corruption ([JUCE PR #1616](https://github.com/juce-framework/JUCE/pull/1616)).
- **What they do:** open at the project rate; treat any driver-side rate change as stop, reopen, re-read channels, buffer sizes and latencies.
- **Crucible:** already covered by `04-audio-quality.md` rule 1 and `02-architecture.md` "Audio path". Add at the ASIO step: a driver rate change or reset request stops the transport and reopens on a normal thread. Never resume at another rate without the visible warning.
- **Severity:** Med. Whether the Lynx card is externally clocked is (unverified).

### A2. The buffer granted is not the buffer requested

- **Problem:** drivers round, clamp or ignore the requested size. WASAPI exclusive mode rejects unaligned sizes. ASIO drivers publish minimum, maximum, preferred and granularity, and some only work at their preferred size. Shared mode picks its own period.
- **Evidence:** Microsoft documents the re-initialise steps for `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` ([IAudioClient::Initialize](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize)). JUCE loops on that error, and forces the preferred size for drivers "which crash if you make dynamic changes to the buffer size" ([juce_WASAPI_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_WASAPI_windows.cpp), [juce_ASIO_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_ASIO_windows.cpp)). Ardour builds its size list from minimum, maximum and granularity ([portaudio_io.cc](https://github.com/Ardour/ardour/blob/master/libs/backends/portaudio/portaudio_io.cc)). A default buffer under pipewire-alsa gives constant xruns ([cpal #1390](https://github.com/RustAudio/cpal/issues/1390)).
- **What they do:** request, read back the granted size, and use the granted size everywhere.
- **Crucible:** the alignment retry is already in `crucible-audio-io/src/wasapi_exclusive.rs` (checked). M2 rule: the engine only uses the granted frame count. ASIO step: size list from granularity, default to the driver's preferred size.
- **Severity:** Med.

### A3. ASIO driver quirks

- **Problem:** one ASIO driver per process; input and output must be the same device. Drivers ask the host to reset when the user changes their panel. Some report success from init while failing. Some only work if called in the order Cubase uses. ASIO timestamps can step backwards. A driver can stay listed after its hardware is unplugged.
- **Evidence:** JUCE notes a Denon driver that returns true from init on failure, copies Cubase's call order because "some devices fail if we don't", debounces reset requests with a timer, and states ASIO cannot use different devices for input and output ([juce_ASIO_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_ASIO_windows.cpp)). A second stream fails because the driver is already loaded ([cpal #1356](https://github.com/RustAudio/cpal/issues/1356)). Focusrite's `systemTime` steps back and derived time overflows after about ten minutes ([cpal #1393](https://github.com/RustAudio/cpal/issues/1393), [#1370](https://github.com/RustAudio/cpal/issues/1370)). An unplugged Focusrite stays selectable ([Audacity #11981](https://github.com/audacity/audacity/issues/11981)).
- **What they do:** one owner object per driver; duplex in one open; reset, resync and buffer-size messages schedule a full close and reopen on a normal thread after a short delay; driver time is never trusted to be monotonic.
- **Crucible:** at the ASIO step (after Q4): one `AudioBackend` owns the driver; duplex only; reset requests become an engine event; time is clamped so it never runs backwards; a watchdog raises a coded error if no callback arrives within a few periods of start.
- **Severity:** High on the Lynx machine. Med on the Behringer.

### A4. Exclusive-mode format negotiation

- **Problem:** devices refuse float and want 24-in-32 or packed 24. In 24-in-32 the valid bits sit at the top of the container; getting it wrong is 48 dB off. Format lists are incomplete.
- **Evidence:** 48 dB too quiet on a PreSonus at 24-in-32 ([cpal PR #1305](https://github.com/RustAudio/cpal/pull/1305)). Only one format reported ([cpal #263](https://github.com/RustAudio/cpal/issues/263)).
- **What they do:** probe formats in a preference order; treat container size and valid bits separately.
- **Crucible:** already covered by D-020 and `crucible-audio-io/src/convert.rs`, which left-justifies and has a test (checked). Add the same test for capture in M2.
- **Severity:** Low (done).

### A5. Unplug, default-device change, sleep and resume

- **Problem:** streams die mid-session. Some APIs fail loudly, some silently switch device, some call back after stop. COM threading mistakes crash device enumeration.
- **Evidence:** `AUDCLNT_E_DEVICE_INVALIDATED` and `AUDCLNT_E_RESOURCES_INVALIDATED` ([IAudioClient::Initialize](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize)). WASAPI never reroutes a stream; it must be recreated ([cpal PR #1267](https://github.com/RustAudio/cpal/pull/1267)). macOS input silently falls back to the default device ([cpal #704](https://github.com/RustAudio/cpal/issues/704)). An enumerator used across threads crashes ([cpal #1302](https://github.com/RustAudio/cpal/issues/1302)); default-device activation fails with `RPC_E_CHANGED_MODE` ([cpal #1402](https://github.com/RustAudio/cpal/issues/1402)). A callback after stop crashed JUCE ([PR #1712](https://github.com/juce-framework/JUCE/pull/1712)). Toggling microphone enhancements invalidates the stream ([JUCE #1629](https://github.com/juce-framework/JUCE/issues/1629)). JUCE listens for add, remove and default-change notifications and rescans asynchronously ([juce_WASAPI_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_WASAPI_windows.cpp)).
- **What they do:** listen for notifications; on loss stop cleanly; never switch device automatically; keep COM objects on the thread that made them; guarantee no callback after stop returns.
- **Crucible:** partly covered (E2009 detects a dead stream). M2: an engine "device lost" state (transport stops, project untouched, banner names the device); reopen only when the same device identity returns. M6 soak test adds sleep and resume.
- **Severity:** High on the Behringer and the laptop.

### A6. Input and output on different devices drift

- **Problem:** two devices have two clocks. Recording from one while playing on another needs drift-tracking resampling and adds unknown latency. On WASAPI even one interface is two endpoints.
- **Evidence:** Meadowlark's I/O design refuses non-duplex device pairs because the intermediate buffer adds "a sometimes unkowable amount of latency" ([rainout DESIGN_DOC.md](https://raw.githubusercontent.com/MeadowlarkDAW/rainout/HEAD/DESIGN_DOC.md)). JUCE refuses exclusive mode when input and output buffer sizes differ ([juce_WASAPI_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_WASAPI_windows.cpp)). Ardour leaves "check drift" as a TODO ([portaudio_io.cc](https://github.com/Ardour/ardour/blob/master/libs/backends/portaudio/portaudio_io.cc)).
- **What they do:** most refuse or warn. macOS aggregate devices correct drift in the OS.
- **Crucible:** M2 recording: input and output on one physical device only. When two WASAPI endpoints open, check equal rate and period. Refuse a mixed pair with a coded error that says why.
- **Severity:** Med. The laptop invites "built-in mic in, headphones out".

### A7. Reported latency is not real latency

- **Problem:** drivers report latency without converter and transport delays, or wrongly. Recordings land late by a fixed amount. USB latency can change after a reconnect or an xrun.
- **Evidence:** Ardour's manual: the extra latency must be measured with a cable from output to input; a digital loop misses converter delay; USB latency on Linux changes after reconnects and xruns ([Latency and Latency-Compensation](https://manual.ardour.org/synchronization/latency-and-latency-compensation/)).
- **What they do:** loopback calibration: play a click, record it, store the offset.
- **Crucible:** `02-architecture.md` offsets recordings by the driver's reported latency. Not enough. M2: an `audio.calibrate` command, stored per machine, device, rate and buffer size in machine settings, never in the project. `crucible diag` shows "uncalibrated".
- **Severity:** High on the Lynx machine, where recording happens. Med elsewhere.

### A8. A late audio thread silently loses recorded input

- **Problem:** when the thread is late, captured audio is overwritten. The take is shorter than the time recorded and nothing says so.
- **Evidence:** a 13 h 15 min recording came out 0.62 s short with 54 xruns ([JUCE #1775](https://github.com/juce-framework/JUCE/issues/1775)).
- **What they do:** larger capture buffers; read the device's discontinuity flag and position counters (flag name `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY`, unverified).
- **Crucible:** M2: the recording path counts discontinuities into the overrun counter and marks the take at the sample where one happened.
- **Severity:** Med.

### A9. Audio thread priority (MMCSS)

- **Problem:** without the "Pro Audio" class the callback competes with everything. The registration must be reverted on the same thread.
- **Evidence:** Microsoft's exclusive-mode example registers "Pro Audio", and WASAPI uses it for its own transport threads below 10 ms ([Exclusive-Mode Streams](https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams)). JUCE registers it ([juce_WASAPI_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_WASAPI_windows.cpp)). cpal discarded the handle and never reverted ([PR #1406](https://github.com/RustAudio/cpal/pull/1406)).
- **What they do:** register at stream start, revert at stop on the same thread.
- **Crucible:** T13 is open. The evidence says do it now, cheaply, before the M1 measurements; record the with and without numbers.
- **Severity:** Med. High on the laptop.

### A10. Windows throttles minimized and hidden apps

- **Problem:** Windows 11 stops guaranteeing raised timer resolution for minimized or occluded processes, and drops their Quality of Service to Low (on battery: efficient cores, low clock). MMCSS threads get "Deadline" QoS; the disk thread and a plugin's own sample-loading threads do not.
- **Evidence:** [timeBeginPeriod remarks](https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod), [Quality of Service](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service). Whether ASIO or exclusive output counts as "audible" for QoS is (unverified).
- **What they do:** event waits instead of sleeps; opt out of power throttling while audio runs (which DAWs do this is unverified).
- **Crucible:** M2: no sleep-based timing in disk or MIDI threads. A seam function marks the process high QoS while the engine runs. Test on the laptop: battery, minimize during playback, watch the dropout counter.
- **Severity:** Med. High on the laptop on battery.

### A11. Power saving, USB suspend and driver latency

- **Problem:** dropouts caused by the system: USB selective suspend, low minimum processor state, drivers with long interrupt handlers (Wi-Fi, ACPI, graphics).
- **Evidence:** Native Instruments tells users to disable USB selective suspend, set processor state to 100% and remove vendor energy apps ([NI support](https://support.native-instruments.com/support/solutions/articles/69000879974-komplete-audio-mk2-audio-issues-windows-)). DPCs and ISRs delay even high-priority audio threads ([LatencyMon](https://www.resplendence.com/latencymon)).
- **What they do:** vendors publish checklists. Hosts cannot fix it.
- **Crucible:** M6: `crucible report` adds power plan, USB selective suspend, minimum processor state and on-battery as read-only facts. Test checklists point to LatencyMon. Never change system settings.
- **Severity:** High on the laptop. Med on the Behringer. Low on the Lynx (PCI, no USB).

### A12. Windows enhancements and shared-mode processing

- **Problem:** shared mode runs enhancement processing (Dolby, Realtek, voice clarity), resamples and limits. Toggling an enhancement reconfigures the stream.
- **Evidence:** capture cut out until raw stream mode was requested ([cpal #1173](https://github.com/RustAudio/cpal/issues/1173)); [JUCE #1629](https://github.com/juce-framework/JUCE/issues/1629).
- **What they do:** request raw mode for shared streams; use exclusive mode for real work.
- **Crucible:** already covered by `04-audio-quality.md` rule 6 and D-009. When the shared fallback is built, request raw mode and keep the label. Treat property changes as A5.
- **Severity:** Med on the laptop (Dolby Atmos, see `07-hardware-and-testing.md`).

### A13. Exclusive mode takes the device from everything else

- **Problem:** other apps and system sounds go silent. Users can disallow exclusive mode per device. Microsoft advises releasing the device when not in the foreground, which a DAW cannot do mid-playback.
- **Evidence:** [Exclusive-Mode Streams](https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams).
- **What they do:** a visible "release audio device" action; a clear message when disallowed.
- **Crucible:** `wasapi_exclusive.rs` already knows `AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED` (checked). M2: an `audio.release` command; the status bar says other apps are muted.
- **Severity:** Low.

---

## 2. Plugins

### P1. Scanning crashes, hangs and endless rescans

- **Problem:** plugins crash or hang when loaded for metadata. A crashed plugin gets rescanned on every start. Full rescans are slow.
- **Evidence:** Ardour runs a separate scanner program, keeps a per-architecture blocklist file, and redoes a cache entry only when the plugin is newer than it ([plugin_manager.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/plugin_manager.cc), [vst3_scan.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/vst3_scan.cc)). JUCE writes the plugin's name to a "dead man's pedal" file before loading and removes it after, so a crash leaves the name behind ([juce_PluginDirectoryScanner.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors/scanning/juce_PluginDirectoryScanner.cpp)). A scan on a background thread still crashes the app; Tracktion scans in a child process ([JUCE forum](https://forum.juce.com/t/scanner-crashing-on-unloadable-plugins-w-licenses-for-other-apps/19720)).
- **What they do:** out-of-process scan; a cache keyed on file identity and date; a blocklist with a reason; user actions to rescan or unblock one plugin.
- **Crucible:** isolation is covered by D-019. Missing (checked: no cache in `crucible-plugin-host`): a scan cache in machine settings keyed by bundle path, size, modified time and scanner version; a blocklist with the crash code or "timeout"; start-up scans only new or changed bundles; `plugin.rescan` and `plugin.unblock` commands. M1 or M3.
- **Severity:** High.

### P2. Antivirus, licensing and heavy instruments slow or stall scans

- **Problem:** Defender scans every plugin binary. Copy protection runs during load. Big sample instruments outlast short timeouts.
- **Evidence:** Steinberg's fix for slow scans is Defender exclusions for the VST folders ([Steinberg help](https://www.steinberg.help/r/slow-vst-plug-in-scanning-on-windows)). Melodyne crashed a scanner inside its licence check ([JUCE forum](https://forum.juce.com/t/scanner-crashing-on-unloadable-plugins-w-licenses-for-other-apps/19720)). Carla's 2 s discovery timeout was too short for Kontakt ([Carla PR #2051](https://github.com/falkTX/Carla/pull/2051)); bridge timeouts block loading ([Carla #1642](https://github.com/falkTX/Carla/issues/1642)).
- **What they do:** long timeouts; a cache so the cost is paid once; a log of slow plugins.
- **Crucible:** D-019's 60 s timeout is right. Add per-plugin scan time to the log and to `plugin.scan`. Whether Spectrasonics shows an authorisation window during a headless scan is (unverified); note it in the M0 checklist.
- **Severity:** Med.

### P3. VST3 threading: everything except processing runs on the UI thread

- **Problem:** VST3 expects every call except `process` and `setProcessing` on the UI thread: state get and set, `setupProcessing`, `setActive`, bus arrangements, latency queries, all controller calls. Calling them elsewhere breaks some plugins.
- **Evidence:** Steinberg: all plug-in calls run on the UI thread except `process` and `setProcessing` ([API documentation](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/API+Documentation/Index.html), [Audio Processor Call Sequence](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/Workflow+Diagrams/Audio+Processor+Call+Sequence.html)). In offline rendering, state calls may come from the processing thread ([FAQ: Processing](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Processing.html)). JUCE asserts the message thread for setup and state and warns "some plugins may break" ([juce_VST3PluginFormatImpl.h](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors_headless/format_types/juce_VST3PluginFormatImpl.h)).
- **What they do:** marshal every non-process call to the main thread.
- **Crucible:** gap. `02-architecture.md` gives the main thread editors and controller calls but leaves plugin creation, activation, state and latency queries unassigned, while the engine-control thread "builds new graph snapshots". Those calls must run on the main thread. The headless engine needs a main-thread job queue that its front end drains: Tauri's event loop in the app, the CLI's own loop for headless export. Decide before plugin work in M3.
- **Severity:** High.

### P4. Plugins call the host on the wrong thread, re-entrantly, or from `process`

- **Problem:** `restartComponent` arrives from the audio thread, the plugin's own thread, inside a state load, during editor creation, or from the constructor. Each `setActive` can trigger another, so a host that reacts at once recurses; a host that locks deadlocks.
- **Evidence:** Ardour commits: from `process` deadlocked ([423a921](https://github.com/Ardour/ardour/commit/423a9213011175f768936d819fc35ee9f9ee790b)); from `setComponentState` deadlocked ([3c57085](https://github.com/Ardour/ardour/commit/3c570857566aa4d7e122b661fcb7c58738d2ac32)); during editor creation recursed ([7227407](https://github.com/Ardour/ardour/commit/7227407aa6c42510af70cb5806ab5c1526d96b5c)); from the constructor crashed ([1dffa00](https://github.com/Ardour/ardour/commit/1dffa00f693c192bce47fe21a40553c7697b8179)). On 2026-11-18 Ardour stopped re-activating on latency change because "all rust programmed nih_plug" plugins call it from a custom thread and recurse ([b358f40](https://github.com/Ardour/ardour/commit/b358f40fa64ea81b55bc388184327a86fe4d739a)). JUCE asserts the UI thread and asks for bug reports against such plugins ([juce_VST3PluginFormatImpl.h](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors_headless/format_types/juce_VST3PluginFormatImpl.h)).
- **What they do:** record the request and return; act on it later on the main thread.
- **Crucible:** M3 rule: every host callback only sets atomic flags or pushes to a lock-free queue, then returns. No lock, no allocation, no direct reaction. Add fake test plugins that call `restartComponent` from `process` and from `setState`, next to the existing crashing fake.
- **Severity:** High.

### P5. Plugin latency and delay compensation

- **Problem:** latency is only valid after activation and can change at run time (preset, oversampling). The spec says deactivate and reactivate; practice now differs.
- **Evidence:** Ardour quoted the spec ([57c6608](https://github.com/Ardour/ardour/commit/57c660836f3ae2d188b78b4fdde8247e97eb7220)), then queried latency after every activation ([b8b232c](https://github.com/Ardour/ardour/commit/b8b232cfab5ae7f247ffa61c7a38238d41a9455a)), then stopped re-activating ([b358f40](https://github.com/Ardour/ardour/commit/b358f40fa64ea81b55bc388184327a86fe4d739a)). A program change that changes latency deadlocked ([c92c8c8](https://github.com/Ardour/ardour/commit/c92c8c8fa28e91b6efc3a3f4ac1666ca65fa62e5)). Steinberg: on `kLatencyChanged` the host reads `getLatencySamples` again ([FAQ: Processing](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Processing.html)).
- **What they do:** read latency after every activation; on change, re-read on the main thread and rebuild delays; do not force re-activation.
- **Crucible:** M3 (delay compensation). New delays arrive as a new snapshot. Show total latency per track. An instrument's own latency cannot be hidden from a live player; show it.
- **Severity:** Med. Omnisphere and Keyscape latencies are (unverified).

### P6. Saving and restoring plugin state

- **Problem:** order matters; the controller must receive the processor's state. Some plugins seek the stream with absolute offsets. State calls off the UI thread misbehave. A restore can trigger callbacks (P4). State depends on sound libraries outside the project.
- **Evidence:** Steinberg: the host passes state to the processor first, then the same data to the controller's `setComponentState`, then rescans parameters ([API documentation](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/API+Documentation/Index.html)). Ardour gives plugins a read-only stream because some "seek the stream using absolute offsets" ([eebe4e4](https://github.com/Ardour/ardour/commit/eebe4e467b45e051f6738e307da9eedbfb92491f)). Spectrasonics: missing patches usually mean an incomplete STEAM install ([knowledgebase](https://www.spectrasonics.net/support/knowledgebase/article/usage%3A+patches-sounds+seem+to+be+missing/89/20)).
- **What they do:** save both blobs; restore processor, then controller from the processor blob, then the controller's own blob; the stream starts at offset 0 and holds exactly the blob.
- **Crucible:** `02-architecture.md` already stores both blobs and keeps them for missing plugins. M3: fixed restore order on the main thread; a stream wrapper where offset 0 is the blob start; plugin version, size and hash beside each blob so a mismatch on another machine is visible.
- **Severity:** High. This is priority 5, "opens identically on three machines".

### P7. Edits inside a plugin window bypass undo, the journal and autosave

- **Problem:** thirty minutes of tweaking inside Omnisphere produce no commands. A crash loses them; undo cannot restore them. Some plugins change state without any parameter edit.
- **Evidence:** a u-he Zebra preset load changes controller state without `performEdit`, only a batch `restartComponent` ([Ardour 21d86b2](https://github.com/Ardour/ardour/commit/21d86b264a2a6fc4cb3aaef64790fad7851955fa)). D-022 lists "a plugin's own editor window" as not a command.
- **What they do:** Ardour re-syncs its automation controls on those notifications. Hosts use the plugin's "dirty" notification (`setDirty`, present in JUCE's host context) to mark the project changed.
- **Crucible:** gap. M3: `setDirty`, `performEdit` and `endEdit` mean "plugin state changed". While dirty, autosave captures plugin blobs on the main thread on a timer. One coarse `plugin.state` journal entry per edit gesture or per editor close (to decide). Snapshot undo covers the project model only; say so in the UI. Measure Omnisphere's `getState` size and time in M0 step B first.
- **Severity:** High.

### P8. Bus and channel layouts

- **Problem:** plugins crash on missing arrangements, refuse changes while active, or report odd bus counts.
- **Evidence:** "Some plug-ins will crash if you pass a nullptr to setBusArrangements!", and some publish parameters only after I/O is set ([juce_VST3PluginFormatImpl.h](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors_headless/format_types/juce_VST3PluginFormatImpl.h)). JUCE-based plugins must be inactive to change arrangements; some report zero channels on an event bus that accepts events ([vst3_plugin.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/vst3_plugin.cc)). Altiverb needs a valid arrangement before it reports anything ([vst3_scan.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/vst3_scan.cc)).
- **What they do:** always set an explicit arrangement, only while inactive.
- **Crucible:** stereo only (D-017) removes most of this. M3: explicit stereo arrangement before first activation, never null; an event bus reporting zero channels still gets events.
- **Severity:** Low.

### P9. Rate and block-size changes, variable and zero-length blocks

- **Problem:** the maximum block can only change through reactivation. Block length varies per call. Zero-length "flush" calls crash some plugins. Many plugins return an error from `setProcessing` yet work.
- **Evidence:** Steinberg: block size may vary but never exceeds the maximum, which changes only on reactivation; hosts may call `process` without buffers to flush parameters ([FAQ: Processing](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Processing.html)). JUCE avoids redundant `setActive`, "a heavy-duty call for some plugins" ([juce_VST3PluginFormatImpl.h](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors_headless/format_types/juce_VST3PluginFormatImpl.h)). `setProcessing` fails in many plugins, some returning `kNotImplemented` ([Steinberg forum](https://forums.steinberg.net/t/iaudioprocessor-setprocessing-fails-in-many-plugins/785558)).
- **What they do:** prepare once with the largest block the device may deliver; split larger blocks; tolerate `kNotImplemented`.
- **Crucible:** vst3-host already splits oversized blocks (`08-decisions.md` facts). M3: maximum block = the device's maximum, not its current size; no zero-length blocks; a rate or buffer change is a full deactivate, setup, activate on the main thread with transport stopped.
- **Severity:** Med.

### P10. VST3 receives MIDI controllers as parameters

- **Problem:** VST3 has no controller events. Sustain pedal, mod wheel and pitch bend reach the plugin only through its MIDI-mapping interface, as parameter changes. A host that sends only notes leaves the sustain pedal dead.
- **Evidence:** vst3-host builds a MIDI-mapping cache and routes pitch bend, aftertouch and controllers through it (`src/internal/plugin_impl.rs`, `midi_mapping.rs`, local). JUCE builds its mappings on the main thread and guards the audio thread's reads ([juce_VST3PluginFormatImpl.h](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors_headless/format_types/juce_VST3PluginFormatImpl.h)).
- **What they do:** build the controller-to-parameter table on the main thread; the audio thread reads a frozen copy.
- **Crucible:** add sustain pedal, mod wheel and pitch bend to the M0 step B checklist for Keyscape and Omnisphere. When the engine splits processor from editor (`02-architecture.md`), the table becomes part of the snapshot.
- **Severity:** High. Keyscape without a sustain pedal is unusable.

### P11. Denormals

- **Problem:** decaying tails fall into the subnormal range and spike CPU. Flush-to-zero is a per-thread setting.
- **Evidence:** "Modifying the MXCSR register only affects the current thread" ([Ardour 4abb907](https://github.com/Ardour/ardour/commit/4abb907a8278c8ab05cd82cb11dfe5a22abc28c5)); Ardour defaults to flush-to-zero plus denormals-are-zero ([8be724d](https://github.com/Ardour/ardour/commit/8be724db1b1812ab4623b2f065de1fad5076493e)). vst3-host has a guard (`src/internal/denormal.rs`, local).
- **What they do:** set both flags at the start of every processing thread.
- **Crucible:** M2: set once at audio-thread start and in any future worker. A test asserts they are set.
- **Severity:** Med.

### P12. NaN, infinity and runaway output

- **Problem:** a misbehaving plugin emits NaN or infinity. One NaN poisons every filter after it; meters show garbage.
- **Evidence:** vst3-host's meters treat non-finite values as silence (`src/audio.rs`, local). Which hosts mute a plugin on NaN is (unverified).
- **What they do:** check each plugin's output; zero a bad block and flag the plugin.
- **Crucible:** M3: after each plugin, a cheap non-finite check; a bad block becomes silence and latches a per-plugin flag in the UI. A fault guard, not a limiter, so `04-audio-quality.md` rule 2 holds.
- **Severity:** Med.

### P13. Editor windows: DPI, resizing and lifecycle

- **Problem:** editors open at the wrong size at 125 to 200% scaling or after moving between monitors. A plugin can change the whole host process's DPI mode. A second editor instance crashes some plugins.
- **Evidence:** Steinberg recommends the content-scale interface on Windows; the plugin resizes and asks the host to follow ([IPlugViewContentScaleSupport](https://steinbergmedia.github.io/vst3_doc/base/classSteinberg_1_1IPlugViewContentScaleSupport.html)). Live sent old physical sizes while dragging between displays ([JUCE forum](https://forum.juce.com/t/wrong-window-size-when-moving-vst3-between-displays/50712)). A JUCE release made plugins change the host's process-wide DPI mode ([JUCE forum](https://forum.juce.com/t/plugins-now-change-the-hosts-process-wide-dpi-awareness-since-8-0-13-commit-57c739c444/69084)). Cantabile runs plugins DPI-unaware so Windows scales them, around every call and on plugin threads ([JUCE forum](https://forum.juce.com/t/windows-per-monitor-dpi-scaling-with-juce-plugins-and-cantabile/34983)). JUCE avoids a second editor instance "because that crashes some plugins" ([juce_VST3PluginFormat.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_processors/format_types/juce_VST3PluginFormat.cpp)).
- **What they do:** pass the scale to plugins that support it; otherwise run that plugin's window DPI-unaware; one editor per instance.
- **Crucible:** partly covered by D-008 (floating windows). M1: decide the plugin window's DPI mode and call content scaling when supported; a per-machine "DPI-unaware" fallback flag per plugin; at most one editor per instance. Test on the laptop at its scaling, then dragged to an external monitor.
- **Severity:** Med. High on the laptop.

### P14. Keyboard focus with plugin windows

- **Problem:** the host's space bar steals keys from a plugin text field, or the plugin swallows the host's shortcuts.
- **Evidence:** REAPER takes a space typed into a plugin field; the fix is a "send all keys" option ([JUCE forum](https://forum.juce.com/t/reaper-eating-space-bar/62875)).
- **What they do:** forward unhandled keys to the host; a per-plugin "send all keys" override.
- **Crucible:** already covered by `02-architecture.md` "Keyboard focus" and M1. Add a per-plugin, per-machine "send all keys to plugin" flag. Test with Omnisphere's patch browser search.
- **Severity:** Med.

### P15. Crash isolation

- **Problem:** in-process plugins can take the app down; out-of-process hosting costs latency.
- **Evidence:** Ardour: at 64 samples and 48 kHz a cycle is 1.3 ms; a context switch for audio code costs 10 to 300 µs; sandboxing is workable at about 700 to 2,000 samples ([Why no plugin crash protection](https://ardour.org/plugins-in-process.html)). Bitwig offers per-plugin isolation and a "reload plug-in" action after a crash ([Bitwig user guide](https://bitwig.com/userguide/latest/vst_plug-in_handling_and_options)).
- **What they do:** Ardour stays in-process. Bitwig isolates, with a per-plugin override.
- **Crucible:** D-016 (in-process, autosave, journal) matches Ardour's reasoning for low latency. If isolation ever comes, make it a per-plugin opt-in for a known-bad plugin, not global.
- **Severity:** Med.

### P16. Missing plugin or sound library on another machine

- **Problem:** a project opens where a plugin or its library is absent or a different version.
- **Evidence:** Ardour keeps a missing plugin's saved state verbatim and passes audio through when it knows the I/O shape ([unknown_processor.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/unknown_processor.cc)).
- **What they do:** a placeholder that preserves state and writes it back unchanged.
- **Crucible:** already covered by `02-architecture.md` and M3. Add a test that a placeholder saves byte-identical blobs. A missing Spectrasonics library is invisible to the host; say "check the STEAM library" in the message.
- **Severity:** Med.

---

## 3. MIDI

### M1. Port identity is unstable

- **Problem:** two identical keyboards share a name; names are truncated; indexes change after a replug; names get paired with the wrong device.
- **Evidence:** JUCE paired WinMM names with the wrong identifiers when one device's details could not be read ([JUCE #1770](https://github.com/juce-framework/JUCE/issues/1770)). Under Windows MIDI Services, apps connected to the wrong one of several same-named devices, and virtual ports disappeared ([Microsoft known issues](https://devblogs.microsoft.com/windows-music-dev/windows-midi-services-rollout-known-issues-and-workarounds)).
- **What they do:** identify by the OS device path, not name or index.
- **Crucible:** already covered in spirit: `07-hardware-and-testing.md` records from all inputs and binds nothing to a name. If per-machine MIDI settings appear, key them by device identity.
- **Severity:** Low.

### M2. Hotplug

- **Problem:** no disconnect notice for inputs; closing an unplugged port blocks for seconds.
- **Evidence:** no way to detect an input disconnect on Windows ([midir #35](https://github.com/Boddlnagg/midir/issues/35)); events lost when a device sends before listening starts ([midir #81](https://github.com/Boddlnagg/midir/issues/81)); closing after unplug blocked the UI thread about 6.7 s ([JUCE #1756](https://github.com/juce-framework/JUCE/issues/1756)); WinMM teardown-order bugs ([JUCE #1728](https://github.com/juce-framework/JUCE/issues/1728)).
- **What they do:** poll the port list off the UI thread; reopen by identity; close on a background thread.
- **Crucible:** M3: the MIDI thread polls every one or two seconds, opens new ports (fits "all inputs") and closes vanished ones off the main thread.
- **Severity:** Med.

### M3. Timestamps and jitter

- **Problem:** events applied at the start of the next block jitter by up to one buffer. Library timestamps have no known relation to the audio clock.
- **Evidence:** midir's timestamp is relative to "some unspecified point in the past" ([midir #205](https://github.com/Boddlnagg/midir/issues/205)). Ardour ignores WinMM timestamps, stamps arrivals with its own microsecond clock and maps them into the cycle with a cycle timer ([winmmemidi_input_device.cc](https://github.com/Ardour/ardour/blob/master/libs/backends/portaudio/winmmemidi_input_device.cc), [portaudio_backend.cc](https://github.com/Ardour/ardour/blob/master/libs/backends/portaudio/portaudio_backend.cc)).
- **What they do:** stamp on arrival with one monotonic clock; the callback notes its own start time; the difference gives each event's sample offset. Variable jitter becomes one constant block of delay.
- **Crucible:** M3, for live playing and MIDI recording. The audible threshold at 256 samples is (unverified).
- **Severity:** Med.

### M4. Stuck notes and chasing

- **Problem:** notes hang after stop, loop wrap, clip deletion or an output change. Starting mid-song misses held notes and the sustain pedal state.
- **Evidence:** a tone keeps playing after stop ([LMMS #2529](https://github.com/LMMS/lmms/issues/2529)); stuck after changing output mid-note ([LMMS #8030](https://github.com/LMMS/lmms/issues/8030)); Ardour fixed chase stuck notes ([f6ebd38](https://github.com/Ardour/ardour/commit/f6ebd386405bfc19e4d188e94c754aa24a881774)) and a panic that never reached the plugin because the buffer was cleared first ([eebc902](https://github.com/Ardour/ardour/commit/eebc902564ea836f608f638c2856134dae46027b)); Ardour resolves notes crossing the loop end ([disk_reader.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/disk_reader.cc)). vst3-host tracks held notes and sends note-offs on panic (`src/internal/plugin_impl.rs`, local).
- **What they do:** a held-note tracker per track; note-offs and sustain-off on stop, loop wrap, locate and track edits; chase controller state at locate.
- **Crucible:** M3. Tracker per instrument track from day one; a `transport.panic` command; chase sustain, pitch bend and program at locate. Test with a fake plugin that logs events.
- **Severity:** High.

### M5. SysEx, MIDI clock and MIDI 2.0

- **Problem:** WinMM rejects long non-SysEx messages. MIDI 2.0 arrives through a new Windows stack.
- **Evidence:** "non-sysex message must not be longer than 3 bytes" ([midir #156](https://github.com/Boddlnagg/midir/issues/156)). Windows MIDI Services makes ports multi-client except old-style drivers ([known issues](https://devblogs.microsoft.com/windows-music-dev/windows-midi-services-rollout-known-issues-and-workarounds), [10-driver limit](https://devblogs.microsoft.com/windows-music-dev/windows-midi-services-and-the-10-midi-driver-limit)); a midir backend for it is proposed ([midir #210](https://github.com/Boddlnagg/midir/issues/210)).
- **Crucible:** v1 needs none of these. Drop non-channel messages on MIDI output. Revisit when midir ships the new backend.
- **Severity:** Low.

---

## 4. Operating-system integration

### O1. Unicode and long paths

- **Problem:** paths over 260 characters fail unless the app opts in; relative paths are always limited; non-ASCII names break naive code.
- **Evidence:** opt-in needs a registry value and a `longPathAware` manifest; module loading is not in Microsoft's list of lifted functions ([Maximum Path Length Limitation](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation)).
- **Crucible:** M2: test a project with accents in its name, inside a deep OneDrive folder, under a non-ASCII user name. Whether Tauri's manifest declares long-path awareness is (unverified).
- **Severity:** Low.

### O2. Antivirus and cloud-synced folders

- **Problem:** real-time scanning slows plugin scans and file access. Cloud placeholders download on first read and could stall disk streaming (unverified for OneDrive).
- **Evidence:** [Steinberg help](https://www.steinberg.help/r/slow-vst-plug-in-scanning-on-windows). Unexplained dropouts in projects copied between Windows machines ([Audacity #11838](https://github.com/audacity/audacity/issues/11838), cause not found).
- **Crucible:** M2: `crucible report` says whether the project sits under a OneDrive folder. Test checklists suggest Defender exclusions for the VST3 folders.
- **Severity:** Med. Dzyan's machines sync OneDrive.

### O3. Files in use cannot be renamed on Windows

- **Problem:** Windows refuses to rename or replace a file another handle holds. Indexers, antivirus and sync clients hold handles briefly.
- **Evidence:** Ardour closes all sources before renames, "mostly for windows which doesn't allow file renaming if the file is in use" ([session_state.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/session_state.cc)).
- **Crucible:** atomic save renames `project.json.tmp` over `project.json` (checked). M2: retry the rename a few times with a short backoff; keep the temp file on failure; coded error with the OS error number. Close audio files before renaming takes.
- **Severity:** Med.

### O4. Permissions and real-time limits on macOS and Linux

- **Problem:** macOS returns silence, not an error, without microphone permission. Linux needs real-time priority and locked-memory limits.
- **Evidence:** all input samples zero until the terminal had microphone access ([cpal #329](https://github.com/RustAudio/cpal/issues/329)); `rtprio 95` and unlimited `memlock` for the audio group ([JACK FAQ](https://jackaudio.org/faq/linux_rt_config.html)).
- **Crucible:** ports only. When they start, add to `03-platform-notes.md`: detect all-zero input on macOS and name the permission; check limits at start on Linux and report them.
- **Severity:** Low today.

### O5. Single instance and device ownership

- **Problem:** a second copy of the app cannot open the same ASIO driver or exclusive device.
- **Evidence:** "you can only have a finite number of ASIO devices open at the same time" ([juce_ASIO_windows.cpp](https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_devices/native/juce_ASIO_windows.cpp)); `AUDCLNT_E_DEVICE_IN_USE` ([IAudioClient::Initialize](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize)).
- **Crucible:** E2004 already explains a device in use. M1: a second launch brings the existing window forward.
- **Severity:** Low.

### O6. Crash recovery when a plugin crashes on load

- **Problem:** after a plugin crash, reopening the project crashes again.
- **Evidence:** D-021 already names the crashing module and writes a minidump. A safe start that loads plugins as placeholders is common practice (unverified per host).
- **Crucible:** M3: a `--safe` start flag; every plugin loads as a placeholder; the module named by D-021 is offered for the blocklist.
- **Severity:** Med.

---

## 5. Project and file portability

### F1. Media paths and relinking

- **Problem:** absolute paths break on another machine; moved media goes missing.
- **Evidence:** Ardour keeps media inside the session folder and manages renames within it ([session_state.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/session_state.cc)).
- **Crucible:** already covered by `02-architecture.md` "Project store" (copy on import, relative paths). M2: a missing file loads as a "missing" clip that keeps its reference; relink searches a chosen folder by name, length and rate.
- **Severity:** Med.

### F2. Imported files at another rate or depth

- **Problem:** files at other rates play at the wrong speed or get resampled silently.
- **Evidence:** Audacity refuses mismatched rates in one track and makes the user resample first ([PR #4739](https://github.com/audacity/audacity/pull/4739)).
- **Crucible:** already covered by `04-audio-quality.md` rule 1. M2: convert once at import into `audio/`, say so in the dialog, record the original rate on the clip.
- **Severity:** Med.

### F3. WAV variants and the 4 GB limit

- **Problem:** plain WAV stops at 4 GB. Readers reject files with a 0xFFFFFFFF length, which streaming writers produce. Extensible headers, float and 24-in-32 all appear in the wild.
- **Evidence:** hound documents a 4 GiB data limit (`src/read.rs`, local) and rejects 0xFFFFFFFF lengths ([hound #39](https://github.com/ruuda/hound/issues/39)); it does read extensible headers. Ardour writes RF64 that downgrades itself to plain WAV below 4 GB, with a BWF time stamp ([sndfilesource.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/sndfilesource.cc)). RF64 is [EBU Tech 3306](https://tech.ebu.ch/publications/tech3306).
- **What they do:** RF64 only when needed; tolerant readers.
- **Crucible:** T7 answered: hound has the 4 GiB limit. 24-bit stereo at 48 kHz reaches it after about 4.1 hours, 32-bit float after about 3.1. Keep WAV writing behind one small function so RF64 or file splitting can replace it (M2). Read 0xFFFFFFFF lengths from the file size.
- **Severity:** Low. Med for a long live recording.

### F4. A crash while recording

- **Problem:** a WAV header records the data length only when the file is finished. A crash mid-take leaves a header that says zero.
- **Evidence:** hound writes the final length on finalise and offers a flush that rewrites the header (`src/write.rs`, local). Ardour keeps "pending" capture state and asks on the next open whether to recover it ([session_state.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/session_state.cc)).
- **What they do:** keep the header current; recover takes on next open.
- **Crucible:** M2: flush the header every few seconds while recording; the journal records "take started, file, position"; recovery repairs the header from the file size and offers the take.
- **Severity:** High. A lost take is the worst failure on a recording machine.

### F5. Autosave, backups and disk space

- **Problem:** autosave must not block, must not corrupt, and must include plugin state. Recording must stop cleanly before the disk fills.
- **Evidence:** Ardour writes a temporary file and renames it, keeps a separate pending file, and tracks free space on every disk the session uses ([session_state.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/session_state.cc)).
- **Crucible:** atomic save and the journal are built; autosave is M2. Add rolling copies in `autosave/`, plugin blobs saved atomically too (P7), and remaining recording time from free space.
- **Severity:** Med.

### F6. Project format versions

- **Problem:** old projects must open in new builds; new projects must not half-open in old ones.
- **Crucible:** already covered: `crucible-project` refuses newer formats and damaged files (`STATUS.md`). Add the producing build's version to `project.json` and one tested upgrade step per format bump.
- **Severity:** Low.

---

## 6. Real-time engine

### R1. Locks, allocation and priority inversion

- **Problem:** a lock, allocation or system call in the callback waits on a lower-priority thread.
- **Evidence:** Ardour pre-allocates its process graph and hands the process threads a prepared node chain ([graph.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/graph.cc)). A plugin allocating inside its own `process` cannot be stopped by any host.
- **Crucible:** already covered by the real-time rule in `02-architecture.md`. Add per-plugin process time to the overrun report so the guilty plugin is named.
- **Severity:** covered.

### R2. Changing the graph while playing

- **Problem:** adding a plugin needs instantiate, setup and activate (main thread, slow for Omnisphere) before the audio thread may see it. Removing one must wait until the audio thread no longer uses it, then deactivate and release on the main thread. Wrong order crashes.
- **Evidence:** Ardour prepares the node chain off the process threads and swaps it in between cycles ([graph.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/graph.cc)).
- **Crucible:** the snapshot swap in `02-architecture.md` covers the mechanism. M3 lifecycle: main thread prepares; engine swaps; the audio thread acknowledges the snapshot's generation number; only then does the main thread tear down what the old snapshot held.
- **Severity:** High.

### R3. Disk streaming

- **Problem:** clips are read ahead of the playhead. A slow disk or a far locate underruns.
- **Evidence:** Ardour's disk reader refills in chunks, handles loop boundaries and counts underruns ([disk_reader.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/disk_reader.cc)). Omnisphere depends on the drive holding STEAM; an internal SSD is recommended ([Omnisphere guide](https://support.spectrasonics.net/manual/Omnisphere2/25/en/topic/optimizing-performance)).
- **Crucible:** `02-architecture.md` has a disk thread with ring buffers. M2: preload each clip's start at locate; count disk underruns separately from callback overruns.
- **Severity:** Med.

### R4. Offline render behaves differently

- **Problem:** offline, sample instruments may wait for disk, some plugins change quality, state calls may come from the render thread, and tails get cut at the end.
- **Evidence:** export uses offline processing mode, where state calls may run on the processing thread ([FAQ: Processing](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Processing.html)). Spectrasonics: "If the host provides two bounce/freeze modes (offline and realt-ime), try them both" ([Omnisphere guide](https://support.spectrasonics.net/manual/Omnisphere2/25/en/topic/performanc-optimzation-new-draft-images)). Ardour skips transport de-clicking when exporting ([disk_reader.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/disk_reader.cc)).
- **What they do:** offline by default with a real-time fallback; render the tail.
- **Crucible:** offline mode is set in the spike (W4010 when refused). M2 and M5: render the tail until silence or a cap; a short pre-roll before the start so instruments settle; offer real-time export when a plugin refuses offline mode.
- **Severity:** High for exports with Omnisphere.

### R5. Export determinism with plugins

- **Problem:** many synths randomise (drift, round robin), so two renders differ (unverified for Omnisphere and Keyscape).
- **Crucible:** the determinism test in `04-audio-quality.md` expects equal hashes. Scope it to projects without third-party plugins. For plugin projects compare length, peak and loudness within a tolerance.
- **Severity:** Low (test design).

### R6. One core per instrument

- **Problem:** one audio thread runs every track on one core.
- **Evidence:** Omnisphere uses one core per instance and "Distributing instruments across multiple cores is managed by the host" ([Omnisphere guide](https://support.spectrasonics.net/manual/Omnisphere2/25/en/topic/performanc-optimzation-new-draft-images)). Ardour runs its graph on worker threads ([graph.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/graph.cc)).
- **Crucible:** `02-architecture.md` has one audio thread. Fine for two or three instruments (unverified). Cheap insurance: render each track from its own buffers only, so a worker pool can run tracks in parallel later; workers then need MMCSS and denormal flags (A9, P11). Measure in M3.
- **Severity:** Med.

### R7. Meter accuracy

- **Problem:** a meter showing the last block's peak at 60 Hz misses peaks between frames.
- **Crucible:** true-peak and the clip latch are already in `04-audio-quality.md`. M5: the audio thread keeps the maximum since the last UI read and the UI swaps it out; the clip latch is set on the audio thread. General technique; no source opened.
- **Severity:** Low.

### R8. Clicks on start, stop and locate

- **Problem:** stopping or jumping mid-waveform clicks; a fade on start hides a first transient.
- **Evidence:** Ardour de-clicks stops and locates with short gain ramps ([disk_reader.cc](https://github.com/Ardour/ardour/blob/master/libs/ardour/disk_reader.cc)). Audacity on WASAPI loses a sharp transient at play start ([Audacity #5084](https://github.com/audacity/audacity/issues/5084)).
- **Crucible:** M2: a short fade on stop and locate for disk audio only, never in export; no fade on start, so a clip starting at the playhead is sample-exact.
- **Severity:** Low.

---

## 7. Editing and interface

### E1. Sample time versus musical time

- **Problem:** converting between samples and beats accumulates error; tempo edits then move things that should stay put.
- **Evidence:** Ardour 7.0 rebuilt its time model with two domains, audio and musical, to fix "inaccuracies converting between audio and musical time" ([Ardour 7.0](https://ardour.org/news/7.0.html)).
- **What they do:** every position carries its domain; conversion only through the tempo map.
- **Crucible:** decide in M2 before clips exist: audio clip positions in samples, MIDI notes and automation in ticks. Retrofitting this is what forced Ardour's rewrite.
- **Severity:** Med.

### E2. Tempo changes and audio clips

- **Problem:** after a tempo change, should an audio clip keep its time or its bar?
- **Crucible:** v1 has one tempo and one time signature per project (`crucible-project`, checked). Keep it that way. On a tempo change, audio clips keep their bar position and are not stretched; MIDI follows beats. Hosts offer per-track "timebase" choices for this (unverified detail).
- **Severity:** Low while the project has one tempo.

### E3. Fades at clip edges

- **Problem:** splits and trims away from a zero crossing click.
- **What they do:** short default fades at clip edges (common practice; default lengths unverified).
- **Crucible:** M4: a short, visible, editable default fade at each edited edge. Note for `04-audio-quality.md`: the null test must use unedited clips, since fades change samples.
- **Severity:** Med.

### E4. Undo granularity for drags and plugin gestures

- **Crucible:** already covered by D-022 (only a drag's final value is a command). Extend to plugin parameter gestures: begin edit to end edit is one undo step (P7). Snapping: grid with a modifier to bypass (no source opened).
- **Severity:** Low.

---

## Top 15 to design in now

| # | Entry | What to decide or build | When |
|---|---|---|---|
| 1 | P3 | Plugin lifecycle on the main thread; a main-thread job queue in the engine API, drained by Tauri or the CLI | Before M3 |
| 2 | P4 | Host callbacks only flag and return; fake plugins that call back from `process` and `setState` | M3 |
| 3 | P10 | Sustain pedal, mod wheel and pitch bend in the M0 step B checklist | M0 |
| 4 | M4 | Held-note tracker, panic command, sustain chase at locate | M3 |
| 5 | P7 | Plugin-window edits mark the project dirty; autosave captures blobs; one journal entry per gesture | M3 |
| 6 | P6 | Fixed restore order, offset-0 stream wrapper, version and hash beside each blob | M3 |
| 7 | F4 | Header flush during recording; take recovery from the journal | M2 |
| 8 | A5 | Engine "device lost" state; reopen only the same device | M2 |
| 9 | A7 | `audio.calibrate` loopback measurement stored per machine | M2 |
| 10 | P1 | Scan cache, blocklist with reason, rescan and unblock commands | M1 or M3 |
| 11 | R2 | Snapshot generation acknowledgement before teardown | M3 |
| 12 | R4 | Export renders tail and pre-roll; real-time fallback | M2, M5 |
| 13 | A3 | One ASIO owner, duplex only, reset requests reopen, callback watchdog | ASIO step |
| 14 | E1 | Positions stored with their time domain | M2 |
| 15 | A9, A10 | MMCSS now; event waits, not sleeps; high QoS while the engine runs | M1, M2 |

## Cheap insurance

Small now, expensive to retrofit.

- Store every position with its domain, samples or ticks (E1).
- A generation number on every engine snapshot (R2).
- A main-thread job queue in the engine API, even before plugins use it (P3).
- One helper that sets MMCSS and denormal flags, called by every audio-rate thread (A9, P11).
- Render each track from its own buffers only, so tracks can run in parallel later (R6).
- A held-note tracker in each instrument track from the first MIDI commit (M4).
- Plugin version, blob size and hash saved beside each state file (P6).
- A scan-cache key that includes the scanner's own version, so a scanner fix forces a rescan (P1).
- WAV writing behind one small function; header flush every few seconds (F3, F4).
- Device and MIDI identity by OS identifier, never by name or index (A5, M1).
- A slot in machine settings for calibrated latency per device, rate and buffer size (A7).
- A `--safe` start flag that loads every plugin as a placeholder (O6).
- `crucible report` gains power plan, USB selective suspend, battery state and "under OneDrive" (A11, O2).
- A non-finite check after each plugin (P12).
- Retry with backoff on the atomic-save rename (O3).
- Fake test plugins for callbacks from the wrong thread, `kNotImplemented` from `setProcessing`, NaN output and a slow `getState`. The crashing and hanging fakes already exist.

## Evidence that touches existing decisions

For Dzyan, per rule 5. Nothing in `08-decisions.md` was changed.

- `02-architecture.md` threads table: plugin creation, activation, state and latency calls are unassigned; VST3 requires the main thread for them (P3). The engine-control thread must not make them.
- `02-architecture.md` audio path: offsetting recordings by the driver's reported latency is not enough; add calibration (A7).
- `04-audio-quality.md` determinism test: valid only without third-party plugins (R5).
- D-022: plugin-window edits are outside the command set, so today they are outside undo, the journal and crash recovery (P7).
- D-016 (in-process plugins): supported by Ardour's numbers (P15). No change suggested.
- D-019 (60 s scan timeout): supported by Carla's experience (P2). The cache and blocklist are missing (P1).
- D-020: 24-in-32 handling and buffer alignment confirmed in code (A2, A4).
- T7: answered. hound has a 4 GiB limit (F3).
- T13: JUCE and Microsoft's own example register "Pro Audio"; do it (A9).
- One audio thread (`02-architecture.md`): fine to start, but Omnisphere expects the host to spread instances across cores (R6).

## Platform notes

- Windows-only entries: A2 to A4, A8 to A13, M1, M2, M5, O1 to O3, O5 and the DPI part of P13. Each fix lives in the seam crates per `03-platform-notes.md`; new Windows facts (QoS, power-plan reading, raw mode, ASIO reset handling) get ledger rows when written.
- macOS: no ASIO or exclusive-mode split, but input silently falls back to the default device on unplug (A5) and returns silence without microphone permission (O4). Aggregate devices correct drift in the OS (A6). Plugin windows are `NSView`s and the OS handles their scaling (P13).
- Linux: real-time priority and locked memory need system limits (O4). pipewire-alsa default buffers cause xruns (A2). USB latency changes after reconnects, so calibration matters more (A7).
- Platform-neutral: every plugin entry except the DPI part of P13, all of sections 6 and 7, and F1 to F6. VST3 threading rules (P3, P4) are the same on every system.
- Linux has no Spectrasonics builds; the P10 and M4 tests there need another instrument (`07-hardware-and-testing.md`).
