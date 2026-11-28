//! Instruments (D-044): the plugins on instrument tracks, and the keyboards that play them.
//!
//! The project says which plugin a track has (`Track::instrument`, by class id). This module
//! keeps the loaded plugins in step with that after every command (`sync`), so undo, redo, open
//! and crash recovery need nothing special: they change the project and the plugins follow.
//!
//! Three kinds of thread meet here:
//!
//! - **The main thread** (the one that runs the program's windows) makes every plugin call that
//!   is not sound: load, settings, the plugin's own window, unload. VST3 requires it
//!   (`12-known-daw-problems.md` P3). Commands run on another thread in the window program, so
//!   they hand those calls over (`Rack::on_main`) and wait. The command-line program runs
//!   commands on its main thread and calls straight through.
//! - **The audio device's thread** makes the sound: `InstSource::fill`, called from the mixer's
//!   `render`. Keys reach it through a queue (`Keys`), notes in clips through a list built when
//!   the project changes. It never allocates, and never waits (see `crucible-plugin-host`'s
//!   `instance.rs` for the one bounded try it makes for the plugin).
//! - **MIDI input threads**, one per keyboard, put what is played into that queue.
//!
//! Which track the keyboards play is not part of the project: like the play position, it is
//! where the person is working now (`track.live`). Which keyboard and channel a track listens
//! to is (`track.input`).
//!
//! Portability: no OS code. Plugins come from `crucible-plugin-host`, keyboards from
//! `crucible-midi-io` (seams).

use crate::command::{Args, Kind, Reply, Spec, opt, req};
use crate::{Effect, Engine};
use crucible_diagnostics::{self as diag, Code, DawError};
use crucible_midi_io as midi;
use crucible_plugin_host::{self as host, catalog};
use crucible_project::{Content, Instrument, Project, STATE_DIR, Track, TrackKind};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// The largest block a plugin is asked for while playing live. The device's requests are cut
/// into pieces of this size.
pub(crate) const LIVE_BLOCK: usize = 1024;
/// How long an instrument is given to ring out after its last note when a whole song is exported.
const RELEASE_SECONDS: f64 = 2.0;
/// Seconds a plugin gets to describe itself when the person points at its file.
const PROBE_SECONDS: u64 = 60;

/// How a front end lets the engine run something on the main thread.
pub type MainThread = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

/// Set when the program is closing, so nothing waits for a main thread that has stopped listening.
static CLOSING: AtomicBool = AtomicBool::new(false);

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "plugin.list",
        args: &[opt("all", Kind::Bool, "effects and everything else too, not only instruments")],
        help: "The plugins found on this machine at the last scan.",
        effect: Effect::Read,
        run: plugin_list,
    },
    Spec {
        name: "plugin.scan",
        args: &[opt("all", Kind::Bool, "ask every plugin again, not only new and changed ones"), opt("timeout", Kind::Int, "seconds allowed per plugin (default 60)")],
        help: "Look for VST3 plugins in the system's plugin folders and ask each new one what it is.",
        effect: Effect::System,
        run: plugin_scan,
    },
    Spec {
        name: "plugin.folder",
        args: &[req("folder", Kind::Text, "a folder that holds plugins, or one plugin's .vst3")],
        help: "Also look for plugins in this folder, now and at every later scan.",
        effect: Effect::System,
        run: plugin_folder,
    },
    Spec {
        name: "instrument.set",
        args: &[crate::TRACK, req("plugin", Kind::Text, "a plugin's name or part of it, its class id, or the path of a .vst3")],
        help: "Put an instrument plugin on an instrument track and let the keyboards play it.",
        effect: Effect::Edit,
        run: instrument_set,
    },
    Spec { name: "instrument.clear", args: &[crate::TRACK], help: "Take the instrument off a track.", effect: Effect::Edit, run: instrument_clear },
    Spec { name: "instrument.show", args: &[crate::TRACK], help: "Open the instrument's own window.", effect: Effect::System, run: instrument_show },
    Spec { name: "instrument.hide", args: &[crate::TRACK], help: "Close the instrument's own window.", effect: Effect::System, run: instrument_hide },
    Spec {
        name: "instrument.status",
        args: &[],
        help: "Each instrument: loaded or not and why, its window, its level; the keyboards connected and what they sent.",
        effect: Effect::Read,
        run: instrument_status,
    },
    Spec {
        name: "instrument.note",
        args: &[crate::TRACK, req("pitch", Kind::Int, "MIDI note number, 0 to 127 (60 is middle C)"), opt("velocity", Kind::Int, "1 to 127 (default 100)"), opt("off", Kind::Bool, "release the note instead")],
        help: "Press or release one key on a track's instrument, as a keyboard would.",
        effect: Effect::System,
        run: instrument_note,
    },
    Spec {
        name: "track.live",
        args: &[crate::TRACK, opt("state", Kind::Choice(crate::ON_OFF), "on or off (default: on)"), opt("only", Kind::Bool, "and take the keyboards off every other track")],
        help: "Choose whether the MIDI keyboards play this track's instrument.",
        effect: Effect::System,
        run: track_live,
    },
    Spec {
        name: "track.input",
        args: &[crate::TRACK, req("keyboard", Kind::Text, "'all', or part of one keyboard's name"), opt("channel", Kind::Int, "MIDI channel 1 to 16, or 0 for all (default: as it is)")],
        help: "Choose which MIDI keyboard and channel a track listens to.",
        effect: Effect::Edit,
        run: track_input,
    },
];

// ---------------------------------------------------------------- keys on their way to the sound

/// One MIDI message on its way to the instruments. `track` 0 goes to every track the keyboards
/// play; another number goes to that track alone (`instrument.note`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Key {
    pub track: u32,
    /// Which keyboard it came from: its place in this session's list.
    pub source: u16,
    pub midi: host::Midi,
}

/// The queue between the MIDI threads and commands (many writers, one at a time) and the audio
/// device's thread (the one reader, which never waits).
pub(crate) struct Keys {
    buf: Box<[AtomicU64]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    write: Mutex<()>,
}

impl Keys {
    fn new() -> Keys {
        Keys { buf: (0..1024).map(|_| AtomicU64::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0), write: Mutex::new(()) }
    }

    /// False when the queue is full (nothing is reading: no audio device is open).
    pub(crate) fn push(&self, k: Key) -> bool {
        let _one_writer = self.write.lock().unwrap_or_else(|p| p.into_inner());
        let h = self.head.load(Relaxed);
        if h - self.tail.load(Relaxed) >= self.buf.len() {
            return false;
        }
        let packed = (u64::from(k.track & 0xFF_FFFF) << 40) | (u64::from(k.source) << 24) | (u64::from(k.midi.status) << 16) | (u64::from(k.midi.a) << 8) | u64::from(k.midi.b);
        self.buf[h % self.buf.len()].store(packed, Relaxed);
        self.head.store(h + 1, std::sync::atomic::Ordering::Release);
        true
    }

    /// The audio thread only.
    pub(crate) fn pop(&self) -> Option<Key> {
        let t = self.tail.load(Relaxed);
        if t == self.head.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let v = self.buf[t % self.buf.len()].load(Relaxed);
        self.tail.store(t + 1, Relaxed);
        Some(Key { track: (v >> 40) as u32, source: (v >> 24) as u16, midi: host::Midi { offset: 0, status: (v >> 16) as u8, a: (v >> 8) as u8, b: v as u8 } })
    }
}

/// A track's level as the audio thread last saw it, for the meters.
pub(crate) struct Meter {
    pub track: u32,
    pub peak: [AtomicU32; 2],
}

// ---------------------------------------------------------------- one instrument in the mix

/// Which keys reach a track.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Route {
    /// The keyboards play this track.
    live: bool,
    /// Only this keyboard, by its place in the session's list. `u16::MAX`: a named keyboard
    /// that is not connected, so none.
    source: Option<u16>,
    /// 0 is every channel, else 1 to 16.
    channel: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Ev {
    frame: u64,
    pitch: u8,
    /// 0 releases the key.
    velocity: u8,
}

/// A track's instrument as the mixer uses it: the plugin's audio half, the notes of the track's
/// clips as a list in time order, and what is sounding now.
pub(crate) struct InstSource {
    proc: host::Processor,
    track: u32,
    route: Route,
    events: Vec<Ev>,
    /// Changes when the notes do, so a mixer that takes over knows whether its list is the same.
    events_id: u64,
    cursor: usize,
    /// The timeline frame expected next while the song runs. Anything else is a jump.
    next: u64,
    rolling: bool,
    /// Keys held down right now, as bits: by the clips, and by the keyboards.
    clip_on: u128,
    key_on: u128,
    /// Let go of the clips' notes, or the keyboards', at the next block.
    drop_clip: bool,
    drop_keys: bool,
    midi: Vec<host::Midi>,
    /// An export waits for the plugin; live playing never does.
    wait: bool,
}

const MIDI_ROOM: usize = 2048;

impl InstSource {
    fn release(midi: &mut Vec<host::Midi>, held: &mut u128) {
        while *held != 0 {
            let pitch = held.trailing_zeros() as u8;
            *held &= !(1u128 << pitch);
            if midi.len() < MIDI_ROOM {
                midi.push(host::Midi::note_off(0, 0, pitch));
            }
        }
    }

    /// Make this block of the instrument's sound. `start` is the timeline frame of its first
    /// sample while the song runs (`rolling`); `keys` is what was played since the last block.
    pub(crate) fn fill(&mut self, start: u64, rolling: bool, keys: &[Key], l: &mut [f32], r: &mut [f32]) {
        let n = l.len() as u64;
        self.midi.clear();
        let jumped = rolling && (!self.rolling || start != self.next);
        if self.drop_clip || jumped || (self.rolling && !rolling) {
            Self::release(&mut self.midi, &mut self.clip_on);
        }
        if self.drop_keys {
            Self::release(&mut self.midi, &mut self.key_on);
        }
        (self.drop_clip, self.drop_keys) = (false, false);
        for k in keys {
            let mine = if k.track != 0 { k.track == self.track } else { self.route.live && self.route.source.is_none_or(|s| s == k.source) && (self.route.channel == 0 || self.route.channel == k.midi.channel() + 1) };
            if !mine {
                continue;
            }
            let bit = 1u128 << (k.midi.a & 127);
            match k.midi.status & 0xF0 {
                0x90 if k.midi.b > 0 => self.key_on |= bit,
                0x80 | 0x90 => self.key_on &= !bit,
                _ => {}
            }
            // A release is never the one to be left out.
            if self.midi.len() < MIDI_ROOM - 256 || k.midi.status & 0xF0 == 0x80 {
                self.midi.push(k.midi);
            }
        }
        if rolling {
            if jumped {
                self.cursor = self.events.partition_point(|e| e.frame < start);
            }
            while let Some(e) = self.events.get(self.cursor).filter(|e| e.frame < start + n) {
                let (offset, bit) = ((e.frame.max(start) - start) as u32, 1u128 << e.pitch);
                if e.velocity > 0 {
                    self.clip_on |= bit;
                    if self.midi.len() < MIDI_ROOM - 256 {
                        self.midi.push(host::Midi::note_on(offset, 0, e.pitch, e.velocity));
                    }
                } else {
                    self.clip_on &= !bit;
                    if self.midi.len() < MIDI_ROOM {
                        self.midi.push(host::Midi::note_off(offset, 0, e.pitch));
                    }
                }
                self.cursor += 1;
            }
            self.next = start + n;
        }
        self.rolling = rolling;
        self.proc.process(&self.midi, l, r, self.wait);
    }

    /// Take over from the same track's instrument in the mixer this one replaces: what is
    /// sounding carries on. Notes whose clips changed are let go, as are held keys the track no
    /// longer listens to. No allocation.
    pub(crate) fn adopt(&mut self, old: &InstSource) {
        (self.clip_on, self.key_on, self.rolling, self.next) = (old.clip_on, old.key_on, old.rolling, old.next);
        if self.events_id == old.events_id {
            self.cursor = old.cursor;
        } else {
            (self.drop_clip, self.next) = (true, u64::MAX);
        }
        self.drop_keys = self.route != old.route;
    }

    pub(crate) fn track(&self) -> u32 {
        self.track
    }

    /// The frame just after the instrument's last note in the clips has rung out.
    pub(crate) fn end(&self, rate: u32) -> u64 {
        self.events.last().map_or(0, |e| e.frame + (RELEASE_SECONDS * f64::from(rate)) as u64)
    }
}

/// A track's MIDI clips as one list of key presses and releases in time order. A note that
/// starts before its clip's left edge does not sound; one that runs past the right edge is cut
/// there. At the same instant releases come first, so a repeated note is struck again.
fn events(p: &Project, track: u32) -> (Vec<Ev>, u64) {
    let fpt = p.frames_per_tick();
    let frame = |tick: i64| (tick.max(0) as f64 * fpt).round() as u64;
    let mut out = Vec::new();
    for c in p.clips.iter().filter(|c| c.track == track) {
        let Content::Midi { notes } = &c.content else { continue };
        for n in notes.iter().filter(|n| n.start >= c.offset && n.start < c.offset + c.length) {
            let on = c.start + (n.start - c.offset);
            let off = (on + n.length).min(c.end());
            out.push(Ev { frame: frame(on), pitch: n.pitch & 127, velocity: n.velocity.clamp(1, 127) });
            out.push(Ev { frame: frame(off).max(frame(on) + 1), pitch: n.pitch & 127, velocity: 0 });
        }
    }
    out.sort_by_key(|e| (e.frame, e.velocity > 0, e.pitch));
    // FNV-1a over the list: the same notes give the same number.
    let id = out.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, e| (h ^ (e.frame << 16 | u64::from(e.pitch) << 8 | u64::from(e.velocity))).wrapping_mul(0x0100_0000_01b3));
    (out, id)
}

// ---------------------------------------------------------------- the keyboards

/// The MIDI inputs of this session. A keyboard keeps its place in `names` for as long as the
/// program runs, also while it is unplugged, so the audio thread can tell them apart by number.
#[derive(Default)]
struct MidiIn {
    names: Vec<String>,
    counts: Vec<Arc<AtomicU64>>,
    /// Keyboards whose first message is in the log.
    heard: Vec<bool>,
    open: Vec<(usize, midi::Connection)>,
    /// Keyboards that would not open, with why (usually: another program has it).
    refused: Vec<(String, String)>,
    error: Option<String>,
    looked: Option<Instant>,
}

impl MidiIn {
    /// Open every keyboard that is connected and not open yet; let go of the ones that left.
    fn refresh(&mut self, keys: &Arc<Keys>) {
        self.looked = Some(Instant::now());
        let report = midi::enumerate_inputs();
        self.error = report.error;
        self.open.retain(|(i, c)| {
            let still = report.inputs.iter().any(|p| p.id == c.id);
            if !still {
                diag::event("MIDI", format!("keyboard disconnected: {}", self.names[*i]));
            }
            still
        });
        self.refused.retain(|(name, _)| report.inputs.iter().any(|p| p.name == *name));
        for p in &report.inputs {
            if self.open.iter().any(|(_, c)| c.id == p.id) || self.refused.iter().any(|(n, _)| *n == p.name) {
                continue;
            }
            let i = self.names.iter().position(|n| *n == p.name).unwrap_or_else(|| {
                self.names.push(p.name.clone());
                self.counts.push(Arc::new(AtomicU64::new(0)));
                self.heard.push(false);
                self.names.len() - 1
            });
            let (keys, count) = (keys.clone(), self.counts[i].clone());
            let opened = midi::connect(&p.id, move |bytes| {
                count.fetch_add(1, Relaxed);
                keys.push(Key { track: 0, source: i as u16, midi: host::Midi { offset: 0, status: bytes[0], a: bytes.get(1).copied().unwrap_or(0), b: bytes.get(2).copied().unwrap_or(0) } });
            });
            match opened {
                Ok(c) => {
                    diag::event("MIDI", format!("keyboard connected: {}", p.name));
                    self.open.push((i, c));
                }
                Err(e) => {
                    diag::warn(e.code, e.message.clone());
                    self.refused.push((p.name.clone(), e.to_string()));
                }
            }
        }
    }

    fn close(&mut self) {
        if !self.open.is_empty() {
            diag::event("MIDI", format!("{} keyboard(s) let go", self.open.len()));
        }
        self.open.clear();
        self.refused.clear();
        self.looked = None;
    }

    /// The place of the keyboard whose name contains `text`, among those open now.
    fn index_of(&self, text: &str) -> u16 {
        let t = text.to_lowercase();
        self.open.iter().map(|(i, _)| *i).find(|i| self.names[*i].to_lowercase().contains(&t)).map_or(u16::MAX, |i| i as u16)
    }
}

// ---------------------------------------------------------------- the rack

enum What {
    Loaded(host::Instance),
    /// Not installed on this machine. Its saved settings are kept as they are.
    Missing(String),
    /// Installed, but it would not load.
    Failed(String),
}

struct Slot {
    class_id: String,
    rate: u32,
    what: What,
    /// Blocks lost (the plugin was busy, the plugin failed) as last put in the log.
    told: (u64, u64),
}

pub(crate) struct Rack {
    post: Option<(MainThread, ThreadId)>,
    pub(crate) probe: Option<host::ProbeCommand>,
    slots: HashMap<u32, Slot>,
    /// Settings of plugins that were taken down this session, by track and class id, so undoing
    /// the removal brings the sound back as it was.
    attic: HashMap<(u32, String), Vec<u8>>,
    /// The tracks the keyboards play.
    live: Vec<u32>,
    midi: MidiIn,
    pub(crate) keys: Arc<Keys>,
    /// The program's main window (its native handle): plugins' windows float above it.
    pub(crate) owner: Option<isize>,
    /// The loaded plugins, for the main thread's regular look-in (`PluginService`).
    serviced: Arc<Mutex<Vec<host::Instance>>>,
    /// Why the instruments cannot be heard, when the audio device would not open.
    pub(crate) silent: Option<String>,
}

/// What a front end calls from its main thread a few times a second, without the engine: keeps
/// the plugins' windows alive and notices settings changed in them.
#[derive(Clone)]
pub struct PluginService(Arc<Mutex<Vec<host::Instance>>>);

impl PluginService {
    pub fn tick(&self) {
        let all = self.0.lock().unwrap_or_else(|p| p.into_inner()).clone();
        for i in &all {
            i.service();
        }
        host::instance::bury();
    }
}

impl Rack {
    pub(crate) fn new() -> Rack {
        Rack { post: None, probe: None, slots: HashMap::new(), attic: HashMap::new(), live: Vec::new(), midi: MidiIn::default(), keys: Arc::new(Keys::new()), owner: None, serviced: Arc::default(), silent: None }
    }

    pub(crate) fn set_main_thread(&mut self, post: MainThread) {
        self.post = Some((post, std::thread::current().id()));
    }

    pub(crate) fn service(&self) -> PluginService {
        PluginService(self.serviced.clone())
    }

    /// Run `f` on the main thread and wait for its answer. Straight away when this is the main
    /// thread, or when no front end has said how to reach it (the command line, tests).
    fn on_main<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Result<R, DawError> {
        match &self.post {
            Some((post, main)) if std::thread::current().id() != *main => {
                let (tx, rx) = mpsc::channel();
                post(Box::new(move || {
                    let _ = tx.send(f());
                }));
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(r) => return Ok(r),
                        Err(mpsc::RecvTimeoutError::Timeout) if !CLOSING.load(Relaxed) => {}
                        Err(_) => return Err(DawError::new(Code::HostInitFailed, "the program is closing, so the plugin was left alone")),
                    }
                }
            }
            _ => Ok(f()),
        }
    }

    fn loaded(&self, track: u32) -> Option<&host::Instance> {
        match &self.slots.get(&track)?.what {
            What::Loaded(i) => Some(i),
            _ => None,
        }
    }

    pub(crate) fn any_loaded(&self) -> bool {
        self.slots.values().any(|s| matches!(s.what, What::Loaded(_)))
    }

    /// Whether a plugin's settings were changed from its own window since the last save.
    pub(crate) fn touched(&self) -> bool {
        self.slots.values().any(|s| matches!(&s.what, What::Loaded(i) if i.counters().touched.load(Relaxed)))
    }

    /// The instrument of track `t` for a mixer, if it is loaded. `export` makes one that waits
    /// for the plugin and takes no keys.
    pub(crate) fn source(&self, p: &Project, t: &Track, export: bool) -> Option<Box<InstSource>> {
        let inst = self.loaded(t.id)?;
        let (events, events_id) = events(p, t.id);
        let route = Route { live: !export && self.live.contains(&t.id), source: t.input.as_deref().map(|name| self.midi.index_of(name)), channel: t.input_channel };
        Some(Box::new(InstSource { proc: inst.processor(), track: t.id, route, events, events_id, cursor: 0, next: u64::MAX, rolling: false, clip_on: 0, key_on: 0, drop_clip: false, drop_keys: false, midi: Vec::with_capacity(MIDI_ROOM), wait: export }))
    }

    /// Tell every loaded plugin that an export begins or has ended.
    pub(crate) fn offline(&self, offline: bool, rate: u32) {
        for (track, inst) in self.slots.iter().filter_map(|(t, s)| match &s.what {
            What::Loaded(i) => Some((*t, i.clone())),
            _ => None,
        }) {
            match self.on_main(move || inst.set_offline(offline, LIVE_BLOCK, rate)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) | Err(e) => diag::warn(e.code, format!("track #{track}: {}", e.message)),
            }
        }
    }

    fn take_down(&mut self, track: u32) {
        let Some(slot) = self.slots.remove(&track) else { return };
        let What::Loaded(inst) = slot.what else { return };
        let i = inst.clone();
        // Its settings are kept for this session, so undoing the removal restores the sound.
        match self.on_main(move || {
            let state = i.state();
            i.unload();
            state
        }) {
            Ok(Ok(bytes)) => {
                self.attic.insert((track, slot.class_id.to_lowercase()), bytes);
            }
            Ok(Err(e)) | Err(e) => diag::warn(e.code, format!("{}: its settings could not be kept: {}", inst.info().name, e.message)),
        }
    }

    fn put_up(&mut self, track: u32, want: &Instrument, rate: u32, cat: &catalog::Catalog, saved: Option<PathBuf>) {
        let what = match cat.plugins.iter().find(|p| p.class_id.eq_ignore_ascii_case(&want.class_id)) {
            None => {
                let why = format!("{} by {} is not among the plugins found on this machine. Install it and scan again; its saved settings are kept.", want.name, want.vendor);
                diag::warn(Code::PluginNotFound, format!("track #{track}: {why}"));
                What::Missing(why)
            }
            Some(item) => {
                let (path, class) = (item.path.clone(), item.class_id.clone());
                match self.on_main(move || host::Instance::load(&path, Some(&class), rate, LIVE_BLOCK)) {
                    Ok(Ok(inst)) => {
                        let state = self.attic.remove(&(track, want.class_id.to_lowercase())).or_else(|| saved.and_then(|f| std::fs::read(&f).map_err(|e| diag::warn(Code::StateRestoreFailed, format!("{}: {e}", f.display()))).ok()));
                        if let Some(bytes) = state {
                            let i = inst.clone();
                            if let Ok(Err(e)) | Err(e) = self.on_main(move || i.set_state(&bytes)) {
                                diag::warn(e.code, e.message);
                            }
                        }
                        What::Loaded(inst)
                    }
                    Ok(Err(e)) | Err(e) => {
                        diag::error(e.code, format!("track #{track}: {}", e.message));
                        What::Failed(e.to_string())
                    }
                }
            }
        };
        self.slots.insert(track, Slot { class_id: want.class_id.clone(), rate, what, told: (0, 0) });
    }

    /// Another project took over: what was kept of the old one's plugins is of no use now.
    pub(crate) fn forget(&mut self) {
        self.attic.clear();
    }

    /// The program is ending: every plugin goes, its window with it.
    pub(crate) fn clear(&mut self, forget: bool) {
        let tracks: Vec<u32> = self.slots.keys().copied().collect();
        for t in tracks {
            self.take_down(t);
        }
        if forget {
            self.attic.clear();
            self.live.clear();
        }
        self.midi.close();
        self.serviced.lock().unwrap_or_else(|p| p.into_inner()).clear();
        let _ = self.on_main(host::instance::bury);
    }
}

/// The program is closing: nothing waits for the main thread from here on.
pub(crate) fn closing() {
    CLOSING.store(true, Relaxed);
}

fn state_file(e: &Engine, i: &Instrument) -> Option<PathBuf> {
    Some(e.work()?.join(STATE_DIR).join(i.state.as_ref()?))
}

/// Bring the loaded plugins in step with the project. Called after every command that is not a
/// read. Returns true when a plugin was loaded or taken down.
pub(crate) fn sync(e: &mut Engine) -> bool {
    let rate = e.project.sample_rate;
    let want: Vec<(u32, Instrument)> = e.project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument).filter_map(|t| Some((t.id, t.instrument.clone()?))).collect();
    let gone: Vec<u32> = e.rack.slots.iter().filter(|(id, s)| !want.iter().any(|(w, i)| w == *id && i.class_id.eq_ignore_ascii_case(&s.class_id) && s.rate == rate)).map(|(id, _)| *id).collect();
    let new: Vec<&(u32, Instrument)> = want.iter().filter(|(id, _)| gone.contains(id) || !e.rack.slots.contains_key(id)).collect();
    if gone.is_empty() && new.is_empty() {
        return false;
    }
    for id in gone {
        e.rack.take_down(id);
    }
    if !new.is_empty() {
        let cat = catalog::load();
        for (id, inst) in new {
            let saved = state_file(e, inst);
            e.rack.put_up(*id, inst, rate, &cat, saved);
        }
    }
    let r = &mut e.rack;
    r.live.retain(|t| matches!(r.slots.get(t), Some(Slot { what: What::Loaded(_), .. })));
    if r.live.is_empty()
        && let Some(first) = e.project.tracks.iter().find(|t| r.loaded(t.id).is_some())
    {
        r.live.push(first.id);
    }
    *r.serviced.lock().unwrap_or_else(|p| p.into_inner()) = r.slots.values().filter_map(|s| match &s.what {
        What::Loaded(i) => Some(i.clone()),
        _ => None,
    }).collect();
    if r.any_loaded() {
        r.midi.refresh(&r.keys);
    } else {
        r.midi.close();
    }
    let _ = r.on_main(host::instance::bury);
    true
}

/// Write every loaded plugin's settings into `target` and note the file in the project, for
/// `project.save`. A plugin that is not loaded keeps the file it had; saving somewhere new takes
/// that file along. Returns what could not be saved, in words.
pub(crate) fn save_states(e: &mut Engine, target: &Path, from: Option<&Path>) -> Vec<String> {
    let mut problems = Vec::new();
    let dir = target.join(STATE_DIR);
    for i in 0..e.project.tracks.len() {
        let (id, Some(inst)) = (e.project.tracks[i].id, e.project.tracks[i].instrument.clone()) else { continue };
        match e.rack.loaded(id).cloned() {
            Some(loaded) => {
                let l = loaded.clone();
                let written = e.rack.on_main(move || l.state()).and_then(|r| r).and_then(|bytes| {
                    let file = format!("track-{id}.vststate");
                    let fail = |err: std::io::Error| DawError::new(Code::StateSaveFailed, format!("{}: {err}", dir.join(&file).display()));
                    std::fs::create_dir_all(&dir).map_err(fail)?;
                    let tmp = dir.join(format!("{file}.part"));
                    std::fs::write(&tmp, &bytes).map_err(fail)?;
                    std::fs::rename(&tmp, dir.join(&file)).map_err(fail)?;
                    Ok(file)
                });
                match written {
                    Ok(file) => {
                        if let Some(t) = e.project.tracks[i].instrument.as_mut() {
                            t.state = Some(file);
                        }
                        loaded.counters().touched.store(false, Relaxed);
                    }
                    Err(err) => {
                        diag::error(err.code, err.message.clone());
                        problems.push(format!("{}'s settings were not saved: {err}", inst.name));
                    }
                }
            }
            None => {
                if let (Some(file), Some(from)) = (&inst.state, from.filter(|f| *f != target))
                    && from.join(STATE_DIR).join(file).is_file()
                    && let Err(err) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(from.join(STATE_DIR).join(file), dir.join(file)))
                {
                    problems.push(format!("{}'s saved settings could not be copied: {err}", inst.name));
                }
            }
        }
    }
    problems
}

// ---------------------------------------------------------------- commands

fn item_json(p: &catalog::Item) -> serde_json::Value {
    json!({ "name": p.name, "vendor": p.vendor, "version": p.version, "category": p.category, "class_id": p.class_id, "instrument": p.is_instrument, "editor": p.has_editor, "path": p.path })
}

fn list_reply(cat: &catalog::Catalog, all: bool, head: String) -> Reply {
    let shown: Vec<&catalog::Item> = cat.plugins.iter().filter(|p| all || p.is_instrument).collect();
    let mut text = head;
    for p in &shown {
        text.push_str(&format!("\n  {} by {} v{}{}", p.name, p.vendor, p.version, if all && !p.category.is_empty() { format!(" ({})", p.category) } else { String::new() }));
    }
    if shown.is_empty() {
        text.push_str(if cat.scanned.is_none() { "\n  not scanned yet: plugin.scan" } else { "\n  none" });
    }
    for f in &cat.failed {
        text.push_str(&format!("\n  could not be read: {} ({})", f.path.display(), f.why));
    }
    Reply::new(
        text,
        json!({
            "plugins": shown.iter().map(|p| item_json(p)).collect::<Vec<_>>(), "others": cat.plugins.len() - cat.plugins.iter().filter(|p| p.is_instrument).count(),
            "failed": cat.failed.iter().map(|f| json!({ "path": f.path, "why": f.why })).collect::<Vec<_>>(),
            "folders": cat.folders.iter().map(|(f, there)| json!({ "folder": f, "exists": there })).collect::<Vec<_>>(), "scanned": cat.scanned,
        }),
    )
}

fn plugin_list(_: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let all = a.flag("all") == Some(true);
    Ok(list_reply(&catalog::load(), all, if all { "Plugins on this machine".to_string() } else { "Instruments on this machine".to_string() }))
}

/// `plugin.scan`, without an engine: a front end may run it while the engine does other things.
pub fn scan(probe: &host::ProbeCommand, all: bool, timeout: Option<u64>) -> Result<Reply, DawError> {
    let (cat, done) = catalog::scan(probe, Duration::from_secs(timeout.unwrap_or(PROBE_SECONDS).max(1)), all, &mut |i, n, b| diag::info(format!("plugin scan: {} of {n}: asking {}", i + 1, b.display())))?;
    let head = format!(
        "Looked in {}: {} plugin file(s), {} asked, {} already known, {} could not be read, in {:.1} s. Instruments:",
        cat.folders.iter().map(|(f, there)| format!("{}{}", f.display(), if *there { "" } else { " (not there)" })).collect::<Vec<_>>().join("; "),
        done.bundles,
        done.asked,
        done.kept,
        done.failed,
        done.seconds
    );
    let mut r = list_reply(&cat, false, head).logged();
    r.data["scan"] = json!(done);
    Ok(r)
}

/// `plugin.folder`, without an engine, like `scan`.
pub fn add_folder(probe: &host::ProbeCommand, folder: &Path) -> Result<Reply, DawError> {
    catalog::add_place(&bundle_of(folder))?;
    scan(probe, false, None)
}

fn plugin_folder(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    add_folder(e.rack.probe.as_ref().ok_or_else(no_probe)?, Path::new(a.need_text("folder")?))
}

/// A path inside a plugin's bundle (people point at the file deep inside it) is the bundle.
fn bundle_of(path: &Path) -> PathBuf {
    let is_bundle = |p: &&Path| p.is_dir() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vst3"));
    path.ancestors().find(is_bundle).unwrap_or(path).to_path_buf()
}

fn no_probe() -> DawError {
    DawError::new(Code::HostInitFailed, "this program was not told how to ask a plugin what it is")
}

fn plugin_scan(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    scan(e.rack.probe.as_ref().ok_or_else(no_probe)?, a.flag("all") == Some(true), a.int("timeout").map(|t| t.max(1) as u64))
}

fn instrument_track(e: &Engine, a: &Args) -> Result<usize, DawError> {
    let i = e.track_at(a, "track")?;
    let t = &e.project.tracks[i];
    if t.kind != TrackKind::Instrument {
        return Err(DawError::new(Code::BadArgument, format!("'{}' is an audio track; instruments go on instrument tracks (track.add --kind instrument)", t.name)));
    }
    Ok(i)
}

fn instrument_set(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = instrument_track(e, a)?;
    let what = a.need_text("plugin")?;
    let path = &bundle_of(Path::new(what));
    let item = if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("vst3")) && path.exists() {
        // A file the person pointed at (Browse): ask it what it is, and remember where it is.
        let found = catalog::add(path, e.rack.probe.as_ref().ok_or_else(no_probe)?, Duration::from_secs(PROBE_SECONDS))?;
        found.iter().find(|p| p.is_instrument).or(found.first()).cloned().ok_or_else(|| DawError::new(Code::PluginLoadFailed, format!("{}: no plugin in it", path.display())))?
    } else {
        catalog::load().find(what)?.clone()
    };
    let id = e.project.tracks[i].id;
    // The settings file on record belongs to the plugin that was there before.
    let state = e.project.tracks[i].instrument.as_ref().filter(|old| old.class_id.eq_ignore_ascii_case(&item.class_id)).and_then(|old| old.state.clone());
    e.project.tracks[i].instrument = Some(Instrument { class_id: item.class_id.clone(), name: item.name.clone(), vendor: item.vendor.clone(), version: item.version.clone(), state });
    sync(e);
    match e.rack.slots.get(&id).map(|s| &s.what) {
        Some(What::Loaded(inst)) => {
            let info = inst.info().clone();
            // The new instrument is the one the keyboards play.
            e.rack.live = vec![id];
            let mut text = format!("{} by {} is on '{}' (loaded in {:.1} s). The keyboards play it now.", info.name, info.vendor, e.project.tracks[i].name, info.load_seconds);
            if !item.is_instrument {
                text.push_str("\nNote: the plugin does not call itself an instrument; it may make no sound from notes.");
            }
            Ok(Reply::new(text, json!({ "track": id, "plugin": info })).logged())
        }
        // `run` puts the project back as it was, and the plugins follow it.
        Some(What::Failed(why)) => Err(DawError::new(Code::PluginLoadFailed, why.clone())),
        _ => Err(DawError::new(Code::PluginNotFound, format!("{} is not installed", item.name))),
    }
}

fn instrument_clear(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = instrument_track(e, a)?;
    let t = &mut e.project.tracks[i];
    match t.instrument.take() {
        Some(old) => Ok(Reply::text(format!("{} taken off '{}'", old.name, t.name))),
        None => Ok(Reply::text(format!("'{}' has no instrument", t.name))),
    }
}

fn need_loaded(e: &Engine, a: &Args) -> Result<(usize, host::Instance), DawError> {
    let i = instrument_track(e, a)?;
    let t = &e.project.tracks[i];
    match e.rack.slots.get(&t.id).map(|s| &s.what) {
        Some(What::Loaded(inst)) => Ok((i, inst.clone())),
        Some(What::Missing(why) | What::Failed(why)) => Err(DawError::new(Code::PluginLoadFailed, why.clone())),
        None => Err(DawError::new(Code::BadArgument, format!("'{}' has no instrument yet (instrument.set)", t.name))),
    }
}

fn instrument_show(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (i, inst) = need_loaded(e, a)?;
    if !inst.info().has_editor {
        return Err(DawError::new(Code::EditorFailed, format!("{} has no window of its own", inst.info().name)));
    }
    if e.rack.post.is_none() && !cfg!(test) {
        return Err(DawError::new(Code::EditorFailed, "a plugin's window needs Crucible's own window; this is the command-line program"));
    }
    let (shown, owner) = (inst.clone(), e.rack.owner);
    e.rack.on_main(move || shown.show(owner))??;
    Ok(Reply::new(format!("{}'s window is open ('{}')", inst.info().name, e.project.tracks[i].name), json!({ "track": e.project.tracks[i].id, "editor": true })).logged())
}

fn instrument_hide(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (i, inst) = need_loaded(e, a)?;
    let hidden = inst.clone();
    e.rack.on_main(move || hidden.hide())?;
    Ok(Reply::new(format!("{}'s window is closed", inst.info().name), json!({ "track": e.project.tracks[i].id, "editor": false })))
}

fn db(bits: u32) -> Option<f64> {
    let x = f32::from_bits(bits);
    (x > 0.0).then(|| 20.0 * f64::from(x).log10())
}

fn instrument_status(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    // Keyboards plugged in or pulled out are noticed here: the window asks a few times a second.
    if e.rack.any_loaded() && e.rack.midi.looked.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
        let before: Vec<usize> = e.rack.midi.open.iter().map(|(i, _)| *i).collect();
        e.rack.midi.refresh(&e.rack.keys);
        if before != e.rack.midi.open.iter().map(|(i, _)| *i).collect::<Vec<_>>() {
            crate::play::live_update(e); // a track that listens to one keyboard by name
        }
    }
    // What happened behind the scenes goes into the log when it is first seen (D-037): a
    // keyboard's first message, and blocks of an instrument's sound that were lost.
    for i in 0..e.rack.midi.names.len() {
        if !e.rack.midi.heard[i] && e.rack.midi.counts[i].load(Relaxed) > 0 {
            e.rack.midi.heard[i] = true;
            diag::event("MIDI", format!("first message from {}", e.rack.midi.names[i]));
        }
    }
    for (track, s) in e.rack.slots.iter_mut() {
        let What::Loaded(i) = &s.what else { continue };
        let (c, name) = (i.counters(), &i.info().name);
        let now = (c.busy.load(Relaxed), c.failed.load(Relaxed));
        if now != s.told {
            diag::event("PLUGIN", format!("{name} on track #{track}: {} block(s) of silence because the plugin was busy with its settings or window, {} because it failed, of {} so far", now.0 - s.told.0, now.1 - s.told.1, c.blocks.load(Relaxed)));
            s.told = now;
        }
    }
    let shown: Vec<(u32, host::Instance)> = e.rack.slots.iter().filter_map(|(t, s)| match &s.what {
        What::Loaded(i) => Some((*t, i.clone())),
        _ => None,
    }).collect();
    let open: HashMap<u32, bool> = e.rack.on_main(move || shown.iter().map(|(t, i)| (*t, i.shown())).collect()).unwrap_or_default();
    let crate::play::LiveLevels { tracks: meters, master, output } = crate::play::live_levels(e);
    let mut text = String::new();
    let mut list = Vec::new();
    for t in e.project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument) {
        let live = e.rack.live.contains(&t.id);
        let peak = meters.iter().find(|m| m.0 == t.id).map(|m| [db(m.1), db(m.2)]);
        let (state, why, extra) = match (e.rack.slots.get(&t.id).map(|s| &s.what), &t.instrument) {
            (Some(What::Loaded(i)), _) => {
                let c = i.counters();
                ("loaded", None, json!({ "plugin": i.info(), "editor": open.get(&t.id).copied().unwrap_or(false), "busy_blocks": c.busy.load(Relaxed), "failed_blocks": c.failed.load(Relaxed), "lost_midi": c.lost_midi.load(Relaxed), "blocks": c.blocks.load(Relaxed), "unsaved": c.touched.load(Relaxed) }))
            }
            (Some(What::Missing(w)), _) => ("missing", Some(w.clone()), json!({})),
            (Some(What::Failed(w)), _) => ("failed", Some(w.clone()), json!({})),
            (None, _) => ("none", None, json!({})),
        };
        text.push_str(&format!("{}: {}{}{}\n", t.name, t.instrument.as_ref().map_or("no instrument".to_string(), |i| format!("{} by {}", i.name, i.vendor)), match state { "loaded" => if live { ", played by the keyboards" } else { "" }, "missing" => ", NOT INSTALLED", "failed" => ", DID NOT LOAD", _ => "" }, why.as_ref().map(|w| format!(" ({w})")).unwrap_or_default()));
        let mut j = json!({ "track": t.id, "state": state, "why": why, "live": live, "peak": peak, "input": t.input, "channel": t.input_channel });
        if let (Some(a), Some(b)) = (j.as_object_mut(), extra.as_object()) {
            a.extend(b.clone());
        }
        list.push(j);
    }
    let m = &e.rack.midi;
    let keyboards: Vec<_> = m.open.iter().map(|(i, _)| json!({ "name": m.names[*i], "messages": m.counts[*i].load(Relaxed) })).collect();
    text.push_str(&match (m.open.len(), e.rack.any_loaded()) {
        (0, false) => "Keyboards: not opened (no instrument is loaded)".to_string(),
        (0, true) => "Keyboards: none connected".to_string(),
        _ => format!("Keyboards: {}", m.open.iter().map(|(i, _)| format!("{} ({} messages)", m.names[*i], m.counts[*i].load(Relaxed))).collect::<Vec<_>>().join(", ")),
    });
    for (name, why) in &m.refused {
        text.push_str(&format!("\nCould not open {name}: {why}"));
    }
    if let Some(s) = &e.rack.silent {
        text.push_str(&format!("\nNo sound: {s}"));
    }
    Ok(Reply::new(
        text,
        json!({
            "instruments": list, "keyboards": keyboards, "refused": m.refused.iter().map(|(n, w)| json!({ "name": n, "why": w })).collect::<Vec<_>>(), "midi_error": m.error,
            "output": output, "silent": e.rack.silent, "peak": master.map(|p| [db(p[0]), db(p[1])]), "unsaved": e.rack.touched(),
        }),
    ))
}

fn instrument_note(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (i, _) = need_loaded(e, a)?;
    let pitch = a.need_int("pitch")?;
    let velocity = a.int("velocity").unwrap_or(100);
    if !(0..=127).contains(&pitch) || !(1..=127).contains(&velocity) {
        return Err(DawError::new(Code::ValueOutOfRange, "pitch is 0 to 127 and velocity 1 to 127"));
    }
    let off = a.flag("off") == Some(true);
    let midi = if off { host::Midi::note_off(0, 0, pitch as u8) } else { host::Midi::note_on(0, 0, pitch as u8, velocity as u8) };
    if !e.rack.keys.push(Key { track: e.project.tracks[i].id, source: 0, midi }) {
        return Err(DawError::new(Code::StreamStartFailed, e.rack.silent.clone().unwrap_or_else(|| "no audio device is taking notes".to_string())));
    }
    Ok(Reply::text(format!("Key {pitch} {} on '{}'", if off { "released" } else { "pressed" }, e.project.tracks[i].name)))
}

fn track_live(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = instrument_track(e, a)?;
    let (id, name) = (e.project.tracks[i].id, e.project.tracks[i].name.clone());
    let on = a.text("state") != Some("off");
    if a.flag("only") == Some(true) {
        e.rack.live.clear();
    }
    e.rack.live.retain(|t| *t != id);
    if on {
        e.rack.live.push(id);
    }
    crate::play::live_update(e);
    Ok(Reply::new(format!("The keyboards {} '{name}'", if on { "play" } else { "no longer play" }), json!({ "live": e.rack.live })))
}

fn track_input(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = instrument_track(e, a)?;
    let keyboard = a.need_text("keyboard")?.trim();
    let channel = a.int("channel");
    if channel.is_some_and(|c| !(0..=16).contains(&c)) {
        return Err(DawError::new(Code::ValueOutOfRange, "MIDI channels are 1 to 16; 0 means all of them"));
    }
    let t = &mut e.project.tracks[i];
    t.input = (!keyboard.eq_ignore_ascii_case("all") && !keyboard.is_empty()).then(|| keyboard.to_string());
    if let Some(c) = channel {
        t.input_channel = c as u8;
    }
    Ok(Reply::text(format!("'{}' listens to {}, {}", t.name, t.input.as_deref().unwrap_or("every keyboard"), if t.input_channel == 0 { "every channel".to_string() } else { format!("channel {}", t.input_channel) })))
}

/// For tests: make the test instrument (`crucible-test-synth`) the one plugin this process
/// knows, in a plugin list of its own, and return its name.
#[cfg(test)]
pub(crate) fn test_synth() -> &'static str {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("crucible-engine-synth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bundle = crucible_test_synth::bundle_into(&dir).expect("the test plugin");
        // SAFETY: set once, and every test that reads the plugin list comes through here first.
        unsafe { std::env::set_var(catalog::FILE_ENV, dir.join("plugins.json")) };
        catalog::add_probed(&bundle, host::probe_in_process(&bundle)).expect("the plugin list");
    });
    crucible_test_synth::NAME
}

/// For tests: the loaded plugin on a track, to reach its settings directly.
#[cfg(test)]
pub(crate) fn test_instance(e: &Engine, track: u32) -> host::Instance {
    e.rack.loaded(track).expect("an instrument is loaded on the track").clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keep_their_order_and_a_full_queue_refuses() {
        let q = Keys::new();
        for n in 0..1024u32 {
            assert!(q.push(Key { track: n % 7, source: (n % 5) as u16, midi: host::Midi::note_on(0, (n % 16) as u8, (n % 128) as u8, 100) }));
        }
        assert!(!q.push(Key { track: 0, source: 0, midi: host::Midi::default() }), "a full queue took another key");
        for n in 0..1024u32 {
            let k = q.pop().unwrap();
            assert_eq!((k.track, k.source, k.midi), (n % 7, (n % 5) as u16, host::Midi::note_on(0, (n % 16) as u8, (n % 128) as u8, 100)));
        }
        assert!(q.pop().is_none());
    }

    #[test]
    fn clip_notes_become_presses_and_releases_in_time_order() {
        let mut e = Engine::new();
        for line in ["track.add Keys --kind instrument", "clip.add Keys 1 4", "note.add 1 60 0 1", "note.add 1 64 1 1", "note.add 1 60 1 1", "note.add 1 67 3.5 2"] {
            for tokens in crate::command::split_statements(line).unwrap() {
                e.execute(&tokens).unwrap_or_else(|err| panic!("{line}: {err}"));
            }
        }
        let (ev, id) = events(e.project(), 1);
        let fpb = 24_000u64; // 120 BPM at 48 kHz
        let got: Vec<(u64, u8, bool)> = ev.iter().map(|x| (x.frame, x.pitch, x.velocity > 0)).collect();
        // The clip starts at beat 1. The last note runs past the clip's end (beat 5) and is cut there.
        assert_eq!(got, vec![(fpb, 60, true), (2 * fpb, 60, false), (2 * fpb, 60, true), (2 * fpb, 64, true), (3 * fpb, 60, false), (3 * fpb, 64, false), (fpb * 9 / 2, 67, true), (5 * fpb, 67, false)]);
        assert_eq!(events(e.project(), 1).1, id, "the same notes must give the same number");
    }
}
