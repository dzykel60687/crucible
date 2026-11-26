//! Live playback (D-034): hear the project, effects included, through an audio device.
//!
//! Two threads besides the caller's:
//!
//! - The **feeder** runs the same `audio::render` an export runs, a little ahead of what is
//!   heard (about 40 ms), into a queue. It reads the clip files and runs every effect. It is not
//!   the audio callback, so reading a disk or a burst of effect work cannot cause a dropout
//!   unless it lasts longer than the queue.
//! - The **audio callback** (the driver's thread) only copies from the queue to the device. It
//!   never allocates, locks, logs or waits (the real-time rule, `docs/02-architecture.md`).
//!
//! Because playback and export share one render path, what is heard is what an export writes,
//! bit for bit, until something is edited during playback.
//!
//! An edit during playback builds a new mixer on the caller's thread and sends it to the feeder,
//! which takes the running effects out of the old one (`Mixer::inherit`), so reverb and echo
//! tails carry on. The change is heard once the queued sound has played: about 40 ms later.
//!
//! Not here yet: instruments played live and input monitoring. Those need their tracks rendered
//! inside the callback for low latency; everything else keeps coming from the feeder.
//!
//! Portability: none of this is OS code. Devices come from `crucible-audio-io` (seam 1).

use crate::audio::{self, Held, Mixer, Reader, Sources, Src};
use crate::command::{Args, Kind, Reply, Spec, opt, req};
use crate::{Effect, Engine};
use crucible_audio_io as io;
use crucible_diagnostics::{self as diag, Code, DawError};
use crucible_project::{PPQ, Project};
use serde_json::json;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// Frames the feeder renders per step. Small, so an edit takes effect quickly.
const BLOCK: usize = 256;
/// How far ahead of the listener the feeder stays, in seconds, unless the device's own buffer
/// needs more (three of its buffers).
const AHEAD_SECONDS: f64 = 0.040;

const DRIVERS: &[&str] = &["auto", "asio", "exclusive", "shared"];

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "transport.play",
        args: &[
            opt("from", Kind::Number, "beat to start at (default: where it is, or the start)"),
            opt("wait", Kind::Bool, "keep the command running until the end of the song, then stop (for one-shot use)"),
            opt("driver", Kind::Choice(DRIVERS), "this time only: asio, exclusive, shared or auto"),
            opt("device", Kind::Text, "this time only: part of the output's name"),
            opt("buffer", Kind::Int, "this time only: driver buffer in frames"),
        ],
        help: "Play the project through the audio output, effects included.",
        effect: Effect::System,
        run: transport_play,
    },
    Spec { name: "transport.stop", args: &[], help: "Stop playback and release the audio device.", effect: Effect::System, run: transport_stop },
    Spec { name: "transport.seek", args: &[req("beat", Kind::Number, "beat to jump to")], help: "Move the play position.", effect: Effect::System, run: transport_seek },
    Spec { name: "transport.status", args: &[], help: "Play position, levels, dropouts and the device in use.", effect: Effect::Read, run: transport_status },
    Spec {
        name: "audio.output",
        args: &[
            opt("driver", Kind::Choice(DRIVERS), "auto picks ASIO if installed, else exclusive, else shared"),
            opt("device", Kind::Text, "part of the output's name, or 'default'"),
            opt("buffer", Kind::Int, "driver buffer in frames, or 0 for the driver's own choice"),
        ],
        help: "Show or set this machine's audio output. Remembered per machine, not in the project.",
        effect: Effect::System,
        run: audio_output,
    },
];

// ---------------------------------------------------------------- the queue between the threads

/// Single-writer, single-reader queue of samples. The feeder pushes, the audio callback pops.
/// `head` and `tail` count samples since the start and never wrap in practice.
struct Ring {
    buf: Vec<AtomicU32>,
    mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl Ring {
    fn new(samples: usize) -> Ring {
        let n = samples.next_power_of_two();
        Ring { buf: (0..n).map(|_| AtomicU32::new(0)).collect(), mask: n - 1, head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }
    fn fill(&self) -> usize {
        self.head.load(Relaxed).saturating_sub(self.tail.load(Relaxed))
    }
    /// The caller has checked there is room.
    fn push(&self, x: &[f32]) {
        let h = self.head.load(Relaxed);
        for (i, v) in x.iter().enumerate() {
            self.buf[(h + i) & self.mask].store(v.to_bits(), Relaxed);
        }
        self.head.store(h + x.len(), std::sync::atomic::Ordering::Release);
    }
    fn pop(&self, out: &mut [f32]) -> usize {
        let t = self.tail.load(Relaxed);
        let n = out.len().min(self.head.load(std::sync::atomic::Ordering::Acquire).saturating_sub(t));
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(self.buf[(t + i) & self.mask].load(Relaxed));
        }
        self.tail.store(t + n, Relaxed);
        n
    }
}

/// Everything both threads and the caller can see. Atomics only on the callback's side.
struct Shared {
    ring: Ring,
    /// Samples the feeder keeps queued, at least.
    ahead: AtomicUsize,
    /// The least the queue held when the driver came for sound, since the log last asked.
    low: AtomicUsize,
    /// The slowest mixing step since the log last asked.
    worst_recent_ns: AtomicU64,
    /// Things the feeder did, for the log (files opened and closed, mixers taken over). The
    /// feeder never waits for this lock: a note it cannot hand over is dropped.
    notes: Mutex<Vec<String>>,
    /// The most samples the driver has asked for in one go. Drivers do not always say their
    /// buffer size truthfully up front (shared mode and some ASIO drivers ask for more later),
    /// so the queue is sized from what they actually take: see `want`.
    asked: AtomicUsize,
    /// Queue position below which queued sound is thrown away, and the timeline frame that
    /// position stands for. Set by a seek.
    skip_to: AtomicUsize,
    seek_frame: AtomicU64,
    /// The timeline frame handed to the driver next.
    played: AtomicU64,
    /// True while an empty queue is expected (just started, just jumped), so it is not a dropout.
    waiting: AtomicBool,
    underruns: AtomicU64,
    /// Master peak since last read, as f32 bits (for non-negative floats, larger bits = larger value).
    peak: [AtomicU32; 2],
    /// Feeder load: nanoseconds spent rendering, frames rendered, and the slowest step.
    render_ns: AtomicU64,
    rendered: AtomicU64,
    worst_ns: AtomicU64,
    /// Mixers taken over after edits, and the timeline frame the latest one starts at.
    swaps: AtomicU64,
    swapped_at: AtomicU64,
    /// Per-track peaks since last read: (track id, left, right). Feeder and caller only.
    tracks: Mutex<Vec<(u32, f32, f32)>>,
    /// Why the feeder stopped, if it did (a clip file became unreadable).
    error: Mutex<Option<DawError>>,
}

enum Msg {
    Mixer(Box<Mixer>),
    Seek(u64),
    Quit,
}

/// The device-independent half of playback: the feeder and its queue. Tests drive it without
/// an audio device by calling the callback themselves.
pub(crate) struct Session {
    sh: Arc<Shared>,
    tx: mpsc::Sender<Msg>,
    feeder: Option<std::thread::JoinHandle<()>>,
    held: Held,
    rate: u32,
    /// The frame after the last sound, tails included.
    end: u64,
}

impl Session {
    pub(crate) fn start(p: &Project, folder: Option<&Path>, from: u64, sources: &Sources) -> Result<Session, DawError> {
        let m = audio::mixer(p, folder, BLOCK, Some(&Held::new()), sources)?;
        let (held, end) = (m.held(p), m.end_frame());
        let sh = Arc::new(Shared {
            ring: Ring::new(1 << 17),
            ahead: AtomicUsize::new(2 * (AHEAD_SECONDS * f64::from(p.sample_rate)) as usize),
            asked: AtomicUsize::new(0),
            low: AtomicUsize::new(usize::MAX),
            worst_recent_ns: AtomicU64::new(0),
            notes: Mutex::new(Vec::new()),
            skip_to: AtomicUsize::new(0),
            seek_frame: AtomicU64::new(from),
            played: AtomicU64::new(from),
            waiting: AtomicBool::new(true),
            underruns: AtomicU64::new(0),
            peak: [AtomicU32::new(0), AtomicU32::new(0)],
            render_ns: AtomicU64::new(0),
            rendered: AtomicU64::new(0),
            worst_ns: AtomicU64::new(0),
            swaps: AtomicU64::new(0),
            swapped_at: AtomicU64::new(from),
            tracks: Mutex::new(Vec::new()),
            error: Mutex::new(None),
        });
        let (tx, rx) = mpsc::channel();
        let sh_t = sh.clone();
        let feeder = std::thread::Builder::new()
            .name("crucible feeder".into())
            .spawn(move || feed(m, from, rx, &sh_t))
            .map_err(|e| DawError::new(Code::StreamStartFailed, format!("could not start the feeder thread: {e}")))?;
        Ok(Session { sh, tx, feeder: Some(feeder), held, rate: p.sample_rate, end })
    }

    /// The audio callback: interleaved stereo out of the queue. Real-time safe.
    pub(crate) fn callback(&self) -> io::OutputCallback {
        let sh = self.sh.clone();
        // A driver's request can arrive as several calls one straight after another; together
        // they are one request. Calls less than a millisecond apart are counted as one.
        let (mut burst, mut before) = (0usize, Instant::now());
        Box::new(move |buf: &mut [f32]| {
            let now = Instant::now();
            burst = if now.duration_since(before) < Duration::from_millis(1) { burst + buf.len() } else { buf.len() };
            before = now;
            sh.asked.fetch_max(burst, Relaxed);
            let skip = sh.skip_to.load(Relaxed);
            if sh.ring.tail.load(Relaxed) < skip {
                sh.ring.tail.store(skip, Relaxed);
                sh.played.store(sh.seek_frame.load(Relaxed), Relaxed);
            }
            sh.low.fetch_min(sh.ring.fill(), Relaxed);
            let got = sh.ring.pop(buf);
            buf[got..].fill(0.0);
            if got > 0 {
                sh.waiting.store(false, Relaxed);
                sh.played.fetch_add((got / 2) as u64, Relaxed);
                let (mut l, mut r) = (0.0f32, 0.0f32);
                for f in buf[..got].as_chunks::<2>().0 {
                    l = l.max(f[0].abs());
                    r = r.max(f[1].abs());
                }
                sh.peak[0].fetch_max(l.to_bits(), Relaxed);
                sh.peak[1].fetch_max(r.to_bits(), Relaxed);
            }
            if got < buf.len() && !sh.waiting.load(Relaxed) {
                sh.underruns.fetch_add(1, Relaxed);
                sh.waiting.store(true, Relaxed); // one dropout, however many callbacks it lasts
            }
        })
    }

    /// The project changed: build its mixer here, hand it to the feeder.
    pub(crate) fn update(&mut self, p: &Project, folder: Option<&Path>, sources: &Sources) -> Result<(), DawError> {
        let m = audio::mixer(p, folder, BLOCK, Some(&self.held), sources)?;
        (self.held, self.end) = (m.held(p), m.end_frame());
        let _ = self.tx.send(Msg::Mixer(Box::new(m)));
        self.wake();
        Ok(())
    }

    pub(crate) fn seek(&self, frame: u64) {
        let _ = self.tx.send(Msg::Seek(frame));
        self.wake();
    }

    fn wake(&self) {
        if let Some(f) = &self.feeder {
            f.thread().unpark();
        }
    }

    pub(crate) fn position(&self) -> u64 {
        self.sh.played.load(Relaxed)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(f) = self.feeder.take() {
            f.thread().unpark();
            let _ = f.join();
        }
    }
}

impl Shared {
    /// How many samples the feeder keeps queued: the configured amount, or three of the driver's
    /// largest requests if that is more, within what the queue can hold.
    fn want(&self) -> usize {
        self.ahead.load(Relaxed).max(3 * self.asked.load(Relaxed)).min(self.ring.buf.len() - BLOCK * 4)
    }

    /// Something the feeder did, for the log. Only when the log records everything, and never
    /// waited for: the feeder must not be held up by whoever is reading the notes.
    fn note(&self, text: impl FnOnce() -> String) {
        if diag::everything()
            && let Ok(mut n) = self.notes.try_lock()
        {
            n.push(text());
        }
    }
}

/// The feeder thread. Owns the mixer and the open clip files.
fn feed(mut m: Mixer, from: u64, rx: mpsc::Receiver<Msg>, sh: &Shared) {
    let _priority = io::AudioPriority::join();
    let mut pos = from;
    sh.note(|| format!("the mixing thread started at frame {from} with {} clips", m.voices.len()));
    let open = |m: &Mixer| -> (Vec<Option<Reader>>, Vec<(u64, Src)>) { (m.voices.iter().map(|_| None).collect(), m.voices.iter().map(|v| (v.end(), v.src().clone())).collect()) };
    let (mut readers, mut spans) = open(&m);
    let (mut l, mut r, mut sl, mut sr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut both = vec![0.0f32; BLOCK * 2];
    let mut peaks: Vec<(u32, f32, f32)> = Vec::new();
    let mut failed = false;
    loop {
        loop {
            match rx.try_recv() {
                Ok(Msg::Mixer(mut new)) => {
                    new.inherit(&mut m);
                    m = *new;
                    (readers, spans) = open(&m);
                    sh.swapped_at.store(pos, Relaxed);
                    sh.swaps.fetch_add(1, Relaxed);
                    sh.note(|| format!("a new mixer took over at frame {pos} ({} clips)", m.voices.len()));
                }
                Ok(Msg::Seek(frame)) => {
                    sh.note(|| format!("jumped from frame {pos} to frame {frame}"));
                    pos = frame;
                    sh.seek_frame.store(frame, Relaxed);
                    sh.waiting.store(true, Relaxed);
                    sh.skip_to.store(sh.ring.head.load(Relaxed), Relaxed);
                }
                Ok(Msg::Quit) | Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if failed || sh.ring.fill() + BLOCK * 2 > sh.want() {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }
        let t0 = Instant::now();
        let result = audio::render(&mut m, pos, &mut l, &mut r, (&mut sl, &mut sr), |i, at, a, b| {
            if readers[i].is_none() {
                readers[i] = Some(Reader::open(&spans[i].1)?);
                sh.note(|| format!("opened {}", spans[i].1.name()));
            }
            readers[i].as_mut().expect("opened above").read(at, a, b)
        });
        if let Err(e) = result {
            // Silence from here on; `transport.status` reports why.
            diag::error(e.code, format!("playback stopped feeding: {}", e.message));
            *sh.error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
            sh.waiting.store(true, Relaxed);
            failed = true;
            continue;
        }
        for (i, (until, _)) in spans.iter().enumerate() {
            if *until <= pos + BLOCK as u64 {
                readers[i] = None;
            }
        }
        for k in 0..BLOCK {
            (both[2 * k], both[2 * k + 1]) = (l[k], r[k]);
        }
        sh.ring.push(&both);
        pos += BLOCK as u64;
        let ns = t0.elapsed().as_nanos() as u64;
        sh.render_ns.fetch_add(ns, Relaxed);
        sh.rendered.fetch_add(BLOCK as u64, Relaxed);
        sh.worst_ns.fetch_max(ns, Relaxed);
        sh.worst_recent_ns.fetch_max(ns, Relaxed);
        m.take_peaks(&mut peaks);
        if let Ok(mut t) = sh.tracks.try_lock() {
            for p in peaks.drain(..) {
                match t.iter_mut().find(|x| x.0 == p.0) {
                    Some(x) => (x.1, x.2) = (x.1.max(p.1), x.2.max(p.2)),
                    None => t.push(p),
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the device half

/// This machine's output choice. Lives beside the logs, not in any project (`03-platform-notes.md`:
/// device choices are per machine).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct OutputChoice {
    driver: Option<io::Driver>,
    device: Option<String>,
    buffer: Option<u32>,
}

fn settings_file() -> Option<PathBuf> {
    Some(diag::data_dir()?.join("audio-output.json"))
}

impl OutputChoice {
    fn load() -> OutputChoice {
        let Some(v) = settings_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
            return OutputChoice::default();
        };
        OutputChoice {
            driver: v["driver"].as_str().and_then(io::Driver::parse),
            device: v["device"].as_str().map(str::to_string),
            buffer: v["buffer"].as_u64().map(|b| b as u32),
        }
    }

    fn save(&self) -> Result<(), DawError> {
        let file = settings_file().ok_or_else(|| DawError::new(Code::OutputChoiceNotSaved, "no per-user folder to keep the audio output choice in"))?;
        let fail = |e: std::io::Error| DawError::new(Code::OutputChoiceNotSaved, format!("{}: {e}", file.display()));
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        std::fs::write(&file, self.json().to_string()).map_err(fail)
    }

    fn json(&self) -> serde_json::Value {
        json!({ "driver": self.driver.map(io::Driver::name), "device": self.device, "buffer": self.buffer })
    }

    fn text(&self) -> String {
        format!(
            "driver {}, device {}, buffer {}",
            self.driver.map_or("auto (ASIO if installed, else exclusive, else shared)", io::Driver::name),
            self.device.as_deref().unwrap_or("the system default"),
            self.buffer.map_or("the driver's own choice".to_string(), |b| format!("{b} frames"))
        )
    }

    /// Apply a command's `driver`, `device` and `buffer` options on top of this choice.
    fn with(&self, a: &Args) -> OutputChoice {
        let mut c = self.clone();
        if let Some(d) = a.text("driver") {
            c.driver = io::Driver::parse(d);
        }
        if let Some(d) = a.text("device") {
            c.device = (d != "default" && !d.is_empty()).then(|| d.to_string());
        }
        if let Some(b) = a.int("buffer") {
            c.buffer = (b > 0).then_some(b as u32);
        }
        c
    }
}

pub(crate) struct Player {
    session: Session,
    out: io::Output,
    began: Instant,
    /// What the log has been told so far, so it hears of each dropout once (`watch`).
    told: Cell<Told>,
}

#[derive(Clone, Copy)]
struct Told {
    at: Instant,
    /// Dropouts by cause, as last reported: queue ran dry, driver overruns, driver timeouts.
    counts: (u64, u64, u64),
    /// Loudest master sample since the last health line.
    peak: f32,
    failed: bool,
    /// When a dropout line was last written: one a second at most, however many there are.
    warned: Option<Instant>,
    /// Late wake-ups of the driver's thread, as last reported.
    late: u64,
    /// Whether the log has been told that the driver asks for more than it reported.
    told_asked: bool,
    /// Mixing time and frames, and late wake-ups, at the last once-a-second line.
    second: (u64, u64, u64),
    /// Something outside (the window's once-a-second line) asks for the numbers itself.
    ticked: bool,
}

/// How often a health line goes into the log while playing: every second while the log records
/// everything (test builds), else every ten.
fn health_every() -> Duration {
    Duration::from_secs(if diag::everything() { 1 } else { 10 })
}

impl Player {
    fn start(p: &Project, folder: Option<&Path>, from: u64, choice: &OutputChoice, sources: &Sources) -> Result<Player, DawError> {
        let session = Session::start(p, folder, from, sources)?;
        let req = io::OutputRequest { driver: choice.driver, device: choice.device.clone(), sample_rate: p.sample_rate, buffer_frames: choice.buffer, channels: 2 };
        diag::event("AUDIO", format!("opening the output | asked for: {}", choice.text()));
        let others = crate::devices::others_on(choice.device.as_deref());
        if !others.is_empty() {
            diag::event("AUDIO", format!("other programs with sound on this output: {}", others.join(", ")));
        }
        let mut out = io::open_output(&req, &mut || session.callback()).map_err(|mut e| {
            // "In use" is far more useful with a name beside it.
            if e.code == Code::DeviceInUse && !others.is_empty() {
                e.message.push_str(&format!(" Programs with sound on it now: {}.", others.join(", ")));
            }
            e
        })?;
        if out.driver() == io::Driver::Exclusive && !others.is_empty() {
            out.notes.push(format!("{} also had sound on this output. Exclusive mode silences other programs while Crucible plays; choose the Shared driver in Settings to let them carry on.", others.join(", ")));
        }
        let ahead = ((AHEAD_SECONDS * f64::from(p.sample_rate)) as usize).max(3 * out.info().buffer_frames as usize);
        session.sh.ahead.store(2 * ahead, Relaxed);
        session.wake();
        for n in &out.notes {
            diag::info(format!("playback: {n}"));
        }
        let pl = Player { session, out, began: Instant::now(), told: Cell::new(Told { at: Instant::now(), counts: (0, 0, 0), peak: 0.0, failed: false, warned: None, late: 0, told_asked: false, second: (0, 0, 0), ticked: false }) };
        let rate = f64::from(p.sample_rate);
        diag::event(
            "AUDIO",
            format!(
                "start | {} | queue {:.0} ms ahead | from {:.2} s | {} tracks, {} clips, {} effects | sound ends at {:.1} s",
                pl.device_text(),
                ahead as f64 * 1000.0 / rate,
                from as f64 / rate,
                p.tracks.len(),
                p.clips.len(),
                p.tracks.iter().map(|t| t.effects.len()).sum::<usize>(),
                pl.session.end as f64 / rate
            ),
        );
        Ok(pl)
    }

    /// Dropouts by cause: the queue ran dry (mixing fell behind), the driver reported an
    /// overrun, the driver never asked for sound.
    fn counts(&self) -> (u64, u64, u64) {
        (self.session.sh.underruns.load(Relaxed), self.out.driver_xruns(), if self.out.driver() == io::Driver::Exclusive { self.out.stats().timeouts } else { 0 })
    }

    fn dropouts(&self) -> u64 {
        let (queue, driver, timeouts) = self.counts();
        queue + driver + timeouts
    }

    /// The loudest master sample on each side since this was last asked, and start again.
    fn take_peak(&self) -> [f32; 2] {
        let p = &self.session.sh.peak;
        [f32::from_bits(p[0].swap(0, Relaxed)), f32::from_bits(p[1].swap(0, Relaxed))]
    }

    fn seconds(&self) -> f64 {
        self.session.position() as f64 / f64::from(self.session.rate)
    }

    /// The numbers that say how playback is coping, in one phrase for the log.
    fn health(&self) -> String {
        let (load, worst) = self.load();
        let (sh, stats) = (&self.session.sh, self.out.stats());
        format!(
            "dropouts {}, late wake-ups {}, mixing {load:.1}% of one core, slowest step {worst:.0}% of its slot, queue {:.0}% full, longest driver gap {:.1} ms, edits taken over {}",
            self.dropouts(),
            stats.late_wakeups,
            100.0 * sh.ring.fill() as f64 / sh.want().max(1) as f64,
            stats.worst_gap_ms,
            sh.swaps.load(Relaxed)
        )
    }

    /// How playback did since this was last asked (about a second ago): the least the queue held
    /// when the driver came for sound, how much of a core mixing took and its slowest step, late
    /// wake-ups, the loudest sample. For the once-a-second line; asking resets the measurements.
    fn second(&self) -> String {
        let mut t = self.told.get();
        let sh = &self.session.sh;
        let rate = f64::from(self.session.rate);
        let (ns, frames, late) = (sh.render_ns.load(Relaxed), sh.rendered.load(Relaxed), self.out.stats().late_wakeups);
        let (dns, dframes) = (ns.saturating_sub(t.second.0) as f64, frames.saturating_sub(t.second.1) as f64);
        let mixing = if dframes > 0.0 { 100.0 * dns / 1e9 / (dframes / rate) } else { 0.0 };
        let slowest = 100.0 * sh.worst_recent_ns.swap(0, Relaxed) as f64 / 1e9 / (BLOCK as f64 / rate);
        let low = sh.low.swap(usize::MAX, Relaxed);
        let queue = if low == usize::MAX { "no request".to_string() } else { format!("low {:.0}%", 100.0 * low as f64 / sh.want().max(1) as f64) };
        t.peak = t.peak.max(f32::from_bits(sh.peak[0].load(Relaxed))).max(f32::from_bits(sh.peak[1].load(Relaxed)));
        let peak = db(t.peak).map_or("silent".to_string(), |d| format!("{d:+.1} dBFS"));
        let line = format!(
            "at {:.1} s | queue {queue} | mixing {mixing:.1}%, slowest step {slowest:.0}% | late wake-ups +{}, dropouts {} | peak {peak}",
            self.seconds(),
            late.saturating_sub(t.second.2),
            self.dropouts()
        );
        (t.second, t.peak, t.at) = ((ns, frames, late), 0.0, Instant::now());
        self.told.set(t);
        line
    }

    /// Keep the log informed while playing: dropouts when they are first seen, with their cause
    /// (one line a second at most, so a machine that cannot keep up does not flood the log); a
    /// device failure once; and a health line every ten seconds. `last` writes whatever is still
    /// unreported, for the end of playback. Called from
    /// `transport.status`, which the window runs many times a second, and from the one-shot
    /// wait loop. Never from an audio thread: those only count (the real-time rule).
    fn watch(&self, peak: Option<[f32; 2]>, last: bool) {
        let mut t = self.told.get();
        if let Some(p) = peak {
            t.peak = t.peak.max(p[0]).max(p[1]);
        }
        let now = self.counts();
        if now != t.counts && (last || t.warned.is_none_or(|w| w.elapsed() >= Duration::from_secs(1))) {
            let mut why = Vec::new();
            if now.0 > t.counts.0 {
                why.push(format!("the queue ran dry {} time(s): mixing fell behind", now.0 - t.counts.0));
            }
            if now.1 > t.counts.1 {
                why.push(format!("the driver reported {} overrun(s)", now.1 - t.counts.1));
            }
            if now.2 > t.counts.2 {
                why.push(format!("the driver did not ask for sound {} time(s)", now.2 - t.counts.2));
            }
            diag::warn(Code::Dropout, format!("near {:.2} s of the song: {} | {}", self.seconds(), why.join("; "), self.health()));
            (t.counts, t.warned) = (now, Some(Instant::now()));
        }
        // A late wake-up is a near miss: on record when it happens, but not a warning.
        let stats = self.out.stats();
        if stats.late_wakeups > t.late && (last || t.warned.is_none_or(|w| w.elapsed() >= Duration::from_secs(1))) {
            diag::event("AUDIO", format!("the driver's thread woke late {} time(s) near {:.2} s | longest gap so far {:.1} ms | {}", stats.late_wakeups - t.late, self.seconds(), stats.worst_gap_ms, self.health()));
            (t.late, t.warned) = (stats.late_wakeups, Some(Instant::now()));
        }
        if !t.failed
            && let Some(err) = self.out.failure()
        {
            diag::error(err.code, format!("the audio device failed near {:.2} s of the song: {}", self.seconds(), err.message));
            t.failed = true;
        }
        let (asked, reported) = (self.session.sh.asked.load(Relaxed) / 2, self.out.info().buffer_frames as usize);
        if !t.told_asked && asked > reported + reported / 2 {
            diag::event("AUDIO", format!("the driver asks for up to {asked} frames at a time, not the {reported} it reported; the queue now keeps {} frames ready", self.session.sh.want() / 2));
            t.told_asked = true;
        }
        // What the mixing thread did since the last look.
        if let Ok(mut notes) = self.session.sh.notes.try_lock() {
            for n in notes.drain(..) {
                diag::event("AUDIO", format!("mixing thread: {n}"));
            }
        }
        self.told.set(t);
        // The once-a-second line, unless the window writes it into its own (`alive`).
        if !t.ticked && t.at.elapsed() >= health_every() {
            diag::event("AUDIO", self.second());
        }
    }

    /// End playback: put the totals in the log, then release the device.
    fn finish(self, why: &str) -> Result<(), DawError> {
        self.watch(None, true);
        diag::event("AUDIO", format!("stop ({why}) | at {:.2} s of the song, {:.1} s after starting | {}", self.seconds(), self.began.elapsed().as_secs_f64(), self.health()));
        let Player { session, out, .. } = self;
        drop(session);
        out.stop()
    }

    fn device_text(&self) -> String {
        let i = self.out.info();
        format!("{} [{}, {}, {} Hz, {}-frame buffer = {:.1} ms]", i.device, self.out.driver().name(), i.format, i.sample_rate, i.buffer_frames, i.buffer_ms)
    }

    /// Feeder load as a share of real time, and its slowest step as a share of that step's length.
    fn load(&self) -> (f64, f64) {
        let sh = &self.session.sh;
        let (ns, frames) = (sh.render_ns.load(Relaxed) as f64, sh.rendered.load(Relaxed) as f64);
        let rate = f64::from(self.session.rate);
        if frames == 0.0 { (0.0, 0.0) } else { (100.0 * ns / 1e9 / (frames / rate), 100.0 * sh.worst_ns.load(Relaxed) as f64 / 1e9 / (BLOCK as f64 / rate)) }
    }

    fn summary(&self) -> String {
        let (load, worst) = self.load();
        let late = self.out.stats().late_wakeups;
        format!("{} dropouts, {late} late wake-ups; mixing used {load:.1}% of one core (slowest step {worst:.0}% of its time slot)", self.dropouts())
    }
}

fn frames_per_beat(p: &Project) -> f64 {
    p.frames_per_tick() * PPQ as f64
}

fn db(x: f32) -> Option<f64> {
    (x > 0.0).then(|| 20.0 * f64::from(x).log10())
}

/// Called by the engine after every command that is not a read.
pub(crate) fn after_command(e: &mut Engine, file: bool, changed: bool) {
    let Some(pl) = &mut e.player else { return };
    let stop_because = if file {
        Some("another project was loaded".to_string())
    } else if !changed {
        None
    } else if e.project.sample_rate != pl.session.rate {
        Some("the project's sample rate changed".to_string())
    } else {
        let t0 = Instant::now();
        let result = pl.session.update(&e.project, e.folder.as_deref(), &e.sources);
        if result.is_ok() {
            diag::event("AUDIO", format!("edit taken over near {:.2} s | new mixer built in {:.1} ms", pl.seconds(), t0.elapsed().as_secs_f64() * 1000.0));
        }
        result.err().map(|err| err.to_string())
    };
    if let Some(why) = stop_because {
        if let Some(pl) = e.player.take()
            && let Err(err) = pl.finish(&why)
        {
            diag::warn(err.code, format!("releasing the audio device: {}", err.message));
        }
        e.stop_note = Some(why);
    }
}

/// For the window's once-a-second log line: how playback did in the last second, or `None` when
/// nothing is playing. Once this has been asked, playback leaves its own once-a-second line out.
pub(crate) fn alive(e: &Engine) -> Option<String> {
    let pl = e.player.as_ref()?;
    let mut t = pl.told.get();
    t.ticked = true;
    pl.told.set(t);
    Some(pl.second())
}

/// This machine's output choice as data, for the device list.
pub(crate) fn choice_json(e: &mut Engine) -> serde_json::Value {
    e.output().json()
}

/// One phrase for the log: where playback is, or that it is not running. `ago` moves the
/// position back by that many seconds: a mark is typed a moment after the thing it is about.
pub(crate) fn where_now(e: &Engine, ago: f64) -> String {
    let Some(pl) = &e.player else { return "not playing".to_string() };
    let at = (pl.seconds() - ago).max(0.0);
    let beat = at * f64::from(pl.session.rate) / frames_per_beat(&e.project);
    format!("playing, {at:.2} s into the song (beat {beat:.2}) through {} | {}", pl.device_text(), pl.health())
}

/// This machine's output choice and the ASIO drivers installed, for the log.
pub(crate) fn output_text(e: &mut Engine) -> String {
    let asio = io::asio_drivers();
    format!("{} | ASIO drivers installed: {}", e.output().text(), if asio.is_empty() { "none".to_string() } else { asio.join("; ") })
}

fn transport_play(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let fpb = frames_per_beat(&e.project);
    let from = a.number("from").map(|b| (b.max(0.0) * fpb).round() as u64);
    if let Some(pl) = &e.player
        && a.flag("wait") != Some(true)
    {
        if let Some(f) = from {
            pl.session.seek(f);
        }
        return Ok(Reply::text(format!("Already playing through {}", pl.device_text())));
    }
    // Release the device before opening it again.
    if let Some(old) = e.player.take() {
        old.finish("starting again")?;
    }
    e.stop_note = None;
    let choice = e.output().with(a);
    let pl = Player::start(&e.project, e.folder.as_deref(), from.unwrap_or(0), &choice, &e.sources)?;
    let mut text = format!("Playing through {}", pl.device_text());
    for n in &pl.out.notes {
        text.push_str(&format!("\nNote: {n}"));
    }
    if a.flag("wait") != Some(true) {
        e.player = Some(pl);
        return Ok(Reply::new(text, json!({ "playing": true })).logged());
    }
    // One-shot use: stay here until the end of the song, then stop.
    let (end, t0) = (pl.session.end, Instant::now());
    let limit = Duration::from_secs_f64((end.saturating_sub(from.unwrap_or(0)) as f64 / f64::from(pl.session.rate)) * 1.5 + 5.0);
    while pl.session.position() < end && pl.out.failure().is_none() && t0.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(20));
        pl.watch(Some(pl.take_peak()), false);
    }
    let played = pl.session.position().saturating_sub(from.unwrap_or(0)) as f64 / f64::from(pl.session.rate);
    let (summary, dropouts, failure) = (pl.summary(), pl.dropouts(), pl.out.failure());
    let data = json!({ "playing": false, "seconds": played, "dropouts": dropouts, "load_percent": pl.load().0, "worst_step_percent": pl.load().1, "device": pl.out.info(), "driver": pl.out.driver().name() });
    let stopped = pl.finish(if failure.is_some() { "the device failed" } else { "the end of the song" });
    if let Some(f) = failure {
        return Err(f);
    }
    stopped?;
    Ok(Reply::new(format!("{text}\nPlayed {played:.1} s: {summary}"), data))
}

fn transport_stop(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let Some(pl) = e.player.take() else { return Ok(Reply::new("Not playing", json!({ "playing": false }))) };
    let fpb = frames_per_beat(&e.project);
    let (frame, summary) = (pl.session.position(), pl.summary());
    let data = json!({ "playing": false, "frame": frame, "beat": frame as f64 / fpb, "dropouts": pl.dropouts() });
    pl.finish("transport.stop")?;
    Ok(Reply::new(format!("Stopped at beat {:.2}: {summary}", frame as f64 / fpb), data).logged())
}

fn transport_seek(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let beat = a.need_number("beat")?.max(0.0);
    match &e.player {
        Some(pl) => pl.session.seek((beat * frames_per_beat(&e.project)).round() as u64),
        None => return Ok(Reply::text("Not playing; transport.play --from <beat> starts there")),
    }
    Ok(Reply::text(format!("Playing from beat {beat:.2}")))
}

fn transport_status(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let fpb = frames_per_beat(&e.project);
    let Some(pl) = &e.player else {
        // Said once: why playback ended by itself, if it did.
        let why = e.stop_note.take();
        return Ok(Reply::new(why.as_ref().map_or("Stopped".to_string(), |w| format!("Stopped: {w}")), json!({ "playing": false, "stopped_because": why })));
    };
    let sh = &pl.session.sh;
    let frame = pl.session.position();
    let (beat, seconds) = (frame as f64 / fpb, frame as f64 / f64::from(pl.session.rate));
    let peak = pl.take_peak();
    pl.watch(Some(peak), false);
    let tracks: Vec<_> = std::mem::take(&mut *sh.tracks.lock().unwrap_or_else(|p| p.into_inner())).into_iter().map(|(id, l, r)| json!({ "id": id, "peak": [db(l), db(r)] })).collect();
    let error = pl.out.failure().or_else(|| sh.error.lock().unwrap_or_else(|p| p.into_inner()).clone());
    let (load, worst) = pl.load();
    let mut text = format!("Playing at beat {beat:.2} ({seconds:.1} s) through {}: {}", pl.device_text(), pl.summary());
    if let Some(err) = &error {
        text.push_str(&format!("\n{err}"));
    }
    Ok(Reply::new(
        text,
        json!({
            "playing": true, "frame": frame, "beat": beat, "seconds": seconds, "end_beat": pl.session.end as f64 / fpb,
            "driver": pl.out.driver().name(), "device": pl.out.info(), "notes": pl.out.notes,
            "ahead_ms": sh.ahead.load(Relaxed) as f64 / 2.0 * 1000.0 / f64::from(pl.session.rate),
            "dropouts": pl.dropouts(), "late_wakeups": pl.out.stats().late_wakeups, "load_percent": load, "worst_step_percent": worst,
            "peak": [db(peak[0]), db(peak[1])], "tracks": tracks,
            "error": error.map(|x| x.to_string()),
        }),
    ))
}

fn audio_output(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let new = e.output().with(a);
    let changed = new != *e.output();
    if changed {
        new.save()?;
        e.output = Some(new.clone());
    }
    let mut text = format!("Audio output{}: {}", if changed { " set" } else { "" }, new.text());
    let asio = io::asio_drivers();
    text.push_str(&if asio.is_empty() { "\nASIO drivers installed: none".to_string() } else { format!("\nASIO drivers installed: {}", asio.join("; ")) });
    if changed && e.player.is_some() {
        text.push_str("\nPlayback keeps its current device until it is stopped and started again.");
    }
    Ok(Reply::new(text, json!({ "choice": new.json(), "asio_drivers": asio })))
}

impl Engine {
    fn output(&mut self) -> &OutputChoice {
        self.output.get_or_insert_with(OutputChoice::load)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(e: &mut Engine, line: &str) -> Reply {
        let mut last = None;
        for tokens in crate::command::split_statements(line).unwrap() {
            last = Some(e.execute(&tokens).unwrap_or_else(|err| panic!("{line}: {err}")));
        }
        last.unwrap()
    }

    /// A saved project in a temp folder: two tracks of noise, a reverb and an echo.
    fn song(name: &str) -> (Engine, PathBuf) {
        let dir = std::env::temp_dir().join(format!("crucible-play-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for (file, frames) in [("a.wav", 30_000usize), ("b.wav", 20_000)] {
            let spec = hound::WavSpec { channels: 2, sample_rate: 48_000, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(dir.join(file), spec).unwrap();
            for _ in 0..frames * 2 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                w.write_sample(((seed >> 40) as i32 - (1 << 23)) / 4).unwrap();
            }
            w.finalize().unwrap();
        }
        let mut e = Engine::new();
        let q = |p: PathBuf| format!("\"{}\"", p.display().to_string().replace('\\', "/"));
        run(&mut e, &format!("project.new Play --discard; project.save {}", q(dir.join("proj"))));
        run(&mut e, &format!("audio.import {}; audio.import {}", q(dir.join("a.wav")), q(dir.join("b.wav"))));
        run(&mut e, "track.add One; track.add Two; clip.place One 0 a.wav; clip.place Two 0.5 b.wav");
        run(&mut e, "fx.add One reverb; fx.set One 1 mix 30; fx.add Two echo; fx.set Two 1 time 60; fx.set Two 1 sync 0; track.pan Two 30");
        (e, dir)
    }

    fn export(e: &mut Engine, dir: &Path, name: &str) -> Vec<f32> {
        let f = dir.join(name);
        run(e, &format!("project.export \"{}\" --bits 32f", f.display().to_string().replace('\\', "/")));
        hound::WavReader::open(f).unwrap().samples::<f32>().map(Result::unwrap).collect()
    }

    /// Stand in for the audio device: pull `frames` stereo frames through the callback in
    /// uneven pieces, waiting for the feeder like a (very patient) driver would.
    fn pull(s: &Session, cb: &mut io::OutputCallback, frames: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * 2);
        let mut k = 0usize;
        while out.len() < frames * 2 {
            let n = ([333usize, 64, 480, 1][k % 4] * 2).min(frames * 2 - out.len());
            k += 1;
            let t0 = Instant::now();
            while s.sh.ring.fill() < n && s.sh.ring.tail.load(Relaxed) >= s.sh.skip_to.load(Relaxed) {
                assert!(t0.elapsed() < Duration::from_secs(10), "the feeder stopped feeding");
                std::thread::sleep(Duration::from_millis(1));
            }
            let mut buf = vec![9.0f32; n];
            cb(&mut buf);
            out.extend(buf);
        }
        out
    }

    #[test]
    fn playback_is_the_export_bit_for_bit() {
        let (mut e, dir) = song("same");
        let want = export(&mut e, &dir, "want.wav");
        let s = Session::start(e.project(), e.folder(), 0, &e.sources).unwrap();
        let mut cb = s.callback();
        let got = pull(&s, &mut cb, want.len() / 2);
        if let Some(i) = (0..want.len()).find(|&i| got[i] != want[i]) {
            let n = (0..want.len()).filter(|&i| got[i] != want[i]).count();
            let worst = (0..want.len()).map(|i| (got[i] - want[i]).abs()).fold(0.0f32, f32::max);
            panic!("playback differs from the export: first at sample {i} of {} ({} vs {}), {n} differ, largest difference {worst:e}", want.len(), got[i], want[i]);
        }
        assert_eq!(s.sh.underruns.load(Relaxed), 0);
        assert_eq!(s.position(), (want.len() / 2) as u64);
        // The callback itself never allocates.
        let mut buf = vec![0.0f32; 512];
        assert_eq!(crate::alloc_count::during(|| cb(&mut buf)), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A gain change during playback: before it the old export, after it the new export, exactly.
    /// That only holds if the reverb and echo carried on untouched through the change.
    #[test]
    fn an_edit_during_playback_keeps_the_effects_running() {
        let (mut e, dir) = song("edit");
        let before = export(&mut e, &dir, "before.wav");
        let mut s = Session::start(e.project(), e.folder(), 0, &e.sources).unwrap();
        let mut cb = s.callback();
        let mut got = pull(&s, &mut cb, 6_000);
        run(&mut e, "track.gain One -6; track.pan Two -40");
        s.update(e.project(), e.folder(), &e.sources).unwrap();
        let after = export(&mut e, &dir, "after.wav");
        let t0 = Instant::now();
        while s.sh.swaps.load(Relaxed) == 0 {
            assert!(t0.elapsed() < Duration::from_secs(10));
            got.extend(pull(&s, &mut cb, 64));
        }
        let at = s.sh.swapped_at.load(Relaxed) as usize * 2;
        got.extend(pull(&s, &mut cb, after.len() / 2 - got.len() / 2));
        assert!(at > 0 && at < after.len());
        assert!(got[..at] == before[..at], "sound before the edit changed");
        assert!(got[at..] == after[at..], "sound after the edit is not the edited project's");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A setting of the effect itself changes: the tail must carry on, not restart from silence.
    #[test]
    fn an_effect_change_during_playback_keeps_its_tail() {
        let (mut e, dir) = song("tail");
        run(&mut e, "fx.set One 1 decay 3");
        let mut s = Session::start(e.project(), e.folder(), 0, &e.sources).unwrap();
        let mut cb = s.callback();
        pull(&s, &mut cb, 31_000); // clip One (30,000 frames) has ended: only its reverb tail sounds on the left
        run(&mut e, "fx.set One 1 mix 60; track.mute Two");
        s.update(e.project(), e.folder(), &e.sources).unwrap();
        let t0 = Instant::now();
        while s.sh.swaps.load(Relaxed) == 0 {
            assert!(t0.elapsed() < Duration::from_secs(10));
            pull(&s, &mut cb, 64);
        }
        let tail = pull(&s, &mut cb, 4_800);
        let rms = (tail.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        assert!(rms > 1e-4, "the reverb tail was cut by the change (rms {rms})");
        // An IR Reverb added while playing has no file yet: playback carries on.
        run(&mut e, "fx.add One irreverb");
        s.update(e.project(), e.folder(), &e.sources).expect("a fileless IR Reverb must not stop playback");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_seek_jumps_without_counting_a_dropout() {
        let (e, dir) = song("seek");
        let s = Session::start(e.project(), e.folder(), 0, &e.sources).unwrap();
        let mut cb = s.callback();
        pull(&s, &mut cb, 2_000);
        s.seek(12_000);
        let t0 = Instant::now();
        while s.sh.skip_to.load(Relaxed) == 0 {
            assert!(t0.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut one = vec![0.0f32; 2];
        cb(&mut one); // the callback notices the jump and throws the queued sound away
        let got = pull(&s, &mut cb, 1_000);
        assert!(s.position() >= 13_000 && s.position() <= 13_001, "position {}", s.position());
        assert!(got.iter().any(|x| *x != 0.0));
        assert_eq!(s.sh.underruns.load(Relaxed), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A driver that asks for far more at a time than the 40 ms the queue starts with: a big
    /// shared-mode or ASIO buffer, which drivers do not always report up front. Before
    /// 2026-11-25 every such request ran the queue dry, so the dropout count climbed without
    /// end. The queue has to grow to fit what the driver really takes.
    #[test]
    fn the_queue_grows_to_fit_a_driver_that_asks_for_big_buffers() {
        let (e, dir) = song("big");
        let s = Session::start(e.project(), e.folder(), 0, &e.sources).unwrap();
        let mut cb = s.callback();
        let mut buf = vec![0.0f32; 2 * 8192]; // 8,192 frames in one request: 171 ms at 48 kHz
        assert!(buf.len() > s.sh.ahead.load(Relaxed), "the test needs a request larger than the starting queue");
        cb(&mut buf); // the first request shows the queue what this driver takes
        let t0 = Instant::now();
        while s.sh.ring.fill() < buf.len() {
            assert!(t0.elapsed() < Duration::from_secs(20), "the queue never grew: it holds {} of the {} samples asked for", s.sh.ring.fill(), buf.len());
            std::thread::sleep(Duration::from_millis(1));
        }
        let before = s.sh.underruns.load(Relaxed);
        cb(&mut buf);
        assert_eq!(s.sh.underruns.load(Relaxed), before, "a request the queue had time to fill was still short");
        assert!(buf.iter().any(|x| *x != 0.0), "the request was answered with silence");
        let _ = std::fs::remove_dir_all(dir);
    }
}
