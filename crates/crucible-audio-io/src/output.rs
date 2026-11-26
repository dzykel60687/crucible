//! One way to open an output, whatever the driver: ASIO, WASAPI exclusive or WASAPI shared.
//!
//! Order of preference when the caller does not choose (D-018): the device's ASIO driver if one
//! is installed, else exclusive mode, else shared mode with a note saying why.

// PLATFORM: portable code. ASIO (Windows only, cpal's "asio" feature) and shared mode go through
// cpal, which is CoreAudio on macOS and ALSA / PipeWire / JACK on Linux, so this same path is
// the whole output layer there. Exclusive mode is the Windows-only module wasapi_exclusive.rs.

use crate::convert::{DeviceSampleFormat, write_interleaved};
use crate::{ExclusiveOutput, OpenedOutput, OutputCallback, StreamStats};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crucible_diagnostics::{Code, DawError};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    Asio,
    Exclusive,
    Shared,
}

impl Driver {
    pub const NAMES: [&'static str; 3] = ["asio", "exclusive", "shared"];

    pub fn name(self) -> &'static str {
        match self {
            Driver::Asio => "asio",
            Driver::Exclusive => "exclusive",
            Driver::Shared => "shared",
        }
    }

    pub fn parse(s: &str) -> Option<Driver> {
        [Driver::Asio, Driver::Exclusive, Driver::Shared].into_iter().find(|d| d.name() == s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct OutputRequest {
    /// `None` picks by the order in the module docs.
    pub driver: Option<Driver>,
    /// Part of the device name (for ASIO: the driver name). `None` is the system default.
    pub device: Option<String>,
    pub sample_rate: u32,
    /// Driver buffer in frames. `None` takes the driver's own choice.
    pub buffer_frames: Option<u32>,
    /// Channels the callback produces. Extra device channels get silence.
    pub channels: usize,
}

/// A running output stream. Dropping it stops the stream.
pub struct Output {
    driver: Driver,
    kind: Kind,
    info: OpenedOutput,
    /// Things the user should know, e.g. why a preferred driver was not used.
    pub notes: Vec<String>,
}

enum Kind {
    Exclusive(ExclusiveOutput),
    Cpal(CpalOutput),
}

impl Output {
    pub fn driver(&self) -> Driver {
        self.driver
    }
    pub fn info(&self) -> &OpenedOutput {
        &self.info
    }
    pub fn stats(&self) -> StreamStats {
        match &self.kind {
            Kind::Exclusive(o) => o.stats(),
            Kind::Cpal(o) => o.stats.snapshot(),
        }
    }
    /// Dropouts the driver itself reported (ASIO and shared mode only).
    pub fn driver_xruns(&self) -> u64 {
        match &self.kind {
            Kind::Exclusive(_) => 0,
            Kind::Cpal(o) => o.stats.xruns.load(Ordering::Relaxed),
        }
    }
    /// Why the stream stopped by itself, if it did.
    pub fn failure(&self) -> Option<DawError> {
        match &self.kind {
            Kind::Exclusive(o) => o.failure(),
            Kind::Cpal(o) => o.failure.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        }
    }
    pub fn stop(self) -> Result<(), DawError> {
        match self.kind {
            Kind::Exclusive(o) => o.stop(),
            Kind::Cpal(mut o) => o.shutdown(),
        }
    }
}

/// Names of the installed ASIO drivers. Empty when there are none or ASIO is not compiled in.
pub fn asio_drivers() -> Vec<String> {
    let Some(host) = asio_host() else { return Vec::new() };
    host.output_devices().map(|d| d.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect()).unwrap_or_default()
}

fn asio_host() -> Option<cpal::Host> {
    let id = cpal::available_hosts().into_iter().find(|h| h.name() == "ASIO")?;
    cpal::host_from_id(id).ok()
}

/// Open an output and start calling the callback for audio. `make` builds the callback; it is
/// asked again when one driver fails and the next is tried.
pub fn open_output(req: &OutputRequest, make: &mut dyn FnMut() -> OutputCallback) -> Result<Output, DawError> {
    let wrap = |driver, kind: Kind, info: OpenedOutput, notes| Output { driver, kind, info, notes };
    let exclusive = |cb| crate::open_exclusive_output(req.device.clone(), req.sample_rate, req.buffer_frames, req.channels, cb);
    match req.driver {
        Some(Driver::Asio) => {
            let o = CpalOutput::open(true, req, make())?;
            Ok(wrap(Driver::Asio, Kind::Cpal(o.0), o.1, vec![]))
        }
        Some(Driver::Exclusive) => {
            let o = exclusive(make())?;
            let info = o.info().clone();
            Ok(wrap(Driver::Exclusive, Kind::Exclusive(o), info, vec![]))
        }
        Some(Driver::Shared) => {
            let o = CpalOutput::open(false, req, make())?;
            Ok(wrap(Driver::Shared, Kind::Cpal(o.0), o.1, vec![SHARED_NOTE.to_string()]))
        }
        None => {
            let mut notes = Vec::new();
            let drivers = asio_drivers();
            let wanted = req.device.as_ref().map(|d| d.to_lowercase());
            if drivers.iter().any(|n| wanted.as_ref().is_none_or(|w| n.to_lowercase().contains(w))) {
                match CpalOutput::open(true, req, make()) {
                    Ok(o) => return Ok(wrap(Driver::Asio, Kind::Cpal(o.0), o.1, notes)),
                    Err(e) => notes.push(format!("ASIO was tried first and failed: {e}")),
                }
            }
            match exclusive(make()) {
                Ok(o) => {
                    let info = o.info().clone();
                    Ok(wrap(Driver::Exclusive, Kind::Exclusive(o), info, notes))
                }
                Err(e) => {
                    notes.push(format!("Exclusive mode was tried and failed: {e}"));
                    notes.push(SHARED_NOTE.to_string());
                    let o = CpalOutput::open(false, req, make())?;
                    Ok(wrap(Driver::Shared, Kind::Cpal(o.0), o.1, notes))
                }
            }
        }
    }
}

const SHARED_NOTE: &str = "Shared mode: Windows mixes this with other programs' sound and converts it to the device's own format, so it is not bit-exact and adds delay.";

// ---------------------------------------------------------------- cpal streams (ASIO, shared)

#[derive(Default)]
struct CpalStats {
    wakeups: AtomicU64,
    late: AtomicU64,
    worst_gap_us: AtomicU64,
    xruns: AtomicU64,
}

impl CpalStats {
    fn snapshot(&self) -> StreamStats {
        StreamStats {
            wakeups: self.wakeups.load(Ordering::Relaxed),
            timeouts: self.xruns.load(Ordering::Relaxed),
            late_wakeups: self.late.load(Ordering::Relaxed),
            worst_gap_ms: self.worst_gap_us.load(Ordering::Relaxed) as f64 / 1000.0,
        }
    }
}

/// An ASIO or shared-mode stream. cpal streams must stay on the thread that made them on some
/// systems, so one thread opens the stream, keeps it, and drops it when told to stop.
struct CpalOutput {
    stop: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<DawError>>>,
    stats: Arc<CpalStats>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Frames converted per step inside the callback. A driver buffer larger than this is filled in
/// several steps, so no buffer size needs an allocation.
const STEP: usize = 2048;

impl CpalOutput {
    fn open(asio: bool, req: &OutputRequest, callback: OutputCallback) -> Result<(CpalOutput, OpenedOutput), DawError> {
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let stats = Arc::new(CpalStats::default());
        let (ready_tx, ready_rx) = mpsc::channel::<Result<OpenedOutput, DawError>>();
        let (stop_t, failure_t, stats_t, req_t) = (stop.clone(), failure.clone(), stats.clone(), req.clone());
        let thread = std::thread::Builder::new()
            .name(if asio { "crucible asio stream" } else { "crucible shared stream" }.into())
            .spawn(move || match start(asio, &req_t, callback, stats_t, failure_t.clone()) {
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
                Ok((stream, info)) => {
                    let _ = ready_tx.send(Ok(info));
                    while !stop_t.load(Ordering::Relaxed) && failure_t.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
                        std::thread::park_timeout(Duration::from_millis(50));
                    }
                    drop(stream);
                }
            })
            .map_err(|e| DawError::new(Code::StreamStartFailed, format!("could not start the stream thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(info)) => Ok((CpalOutput { stop, failure, stats, thread: Some(thread) }, info)),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(DawError::new(Code::StreamStartFailed, "the stream thread ended before opening the device")),
        }
    }

    fn shutdown(&mut self) -> Result<(), DawError> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
        self.failure.lock().unwrap_or_else(|p| p.into_inner()).clone().map_or(Ok(()), Err)
    }
}

impl Drop for CpalOutput {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn start(
    asio: bool,
    req: &OutputRequest,
    mut callback: OutputCallback,
    stats: Arc<CpalStats>,
    failure: Arc<Mutex<Option<DawError>>>,
) -> Result<(cpal::Stream, OpenedOutput), DawError> {
    let what = if asio { "ASIO" } else { "shared-mode" };
    let host = if asio {
        asio_host().ok_or_else(|| DawError::new(Code::NoOutputDevice, "no ASIO driver is installed, or this build has no ASIO support"))?
    } else {
        cpal::default_host()
    };
    let named = |d: &cpal::Device| d.description().map(|x| x.name().to_string()).unwrap_or_default();
    let device = match &req.device {
        None => host.default_output_device().ok_or_else(|| {
            DawError::new(Code::NoOutputDevice, if asio { "no ASIO driver is installed on this machine (audio.output shows what is)".to_string() } else { format!("no default {what} output") })
        })?,
        Some(needle) => {
            let lower = needle.to_lowercase();
            let all = host.output_devices().map_err(|e| DawError::new(Code::AudioDriverError, format!("listing {what} outputs: {e}")))?;
            let mut hits: Vec<cpal::Device> = all.filter(|d| named(d).to_lowercase().contains(&lower)).collect();
            match hits.len() {
                1 => hits.remove(0),
                0 => return Err(DawError::new(Code::DeviceNotFound, format!("no {what} output's name contains '{needle}'"))),
                _ => return Err(DawError::new(Code::DeviceAmbiguous, format!("'{needle}' matches several {what} outputs: {}", hits.iter().map(named).collect::<Vec<_>>().join("; ")))),
            }
        }
    };
    let name = named(&device);
    let driver_err = |e: cpal::Error| {
        let code = match e.kind() {
            cpal::ErrorKind::DeviceBusy => Code::DeviceInUse,
            cpal::ErrorKind::UnsupportedConfig => Code::FormatNotSupported,
            cpal::ErrorKind::DeviceNotAvailable => Code::DeviceNotFound,
            _ => Code::StreamStartFailed,
        };
        DawError::new(code, format!("{name} ({what}): {e}"))
    };

    // Best format the driver offers at this rate. Floats first: they need no conversion.
    const ORDER: [(cpal::SampleFormat, DeviceSampleFormat); 5] = [
        (cpal::SampleFormat::F32, DeviceSampleFormat::F32),
        (cpal::SampleFormat::I32, DeviceSampleFormat::I32),
        (cpal::SampleFormat::I24, DeviceSampleFormat::I24Low),
        (cpal::SampleFormat::F64, DeviceSampleFormat::F64),
        (cpal::SampleFormat::I16, DeviceSampleFormat::I16),
    ];
    let ranges: Vec<cpal::SupportedStreamConfigRange> = device.supported_output_configs().map_err(driver_err)?.collect();
    let fits = |r: &&cpal::SupportedStreamConfigRange| r.min_sample_rate() <= req.sample_rate && req.sample_rate <= r.max_sample_rate();
    let Some((range, fmt)) = ORDER.iter().find_map(|(cf, ours)| {
        // Stereo if the driver lists it; otherwise the fewest channels that hold the source.
        let mut c: Vec<_> = ranges.iter().filter(fits).filter(|r| r.sample_format() == *cf && usize::from(r.channels()) >= req.channels.min(2)).collect();
        c.sort_by_key(|r| r.channels());
        c.first().map(|r| (*r, *ours))
    }) else {
        let rates: Vec<String> = ranges.iter().map(|r| format!("{}-{} Hz {}", r.min_sample_rate(), r.max_sample_rate(), r.sample_format())).collect();
        return Err(DawError::new(
            Code::FormatNotSupported,
            format!("{name} ({what}) does not offer {} Hz. It offers: {}", req.sample_rate, if rates.is_empty() { "nothing".to_string() } else { rates.join(", ") }),
        ));
    };
    let dev_ch = usize::from(range.channels());
    let config = cpal::StreamConfig {
        channels: range.channels(),
        sample_rate: req.sample_rate,
        buffer_size: req.buffer_frames.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
    };

    let src_ch = req.channels.max(1);
    let bps = fmt.bytes_per_sample();
    let rate = f64::from(req.sample_rate);
    let mut scratch = vec![0.0f32; STEP * src_ch];
    let mut last: Option<Instant> = None;
    let mut marked = false;
    let granted = Arc::new(AtomicU64::new(0));
    let (granted_cb, stats_cb, stats_err) = (granted.clone(), stats.clone(), stats);
    let stream = device
        .build_output_stream_raw(
            config,
            range.sample_format(),
            move |data: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {
                if !marked {
                    crucible_diagnostics::mark_realtime_thread();
                    marked = true;
                }
                let bytes = data.bytes_mut();
                let frames = bytes.len() / (dev_ch * bps);
                let now = Instant::now();
                if let Some(prev) = last {
                    let gap = now.duration_since(prev).as_micros() as u64;
                    stats_cb.worst_gap_us.fetch_max(gap, Ordering::Relaxed);
                    if gap as f64 > 1.5e6 * frames as f64 / rate {
                        stats_cb.late.fetch_add(1, Ordering::Relaxed);
                    }
                }
                last = Some(now);
                stats_cb.wakeups.fetch_add(1, Ordering::Relaxed);
                granted_cb.fetch_max(frames as u64, Ordering::Relaxed);
                for chunk in bytes.chunks_mut(STEP * dev_ch * bps) {
                    let n = chunk.len() / (dev_ch * bps);
                    let src = &mut scratch[..n * src_ch];
                    callback(src);
                    write_interleaved(src, src_ch, chunk, dev_ch, fmt);
                }
            },
            move |e: cpal::Error| {
                if e.kind() == cpal::ErrorKind::Xrun {
                    stats_err.xruns.fetch_add(1, Ordering::Relaxed);
                } else {
                    // The stream is ending: no longer real-time.
                    *failure.lock().unwrap_or_else(|p| p.into_inner()) = Some(DawError::new(Code::StreamStopped, e.to_string()));
                }
            },
            None,
        )
        .map_err(driver_err)?;
    stream.play().map_err(driver_err)?;

    // The buffer the driver really uses shows in the first callbacks.
    let t0 = Instant::now();
    while granted.load(Ordering::Relaxed) == 0 && t0.elapsed() < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(2));
    }
    // Shared mode often starts with a short request and settles on a longer one.
    std::thread::sleep(Duration::from_millis(60));
    let buffer_frames = granted.load(Ordering::Relaxed) as u32;
    Ok((
        stream,
        OpenedOutput {
            device: name,
            sample_rate: req.sample_rate,
            device_channels: range.channels(),
            format: fmt.label().to_string(),
            requested_period_ms: req.buffer_frames.map_or(0.0, |f| f64::from(f) * 1000.0 / rate),
            buffer_frames,
            buffer_ms: f64::from(buffer_frames) * 1000.0 / rate,
        },
    ))
}
