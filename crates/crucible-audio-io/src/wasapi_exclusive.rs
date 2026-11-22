//! WASAPI exclusive mode: capability checks, a timing test, and an output stream.

// PLATFORM: Windows only (WASAPI exclusive mode through the `wasapi` crate).
//   macOS equivalent: none needed. CoreAudio is already bit-perfect and low latency.
//   Linux equivalent: ALSA "hw:" devices directly, or JACK / PipeWire with a small quantum.
// Callers use the portable functions in lib.rs and never see this module.
//
// Why this exists: cpal 0.18 only opens WASAPI in shared mode (verified 2026-11-20). Shared
// mode resamples to Windows' mix format and adds latency. Exclusive mode gives the app the
// device directly. It is required for the laptop, which has no ASIO driver (D-009, D-018).
//
// Audio threads join the "Pro Audio" MMCSS class (Avrt API), as JUCE and Microsoft's own WASAPI
// sample do (docs/12-known-daw-problems.md A9). Set CRUCIBLE_NO_MMCSS=1 to compare without it (T13).

use crate::convert::{DeviceSampleFormat, write_interleaved};
use crate::{
    ExclusiveDeviceReport, ExclusiveFormat, ExclusiveStreamReport, OpenedOutput, OutputCallback,
    StreamDirection, StreamStats,
};
use crucible_diagnostics::{Code, DawError};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WasapiError, WaveFormat};

const RATES: [usize; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// (container bits, valid bits, sample type, our writer format, label), best quality first.
const FORMATS: [(usize, usize, SampleType, DeviceSampleFormat, &str); 5] = [
    (32, 32, SampleType::Float, DeviceSampleFormat::F32, "32-bit float"),
    (32, 24, SampleType::Int, DeviceSampleFormat::I24In32, "24-bit int in 32"),
    (24, 24, SampleType::Int, DeviceSampleFormat::I24Packed, "24-bit int packed"),
    (32, 32, SampleType::Int, DeviceSampleFormat::I24In32, "32-bit int"),
    (16, 16, SampleType::Int, DeviceSampleFormat::I16, "16-bit int"),
];

/// Runs `f` on a fresh thread with COM initialised there, so this module never disturbs COM
/// state on the caller's thread (cpal also uses COM).
/// Joins the calling thread to the MMCSS "Pro Audio" class until dropped, so Windows schedules
/// it ahead of ordinary work. Logs the outcome; call it before `mark_realtime_thread`, which
/// silences logging on the thread.
struct ProAudio(windows_sys::Win32::Foundation::HANDLE);

impl ProAudio {
    fn join() -> Option<ProAudio> {
        if std::env::var_os("CRUCIBLE_NO_MMCSS").is_some() {
            crucible_diagnostics::note("audio thread: MMCSS skipped (CRUCIBLE_NO_MMCSS is set)");
            return None;
        }
        let name: Vec<u16> = "Pro Audio".encode_utf16().chain(Some(0)).collect();
        let mut index = 0u32;
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; `index` is a
        // valid out-pointer. The returned handle is released in Drop on this same thread.
        let h = unsafe { windows_sys::Win32::System::Threading::AvSetMmThreadCharacteristicsW(name.as_ptr(), &mut index) };
        if h.is_null() {
            let err = std::io::Error::last_os_error();
            crucible_diagnostics::warn(Code::LateWakeup, format!("audio thread could not join MMCSS \"Pro Audio\": {err}. Timing may suffer under load."));
            None
        } else {
            crucible_diagnostics::note(format!("audio thread: MMCSS \"Pro Audio\" (task index {index})"));
            Some(ProAudio(h))
        }
    }
}

impl Drop for ProAudio {
    fn drop(&mut self) {
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this thread.
        unsafe { windows_sys::Win32::System::Threading::AvRevertMmThreadCharacteristics(self.0) };
    }
}

fn on_com_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(move || {
        let _ = wasapi::initialize_mta();
        let out = f();
        wasapi::deinitialize();
        out
    })
    .join()
    .expect("WASAPI worker thread panicked")
}

// ---------------------------------------------------------------- capability probe

/// Check every active device, both directions, for exclusive-mode formats and periods.
/// Only *asks* the drivers. Never opens a stream, so it cannot disturb running audio.
pub fn probe_all() -> Result<Vec<ExclusiveDeviceReport>, DawError> {
    on_com_thread(|| {
        let enumerator = DeviceEnumerator::new().map_err(|e| explain(&e))?;
        let mut out = Vec::new();
        for (direction, dir) in [(Direction::Render, StreamDirection::Output), (Direction::Capture, StreamDirection::Input)] {
            let collection = enumerator.get_device_collection(direction).map_err(|e| explain(&e))?;
            let count = collection.get_nbr_devices().map_err(|e| explain(&e))?;
            for i in 0..count {
                if let Ok(device) = collection.get_device_at_index(i) {
                    out.push(probe_device(&device, dir));
                }
            }
        }
        Ok(out)
    })
}

fn probe_device(device: &wasapi::Device, direction: StreamDirection) -> ExclusiveDeviceReport {
    let mut report = ExclusiveDeviceReport {
        name: device.get_friendlyname().unwrap_or_else(|_| "<unnamed>".into()),
        id: device.get_id().ok(),
        direction,
        native_channels: None,
        default_period_ms: None,
        min_period_ms: None,
        formats: vec![],
        error: None,
    };
    let client = match device.get_iaudioclient() {
        Ok(c) => c,
        Err(e) => {
            report.error = Some(explain(&e).to_string());
            return report;
        }
    };
    let channels = device_channels(device);
    report.native_channels = Some(channels as u16);
    if let Ok((default, min)) = client.get_device_period() {
        report.default_period_ms = Some(default as f64 / 10_000.0);
        report.min_period_ms = Some(min as f64 / 10_000.0);
    }
    for (store, valid, kind, _, label) in FORMATS {
        let rates: Vec<u32> = RATES
            .iter()
            .copied()
            .filter(|&rate| {
                let fmt = WaveFormat::new(store, valid, kind, rate, channels, None);
                client.is_supported_exclusive_with_quirks(&fmt).is_ok()
            })
            .map(|r| r as u32)
            .collect();
        if !rates.is_empty() {
            report.formats.push(ExclusiveFormat { format: label.to_string(), sample_rates: rates });
        }
    }
    report
}

fn device_channels(device: &wasapi::Device) -> usize {
    device.get_device_format().map(|f| f.get_nchannels() as usize).unwrap_or(2).max(1)
}

// ---------------------------------------------------------------- opening a stream

/// A render client that has been initialised exclusively and is ready to start.
struct Opened {
    client: wasapi::AudioClient,
    render: wasapi::AudioRenderClient,
    event: wasapi::Handle,
    fmt: DeviceSampleFormat,
    info: OpenedOutput,
}

fn open_render(device_name: Option<&str>, rate: u32, period_frames: Option<u32>) -> Result<Opened, DawError> {
    let enumerator = DeviceEnumerator::new().map_err(|e| explain(&e))?;
    let device = match device_name {
        None => enumerator.get_default_device(Direction::Render).map_err(|e| explain(&e))?,
        Some(needle) => find_render_device(&enumerator, needle)?,
    };
    let name = device.get_friendlyname().unwrap_or_default();
    let channels = device_channels(&device);
    let mut client = device.get_iaudioclient().map_err(|e| explain(&e))?;

    let (format, fmt, label) = FORMATS
        .iter()
        .find_map(|(store, valid, kind, fmt, label)| {
            let f = WaveFormat::new(*store, *valid, *kind, rate as usize, channels, None);
            client.is_supported_exclusive_with_quirks(&f).ok().map(|f| (f, *fmt, *label))
        })
        .ok_or_else(|| {
            DawError::new(Code::FormatNotSupported, format!("{name} accepts none of the tested formats at {rate} Hz in exclusive mode"))
        })?;

    let (_, min_period) = client.get_device_period().map_err(|e| explain(&e))?;
    let wanted = match period_frames {
        Some(frames) => wasapi::calculate_period_100ns(frames as i64, rate as i64),
        None => min_period,
    };
    // 128-byte alignment keeps Intel HD Audio drivers happy (per the wasapi crate's example).
    let mut period = client
        .calculate_aligned_period_near(wanted, Some(128), &format)
        .map_err(|e| explain(&e))?;
    let requested_period_ms = period as f64 / 10_000.0;

    // Exclusive mode can refuse an unaligned buffer. Microsoft's documented recovery is to
    // re-create the client with the size the driver suggests.
    if let Err(e) = client.initialize_client(&format, Direction::Render, StreamMode::EventsExclusive { period_hns: period }, None) {
        if hresult(&e) != Some(AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED) {
            return Err(explain(&e));
        }
        let frames = client.get_buffer_size().map_err(|e| explain(&e))?;
        period = wasapi::calculate_period_100ns(frames as i64, rate as i64);
        client = device.get_iaudioclient().map_err(|e| explain(&e))?;
        client
            .initialize_client(&format, Direction::Render, StreamMode::EventsExclusive { period_hns: period }, None)
            .map_err(|e| explain(&e))?;
    }

    let event = client.set_get_eventhandle().map_err(|e| explain(&e))?;
    let render = client.get_audiorenderclient().map_err(|e| explain(&e))?;
    let buffer_frames = client.get_buffer_size().map_err(|e| explain(&e))?;

    Ok(Opened {
        client,
        render,
        event,
        fmt,
        info: OpenedOutput {
            device: name,
            sample_rate: rate,
            device_channels: channels as u16,
            format: label.to_string(),
            requested_period_ms,
            buffer_frames,
            buffer_ms: buffer_frames as f64 * 1000.0 / rate as f64,
        },
    })
}

/// The event-driven render loop shared by the timing test and the real output stream.
/// `fill` writes `frames` frames of interleaved f32 at `src_ch` channels.
///
/// Real-time rule: everything this loop needs is allocated before `start_stream`.
fn render_loop(
    o: &Opened,
    src_ch: usize,
    stop: &AtomicBool,
    stats: &SharedStats,
    deadline: Option<Instant>,
    fill: &mut dyn FnMut(&mut [f32]),
) -> Result<(), DawError> {
    let dev_ch = o.info.device_channels as usize;
    let bps = o.fmt.bytes_per_sample();
    let max_frames = o.info.buffer_frames as usize;
    let mut float_buf = vec![0.0f32; max_frames * src_ch];
    let mut byte_buf = vec![0u8; max_frames * dev_ch * bps];

    // Pre-fill one buffer before starting, as Microsoft recommends.
    float_buf.fill(0.0);
    fill(&mut float_buf);
    write_interleaved(&float_buf, src_ch, &mut byte_buf, dev_ch, o.fmt);
    o.render.write_to_device(&byte_buf, None).map_err(|e| explain(&e))?;
    o.client.start_stream().map_err(|e| explain(&e))?;

    let period = Duration::from_secs_f64(o.info.buffer_frames as f64 / o.info.sample_rate as f64);
    let late = period.mul_f64(1.5);
    let timeout_ms = ((period.as_secs_f64() * 4000.0).ceil() as u32).max(10);
    let mut last = Instant::now();

    let result = loop {
        if stop.load(Ordering::Relaxed) || deadline.is_some_and(|d| Instant::now() >= d) {
            break Ok(());
        }
        if o.event.wait_for_event(timeout_ms).is_err() {
            stats.timeouts.fetch_add(1, Ordering::Relaxed);
            // A removed or reset device stops signalling. Asking for buffer space then fails
            // with the reason, which ends the stream with a proper error instead of silence.
            if let Err(e) = o.client.get_available_space_in_frames() {
                break Err(explain(&e));
            }
            continue;
        }
        let now = Instant::now();
        let gap = now - last;
        last = now;
        stats.wakeups.fetch_add(1, Ordering::Relaxed);
        if gap > late {
            stats.late.fetch_add(1, Ordering::Relaxed);
        }
        stats.worst_gap_us.fetch_max(gap.as_micros() as u64, Ordering::Relaxed);

        let space = match o.client.get_available_space_in_frames() {
            Ok(s) => (s as usize).min(max_frames),
            Err(e) => break Err(explain(&e)),
        };
        if space == 0 {
            continue;
        }
        let fb = &mut float_buf[..space * src_ch];
        fb.fill(0.0);
        fill(fb);
        let bb = &mut byte_buf[..space * dev_ch * bps];
        write_interleaved(fb, src_ch, bb, dev_ch, o.fmt);
        if let Err(e) = o.render.write_to_device(bb, None) {
            break Err(explain(&e));
        }
    };
    let _ = o.client.stop_stream();
    result
}

#[derive(Default)]
struct SharedStats {
    wakeups: AtomicU64,
    timeouts: AtomicU64,
    late: AtomicU64,
    worst_gap_us: AtomicU64,
}

impl SharedStats {
    fn snapshot(&self) -> StreamStats {
        StreamStats {
            wakeups: self.wakeups.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            late_wakeups: self.late.load(Ordering::Relaxed),
            worst_gap_ms: self.worst_gap_us.load(Ordering::Relaxed) as f64 / 1000.0,
        }
    }
}

// ---------------------------------------------------------------- timing test

/// Open the device exclusively, play **silence** for `seconds`, and measure whether the
/// driver's timing holds. Silence keeps the test inaudible.
pub fn stream_test(device_name: Option<String>, rate: u32, seconds: f64) -> ExclusiveStreamReport {
    on_com_thread(move || {
        let _mmcss = ProAudio::join();
        crucible_diagnostics::mark_realtime_thread();
        let stats = SharedStats::default();
        let stop = AtomicBool::new(false);
        let outcome = open_render(device_name.as_deref(), rate, None).and_then(|o| {
            let deadline = Instant::now() + Duration::from_secs_f64(seconds);
            render_loop(&o, 2, &stop, &stats, Some(deadline), &mut |_buf| {}).map(|()| o.info)
        });
        let s = stats.snapshot();
        match outcome {
            Ok(info) => ExclusiveStreamReport {
                expected_wakeups: (seconds * 1000.0 / info.buffer_ms).round() as u64,
                output: Some(info),
                seconds,
                stats: s,
                error: None,
            },
            Err(e) => ExclusiveStreamReport { output: None, seconds, expected_wakeups: 0, stats: s, error: Some(e.to_string()) },
        }
    })
}

// ---------------------------------------------------------------- output stream

/// A running exclusive-mode output. Dropping it stops the stream and joins its thread.
pub struct ExclusiveOutput {
    stop: Arc<AtomicBool>,
    /// Set by the audio thread when the stream ends on its own with an error.
    failure: Arc<Mutex<Option<DawError>>>,
    stats: Arc<SharedStats>,
    thread: Option<std::thread::JoinHandle<Result<(), DawError>>>,
    info: OpenedOutput,
}

impl ExclusiveOutput {
    pub fn info(&self) -> &OpenedOutput {
        &self.info
    }
    pub fn stats(&self) -> StreamStats {
        self.stats.snapshot()
    }
    /// Why the stream stopped by itself, if it did (device unplugged, driver reset).
    pub fn failure(&self) -> Option<DawError> {
        self.failure.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    /// Stop and return the error that ended the stream early, if any.
    pub fn stop(mut self) -> Result<(), DawError> {
        self.shutdown()
    }
    fn shutdown(&mut self) -> Result<(), DawError> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take() {
            Some(t) => t.join().unwrap_or_else(|_| Err(DawError::new(Code::Panic, "the audio thread panicked"))),
            None => Ok(()),
        }
    }
}

impl Drop for ExclusiveOutput {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Open an output exclusively and start calling `callback` for audio.
///
/// The callback receives interleaved f32 at `src_channels` channels. Extra device channels
/// get silence. Returns once the device is open and running, or with the reason it failed.
pub fn open_output(
    device_name: Option<String>,
    rate: u32,
    period_frames: Option<u32>,
    src_channels: usize,
    mut callback: OutputCallback,
) -> Result<ExclusiveOutput, DawError> {
    let stop = Arc::new(AtomicBool::new(false));
    let stats = Arc::new(SharedStats::default());
    let (ready_tx, ready_rx) = mpsc::channel::<Result<OpenedOutput, DawError>>();
    let failure = Arc::new(Mutex::new(None));
    let (stop_t, stats_t, failure_t) = (stop.clone(), stats.clone(), failure.clone());

    let thread = std::thread::Builder::new()
        .name("crucible exclusive audio".into())
        .spawn(move || {
            let _mmcss = ProAudio::join();
            crucible_diagnostics::mark_realtime_thread();
            let _ = wasapi::initialize_mta();
            let result = match open_render(device_name.as_deref(), rate, period_frames) {
                Err(e) => {
                    let _ = ready_tx.send(Err(e.clone()));
                    Err(e)
                }
                Ok(o) => {
                    let _ = ready_tx.send(Ok(o.info.clone()));
                    render_loop(&o, src_channels.max(1), &stop_t, &stats_t, None, &mut *callback)
                }
            };
            wasapi::deinitialize();
            if let Err(e) = &result {
                // Not real-time any more: the stream has ended.
                *failure_t.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.clone());
            }
            result
        })
        .map_err(|e| DawError::new(Code::StreamStartFailed, format!("could not start the audio thread: {e}")))?;

    match ready_rx.recv() {
        Ok(Ok(info)) => Ok(ExclusiveOutput { stop, failure, stats, thread: Some(thread), info }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(DawError::new(Code::StreamStartFailed, "the audio thread ended before opening the device")),
    }
}

// ---------------------------------------------------------------- helpers

fn find_render_device(enumerator: &DeviceEnumerator, needle: &str) -> Result<wasapi::Device, DawError> {
    let lower = needle.to_lowercase();
    let collection = enumerator.get_device_collection(Direction::Render).map_err(|e| explain(&e))?;
    let count = collection.get_nbr_devices().map_err(|e| explain(&e))?;
    let mut matches = Vec::new();
    for i in 0..count {
        if let Ok(d) = collection.get_device_at_index(i) {
            let name = d.get_friendlyname().unwrap_or_default();
            if name.to_lowercase().contains(&lower) {
                matches.push((name, d));
            }
        }
    }
    match matches.len() {
        1 => Ok(matches.pop().expect("one match").1),
        0 => Err(DawError::new(Code::DeviceNotFound, format!("no active output's name contains '{needle}'"))),
        _ => Err(DawError::new(
            Code::DeviceAmbiguous,
            format!(
                "'{needle}' matches several outputs: {}",
                matches.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join("; ")
            ),
        )),
    }
}

// AUDCLNT_E_* and other HRESULTs, checked against windows-sys 0.61 on 2026-11-20. Kept as
// numbers so this crate does not need the `windows` crate directly.
const AUDCLNT_E_BUFFER_TOO_LARGE: u32 = 0x8889_0006;
const AUDCLNT_E_UNSUPPORTED_FORMAT: u32 = 0x8889_0008;
const AUDCLNT_E_DEVICE_IN_USE: u32 = 0x8889_000A;
const AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED: u32 = 0x8889_000E;
const AUDCLNT_E_SERVICE_NOT_RUNNING: u32 = 0x8889_0010;
const AUDCLNT_E_INCORRECT_BUFFER_SIZE: u32 = 0x8889_0015;
const AUDCLNT_E_BUFFER_SIZE_ERROR: u32 = 0x8889_0016;
const AUDCLNT_E_CPUUSAGE_EXCEEDED: u32 = 0x8889_0017;
const AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED: u32 = 0x8889_0019;
const AUDCLNT_E_INVALID_DEVICE_PERIOD: u32 = 0x8889_0020;
const AUDCLNT_E_DEVICE_INVALIDATED: u32 = 0x8889_0004;
const AUDCLNT_E_RESOURCES_INVALIDATED: u32 = 0x8889_0026;
const E_ACCESSDENIED: u32 = 0x8007_0005;
const E_NOTFOUND: u32 = 0x8007_0490;

fn hresult(e: &WasapiError) -> Option<u32> {
    match e {
        WasapiError::Windows(w) => Some(w.code().0 as u32),
        _ => None,
    }
}

/// Turn a WASAPI failure into a coded error a person can act on. The HRESULT and Windows' own
/// text are always kept, so the exact driver answer is in the log.
fn explain(e: &WasapiError) -> DawError {
    let Some(hr) = hresult(e) else {
        return DawError::new(Code::AudioDriverError, e.to_string());
    };
    let (code, what) = match hr {
        AUDCLNT_E_DEVICE_IN_USE => (Code::DeviceInUse, "the device is in use by another program in exclusive mode"),
        AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED => (Code::ExclusiveNotAllowed, "exclusive mode is switched off for this device"),
        AUDCLNT_E_UNSUPPORTED_FORMAT => (Code::FormatNotSupported, "the device does not accept this format in exclusive mode"),
        AUDCLNT_E_BUFFER_TOO_LARGE
        | AUDCLNT_E_INCORRECT_BUFFER_SIZE
        | AUDCLNT_E_BUFFER_SIZE_ERROR
        | AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
        | AUDCLNT_E_INVALID_DEVICE_PERIOD => (Code::BufferRejected, "the driver rejected the buffer size"),
        AUDCLNT_E_DEVICE_INVALIDATED | AUDCLNT_E_RESOURCES_INVALIDATED => {
            (Code::StreamStopped, "the device was removed, disabled or reconfigured")
        }
        AUDCLNT_E_SERVICE_NOT_RUNNING => (Code::AudioServiceDown, "the Windows Audio service is not running"),
        AUDCLNT_E_CPUUSAGE_EXCEEDED => (Code::AudioCpuExceeded, "Windows stopped the stream for using too much CPU"),
        E_NOTFOUND => (Code::NoOutputDevice, "no such audio device"),
        E_ACCESSDENIED => (Code::AudioDriverError, "access denied (for inputs, check the Windows microphone privacy setting)"),
        _ => (Code::AudioDriverError, "the driver returned an error"),
    };
    // Windows often has no text for audio HRESULTs and just repeats the number.
    let raw = e.to_string();
    let hex = format!("{hr:#010X}");
    if raw.to_uppercase().contains(&hex[2..]) {
        DawError::new(code, format!("{what} (HRESULT {hex})"))
    } else {
        DawError::new(code, format!("{what} (HRESULT {hex}: {})", raw.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hresults_map_to_codes_and_keep_the_number() {
        let e = WasapiError::Windows(windows_core_error(AUDCLNT_E_DEVICE_IN_USE));
        let d = explain(&e);
        assert_eq!(d.code, Code::DeviceInUse);
        assert!(d.message.contains("0x8889000A"), "{}", d.message);
        let unknown = explain(&WasapiError::Windows(windows_core_error(0x8889_00FF)));
        assert_eq!(unknown.code, Code::AudioDriverError);
        assert!(unknown.message.contains("0x888900FF"), "{}", unknown.message);
    }

    fn windows_core_error(hr: u32) -> windows_core::Error {
        windows_core::Error::from_hresult(windows_core::HRESULT(hr as i32))
    }
}
