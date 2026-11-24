# 02. Architecture

Status of this document: first draft. Items tagged **(unverified)** are claims from memory or
from a single source. They are listed in `09-open-questions.md` section B and must be checked
when the code first touches them.

## The shape

```
+------------------------------------------------------------+
|  UI: TypeScript in the system web view                     |
|  DOM for chrome, canvas for timeline, piano roll, meters   |
+------------------------------+-----------------------------+
                               |  THE BRIDGE
                               |  commands down, state and events up
+------------------------------+-----------------------------+
|  App shell: Rust, Tauri 2. A thin adapter. No audio logic  |
+------------------------------+-----------------------------+
                               |  plain Rust calls
+------------------------------+-----------------------------+
|  Engine: Rust library. Headless. No Tauri, no OS calls     |
|  transport, tracks, mixing, rendering, undo, project model |
+-----+----------------+----------------+--------------------+
      |                |                |
+-----+------+  +------+-----+  +-------+--------+  +---------------+
| SEAM 1     |  | SEAM 1     |  | Plugin host    |  | Project store |
| audio I/O  |  | MIDI I/O   |  | VST3           |  | files on disk |
|            |  |            |  | SEAM 2 inside: |  |               |
|            |  |            |  | editor windows |  |               |
+------------+  +------------+  +----------------+  +---------------+
```

Two ideas carry the whole design.

**The engine is headless.** It is a library driven by commands and it emits state and events.
It knows nothing about Tauri or the web view. A command-line harness can drive it. This makes
the interface replaceable (egui is the recorded fallback) and makes the engine testable by a
machine with no screen and no ears.
Decided in D-022: every user action is a *named* command in a table, so the GUI, an in-app
console and the `crucible` CLI are three front ends over the same set. Built: `crucible-engine/src/command.rs`.

**The operating system is behind two seams.** Seam 1 is audio and MIDI devices. Seam 2 is
plugin editor windows. Everything else is portable Rust. See `03-platform-notes.md`.

## Workspace layout

```
crucible/
  Cargo.toml                    workspace root
  crates/
    crucible-engine/            headless core. Portable. No OS calls.
    crucible-dsp/               built-in effects (Reverb, IR Reverb, Delay, Echo) and their
                                parameter tables. Plain arithmetic. Portable.
    crucible-audio-io/          SEAM 1. AudioBackend trait plus one module per driver API.
    crucible-midi-io/           SEAM 1. MidiBackend trait.
    crucible-plugin-host/       VST3 hosting and scanning. SEAM 2 lives in an editor_window module.
    crucible-project/           file format, save, load, autosave, crash journal. Portable.
    crucible-cli/               the `crucible` binary: command-line front end over every command,
                                plus diagnostics (devices, plugins, report). Shell, scripts, JSON.
    crucible-diagnostics/       error codes, session logs, crash capture, machine facts. Every crate
                                reports through it. Native crash capture is its only OS code.
                                See 11-error-codes.md.
  app/                          the window (crucible-app.exe)
    src-tauri/                  Tauri 2 shell. One `exec` command into the engine. Adapter only.
    src/                        TypeScript UI, no framework. Canvas for timeline, piano roll, meters.
                                demo.ts holds every stand-in for engine features not built yet
                                (record, loop, loudness, plugins); transport.ts is the real transport.
  spikes/
    m0-omnisphere/              throwaway test program. Deleted or archived after M0.
  docs/
```

## Threads

Getting this wrong is the classic way audio software fails, so it is fixed up front.

| Thread | Owns | Rules |
|---|---|---|
| **Main** | The OS event loop, which Tauri owns (the CLI runs its own for headless export). Plugin editor windows. **Every VST3 call except `process` and `setProcessing`:** creation, `setupProcessing`, `setActive`, bus layout, state get and set, latency queries, all controller calls. | Never blocks on the audio thread. The engine posts plugin work to a main-thread job queue that the front end drains (`12-known-daw-problems.md` P3). |
| **Audio** | The driver callback. Runs the render graph. Calls each plugin's `process`. | The real-time rule, below. |
| **Engine control** | Applies commands, edits the project model, builds new graph snapshots. | Hands snapshots to the audio thread by atomic swap. Never calls into a plugin itself: it asks the main thread. |
| **Feeder** (built 2026-11-23, D-034) | Playback of everything that is not being played live: runs `audio::render`, clip files and effects included, about 40 ms ahead of the listener into a lock-free queue. | Audio priority, but not the callback: it may read disks and take bursts. The callback only copies from its queue. Tracks played live (M3) will be rendered in the callback instead. |
| **Disk** | Record writer, clip stream reader, waveform peak builder. | Talk to the audio thread only through ring buffers. |
| **Scanner process** | A separate executable that loads plugins to read their metadata. | If a plugin crashes during a scan, only the scanner dies. |

Host callbacks from plugins (`restartComponent`, `setDirty`, parameter edits) arrive on any thread,
including the audio thread and in the middle of a state load. They only set a flag or push to a
lock-free queue and return; the main thread acts on them later (`12-known-daw-problems.md` P4).
Corrected 2026-11-21 after that research; the earlier table left plugin lifecycle calls unassigned.

### The real-time rule

Inside the audio callback there is no memory allocation, no mutex, no logging, no file or
network access, no waiting on another thread, and no call whose duration is unbounded.

How it is enforced:

- Commands reach the audio thread through a lock-free single-producer single-consumer queue.
- The audio thread reports meters, playhead position and overruns through another such queue or through atomics.
- The render graph is an immutable snapshot. Editing builds a new snapshot off-thread and swaps a pointer.
- Old snapshots are freed on another thread, never on the audio thread.
- Debug builds wrap the callback in an allocation detector so a stray allocation fails a test.
- An overrun counter is always visible in the UI. An overrun is a bug, not a statistic.

One public spike (see `10-references.md`, the Tauri DAW blog) holds the plugin in a mutex and
uses a non-blocking try-lock in the callback. That is acceptable for the M0 throwaway program
only. The engine does not do it.

## The bridge

The web view and the Rust side are separate worlds that exchange messages. This is the main
engineering cost of choosing Tauri, so what crosses and how often is fixed here.

| Data | Direction | Rate | Form |
|---|---|---|---|
| User actions: play, record, move clip, change gain | UI to engine | On demand | Typed command, JSON |
| Project state: tracks, clips, plugin lists | Engine to UI | After every command | Whole state once, then only changed clips, by revision number (D-032) |
| Meters | Engine to UI | At most 60 per second | Small binary payload, all meters in one message |
| Playhead | Engine to UI | About 30 per second | Position plus timestamp. The UI interpolates every frame. |
| Waveform peaks | Engine to UI | Once per file per session | `audio.peaks`: loudest sample per 256 frames, one byte each, base64. Computed at import, cached in `cache/peaks/`. One resolution is enough so far (0.4 s and 150 KB for 10 minutes) |
| Audio samples | Never crosses | | Audio stays in the engine |

Rules:

- The engine is the single source of truth. The UI holds a read-only mirror.
- Commands go one at a time, in the order issued (`app/src/engine.ts`). The window runs them off
  its main thread, so a slow command never freezes painting.
- Undo and redo live in the engine, as a command history over the project model.
- Command and event types are defined once in Rust and the TypeScript types are generated from
  them, so the two sides cannot drift. **(unverified: the exact tooling)**
- The timeline, piano roll and meters are drawn on a canvas. They are never built from page
  elements. A few thousand clips as page elements will crawl.
- High-rate updates bypass the UI framework and write straight to the canvas.

## Audio path

- **Processing format:** 32-bit float, stereo, at the project sample rate. Proposed, see decision D-013.
- **One render function.** Live playback and offline export run the same graph code. Export is
  that function called in a loop writing to a file. This is what makes export results trustworthy.
  Built for audio clips (2026-11-22, `crucible-engine/src/audio.rs`): `render` mixes one block
  and gets each clip's samples through a callback. Export's callback reads the files, streaming;
  playback's will read buffers a disk thread keeps full, because the audio thread may not touch
  files. Gain, balance pan (D-030), mute, solo and master are applied; MIDI is silent until M3.
- **Sample rate:** each project has one rate. The device is opened at that rate. If a device
  cannot do it, the engine converts at the device boundary only and shows a visible warning.
- **Buffer size:** a per-machine setting, never stored in the project.
- **Latency compensation:** plugins report their delay. The engine delays the other tracks to
  match. Recording is offset by the driver's reported input and output latency.

Graph for version 1, deliberately fixed and simple:

```
audio clips  ----+
                 +--> track inserts --> gain, pan, mute, solo --+
instrument  -----+                                              +--> master inserts --> output
  ^                                                             |
  MIDI clips and live MIDI                    other tracks -----+
```

No sends or buses in version 1.

## Plugin hosting

- **Format:** VST3 only for version 1.
- **First candidate library:** `vst3-host`, an MIT Rust crate. Its own docs say third-party
  plugins are exercised mainly on macOS, that Windows is tested only with a bundled test synth,
  and that its crash-isolation helper has not been run on Windows. So M0 is the real test.
- **Fallback:** the lower-level `vst3` bindings crate with our own host layer, written against
  Steinberg's SDK. The SDK has been MIT-licensed since version 3.8 and includes hosting examples.
- **Plugin identity:** a project stores each plugin's class ID, name, vendor and version. Never
  a file path. Each machine resolves the ID through its own scan cache.
- **Plugin state:** both state blobs, component and controller, are saved as binary files in the
  project folder. A missing plugin loads as a placeholder that keeps its blobs untouched.
- **Editor windows:** always floating top-level native windows owned by the main window. Never
  embedded in the web view. Created and serviced on the main thread.
- **Offline render:** plugins are told the render is offline so sample-based instruments load
  everything before producing sound.
- **Scanning:** each bundle is probed by a child copy of our own executable with a timeout. See D-019.
- **Processor and editor split:** vst3-host's convenient playback path locks the plugin inside
  the audio callback, and its lock-free runner takes the plugin away from the editor. The engine
  must keep the processor side on the audio thread and the editor side on the main thread
  separately. Design this before M2. The M0 spike uses the locking path and accepts the risk.
- **Crash safety, version 1:** plugins run in-process. A plugin crash can take the app down, so
  the defence is frequent autosave plus the crash journal in the project store. Out-of-process
  hosting is revisited after M3. It costs latency and a great deal of complexity.

Known facts about the reference plugins:

- Omnisphere and Keyscape load multi-gigabyte sample libraries on their own threads.
- Their saved state refers to sounds in the Spectrasonics library folder. The same library must
  be installed on every machine, or recall will fail no matter what the DAW does.
- Authorisation is handled by Spectrasonics inside the plugin. The host is not involved.

## Project store

A project is a folder that can be copied as one unit.

```
MySong/
  project.json          human-readable, has a schema version number
  audio/                recorded and imported audio. WAV. Referenced by relative path.
  plugin-state/         one binary file per plugin instance
  cache/peaks/          regenerable waveform outlines, one per audio file. Safe to delete.
  autosave/             rolling copies of project.json
  journal.log           commands since last save, for crash recovery
```

- Saving is atomic: write a temporary file, flush it to disk, then rename it over the old one.
- Nothing machine-specific goes in here. No device names, buffer sizes, plugin paths or window positions.
- Machine settings live in the per-user application data folder.
- Imported audio is copied into `audio/` by default so the folder stays self-contained. WAV is
  copied byte for byte; MP3 and FLAC are decoded once into 32-bit float WAV (D-031).

## Candidate crates

All are candidates. Before adopting any, check its licence, last release date and Windows support.
Record the outcome in `08-decisions.md`.

| Need | Candidate | Notes |
|---|---|---|
| Audio devices | `cpal` 0.18 | ASIO support is behind a build feature that needs the ASIO SDK and LLVM. Its WASAPI path is shared mode only (verified 2026-11-20). In use for device listing. |
| WASAPI exclusive mode | `wasapi` | Needed, since cpal cannot do exclusive mode. Lives behind our own trait. **(unverified: licence and maintenance)** |
| MIDI devices | `midir` | Cross-platform. |
| VST3 hosting | `vst3-host`, then `vst3` | See above. |
| Lock-free queues | `rtrb` | For audio-thread messaging. |
| Allocation detector | `assert_no_alloc` | Debug builds only. |
| WAV read and write | `hound` | Already used in `..\music-editor`. **(unverified: files larger than 4 GB)** |
| Decoding other formats | `symphonia` | For import. |
| FLAC encoding | `flacenc` or a libFLAC binding | **(unverified: which is better maintained)** |
| Sample-rate conversion | `rubato` | `..\music-editor` uses soxr instead. Compare quality before choosing. |
| Loudness metering | `ebur128` | Already used in `..\music-editor`. |
| Built-in effects | reuse `..\music-editor\src\dsp` | Dzyan's own MIT code: EQ, dynamics, reverb, delay. |
| Type generation for the bridge | `tauri-specta` or `ts-rs` | **(unverified)** |

Front end, proposed in decision D-015: TypeScript, React, Vite, Tailwind, Radix primitives for
menus and dialogs, a small state store, and the Canvas 2D API. WebGL only if the M1 stress test demands it.

## Known hard problems

These are where the time will go. None is a surprise.

| Problem | Why it is hard | Plan |
|---|---|---|
| Plugin windows and the main thread | Windows must be created and pumped on the main thread, which Tauri owns. A public write-up reports crashes on every attempt until this was done right. | Use Tauri's run-on-main-thread hook. Service plugin windows from the main event loop. Prove it in M0 and again in M1. |
| Creating windows from a command handler | Tauri's docs say creating a window from a synchronous command deadlocks on Windows. | Create windows only from async commands or from the main-thread hook. |
| Keyboard focus | When a plugin window has focus, the space bar should still start and stop playback. | Forward keys the plugin does not handle. Test with Omnisphere's patch browser, which wants typing. |
| Plugins that deviate from the spec | Closed binaries, no source. | Test against the reference plugins early and often. Keep a compatibility table in `07-hardware-and-testing.md`. |
| Glitch-free audio under load | Sample-streaming instruments spike CPU and disk. | Overrun counter, per-machine buffer size, and a "freeze track to audio" feature later. |
| Canvas timeline speed | Depends entirely on how it is coded. | M1 stress prototype with thousands of clips before any real UI is built. |
| Three audio drivers | ASIO, exclusive mode and three different devices. | One trait, one test checklist per machine. |
| Machine-untestable behaviour | A model cannot hear or see the plugin. | See the testing strategy in `07-hardware-and-testing.md`. |

## Platform notes

- Everything in "Engine", "The bridge", "Project store" and the front end is platform-neutral by design.
- Audio and MIDI device code is Windows-specific today and sits in the two seam crates.
- VST3 hosting is mostly portable. Three parts are not: locating plugin folders, loading the
  plugin module, and the editor window. All three sit in `crucible-plugin-host` behind small functions.
- Path handling: always use relative paths with forward slashes inside `project.json`. Never
  store a drive letter. File names must be valid on all three systems.
- The full ledger is `03-platform-notes.md`.
