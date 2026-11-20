//! THROWAWAY spike for milestone M0 step B. Not part of the product.
//!
//! Answers one question: can a Rust program host Omnisphere and Keyscape on Windows,
//! show their editors, play them live from MIDI, keep their state, and render offline?
//!
//! Known shortcuts, acceptable only because this is a spike (docs/02-architecture.md):
//! - Audio goes out through WASAPI. By default in *shared* mode to the Windows default
//!   output. With `--exclusive`, in exclusive mode through `daw-audio-io` for low latency.
//!   No ASIO yet (open question Q4).
//! - vst3-host's playback path locks the plugin with a mutex inside the audio callback.
//!   If the editor holds that lock for a long time (e.g. while loading a big patch), audio
//!   can drop out. That is a host-design artefact to note, not a verdict on the plugin.
//! - The plugin runs in this process. A plugin crash ends the program; the crash handler in
//!   `daw-diagnostics` records which module crashed and writes a dump next to the log.

mod exclusive_backend;
mod pump;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use daw_diagnostics::{self as diag, Code, CodeExt, DawError};
use exclusive_backend::{ExclusiveBackend, StreamSlot};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use vst3_host::window::PluginWindow;
use vst3_host::{AudioBuffers, AudioConfig, MidiChannel, MidiEvent, ProcessMode, Vst3Host};

const USAGE: &str = "\
m0-play: M0 step B test program (throwaway)

USAGE:
  m0-play list
      Show VST3 bundles, MIDI inputs and the default audio output.

  m0-play play <plugin> [--exclusive] [--device NAME] [--rate HZ] [--block N] [--state FILE]
      Load the plugin, open its editor, play it from every MIDI input.
      <plugin> is a .vst3 path or part of a bundle name, e.g. omnisphere.
      --exclusive  take the output device over (WASAPI exclusive mode): low latency.
                   Without it, audio goes through Windows shared mode (higher latency).
      --device     exclusive mode only: part of the output's name. Default: Windows default output.
      --block      buffer size in frames. Exclusive default: the driver's minimum.
      Console commands while running (type, then Enter):
        c  play a test chord for 1.5 s     p  all notes off (panic)
        s  save plugin state now           t  show audio timing so far
        m  write a marker in the log (e.g. 'm heard a click')
        q  save state and quit
      Closing the plugin window also saves state and quits.

  m0-play render <plugin> [--seconds S] [--out FILE.wav] [--state FILE]
                          [--rate HZ] [--block N] [--preroll S]
      Render a held C-major chord offline to a 32-bit float WAV, and report its peak.

Defaults: --block 256 (play, shared) / driver minimum (play, exclusive) / 512 (render),
          --rate 48000 (exclusive and render), --seconds 10, --preroll 3,
          --state m0-state-<plugin>.bin, --out m0-render-<plugin>.wav

Every run writes a log (path printed at start and end). Errors print a code such as E2004
and what to try; `crucible codes` lists them all. `crucible report` gathers the logs into one file.
";

/// How often the main loop checks audio timing and how often it writes a heartbeat line.
const TIMING_CHECK: Duration = Duration::from_secs(1);
const HEARTBEAT: Duration = Duration::from_secs(60);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    if matches!(command, "list" | "play" | "render") {
        let label = match args.get(1) {
            Some(p) if command != "list" => format!("{command}-{}", short_label(p)),
            _ => command.to_string(),
        };
        if let Some(p) = diag::start("m0-play", &label) {
            println!("Log file: {}\n", p.display());
        }
    }
    let result = run(&args);
    if result.is_ok()
        && command != "list"
        && let Some(p) = diag::log_path()
    {
        println!("\nLog file: {}", p.display());
    }
    std::process::exit(diag::finish(result));
}

/// A plugin argument shortened for a log file name: "omnisphere", not a whole path.
fn short_label(arg: &str) -> String {
    Path::new(arg).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| arg.to_string())
}

fn run(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => list(),
        Some("play") => play(&Opts::parse(&args[1..])?),
        Some("render") => render(&Opts::parse(&args[1..])?),
        Some(other) if other != "help" && other != "--help" && other != "-h" => {
            print!("{USAGE}");
            Err(usage(format!("unknown command '{other}'")))
        }
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    }
}

fn usage(msg: impl Into<String>) -> anyhow::Error {
    DawError::new(Code::Usage, msg).into()
}

struct Opts {
    plugin: String,
    block: Option<usize>,
    state: Option<PathBuf>,
    seconds: f64,
    out: Option<PathBuf>,
    rate: Option<u32>,
    preroll: f64,
    exclusive: bool,
    device: Option<String>,
}

impl Opts {
    fn parse(args: &[String]) -> Result<Self> {
        let mut it = args.iter();
        let plugin = it.next().ok_or_else(|| usage("missing <plugin>. Run m0-play with no arguments for help."))?.clone();
        let mut o = Opts { plugin, block: None, state: None, seconds: 10.0, out: None, rate: None, preroll: 3.0, exclusive: false, device: None };
        while let Some(a) = it.next() {
            let mut val = || it.next().ok_or_else(|| usage(format!("{a} needs a value")));
            let bad = |what: &str| usage(format!("{a} needs {what}"));
            match a.as_str() {
                "--block" => o.block = Some(val()?.parse().map_err(|_| bad("a whole number of frames"))?),
                "--state" => o.state = Some(PathBuf::from(val()?)),
                "--seconds" => o.seconds = val()?.parse().map_err(|_| bad("a number of seconds"))?,
                "--out" => o.out = Some(PathBuf::from(val()?)),
                "--rate" => o.rate = Some(val()?.parse().map_err(|_| bad("a sample rate in Hz"))?),
                "--preroll" => o.preroll = val()?.parse().map_err(|_| bad("a number of seconds"))?,
                "--exclusive" => o.exclusive = true,
                "--device" => o.device = Some(val()?.clone()),
                other => return Err(usage(format!("unknown option '{other}'"))),
            }
        }
        diag::info(format!(
            "options: plugin '{}', exclusive {}, device {:?}, rate {:?}, block {:?}, state {:?}, seconds {}, preroll {}, out {:?}",
            o.plugin, o.exclusive, o.device, o.rate, o.block, o.state, o.seconds, o.preroll, o.out
        ));
        Ok(o)
    }
}

// ---------------------------------------------------------------- list

fn list() -> Result<()> {
    let folders = daw_plugin_host::plugin_paths::standard_vst3_folders();
    let (reports, bundles) = daw_plugin_host::find_bundles(&folders);
    diag::note("VST3 folders:");
    for r in &reports {
        diag::note(format!("  {}  ({})", r.folder.display(), if r.exists { "present" } else { "not present" }));
    }
    diag::note(format!("VST3 bundles ({}):", bundles.len()));
    for b in &bundles {
        diag::note(format!("  {}", b.display()));
    }

    diag::note("MIDI inputs:");
    match vst3_host::list_midi_input_ports() {
        Ok(ports) if ports.is_empty() => diag::note(format!("  none ({})", Code::NoMidiInputs.id())),
        Ok(ports) => ports.iter().for_each(|p| diag::note(format!("  {}", p.name()))),
        Err(e) => diag::warn(Code::MidiUnavailable, e.to_string()),
    }

    diag::note("Default audio output (WASAPI shared mode):");
    match default_output() {
        Some((name, rate)) => diag::note(format!("  {name} at {rate} Hz")),
        None => diag::note(format!("  none ({})", Code::NoOutputDevice.id())),
    }
    Ok(())
}

fn default_output() -> Option<(String, u32)> {
    let device = cpal::default_host().default_output_device()?;
    let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
    let rate = device.default_output_config().ok()?.sample_rate();
    Some((name, rate))
}

fn no_output() -> DawError {
    DawError::new(Code::NoOutputDevice, "Windows reports no default output device")
}

/// Resolve `<plugin>`: an existing path, or a unique case-insensitive match on bundle names.
fn resolve_plugin(arg: &str) -> Result<PathBuf> {
    let p = Path::new(arg);
    if p.exists() {
        return Ok(p.to_path_buf());
    }
    let folders = daw_plugin_host::plugin_paths::standard_vst3_folders();
    let (_, bundles) = daw_plugin_host::find_bundles(&folders);
    let needle = arg.to_lowercase();
    let stem = |b: &PathBuf| b.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let exact: Vec<&PathBuf> = bundles.iter().filter(|b| stem(b) == needle).collect();
    if exact.len() == 1 {
        return Ok(exact[0].clone());
    }
    let partial: Vec<&PathBuf> = bundles.iter().filter(|b| stem(b).contains(&needle)).collect();
    let searched: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
    match partial.len() {
        1 => Ok(partial[0].clone()),
        0 => Err(DawError::new(
            Code::PluginNotFound,
            format!("no VST3 bundle matches '{arg}'. Searched: {}. {} bundles found there.", searched.join("; "), bundles.len()),
        )
        .into()),
        _ => {
            let names: Vec<String> = partial.iter().map(|b| b.display().to_string()).collect();
            Err(DawError::new(Code::PluginAmbiguous, format!("'{arg}' matches: {}", names.join("; "))).into())
        }
    }
}

fn plugin_slug(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect())
        .unwrap_or_else(|| "plugin".into())
}

/// Load a plugin with vst3-host's step-by-step loading messages recorded in the log.
/// If the plugin crashes while loading, the last recorded step shows how far it got.
fn load_plugin(host: &mut Vst3Host, path: &Path) -> Result<vst3_host::Plugin> {
    diag::info(format!("loading {} (vst3-host loading steps follow)", path.display()));
    diag::set_verbose(true);
    let result = host.load_plugin(path);
    diag::set_verbose(false);
    result.code(Code::PluginLoadFailed).with_context(|| format!("loading {}", path.display()))
}

fn restore_state(plugin: &mut vst3_host::Plugin, state_file: &Path) -> Result<()> {
    if !state_file.exists() {
        diag::note(format!("no saved state at {}; the plugin starts with its default sound", state_file.display()));
        return Ok(());
    }
    let bytes = std::fs::read(state_file).code(Code::FileRead).with_context(|| format!("reading {}", state_file.display()))?;
    match plugin.load_state(&bytes) {
        Ok(()) => diag::note(format!("restored state from {} ({} bytes)", state_file.display(), bytes.len())),
        Err(e) => diag::warn(Code::StateRestoreFailed, format!("{} ({} bytes): {e}", state_file.display(), bytes.len())),
    }
    Ok(())
}

// ---------------------------------------------------------------- play

fn play(o: &Opts) -> Result<()> {
    let path = resolve_plugin(&o.plugin)?;
    let slug = plugin_slug(&path);
    let state_file = o.state.clone().unwrap_or_else(|| PathBuf::from(format!("m0-state-{slug}.bin")));
    let block = o.block.unwrap_or(if o.exclusive { 512 } else { 256 });
    let rate = if o.exclusive { o.rate.unwrap_or(48_000) } else { default_output().ok_or_else(no_output)?.1 };

    diag::note(format!("mode: play | plugin: {}", path.display()));
    if o.exclusive {
        diag::note(format!(
            "output: WASAPI exclusive | device: {} | {rate} Hz | buffer: {}",
            o.device.as_deref().unwrap_or("Windows default"),
            o.block.map(|b| format!("{b} frames requested")).unwrap_or_else(|| "driver minimum".into()),
        ));
    } else {
        let (device, _) = default_output().ok_or_else(no_output)?;
        diag::note(format!("output: WASAPI shared | {device} | {rate} Hz | requested block {block} frames"));
        if o.device.is_some() {
            diag::note("note: --device only applies with --exclusive; using the Windows default output");
        }
    }

    let t = Instant::now();
    let mut host = Vst3Host::builder().sample_rate(rate as f64).block_size(block).build().code(Code::HostInitFailed)?;
    let mut plugin = load_plugin(&mut host, &path)?;
    let info = plugin.info();
    diag::note(format!("loaded in {:.1}s: {} by {} v{}", t.elapsed().as_secs_f64(), info.name, info.vendor, info.version));
    restore_state(&mut plugin, &state_file)?;

    let config = AudioConfig { sample_rate: rate as f64, block_size: block, input_channels: 0, output_channels: 2, ..Default::default() };
    let slot: StreamSlot = StreamSlot::default();
    let open_error = Arc::new(Mutex::new(None));
    let handle = if o.exclusive {
        let backend = ExclusiveBackend { device: o.device.clone(), period_frames: o.block.map(|b| b as u32), slot: slot.clone(), open_error: open_error.clone() };
        vst3_host::play_with_backend(&backend, plugin, config)
    } else {
        let backend = vst3_host::backends::CpalBackend::new().code(Code::StreamStartFailed)?;
        vst3_host::play_with_backend(&backend, plugin, config)
    };
    let handle = match handle {
        Ok(h) => h,
        Err(e) => {
            // Prefer the coded reason our exclusive backend recorded (e.g. E2004 device in use).
            let coded = open_error.lock().unwrap_or_else(|p| p.into_inner()).take();
            let err = coded.unwrap_or_else(|| DawError::new(Code::StreamStartFailed, format!("{e:#}")));
            return Err(anyhow::Error::from(err).context("starting audio"));
        }
    };
    diag::note("audio started");
    if let Some(out) = slot.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        let i = out.info();
        diag::note(format!(
            "exclusive output: {} | {} | {} ch | buffer {} frames = {:.2} ms",
            i.device, i.format, i.device_channels, i.buffer_frames, i.buffer_ms
        ));
    }

    // MIDI: connect every input. The connections must stay alive for the whole session.
    let mut midi_connections = Vec::new();
    match vst3_host::list_midi_input_ports() {
        Ok(ports) => {
            for port in ports {
                match vst3_host::bind_to_handle(&port, &handle) {
                    Ok(c) => {
                        diag::note(format!("MIDI input connected: {}", port.name()));
                        midi_connections.push(c);
                    }
                    Err(e) => diag::warn(Code::MidiPortBusy, format!("{}: {e}", port.name())),
                }
            }
        }
        Err(e) => diag::warn(Code::MidiUnavailable, e.to_string()),
    }
    if midi_connections.is_empty() {
        diag::warn(Code::NoMidiInputs, "no MIDI input is connected. Use the 'c' console command to test sound.");
    }

    // Editor window: created and serviced on this (the main) thread.
    let mut window = PluginWindow::new(handle.plugin());
    match window.open() {
        Ok(()) => diag::note("editor window opened"),
        Err(e) => diag::error(Code::EditorFailed, format!("{e}. Continuing without the window; type q to quit.")),
    }

    // Console commands arrive on another thread so the main loop never blocks.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    println!("\nRunning. Commands: c = test chord, p = panic, s = save state, t = timing, m <text> = log marker, q = quit.\n");

    let chord = [60u8, 64, 67];
    let mut chord_off_at: Option<Instant> = None;
    let started = Instant::now();
    let mut window_errors = 0u32;
    let mut monitor = TimingMonitor::new(started);

    'main: loop {
        pump::pump_messages();
        if window.is_open() {
            if let Err(e) = window.service_platform_events() {
                window_errors += 1;
                if window_errors <= 5 {
                    diag::warn(Code::EditorEventError, e.to_string());
                } else if window_errors == 6 {
                    diag::info("further editor service errors are counted, not logged");
                }
            }
            if window.closed_by_user() {
                diag::note("editor window closed by user");
                break 'main;
            }
        }

        while let Ok(cmd) = rx.try_recv() {
            let cmd = cmd.trim();
            match cmd.split_once(' ').map(|(c, _)| c).unwrap_or(cmd) {
                "q" => break 'main,
                "c" => {
                    for n in chord {
                        handle.send_midi(MidiEvent::NoteOn { channel: MidiChannel::Ch1, note: n, velocity: 100 });
                    }
                    chord_off_at = Some(Instant::now() + Duration::from_millis(1500));
                    diag::note("test chord sent");
                }
                "p" => {
                    handle.midi_panic();
                    diag::note("panic sent");
                }
                "s" => save_state(&handle, &state_file),
                "t" => log_timing(&slot),
                "m" => diag::note(format!("MARKER: {}", cmd.strip_prefix('m').unwrap_or("").trim())),
                "" => {}
                other => println!("unknown command '{other}'. Use c, p, s, t, m or q."),
            }
        }

        if chord_off_at.is_some_and(|t| Instant::now() >= t) {
            for n in chord {
                handle.send_midi(MidiEvent::NoteOff { channel: MidiChannel::Ch1, note: n, velocity: 0 });
            }
            chord_off_at = None;
        }

        monitor.check(&slot, window_errors);
        std::thread::sleep(Duration::from_millis(5));
    }

    diag::note(format!("session length {:.0}s", started.elapsed().as_secs_f64()));
    log_timing(&slot);
    if window_errors > 0 {
        diag::info(format!("editor service errors in total: {window_errors}"));
    }
    save_state(&handle, &state_file);
    window.close();
    drop(midi_connections);
    handle.stop();
    diag::note("stopped cleanly");
    Ok(())
}

/// Watches the exclusive stream from the main thread. The audio thread only bumps counters;
/// this turns changes in those counters into timestamped log lines, so a click Dzyan hears
/// can be matched to a line in the log.
struct TimingMonitor {
    last_check: Instant,
    last_heartbeat: Instant,
    started: Instant,
    last: daw_audio_io::StreamStats,
    stream_failure_reported: bool,
}

impl TimingMonitor {
    fn new(started: Instant) -> Self {
        Self { last_check: started, last_heartbeat: started, started, last: Default::default(), stream_failure_reported: false }
    }

    fn check(&mut self, slot: &StreamSlot, window_errors: u32) {
        let now = Instant::now();
        if now - self.last_check < TIMING_CHECK {
            return;
        }
        self.last_check = now;
        let guard = slot.lock().unwrap_or_else(|p| p.into_inner());
        let stats = guard.as_ref().map(|o| o.stats());
        let failure = guard.as_ref().and_then(|o| o.failure());
        drop(guard);

        if let Some(st) = stats {
            let dropouts = st.timeouts - self.last.timeouts;
            let late = st.late_wakeups - self.last.late_wakeups;
            if dropouts > 0 {
                diag::warn(Code::Dropout, format!("{dropouts} in the last second ({} in total)", st.timeouts));
            }
            if late > 0 {
                // Log only: frequent late wake-ups on a laptop are common and would flood the console.
                diag::info(format!(
                    "{} {late} late wake-up(s) in the last second ({} in total, worst gap so far {:.2} ms)",
                    Code::LateWakeup.id(),
                    st.late_wakeups,
                    st.worst_gap_ms
                ));
            }
            self.last = st;
        }
        if let Some(e) = failure
            && !self.stream_failure_reported
        {
            self.stream_failure_reported = true;
            diag::error(e.code, format!("{}. Audio has stopped; quit (q) and start again.", e.message));
        }
        if now - self.last_heartbeat >= HEARTBEAT {
            self.last_heartbeat = now;
            let mem = diag::process_memory_mb().map(|m| format!("{m:.0} MB")).unwrap_or_else(|| "?".into());
            let timing = stats
                .map(|s| format!("{} buffers, {} dropouts, {} late", s.wakeups, s.timeouts, s.late_wakeups))
                .unwrap_or_else(|| "timing not measured in shared mode".into());
            diag::info(format!(
                "heartbeat: up {:.0} min | memory {mem} | {timing} | editor errors {window_errors}",
                self.started.elapsed().as_secs_f64() / 60.0
            ));
        }
    }
}

/// Exclusive mode only: how steadily the driver has been asking for audio.
fn log_timing(slot: &StreamSlot) {
    let guard = slot.lock().unwrap_or_else(|p| p.into_inner());
    match guard.as_ref() {
        Some(out) => {
            let st = out.stats();
            diag::note(format!(
                "timing: {} buffers | {} driver timeouts (dropouts) | {} late wake-ups | worst gap {:.2} ms",
                st.wakeups, st.timeouts, st.late_wakeups, st.worst_gap_ms
            ));
        }
        None => diag::note("timing: only measured in --exclusive mode"),
    }
}

fn save_state(handle: &vst3_host::AudioHandle, file: &Path) {
    let plugin = handle.plugin();
    let result = plugin.lock().unwrap_or_else(|p| p.into_inner()).save_state();
    let bytes = match result {
        Ok(b) => b,
        Err(e) => {
            diag::error(Code::StateSaveFailed, format!("the plugin refused to save its state: {e}"));
            return;
        }
    };
    match std::fs::write(file, &bytes) {
        Ok(()) => diag::note(format!("state saved to {} ({} bytes)", file.display(), bytes.len())),
        Err(e) => diag::error(Code::StateSaveFailed, format!("writing {}: {e}", file.display())),
    }
}

// ---------------------------------------------------------------- render

fn render(o: &Opts) -> Result<()> {
    let path = resolve_plugin(&o.plugin)?;
    let slug = plugin_slug(&path);
    let rate = o.rate.unwrap_or(48_000);
    let block = o.block.unwrap_or(512);
    let out = o.out.clone().unwrap_or_else(|| PathBuf::from(format!("m0-render-{slug}.wav")));
    let state_file = o.state.clone().unwrap_or_else(|| PathBuf::from(format!("m0-state-{slug}.bin")));
    diag::note(format!("mode: render | plugin: {} | {rate} Hz | block {block} | {}s", path.display(), o.seconds));

    let mut host = Vst3Host::builder().sample_rate(rate as f64).block_size(block).build().code(Code::HostInitFailed)?;
    let mut plugin = load_plugin(&mut host, &path)?;
    diag::note(format!("loaded: {} by {} v{}", plugin.info().name, plugin.info().vendor, plugin.info().version));
    restore_state(&mut plugin, &state_file)?;

    match plugin.set_process_mode(ProcessMode::Offline) {
        Ok(()) => diag::note("offline process mode set"),
        Err(e) => diag::warn(Code::OfflineRefused, e.to_string()),
    }
    plugin.start_processing().code(Code::PluginProcessFailed).context("starting processing")?;

    let mut buffers = AudioBuffers::new(0, 2, block, rate as f64);

    // Pre-roll: give sample-streaming instruments time to finish loading, both in wall-clock
    // time (their loader threads) and in processed silence.
    let preroll_blocks = ((o.preroll * rate as f64) / block as f64).ceil() as usize;
    for i in 0..preroll_blocks {
        buffers.clear();
        plugin.process_audio(&mut buffers).code(Code::PluginProcessFailed).with_context(|| format!("pre-roll block {i}"))?;
    }
    std::thread::sleep(Duration::from_secs_f64(o.preroll.max(0.0)));

    let total = (o.seconds * rate as f64).round() as usize;
    let release_at = ((o.seconds - 2.0).max(o.seconds * 0.5) * rate as f64) as usize;
    let chord = [60u8, 64, 67];
    for n in chord {
        plugin
            .send_midi_event(MidiEvent::NoteOn { channel: MidiChannel::Ch1, note: n, velocity: 100 })
            .code(Code::PluginProcessFailed)
            .context("sending the test chord")?;
    }

    let mut left = Vec::with_capacity(total);
    let mut right = Vec::with_capacity(total);
    let mut pos = 0usize;
    while pos < total {
        buffers.clear();
        if pos <= release_at && release_at < pos + block {
            let offset = (release_at - pos) as i32;
            for n in chord {
                plugin
                    .send_midi_event_at(MidiEvent::NoteOff { channel: MidiChannel::Ch1, note: n, velocity: 0 }, offset)
                    .code(Code::PluginProcessFailed)
                    .context("releasing the test chord")?;
            }
        }
        plugin
            .process_audio(&mut buffers)
            .code(Code::PluginProcessFailed)
            .with_context(|| format!("processing at sample {pos} of {total}"))?;
        let take = block.min(total - pos);
        left.extend_from_slice(&buffers.outputs[0][..take]);
        right.extend_from_slice(&buffers.outputs.get(1).unwrap_or(&buffers.outputs[0])[..take]);
        pos += take;
    }
    let _ = plugin.stop_processing();

    write_wav(&out, rate, &left, &right).code(Code::FileWrite).with_context(|| format!("writing {}", out.display()))?;

    let peak = left.iter().chain(&right).fold(0.0f32, |m, s| if s.is_finite() { m.max(s.abs()) } else { m });
    let non_finite = left.iter().chain(&right).filter(|s| !s.is_finite()).count();
    let peak_db = if peak > 0.0 { format!("{:.1} dBFS", 20.0 * peak.log10()) } else { "silence".into() };
    diag::note(format!("wrote {} ({} frames)", out.display(), total));
    diag::note(format!("peak: {peak_db}"));
    if peak == 0.0 {
        diag::warn(Code::RenderSilent, format!("{} frames, all zero", total));
    }
    if non_finite > 0 {
        return Err(DawError::new(Code::RenderNonFinite, format!("{non_finite} NaN/infinity samples in {}", out.display())).into());
    }
    Ok(())
}

fn write_wav(out: &Path, rate: u32, left: &[f32], right: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(out, spec)?;
    for (l, r) in left.iter().zip(right) {
        w.write_sample(*l)?;
        w.write_sample(*r)?;
    }
    w.finalize()
}
