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
//! **Instruments** (D-044) are the exception: a key must be heard at once, not 40 ms later, so
//! their tracks are mixed inside the audio callback (`Stage`), from a mixer of their own, and
//! added to what the queue holds. While an instrument is loaded the device stays open between
//! Stop and Play (`Rig`, kept in `Engine::standby`), so the keyboards always sound. The feeder
//! and its queue come and go with Play and Stop as before.
//!
//! Not here yet: input monitoring.
//!
//! Portability: none of this is OS code. Devices come from `crucible-audio-io` (seam 1).

use crate::audio::{self, Held, Mixer, Part, Reader, Sources, Src};
use crate::rack::{self, LIVE_BLOCK};
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
    /// Where the song ends: the frame after the last sound, tails included, or further out if
    /// the person moved the end there (D-043). 0 when there is nothing to play.
    end: u64,
}

impl Session {
    pub(crate) fn start(p: &Project, folder: Option<&Path>, from: u64, sources: &Sources) -> Result<Session, DawError> {
        let m = audio::mixer(p, folder, BLOCK, Some(&Held::new()), sources)?;
        let (held, end) = (m.held(p), audio::song_end(p, &m));
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

    /// The audio callback for the queue alone: interleaved stereo out of it. Real-time safe.
    /// Tests drive this; a real device gets `Stage::callback`, which adds the instruments.
    #[cfg(test)]
    pub(crate) fn callback(&self) -> io::OutputCallback {
        let sh = self.sh.clone();
        let mut burst = (0usize, Instant::now());
        Box::new(move |buf: &mut [f32]| {
            tape(&sh, buf, &mut burst);
        })
    }

    /// The project changed: build its mixer here, hand it to the feeder.
    pub(crate) fn update(&mut self, p: &Project, folder: Option<&Path>, sources: &Sources) -> Result<(), DawError> {
        let m = audio::mixer(p, folder, BLOCK, Some(&self.held), sources)?;
        (self.held, self.end) = (m.held(p), audio::song_end(p, &m));
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

/// The queue's half of the audio callback: fill `buf` from the queue, silence where it runs
/// short. Returns the timeline frame of the first sample handed over and how many samples came
/// from the queue. Real-time safe.
fn tape(sh: &Shared, buf: &mut [f32], burst: &mut (usize, Instant)) -> (u64, usize) {
    // A driver's request can arrive as several calls one straight after another; together
    // they are one request. Calls less than a millisecond apart are counted as one.
    let now = Instant::now();
    burst.0 = if now.duration_since(burst.1) < Duration::from_millis(1) { burst.0 + buf.len() } else { buf.len() };
    burst.1 = now;
    sh.asked.fetch_max(burst.0, Relaxed);
    let skip = sh.skip_to.load(Relaxed);
    if sh.ring.tail.load(Relaxed) < skip {
        sh.ring.tail.store(skip, Relaxed);
        sh.played.store(sh.seek_frame.load(Relaxed), Relaxed);
    }
    sh.low.fetch_min(sh.ring.fill(), Relaxed);
    let from = sh.played.load(Relaxed);
    let got = sh.ring.pop(buf);
    buf[got..].fill(0.0);
    if got > 0 {
        sh.waiting.store(false, Relaxed);
        sh.played.fetch_add((got / 2) as u64, Relaxed);
    }
    if got < buf.len() && !sh.waiting.load(Relaxed) {
        sh.underruns.fetch_add(1, Relaxed);
        sh.waiting.store(true, Relaxed); // one dropout, however many callbacks it lasts
    }
    (from, got)
}

/// Leave the loudest sample of each side where the meters read it.
fn publish_peak(peak: &[AtomicU32; 2], buf: &[f32]) {
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for f in buf.as_chunks::<2>().0 {
        l = l.max(f[0].abs());
        r = r.max(f[1].abs());
    }
    peak[0].fetch_max(l.to_bits(), Relaxed);
    peak[1].fetch_max(r.to_bits(), Relaxed);
}

// ---------------------------------------------------------------- the stage: what the device plays

/// A place where one thread leaves something for another. Neither waits for the other: the
/// audio thread only ever tries (`try_take`, `try_give`), and tries again at its next block.
struct Slot<T>(Mutex<Option<Box<T>>>);

impl<T> Slot<T> {
    fn new() -> Slot<T> {
        Slot(Mutex::new(None))
    }
    /// Leave `b`; returns what was still there, not yet taken.
    fn put(&self, b: Box<T>) -> Option<Box<T>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).replace(b)
    }
    fn take(&self) -> Option<Box<T>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
    fn try_take(&self) -> Option<Box<T>> {
        self.0.try_lock().ok()?.take()
    }
}

/// Things the audio thread is done with are strung into a chain and left for another thread to
/// free: the audio thread never frees memory.
trait Linked: Sized {
    fn next(&mut self) -> &mut Option<Box<Self>>;
}

impl Linked for Mixer {
    fn next(&mut self) -> &mut Option<Box<Mixer>> {
        &mut self.next
    }
}

/// The queue the device plays from, or none while stopped.
struct Tape {
    sh: Option<Arc<Shared>>,
    next: Option<Box<Tape>>,
}

impl Linked for Tape {
    fn next(&mut self) -> &mut Option<Box<Tape>> {
        &mut self.next
    }
}

/// Put `old` on the audio thread's own pile, then try to leave the pile in `slot`.
fn discard<T: Linked>(pile: &mut Option<Box<T>>, slot: &Slot<T>, old: Option<Box<T>>) {
    if let Some(mut old) = old {
        *old.next() = pile.take();
        *pile = Some(old);
    }
    if pile.is_none() {
        return;
    }
    let Ok(mut there) = slot.0.try_lock() else { return };
    let Some(mut mine) = pile.take() else { return };
    let mut last = &mut mine;
    while last.next().is_some() {
        last = last.next().as_mut().expect("checked just above");
    }
    *last.next() = there.take();
    *there = Some(mine);
}

/// What the device's callback and the rest of the engine share for as long as the device is open.
pub(crate) struct Stage {
    tape: Slot<Tape>,
    live: Slot<Mixer>,
    old_tapes: Slot<Tape>,
    old_mixers: Slot<Mixer>,
    keys: Arc<rack::Keys>,
    /// An export has the instruments: the callback leaves them alone.
    paused: AtomicBool,
    /// Callbacks so far.
    calls: AtomicU64,
    /// Loudest sample handed to the device on each side since last read, as f32 bits (for
    /// non-negative floats, larger bits = larger value).
    peak: [AtomicU32; 2],
}

impl Stage {
    fn new(keys: Arc<rack::Keys>) -> Arc<Stage> {
        Arc::new(Stage { tape: Slot::new(), live: Slot::new(), old_tapes: Slot::new(), old_mixers: Slot::new(), keys, paused: AtomicBool::new(false), calls: AtomicU64::new(0), peak: [AtomicU32::new(0), AtomicU32::new(0)] })
    }

    /// See `Rig::restage`. Returns what the new mixer holds, its meters, and whether it has any
    /// track at all.
    fn restage(&self, held: &Held, p: &Project, folder: Option<&Path>, sources: &Sources, rack: &rack::Rack) -> Result<(Held, Vec<Arc<rack::Meter>>, bool), DawError> {
        let m = audio::mixer_for(p, folder, LIVE_BLOCK, Some(held), sources, Part::Live, Some(rack))?;
        let facts = (m.held(p), m.meters(), !m.is_empty());
        let mut m = Box::new(m);
        // One the audio thread has not taken yet is replaced; what it brought moves on.
        if let Some(mut waiting) = self.live.take() {
            m.inherit(&mut waiting);
        }
        drop(self.live.put(m));
        Ok(facts)
    }

    /// The device's callback: what the queue holds (while playing), plus the instruments, mixed
    /// here and now. Real-time safe: no allocation, no waiting (see `discard` and `Slot`).
    pub(crate) fn callback(self: &Arc<Stage>) -> io::OutputCallback {
        let st = self.clone();
        let (mut tape_now, mut live_now) = (Box::new(Tape { sh: None, next: None }), Box::new(Mixer::empty()));
        let (mut old_tapes, mut old_mixers): (Option<Box<Tape>>, Option<Box<Mixer>>) = (None, None);
        let mut burst = (0usize, Instant::now());
        let (mut l, mut r, mut sl, mut sr) = (vec![0.0f32; LIVE_BLOCK], vec![0.0f32; LIVE_BLOCK], vec![0.0f32; LIVE_BLOCK], vec![0.0f32; LIVE_BLOCK]);
        // Song time for the instruments' effects while the song itself stands still.
        let mut clock = 0u64;
        Box::new(move |buf: &mut [f32]| {
            st.calls.fetch_add(1, Relaxed);
            let was = st.tape.try_take().map(|new| std::mem::replace(&mut tape_now, new));
            discard(&mut old_tapes, &st.old_tapes, was);
            let was = st.live.try_take().map(|mut new| {
                new.inherit(&mut live_now);
                std::mem::replace(&mut live_now, new)
            });
            discard(&mut old_mixers, &st.old_mixers, was);

            let (from, got) = match &tape_now.sh {
                Some(sh) => tape(sh, buf, &mut burst),
                None => {
                    buf.fill(0.0);
                    (0, 0)
                }
            };
            live_now.live.keys.clear();
            if live_now.is_empty() || st.paused.load(Relaxed) {
                // Nobody to play them: keys must not pile up and sound later.
                while st.keys.pop().is_some() {}
            } else {
                let keys = &mut live_now.live.keys;
                while keys.len() < keys.capacity()
                    && let Some(k) = st.keys.pop()
                {
                    keys.push(k);
                }
                let (frames, rolling) = (buf.len() / 2, got / 2);
                let mut at = 0;
                while at < frames {
                    // While the queue has sound the song is running: the clips' notes play at
                    // their places. Past that (stopped, or the queue ran dry) only the keys do.
                    let run = at < rolling;
                    let k = LIVE_BLOCK.min(if run { rolling - at } else { frames - at });
                    live_now.live.rolling = run;
                    let start = if run { from + at as u64 } else { clock };
                    let _ = audio::render(&mut live_now, start, &mut l[..k], &mut r[..k], (&mut sl, &mut sr), |_, _, _, _| Ok(()));
                    for j in 0..k {
                        buf[2 * (at + j)] += l[j];
                        buf[2 * (at + j) + 1] += r[j];
                    }
                    live_now.live.keys.clear();
                    at += k;
                    clock += k as u64;
                }
                live_now.publish_peaks();
            }
            publish_peak(&st.peak, buf);
        })
    }
}

/// The audio device, open, with its stage. Lives in a `Player` while the song plays and in
/// `Engine::standby` between Stop and Play while an instrument is loaded.
pub(crate) struct Rig {
    out: io::Output,
    stage: Arc<Stage>,
    choice: OutputChoice,
    rate: u32,
    /// The effects the instruments' mixer holds, to carry their sound over an edit.
    held: Held,
    meters: Vec<Arc<rack::Meter>>,
    /// Whether the instruments' mixer the audio thread has, or will take, has any track in it.
    live: bool,
}

impl Rig {
    fn open(choice: &OutputChoice, rate: u32, keys: &Arc<rack::Keys>) -> Result<Rig, DawError> {
        let stage = Stage::new(keys.clone());
        let req = io::OutputRequest { driver: choice.driver, device: choice.device.clone(), sample_rate: rate, buffer_frames: choice.buffer, channels: 2 };
        diag::event("AUDIO", format!("opening the output | asked for: {}", choice.text()));
        let others = crate::devices::others_on(choice.device.as_deref());
        if !others.is_empty() {
            diag::event("AUDIO", format!("other programs with sound on this output: {}", others.join(", ")));
        }
        let mut out = io::open_output(&req, &mut || stage.callback()).map_err(|mut e| {
            // "In use" is far more useful with a name beside it.
            if e.code == Code::DeviceInUse && !others.is_empty() {
                e.message.push_str(&format!(" Programs with sound on it now: {}.", others.join(", ")));
            }
            e
        })?;
        if out.driver() == io::Driver::Exclusive && !others.is_empty() {
            out.notes.push(format!("{} also had sound on this output. Exclusive mode silences other programs while Crucible plays; choose the Shared driver in Settings to let them carry on.", others.join(", ")));
        }
        for n in &out.notes {
            diag::info(format!("playback: {n}"));
        }
        Ok(Rig { out, stage, choice: choice.clone(), rate, held: Held::new(), meters: Vec::new(), live: false })
    }

    fn close(self, why: &str) -> Result<(), DawError> {
        diag::event("AUDIO", format!("output released ({why}) | {}", self.device_text()));
        let Rig { out, stage, .. } = self;
        let result = out.stop();
        drop(stage);
        result
    }

    /// Build the instruments' mixer from the project as it is now and leave it for the audio
    /// thread, which carries the running effects and the sounding notes over into it.
    fn restage(&mut self, p: &Project, folder: Option<&Path>, sources: &Sources, rack: &rack::Rack) -> Result<(), DawError> {
        let (held, meters, live) = self.stage.restage(&self.held, p, folder, sources, rack)?;
        (self.held, self.meters, self.live) = (held, meters, live);
        Ok(())
    }

    /// Free what the audio thread is done with.
    fn tidy(&self) {
        drop(self.stage.old_tapes.take());
        drop(self.stage.old_mixers.take());
    }

    fn device_text(&self) -> String {
        let i = self.out.info();
        format!("{} [{}, {}, {} Hz, {}-frame buffer = {:.1} ms]", i.device, self.out.driver().name(), i.format, i.sample_rate, i.buffer_frames, i.buffer_ms)
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
    rig: Rig,
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
    /// Start the song on an open device. On failure the device comes back with the error.
    fn start(p: &Project, folder: Option<&Path>, from: u64, rig: Rig, sources: &Sources) -> Result<Player, Box<(DawError, Rig)>> {
        let session = match Session::start(p, folder, from, sources) {
            Ok(s) => s,
            Err(e) => return Err(Box::new((e, rig))),
        };
        let ahead = ((AHEAD_SECONDS * f64::from(p.sample_rate)) as usize).max(3 * rig.out.info().buffer_frames as usize);
        session.sh.ahead.store(2 * ahead, Relaxed);
        session.wake();
        rig.stage.peak[0].store(0, Relaxed);
        rig.stage.peak[1].store(0, Relaxed);
        drop(rig.stage.tape.put(Box::new(Tape { sh: Some(session.sh.clone()), next: None })));
        let pl = Player { session, rig, began: Instant::now(), told: Cell::new(Told { at: Instant::now(), counts: (0, 0, 0), peak: 0.0, failed: false, warned: None, late: 0, told_asked: false, second: (0, 0, 0), ticked: false }) };
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
        (self.session.sh.underruns.load(Relaxed), self.rig.out.driver_xruns(), if self.rig.out.driver() == io::Driver::Exclusive { self.rig.out.stats().timeouts } else { 0 })
    }

    fn dropouts(&self) -> u64 {
        let (queue, driver, timeouts) = self.counts();
        queue + driver + timeouts
    }

    /// The loudest master sample on each side since this was last asked, and start again.
    fn take_peak(&self) -> [f32; 2] {
        let p = &self.rig.stage.peak;
        [f32::from_bits(p[0].swap(0, Relaxed)), f32::from_bits(p[1].swap(0, Relaxed))]
    }

    fn seconds(&self) -> f64 {
        self.session.position() as f64 / f64::from(self.session.rate)
    }

    /// The numbers that say how playback is coping, in one phrase for the log.
    fn health(&self) -> String {
        let (load, worst) = self.load();
        let (sh, stats) = (&self.session.sh, self.rig.out.stats());
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
        let (ns, frames, late) = (sh.render_ns.load(Relaxed), sh.rendered.load(Relaxed), self.rig.out.stats().late_wakeups);
        let (dns, dframes) = (ns.saturating_sub(t.second.0) as f64, frames.saturating_sub(t.second.1) as f64);
        let mixing = if dframes > 0.0 { 100.0 * dns / 1e9 / (dframes / rate) } else { 0.0 };
        let slowest = 100.0 * sh.worst_recent_ns.swap(0, Relaxed) as f64 / 1e9 / (BLOCK as f64 / rate);
        let low = sh.low.swap(usize::MAX, Relaxed);
        let queue = if low == usize::MAX { "no request".to_string() } else { format!("low {:.0}%", 100.0 * low as f64 / sh.want().max(1) as f64) };
        t.peak = t.peak.max(f32::from_bits(self.rig.stage.peak[0].load(Relaxed))).max(f32::from_bits(self.rig.stage.peak[1].load(Relaxed)));
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
        let stats = self.rig.out.stats();
        if stats.late_wakeups > t.late && (last || t.warned.is_none_or(|w| w.elapsed() >= Duration::from_secs(1))) {
            diag::event("AUDIO", format!("the driver's thread woke late {} time(s) near {:.2} s | longest gap so far {:.1} ms | {}", stats.late_wakeups - t.late, self.seconds(), stats.worst_gap_ms, self.health()));
            (t.late, t.warned) = (stats.late_wakeups, Some(Instant::now()));
        }
        if !t.failed
            && let Some(err) = self.rig.out.failure()
        {
            diag::error(err.code, format!("the audio device failed near {:.2} s of the song: {}", self.seconds(), err.message));
            t.failed = true;
        }
        let (asked, reported) = (self.session.sh.asked.load(Relaxed) / 2, self.rig.out.info().buffer_frames as usize);
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

    /// End playback: put the totals in the log and hand the device back, still open. The caller
    /// keeps it for the instruments or releases it (`park`).
    fn finish(self, why: &str) -> Rig {
        self.watch(None, true);
        diag::event("AUDIO", format!("stop ({why}) | at {:.2} s of the song, {:.1} s after starting | {}", self.seconds(), self.began.elapsed().as_secs_f64(), self.health()));
        let Player { session, rig, .. } = self;
        drop(rig.stage.tape.put(Box::new(Tape { sh: None, next: None })));
        drop(session);
        rig
    }

    fn device_text(&self) -> String {
        self.rig.device_text()
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
        let late = self.rig.out.stats().late_wakeups;
        format!("{} dropouts, {late} late wake-ups; mixing used {load:.1}% of one core (slowest step {worst:.0}% of its time slot)", self.dropouts())
    }
}

fn frames_per_beat(p: &Project) -> f64 {
    p.frames_per_tick() * PPQ as f64
}

fn db(x: f32) -> Option<f64> {
    (x > 0.0).then(|| 20.0 * f64::from(x).log10())
}

/// Keep the device for the instruments, or let it go when none is loaded.
fn park(e: &mut Engine, rig: Rig, why: &str) -> Result<(), DawError> {
    if e.rack.any_loaded() {
        diag::event("AUDIO", format!("the output stays open for the instruments | {}", rig.device_text()));
        e.standby = Some(rig);
        Ok(())
    } else {
        rig.close(why)
    }
}

/// The instruments or what they play changed: give the audio thread a new mixer for them, and
/// open or release the device as instruments come and go. Never an error: instruments that
/// cannot be heard say why in `instrument.status`.
pub(crate) fn live_update(e: &mut Engine) {
    let want = e.rack.any_loaded();
    if e.player.is_none() {
        let (rate, choice) = (e.project.sample_rate, e.output().clone());
        if let Some(r) = e.standby.take_if(|r| !want || r.rate != rate || r.choice != choice)
            && let Err(err) = r.close(if want { "the output or the rate changed" } else { "no instrument is loaded" })
        {
            diag::warn(err.code, format!("releasing the audio device: {}", err.message));
        }
        if !want {
            e.rack.silent = None;
            return;
        }
        if cfg!(test) {
            // Tests stand in for the device themselves (`tests::Desk`); none is ever opened.
            e.rack.silent = Some("tests have no audio device".to_string());
            return;
        }
        if e.standby.is_none() {
            match Rig::open(&choice, rate, &e.rack.keys) {
                Ok(r) => {
                    diag::event("AUDIO", format!("output open for the instruments | {}", r.device_text()));
                    (e.standby, e.rack.silent) = (Some(r), None);
                }
                Err(err) => {
                    let why = err.to_string();
                    if e.rack.silent.as_ref() != Some(&why) {
                        diag::warn(err.code, format!("the instruments cannot be heard: {}", err.message));
                    }
                    e.rack.silent = Some(why);
                    return;
                }
            }
        }
    }
    let folder = crate::work_dir(&e.folder, &e.scratch);
    let rig = match (&mut e.player, &mut e.standby) {
        (Some(pl), _) => &mut pl.rig,
        (None, Some(r)) => r,
        _ => return,
    };
    rig.tidy();
    if rig.rate != e.project.sample_rate || (!want && !rig.live) {
        return;
    }
    if let Err(err) = rig.restage(&e.project, folder, &e.sources, &e.rack) {
        diag::warn(err.code, format!("the instruments keep their old settings: {}", err.message));
    }
}

/// An export takes the instruments (`on`), or gives them back. Waits until the audio thread has
/// let go of them: two of its rounds, or a moment if the device is not running.
pub(crate) fn pause_live(e: &Engine, on: bool) {
    let Some(stage) = e.player.as_ref().map(|p| &p.rig).or(e.standby.as_ref()).map(|r| r.stage.clone()) else { return };
    stage.paused.store(on, Relaxed);
    if on {
        let (seen, t0) = (stage.calls.load(Relaxed), Instant::now());
        while stage.calls.load(Relaxed) < seen + 2 && t0.elapsed() < Duration::from_millis(250) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// For `instrument.status` while the song is stopped: each instrument track's level and the
/// output's since last asked (as f32 bits), and the device in words. While the song plays,
/// `transport.status` reports the levels, so this leaves them alone.
pub(crate) fn live_levels(e: &Engine) -> LiveLevels {
    match (&e.player, &e.standby) {
        (Some(pl), _) => LiveLevels { tracks: Vec::new(), master: None, output: json!({ "open": true, "playing": true, "device": pl.device_text() }) },
        (None, Some(r)) => LiveLevels {
            tracks: r.meters.iter().map(|m| (m.track, m.peak[0].swap(0, Relaxed), m.peak[1].swap(0, Relaxed))).collect(),
            master: Some([r.stage.peak[0].swap(0, Relaxed), r.stage.peak[1].swap(0, Relaxed)]),
            output: json!({ "open": true, "playing": false, "device": r.device_text(), "driver": r.out.driver().name(), "buffer_ms": r.out.info().buffer_ms, "dropouts": r.out.driver_xruns() }),
        },
        _ => LiveLevels { tracks: Vec::new(), master: None, output: json!({ "open": false, "playing": false }) },
    }
}

/// See `live_levels`. Levels are f32 bits.
pub(crate) struct LiveLevels {
    /// Track id, left, right.
    pub tracks: Vec<(u32, u32, u32)>,
    pub master: Option<[u32; 2]>,
    pub output: serde_json::Value,
}

/// The program is closing or the project is going: let the device go.
pub(crate) fn release(e: &mut Engine, why: &str) {
    if let Some(pl) = e.player.take() {
        e.standby = Some(pl.finish(why));
    }
    if let Some(r) = e.standby.take()
        && let Err(err) = r.close(why)
    {
        diag::warn(err.code, format!("releasing the audio device: {}", err.message));
    }
}

/// Called by the engine after every command that is not a read. `plugins`: an instrument was
/// loaded or taken down.
pub(crate) fn after_command(e: &mut Engine, file: bool, changed: bool, plugins: bool) {
    after_command_playing(e, file, changed);
    if file || changed || plugins {
        live_update(e);
    }
}

fn after_command_playing(e: &mut Engine, file: bool, changed: bool) {
    let Some(pl) = &mut e.player else { return };
    let stop_because = if file {
        Some("another project was loaded".to_string())
    } else if !changed {
        None
    } else if e.project.sample_rate != pl.session.rate {
        Some("the project's sample rate changed".to_string())
    } else {
        let t0 = Instant::now();
        let result = pl.session.update(&e.project, crate::work_dir(&e.folder, &e.scratch), &e.sources);
        if result.is_ok() {
            diag::event("AUDIO", format!("edit taken over near {:.2} s | new mixer built in {:.1} ms", pl.seconds(), t0.elapsed().as_secs_f64() * 1000.0));
        }
        result.err().map(|err| err.to_string())
    };
    if let Some(why) = stop_because {
        if let Some(pl) = e.player.take() {
            let rig = pl.finish(&why);
            if let Err(err) = park(e, rig, &why) {
                diag::warn(err.code, format!("releasing the audio device: {}", err.message));
            }
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
    if let Some(old) = e.player.take() {
        e.standby = Some(old.finish("starting again"));
    }
    e.stop_note = None;
    // The device the instruments already have open is used as it is; anything else is opened.
    let (choice, rate) = (e.output().with(a), e.project.sample_rate);
    if let Some(r) = e.standby.take_if(|r| r.choice != choice || r.rate != rate) {
        r.close("another output was asked for")?;
    }
    let rig = match e.standby.take() {
        Some(r) => r,
        None => Rig::open(&choice, rate, &e.rack.keys)?,
    };
    let pl = match Player::start(&e.project, e.work(), from.unwrap_or(0), rig, &e.sources) {
        Ok(pl) => pl,
        Err(failed) => {
            let (err, rig) = *failed;
            park(e, rig, "playback could not start")?;
            return Err(err);
        }
    };
    let mut text = format!("Playing through {}", pl.device_text());
    for n in &pl.rig.out.notes {
        text.push_str(&format!("\nNote: {n}"));
    }
    e.player = Some(pl);
    live_update(e);
    if a.flag("wait") != Some(true) {
        return Ok(Reply::new(text, json!({ "playing": true })).logged());
    }
    let Some(pl) = e.player.take() else { return Ok(Reply::text(text)) };
    // One-shot use: stay here until the end of the song, then stop.
    let (end, t0) = (pl.session.end, Instant::now());
    let limit = Duration::from_secs_f64((end.saturating_sub(from.unwrap_or(0)) as f64 / f64::from(pl.session.rate)) * 1.5 + 5.0);
    while pl.session.position() < end && pl.rig.out.failure().is_none() && t0.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(20));
        pl.watch(Some(pl.take_peak()), false);
    }
    let played = pl.session.position().saturating_sub(from.unwrap_or(0)) as f64 / f64::from(pl.session.rate);
    let (summary, dropouts, failure) = (pl.summary(), pl.dropouts(), pl.rig.out.failure());
    let data = json!({ "playing": false, "seconds": played, "dropouts": dropouts, "load_percent": pl.load().0, "worst_step_percent": pl.load().1, "device": pl.rig.out.info(), "driver": pl.rig.out.driver().name() });
    let why = if failure.is_some() { "the device failed" } else { "the end of the song" };
    let rig = pl.finish(why);
    let stopped = park(e, rig, why);
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
    let rig = pl.finish("transport.stop");
    park(e, rig, "transport.stop")?;
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
    // The end of the song: stop there (D-043). Before 2026-11-26 playback ran on in silence
    // until Stop. A project with nothing in it has no end and plays until stopped.
    if e.player.as_ref().is_some_and(|pl| pl.session.end > 0 && pl.session.position() >= pl.session.end)
        && let Some(pl) = e.player.take()
    {
        let (end_beat, summary) = (pl.session.end as f64 / fpb, pl.summary());
        let rig = pl.finish("the end of the song");
        park(e, rig, "the end of the song")?;
        return Ok(Reply::new(format!("Stopped at the end of the song, beat {end_beat:.2}: {summary}"), json!({ "playing": false, "ended": true, "beat": end_beat, "stopped_because": null })).logged());
    }
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
    let mut tracks: Vec<_> = std::mem::take(&mut *sh.tracks.lock().unwrap_or_else(|p| p.into_inner())).into_iter().map(|(id, l, r)| json!({ "id": id, "peak": [db(l), db(r)] })).collect();
    // The instruments' levels come from the audio thread itself.
    tracks.extend(pl.rig.meters.iter().map(|m| json!({ "id": m.track, "peak": [db(f32::from_bits(m.peak[0].swap(0, Relaxed))), db(f32::from_bits(m.peak[1].swap(0, Relaxed)))] })));
    let error = pl.rig.out.failure().or_else(|| sh.error.lock().unwrap_or_else(|p| p.into_inner()).clone());
    let (load, worst) = pl.load();
    let mut text = format!("Playing at beat {beat:.2} ({seconds:.1} s) through {}: {}", pl.device_text(), pl.summary());
    if let Some(err) = &error {
        text.push_str(&format!("\n{err}"));
    }
    Ok(Reply::new(
        text,
        json!({
            "playing": true, "frame": frame, "beat": beat, "seconds": seconds, "end_beat": pl.session.end as f64 / fpb,
            "driver": pl.rig.out.driver().name(), "device": pl.rig.out.info(), "notes": pl.rig.out.notes,
            "ahead_ms": sh.ahead.load(Relaxed) as f64 / 2.0 * 1000.0 / f64::from(pl.session.rate),
            "dropouts": pl.dropouts(), "late_wakeups": pl.rig.out.stats().late_wakeups, "load_percent": load, "worst_step_percent": worst,
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
    } else if changed {
        live_update(e); // the instruments move to the new output now
    }
    Ok(Reply::new(text, json!({ "choice": new.json(), "asio_drivers": asio })))
}

impl Engine {
    pub(crate) fn output(&mut self) -> &OutputChoice {
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

    // ---- instruments (D-044), on the test instrument: one sine per key, velocity/127 * 0.25 loud

    /// Stands in for the open device: a stage, its callback, and what its mixer holds.
    struct Desk {
        stage: Arc<Stage>,
        cb: io::OutputCallback,
        held: Held,
    }

    impl Desk {
        fn new(e: &Engine) -> Desk {
            let stage = Stage::new(e.rack.keys.clone());
            let mut d = Desk { cb: stage.callback(), stage, held: Held::new() };
            d.restage(e);
            d
        }
        /// What `live_update` does after a command, when a device is open.
        fn restage(&mut self, e: &Engine) {
            self.held = self.stage.restage(&self.held, e.project(), e.work(), &e.sources, &e.rack).unwrap().0;
        }
        /// `frames` of stereo sound with the song stopped, in pieces of uneven size.
        fn hear(&mut self, frames: usize) -> Vec<f32> {
            let mut out = Vec::new();
            for (k, n) in [300usize, 1500, 64].iter().cycle().enumerate() {
                let n = (*n).min(frames - out.len() / 2);
                if n == 0 || k > 10_000 {
                    break;
                }
                let mut buf = vec![9.0f32; n * 2];
                (self.cb)(&mut buf);
                out.extend(buf);
            }
            out
        }
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    /// A full-velocity key on the test instrument, as an RMS level.
    const KEY: f64 = 0.25 * std::f64::consts::FRAC_1_SQRT_2;

    /// An engine with the test instrument on an instrument track named Keys (track id 1).
    fn band() -> Engine {
        let synth = rack::test_synth();
        let mut e = Engine::new();
        run(&mut e, &format!("project.new Band --discard; track.add Keys --kind instrument; instrument.set Keys \"{synth}\""));
        e
    }

    #[test]
    fn a_key_sounds_at_once_and_follows_the_fader() {
        let mut e = band();
        assert_eq!(e.project().tracks[0].instrument.as_ref().unwrap().name, crucible_test_synth::NAME);
        let mut d = Desk::new(&e);
        assert_eq!(rms(&d.hear(2_000)), 0.0, "silence before any key");

        run(&mut e, "instrument.note Keys 69 --velocity 127");
        let on = d.hear(4_800);
        assert!(on[0] == 0.0 && on[2] != 0.0, "the key must sound in the very block that takes it");
        assert!((rms(&on) - KEY).abs() < 0.005, "level {} vs {KEY}", rms(&on));
        assert!(on.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]), "centre pan: both sides alike");

        // The fader moves while the key is held: the note carries on, 6 dB down, not struck again.
        run(&mut e, "track.gain Keys -6.0206; track.pan Keys 100");
        d.restage(&e);
        let down = d.hear(4_800);
        let (left, right): (Vec<f32>, Vec<f32>) = down.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
        assert_eq!(rms(&left), 0.0, "panned hard right");
        assert!((rms(&right) - KEY / 2.0).abs() < 0.005, "level after the fader moved: {}", rms(&right));

        // Muted: silence, but the plugin still hears the key come up, so nothing is stuck later.
        run(&mut e, "track.mute Keys on");
        d.restage(&e);
        run(&mut e, "instrument.note Keys 69 --off");
        assert_eq!(rms(&d.hear(2_000)), 0.0);
        run(&mut e, "track.mute Keys off; track.pan Keys 0");
        d.restage(&e);
        assert_eq!(rms(&d.hear(2_000)), 0.0, "a key released while the track was muted sounded again");

        // The device's callback never allocates, with an instrument sounding.
        run(&mut e, "instrument.note Keys 60");
        d.hear(1_000);
        let mut buf = vec![0.0f32; 960];
        assert_eq!(crate::alloc_count::during(|| (d.cb)(&mut buf)), 0, "the callback allocated");
        assert!(rms(&buf) > 0.01);
        e.close();
    }

    #[test]
    fn the_keyboards_play_one_track_and_obey_its_channel() {
        let synth = rack::test_synth();
        let mut e = band();
        run(&mut e, &format!("track.add Pad --kind instrument; instrument.set Pad \"{synth}\"; track.pan Keys -100; track.pan Pad 100"));
        let mut d = Desk::new(&e);
        // The newest instrument is the one the keyboards play: Pad, on the right.
        let key = |e: &Engine, status: u8, pitch: u8, vel: u8| assert!(e.rack.keys.push(rack::Key { track: 0, source: 0, midi: crucible_plugin_host::Midi { offset: 0, status, a: pitch, b: vel } }));
        let sides = |x: &[f32]| -> (f64, f64) {
            let (l, r): (Vec<f32>, Vec<f32>) = x.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
            (rms(&l), rms(&r))
        };
        key(&e, 0x90, 60, 127);
        let (l, r) = sides(&d.hear(4_800));
        assert!(l == 0.0 && (r - KEY).abs() < 0.005, "only Pad should sound: left {l}, right {r}");

        // The keyboards move to Keys while the key is still down: Pad lets go of it.
        run(&mut e, "track.live Keys --only");
        d.restage(&e);
        let (l, r) = sides(&d.hear(4_800));
        assert_eq!((l, r), (0.0, 0.0), "a held key rang on after the keyboards left the track");
        key(&e, 0x90, 60, 127);
        let (l, r) = sides(&d.hear(4_800));
        assert!((l - KEY).abs() < 0.005 && r == 0.0, "only Keys should sound: left {l}, right {r}");
        key(&e, 0x80, 60, 0);

        // Both tracks, Pad on channel 2 only: a key on channel 1 reaches Keys alone.
        run(&mut e, "track.live Pad; track.input Pad all --channel 2");
        d.restage(&e);
        d.hear(1_000);
        key(&e, 0x90, 64, 127);
        let (l, r) = sides(&d.hear(4_800));
        assert!(l > 0.1 && r == 0.0, "channel 1 reached the track that listens to channel 2: left {l}, right {r}");
        key(&e, 0x91, 67, 127);
        let (_, r) = sides(&d.hear(4_800));
        assert!(r > 0.1, "channel 2 did not reach its track");
        // A keyboard named in the project that is not connected: the track hears none.
        run(&mut e, "track.input Pad \"no such keyboard\" --channel 0");
        d.restage(&e);
        d.hear(1_000);
        key(&e, 0x90, 72, 127);
        let (l, r) = sides(&d.hear(4_800));
        assert!(l > 0.1 && r == 0.0, "left {l}, right {r}");
        e.close();
    }

    /// Notes in a clip, played live through the device's callback, are the export, bit for bit:
    /// each note starts and ends on its exact sample whatever the sizes of the device's requests.
    #[test]
    fn clip_notes_play_as_the_export_writes_them() {
        let mut e = band();
        let dir = std::env::temp_dir().join(format!("crucible-play-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(&mut e, "clip.add Keys 0.25 2; note.add 1 60 0 0.5; note.add 1 64 0.25 0.5 --velocity 64; note.add 1 67 1 0.75; track.gain Keys -3");
        let want = export(&mut e, &dir, "notes.wav");
        assert!(want.len() / 2 > 48_000 + 54_000, "the export includes the time the instrument rings out: {} frames", want.len() / 2);
        let first = want.iter().position(|x| *x != 0.0).unwrap() / 2;
        assert_eq!(first, 6_001, "the first note starts a quarter beat in (frame 6000; a sine's first sample is 0)");

        let s = Session::start(e.project(), e.folder(), 0, &e.sources).unwrap();
        let mut d = Desk::new(&e);
        drop(d.stage.tape.put(Box::new(Tape { sh: Some(s.sh.clone()), next: None })));
        let got = pull(&s, &mut d.cb, want.len() / 2);
        if let Some(i) = (0..want.len()).find(|&i| got[i] != want[i]) {
            panic!("live differs from the export: first at sample {i} of {} ({} vs {})", want.len(), got[i], want[i]);
        }

        // Stop in the middle of a note: it is let go, not left ringing.
        s.seek(24_000 + 6_000 + 2_000); // inside the third note
        let t0 = Instant::now();
        while s.sh.skip_to.load(Relaxed) == 0 {
            assert!(t0.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
        (d.cb)(&mut [0.0f32; 2]);
        // A jump lands inside the note: it does not sound (notes start at their start), and the
        // next one does.
        assert_eq!(rms(&pull(&s, &mut d.cb, 2_000)), 0.0);
        s.seek(6_000);
        let t0 = Instant::now();
        while s.sh.seek_frame.load(Relaxed) != 6_000 {
            assert!(t0.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(1));
        }
        (d.cb)(&mut [0.0f32; 2]);
        assert!(rms(&pull(&s, &mut d.cb, 4_000)) > 0.05, "the first note did not sound after jumping back to it");
        drop(d.stage.tape.put(Box::new(Tape { sh: None, next: None })));
        d.hear(64);
        assert_eq!(rms(&d.hear(2_000)), 0.0, "a clip's note rang on after Stop");
        drop(s);
        e.close();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_instrument_keeps_its_settings_through_save_open_and_undo() {
        let mut e = band();
        let dir = std::env::temp_dir().join(format!("crucible-play-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let q = |p: PathBuf| format!("\"{}\"", p.display().to_string().replace('\\', "/"));
        // Half level: the one setting the test instrument has. Its saved form is those 8 bytes.
        let inst = rack::test_instance(&e, 1);
        let mut state = inst.state().unwrap();
        let at = state.windows(8).position(|w| w == 1.0f64.to_le_bytes()).expect("the level in the settings");
        state[at..at + 8].copy_from_slice(&0.5f64.to_le_bytes());
        inst.set_state(&state).unwrap();
        let level = |e: &mut Engine| {
            let mut d = Desk::new(e);
            run(e, "instrument.note Keys 69 --velocity 127");
            let x = rms(&d.hear(4_800));
            run(e, "instrument.note Keys 69 --off");
            d.hear(64);
            x
        };
        assert!((level(&mut e) - KEY / 2.0).abs() < 0.005);

        // Taken off and brought back by undo: the same sound, not the plugin's defaults.
        run(&mut e, "instrument.clear Keys");
        assert!(e.project().tracks[0].instrument.is_none());
        run(&mut e, "undo");
        assert!((level(&mut e) - KEY / 2.0).abs() < 0.005, "undo brought the plugin back without its settings");

        // Saved, closed, opened: the settings come from the project's folder.
        run(&mut e, &format!("project.save {}", q(dir.join("song"))));
        let file = e.project().tracks[0].instrument.as_ref().unwrap().state.clone().expect("the project names the settings file");
        assert_eq!(std::fs::read(dir.join("song").join(crucible_project::STATE_DIR).join(&file)).unwrap(), state);
        assert!(!e.is_dirty());
        run(&mut e, "project.new --discard");
        assert!(!e.rack.any_loaded(), "the old project's plugin is still loaded");
        run(&mut e, &format!("project.open {}", q(dir.join("song"))));
        assert!((level(&mut e) - KEY / 2.0).abs() < 0.005, "the settings did not come back with the project");

        // A machine without the plugin: the track says so, and a save elsewhere keeps the settings.
        let json = std::fs::read_to_string(dir.join("song/project.json")).unwrap();
        let id = e.project().tracks[0].instrument.as_ref().unwrap().class_id.clone();
        std::fs::write(dir.join("song/project.json"), json.replace(&id, "00000000000000000000000000000BAD")).unwrap();
        run(&mut e, &format!("project.new --discard; project.open {}", q(dir.join("song"))));
        let status = run(&mut e, "instrument.status");
        assert_eq!(status.data["instruments"][0]["state"], "missing", "{}", status.text);
        run(&mut e, &format!("project.save {}", q(dir.join("copy"))));
        assert_eq!(std::fs::read(dir.join("copy").join(crucible_project::STATE_DIR).join(&file)).unwrap(), state, "the missing plugin's settings were not kept");

        // A plugin that is not there at all is refused, and the track stays as it was.
        let err = e.execute(&["instrument.set".into(), "Keys".into(), "no such plugin".into()]).unwrap_err();
        assert_eq!(err.code, Code::PluginNotFound);
        assert_eq!(e.project().tracks[0].instrument.as_ref().unwrap().class_id, "00000000000000000000000000000BAD");
        e.close();
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
