//! One loaded plugin, in two halves that live on different threads.
//!
//! - [`Instance`] is the **control** half: load, saved settings ("state"), the editor window,
//!   unload. VST3 wants every one of those calls on one thread, the thread that also runs the
//!   program's windows (`12-known-daw-problems.md` P3). Whoever owns an `Instance` calls it from
//!   that thread only. The thread that loaded a plugin is its control thread.
//! - [`Processor`] is the **audio** half: notes in, sound out. It may live on the audio device's
//!   own thread.
//!
//! Both halves reach the same plugin through one lock (the host library, vst3-host, keeps a
//! plugin's audio and control sides in one object). The audio half never waits for it: it tries
//! for a short moment (`SPIN`), and if the control half still holds it, that block is silence and
//! is counted. The control half holds it only for the length of one call. Short calls (the
//! regular `service`) fit inside the moment the audio half is willing to try; long ones (reading
//! or loading a big instrument's settings, opening its window) cost that one instrument a gap.
//! D-044 records this as the price of the host library, and what would remove it.
//!
//! Portability: none of this is OS code. The editor's native window is vst3-host's.

use crucible_diagnostics::{self as diag, Code, DawError};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};
use vst3_host::{AudioBuffers, MidiChannel, MidiEvent, Plugin, PluginWindow, ProcessMode, Vst3Host};

/// How long the audio half keeps trying for the plugin before it gives the block up.
const SPIN: Duration = Duration::from_micros(150);

/// One MIDI message for a plugin: the three bytes a keyboard sends, and the sample inside the
/// block where it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Midi {
    pub offset: u32,
    pub status: u8,
    pub a: u8,
    pub b: u8,
}

impl Midi {
    pub fn note_on(offset: u32, channel: u8, pitch: u8, velocity: u8) -> Midi {
        Midi { offset, status: 0x90 | (channel & 15), a: pitch & 127, b: velocity & 127 }
    }
    pub fn note_off(offset: u32, channel: u8, pitch: u8) -> Midi {
        Midi { offset, status: 0x80 | (channel & 15), a: pitch & 127, b: 0 }
    }
    /// 0 to 15.
    pub fn channel(&self) -> u8 {
        self.status & 15
    }

    fn event(&self) -> Option<MidiEvent> {
        let channel = MidiChannel::from_index(self.channel())?;
        let (a, b) = (self.a & 127, self.b & 127);
        Some(match self.status & 0xF0 {
            0x90 if b > 0 => MidiEvent::NoteOn { channel, note: a, velocity: b },
            0x80 | 0x90 => MidiEvent::NoteOff { channel, note: a, velocity: b },
            0xA0 => MidiEvent::PolyAftertouch { channel, note: a, pressure: b },
            0xB0 => MidiEvent::ControlChange { channel, controller: a, value: b },
            0xD0 => MidiEvent::ChannelAftertouch { channel, pressure: a },
            0xE0 => MidiEvent::PitchBend { channel, value: u16::from(a) | (u16::from(b) << 7) },
            // Program changes have no VST3 event (they go through the plugin's own program
            // lists); everything else is not a channel message.
            _ => return None,
        })
    }
}

/// What a loaded plugin says about itself.
#[derive(Debug, Clone, Serialize)]
pub struct Loaded {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub class_id: String,
    pub path: PathBuf,
    pub has_editor: bool,
    pub load_seconds: f64,
}

/// Counted by the audio half, read by anyone.
#[derive(Default)]
pub struct Counters {
    /// Blocks given up because the control half held the plugin.
    pub busy: AtomicU64,
    /// Blocks the plugin itself failed.
    pub failed: AtomicU64,
    /// Blocks processed.
    pub blocks: AtomicU64,
    /// MIDI messages lost: more arrived while the plugin was busy than could be kept for it.
    pub lost_midi: AtomicU64,
    /// The plugin's settings were changed from its own window since this was last cleared.
    pub touched: AtomicBool,
    /// The plugin was taken down: its audio halves make silence.
    pub unloaded: AtomicBool,
}

type Shared = Arc<Mutex<Plugin>>;

thread_local! {
    /// Open editor windows, by instance. They hold native window handles, which belong to the
    /// thread that made them: the control thread.
    static WINDOWS: RefCell<HashMap<u64, PluginWindow>> = RefCell::new(HashMap::new());
    /// Plugins taken down while an audio half somewhere still holds them. A plugin must be
    /// destroyed on its control thread, so the last hold is kept here until it is the last.
    static LEAVING: RefCell<Vec<Shared>> = const { RefCell::new(Vec::new()) };
}

/// Destroy the plugins that were taken down and that no audio half holds any more. Call it now
/// and then from the control thread (the regular `service` round does).
pub fn bury() {
    LEAVING.with_borrow_mut(|all| all.retain(|p| Arc::strong_count(p) > 1));
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct Instance {
    id: u64,
    plugin: Shared,
    info: Arc<Loaded>,
    counters: Arc<Counters>,
    /// The largest block the plugin was told to expect.
    block: usize,
}

fn lock(p: &Shared) -> MutexGuard<'_, Plugin> {
    p.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Instance {
    /// Load a plugin from its bundle, ready to make sound at `rate` in blocks of up to `block`
    /// frames. `class_id` picks one plugin out of a bundle that holds several.
    pub fn load(bundle: &Path, class_id: Option<&str>, rate: u32, block: usize) -> Result<Instance, DawError> {
        let t0 = Instant::now();
        let fail = |what: &str, e: vst3_host::Error| DawError::new(Code::PluginLoadFailed, format!("{}: {what}: {e}", bundle.display()));
        let mut host = Vst3Host::builder().sample_rate(f64::from(rate)).block_size(block).build().map_err(|e| DawError::new(Code::HostInitFailed, e.to_string()))?;
        diag::info(format!("loading plugin {}{}", bundle.display(), class_id.map(|c| format!(" (class {c})")).unwrap_or_default()));
        // The host library reports each loading step through `log`; a plugin that crashes while
        // loading leaves the step it reached as the log's last words.
        diag::set_verbose(true);
        let loaded = match class_id {
            Some(c) => host.load_plugin_class(bundle, c),
            None => host.load_plugin(bundle),
        };
        diag::set_verbose(false);
        let mut plugin = loaded.map_err(|e| fail("it would not load", e))?;
        plugin.start_processing().map_err(|e| fail("it would not start", e))?;
        let i = plugin.info();
        let info = Loaded {
            name: i.name.clone(),
            vendor: i.vendor.clone(),
            version: i.version.clone(),
            class_id: i.uid.clone(),
            path: bundle.to_path_buf(),
            has_editor: plugin.has_editor(),
            load_seconds: t0.elapsed().as_secs_f64(),
        };
        diag::info(format!("loaded {} by {} v{} in {:.2} s (class {}, {} output channels, editor: {})", info.name, info.vendor, info.version, info.load_seconds, info.class_id, plugin.output_channel_count(), info.has_editor));
        Ok(Instance { id: NEXT_ID.fetch_add(1, Relaxed), plugin: Arc::new(Mutex::new(plugin)), info: Arc::new(info), counters: Arc::new(Counters::default()), block })
    }

    pub fn info(&self) -> &Loaded {
        &self.info
    }

    pub fn counters(&self) -> &Arc<Counters> {
        &self.counters
    }

    /// An audio half for this plugin. Allocates its buffers here, so `process` never does.
    pub fn processor(&self) -> Processor {
        Processor { plugin: self.plugin.clone(), counters: self.counters.clone(), buf: AudioBuffers::new(0, 2, self.block, 48_000.0), block: self.block, pending: Vec::with_capacity(PENDING) }
    }

    /// The plugin's settings as it wants them saved: an opaque block of bytes.
    pub fn state(&self) -> Result<Vec<u8>, DawError> {
        let t0 = Instant::now();
        let bytes = lock(&self.plugin).save_state().map_err(|e| DawError::new(Code::StateSaveFailed, format!("{}: {e}", self.info.name)))?;
        diag::info(format!("{}: settings read, {} bytes in {:.0} ms", self.info.name, bytes.len(), t0.elapsed().as_secs_f64() * 1000.0));
        Ok(bytes)
    }

    pub fn set_state(&self, bytes: &[u8]) -> Result<(), DawError> {
        let t0 = Instant::now();
        lock(&self.plugin).load_state(bytes).map_err(|e| DawError::new(Code::StateRestoreFailed, format!("{} ({} bytes): {e}", self.info.name, bytes.len())))?;
        diag::info(format!("{}: settings restored, {} bytes in {:.0} ms", self.info.name, bytes.len(), t0.elapsed().as_secs_f64() * 1000.0));
        Ok(())
    }

    /// Open the plugin's own window; nothing new if it is open already. `owner` is the program's
    /// main window (its native handle): the plugin's window then floats above it.
    pub fn show(&self, owner: Option<isize>) -> Result<(), DawError> {
        if self.shown() {
            return Ok(());
        }
        let mut w = PluginWindow::new(self.plugin.clone());
        w.open().map_err(|e| DawError::new(Code::EditorFailed, format!("{}: {e}", self.info.name)))?;
        WINDOWS.with_borrow_mut(|all| all.insert(self.id, w));
        let floats = owner.is_some_and(|o| crate::editor_window::float_above(o) > 0);
        diag::info(format!("{}: window opened{}", self.info.name, if floats { ", floating above the main window" } else { "" }));
        Ok(())
    }

    pub fn hide(&self) {
        if WINDOWS.with_borrow_mut(|all| all.remove(&self.id)).is_some() {
            diag::info(format!("{}: window closed", self.info.name));
        }
    }

    pub fn shown(&self) -> bool {
        WINDOWS.with_borrow(|all| all.get(&self.id).is_some_and(|w| w.is_open()))
    }

    /// The regular look-in, a few times a second from the control thread: lets the window follow
    /// the plugin (a resize it asked for, a display with another scale), closes it when the
    /// person closed it, and notes whether the plugin's settings were changed from its window.
    /// Never waits for the audio half: what it cannot do now it does next time.
    pub fn service(&self) -> Serviced {
        let mut out = Serviced::default();
        WINDOWS.with_borrow_mut(|all| {
            let Some(w) = all.get(&self.id) else { return };
            if w.closed_by_user() {
                all.remove(&self.id);
                out.closed = true;
            } else if let Err(e) = w.service_platform_events() {
                diag::warn(Code::EditorEventError, format!("{}: {e}", self.info.name));
            }
        });
        if out.closed {
            diag::info(format!("{}: window closed by the person", self.info.name));
        }
        let mut guard = match self.plugin.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return out,
        };
        if self.counters.unloaded.load(Relaxed) {
            return out;
        }
        let p = &mut *guard;
        let edits = p.take_parameter_edits().len();
        let dirty = p.take_host_notifications().iter().any(|n| matches!(n, vst3_host::HostNotification::DirtyChanged(true)));
        let restart = p.service_host_requests().ok().filter(|f| !f.is_empty());
        drop(guard);
        if edits > 0 || dirty {
            self.counters.touched.store(true, Relaxed);
            out.touched = true;
        }
        if let Some(f) = restart {
            diag::info(format!("{}: asked the host to re-read it (restart flags {:#x})", self.info.name, f.bits()));
        }
        out
    }

    /// Switch between playing live and rendering a file. A plugin told it is offline may take as
    /// long as it needs per block, which is how sampled instruments avoid missing notes in an
    /// export. `block` is the largest block that will follow.
    pub fn set_offline(&self, offline: bool, block: usize, rate: u32) -> Result<(), DawError> {
        let mut p = lock(&self.plugin);
        let fail = |what: &str, e: vst3_host::Error| DawError::new(Code::PluginProcessFailed, format!("{}: {what}: {e}", self.info.name));
        p.stop_processing().map_err(|e| fail("it would not stop", e))?;
        if let Err(e) = p.set_process_mode(if offline { ProcessMode::Offline } else { ProcessMode::Realtime }) {
            diag::warn(Code::OfflineRefused, format!("{}: {e}", self.info.name));
        }
        p.reconfigure(f64::from(rate), block).map_err(|e| fail("it refused the new block size", e))?;
        p.start_processing().map_err(|e| fail("it would not start again", e))
    }

    /// For tests: hold the plugin for `how_long`, as a long settings call would, saying when.
    #[doc(hidden)]
    pub fn hold_for_test(&self, how_long: Duration, holding: std::sync::mpsc::Sender<()>) {
        let _g = lock(&self.plugin);
        let _ = holding.send(());
        std::thread::sleep(how_long);
    }

    /// Take the plugin down. Its audio halves, wherever they are, make silence from here on.
    pub fn unload(&self) {
        self.hide();
        if self.counters.unloaded.swap(true, Relaxed) {
            return;
        }
        let _ = lock(&self.plugin).stop_processing();
        LEAVING.with_borrow_mut(|all| all.push(self.plugin.clone()));
        diag::info(format!("{}: unloaded", self.info.name));
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Serviced {
    /// The person closed the plugin's window.
    pub closed: bool,
    /// The plugin's settings were changed from its window.
    pub touched: bool,
}

/// MIDI messages kept for a plugin while it is busy. A few milliseconds of a keyboard's worth.
const PENDING: usize = 1024;

pub struct Processor {
    plugin: Shared,
    counters: Arc<Counters>,
    buf: AudioBuffers,
    block: usize,
    /// What arrived while the plugin was busy: handed over at the start of the next block that
    /// gets it. Until 2026-11-27 a busy block threw its keys away, so a released key could be
    /// lost and its note left sounding.
    pending: Vec<Midi>,
}

/// A key coming up, or a controller (the sustain pedal among them): what must never be lost.
fn is_release(m: &Midi) -> bool {
    matches!(m.status & 0xF0, 0x80 | 0xB0) || (m.status & 0xF0 == 0x90 && m.b == 0)
}

impl Processor {
    /// One block: `midi` in (sorted by offset), sound out, replacing what `l` and `r` held.
    /// False when the block is silence because the plugin was busy, gone or failed.
    ///
    /// `wait` is for renders that are not live (an export): wait for the plugin as long as it
    /// takes. Live callers pass false and never wait longer than `SPIN`.
    pub fn process(&mut self, midi: &[Midi], l: &mut [f32], r: &mut [f32], wait: bool) -> bool {
        l.fill(0.0);
        r.fill(0.0);
        let mut guard = if wait {
            lock(&self.plugin)
        } else {
            let t0 = Instant::now();
            loop {
                match self.plugin.try_lock() {
                    Ok(g) => break g,
                    Err(TryLockError::Poisoned(p)) => break p.into_inner(),
                    Err(TryLockError::WouldBlock) if t0.elapsed() < SPIN => std::hint::spin_loop(),
                    Err(TryLockError::WouldBlock) => {
                        self.counters.busy.fetch_add(1, Relaxed);
                        // Kept for the next block. A release always finds room: a press makes way.
                        for m in midi {
                            if self.pending.len() == self.pending.capacity()
                                && is_release(m)
                                && let Some(i) = self.pending.iter().rposition(|p| !is_release(p))
                            {
                                self.pending.remove(i);
                            }
                            if self.pending.len() < self.pending.capacity() {
                                self.pending.push(Midi { offset: 0, ..*m });
                            } else {
                                self.counters.lost_midi.fetch_add(1, Relaxed);
                            }
                        }
                        return false;
                    }
                }
            }
        };
        if self.counters.unloaded.load(Relaxed) {
            self.pending.clear();
            return false;
        }
        let p = &mut *guard;
        for m in self.pending.drain(..) {
            if let Some(e) = m.event() {
                let _ = p.send_midi_event_at(e, 0);
            }
        }
        let (n, mut at, mut next, mut ok) = (l.len().min(r.len()), 0usize, 0usize, true);
        while at < n {
            let k = self.block.min(n - at);
            while next < midi.len() && (midi[next].offset as usize) < at + k {
                if let Some(e) = midi[next].event() {
                    let _ = p.send_midi_event_at(e, (midi[next].offset as usize).saturating_sub(at) as i32);
                }
                next += 1;
            }
            for ch in &mut self.buf.outputs {
                ch.resize(k, 0.0); // within what `processor` allocated: no allocation
                ch.fill(0.0);
            }
            self.buf.block_size = k;
            if p.process_audio(&mut self.buf).is_ok() {
                let out = &self.buf.outputs;
                l[at..at + k].copy_from_slice(&out[0][..k]);
                r[at..at + k].copy_from_slice(&out.get(1).unwrap_or(&out[0])[..k]);
            } else {
                self.counters.failed.fetch_add(1, Relaxed);
                ok = false;
            }
            self.counters.blocks.fetch_add(1, Relaxed);
            at += k;
        }
        ok
    }
}
