//! SEAM 1: audio devices.
//!
//! Everything that talks to the operating system's audio APIs lives in this crate.
//! Callers see plain data types and, later, an `AudioBackend` trait. They never see
//! cpal, WASAPI or ASIO directly.
//!
//! Today this crate only *enumerates* devices (milestone M0 step A). Opening streams
//! comes in M0 step B.

// PLATFORM: portable today. Enumeration goes through cpal, which wraps
//   Windows: WASAPI (and ASIO, when the "asio" cargo feature is enabled)
//   macOS:   CoreAudio
//   Linux:   ALSA, and PulseAudio / PipeWire / JACK behind cpal features
// Windows-specific modules of this crate:
//   - wasapi_exclusive.rs: WASAPI exclusive mode (cpal 0.18.2 only does shared mode, T2).
//   - wasapi_endpoints.rs: the device list with rates and users, and plug detection.
// ASIO streams and shared-mode streams go through cpal (output.rs). The "asio" feature needs
// the ASIO SDK and LLVM at build time, see .cargo/config.toml (D-035).
//
// Errors carry crucible_diagnostics codes (E2xxx), so every failure is searchable in the logs.

use cpal::traits::{DeviceTrait, HostTrait};
use crucible_diagnostics::DawError;
use serde::Serialize;

pub mod convert;
mod output;
#[cfg(windows)]
mod wasapi_endpoints;
#[cfg(windows)]
mod wasapi_exclusive;

pub use output::{Driver, Output, OutputRequest, asio_drivers, open_output};

/// Raises the calling thread to audio priority until dropped. For threads that feed the audio
/// callback and must not be held up by ordinary work.
pub struct AudioPriority {
    #[cfg(windows)]
    _inner: Option<wasapi_exclusive::ProAudio>,
}

impl AudioPriority {
    pub fn join() -> AudioPriority {
        // PLATFORM: Windows uses MMCSS "Pro Audio". macOS: a time-constraint thread policy
        // (thread_policy_set). Linux: SCHED_FIFO through rtkit. Both still to do; without
        // them this does nothing there.
        AudioPriority {
            #[cfg(windows)]
            _inner: wasapi_exclusive::ProAudio::join(),
        }
    }
}

// ---------------------------------------------------------------- exclusive mode
//
// Portable API. Implemented on Windows by `wasapi_exclusive`. On other systems these return
// "not applicable": macOS and Linux have no shared/exclusive split of this kind.

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum StreamDirection {
    Output,
    Input,
}

/// What one device accepts in exclusive mode.
#[derive(Debug, Clone, Serialize)]
pub struct ExclusiveDeviceReport {
    pub name: String,
    pub id: Option<String>,
    pub direction: StreamDirection,
    pub native_channels: Option<u16>,
    /// The driver's default processing period, in milliseconds.
    pub default_period_ms: Option<f64>,
    /// The shortest period the driver allows. Roughly the best-case buffer latency.
    pub min_period_ms: Option<f64>,
    /// Sample formats accepted exclusively, each with the rates that work.
    pub formats: Vec<ExclusiveFormat>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExclusiveFormat {
    pub format: String,
    pub sample_rates: Vec<u32>,
}

/// What an exclusive-mode output actually got from the driver.
#[derive(Debug, Clone, Serialize)]
pub struct OpenedOutput {
    pub device: String,
    pub sample_rate: u32,
    pub device_channels: u16,
    /// Sample format sent to the device, e.g. "24-bit int in 32".
    pub format: String,
    pub requested_period_ms: f64,
    /// Frames per driver buffer actually granted.
    pub buffer_frames: u32,
    /// The granted buffer as time. Output latency is at least this.
    pub buffer_ms: f64,
}

/// Running timing statistics for an exclusive-mode stream.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct StreamStats {
    pub wakeups: u64,
    /// Waits where the driver never signalled. Any value above zero means audible dropouts.
    pub timeouts: u64,
    /// Wake-ups more than 1.5 buffer periods after the previous one. A risk of dropouts.
    pub late_wakeups: u64,
    pub worst_gap_ms: f64,
}

/// Result of holding a device exclusively for a few seconds while playing silence.
#[derive(Debug, Clone, Serialize)]
pub struct ExclusiveStreamReport {
    /// `None` when the device could not be opened; see `error`.
    pub output: Option<OpenedOutput>,
    pub seconds: f64,
    pub expected_wakeups: u64,
    pub stats: StreamStats,
    pub error: Option<String>,
}

/// Audio callback for an output stream: fill the interleaved f32 buffer.
/// Runs on the audio thread, so it must follow the real-time rule (docs/02-architecture.md).
pub type OutputCallback = Box<dyn FnMut(&mut [f32]) + Send + 'static>;

/// A running exclusive-mode output stream. Dropping it stops the stream.
pub struct ExclusiveOutput {
    #[cfg(windows)]
    inner: wasapi_exclusive::ExclusiveOutput,
}

impl ExclusiveOutput {
    pub fn info(&self) -> &OpenedOutput {
        #[cfg(windows)]
        {
            self.inner.info()
        }
        #[cfg(not(windows))]
        {
            unreachable!("ExclusiveOutput cannot be constructed on this system")
        }
    }

    pub fn stats(&self) -> StreamStats {
        #[cfg(windows)]
        {
            self.inner.stats()
        }
        #[cfg(not(windows))]
        {
            StreamStats::default()
        }
    }

    /// Why the stream stopped by itself, if it did: device unplugged, driver reset, and so on.
    /// `None` while it is running normally.
    pub fn failure(&self) -> Option<DawError> {
        #[cfg(windows)]
        {
            self.inner.failure()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// Stop the stream. Returns the error that ended it early, if any.
    pub fn stop(self) -> Result<(), DawError> {
        #[cfg(windows)]
        {
            self.inner.stop()
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
fn not_applicable() -> DawError {
    DawError::new(crucible_diagnostics::Code::ExclusiveNotApplicable, "exclusive mode is a Windows concept")
}

/// Open an output device exclusively and start calling `callback` for audio.
///
/// - `device`: part of the device name, or `None` for the Windows default output.
/// - `period_frames`: requested buffer size, or `None` for the driver's minimum.
/// - `src_channels`: channels the callback produces. Extra device channels get silence.
pub fn open_exclusive_output(
    device: Option<String>,
    sample_rate: u32,
    period_frames: Option<u32>,
    src_channels: usize,
    callback: OutputCallback,
) -> Result<ExclusiveOutput, DawError> {
    #[cfg(windows)]
    {
        wasapi_exclusive::open_output(device, sample_rate, period_frames, src_channels, callback)
            .map(|inner| ExclusiveOutput { inner })
    }
    #[cfg(not(windows))]
    {
        let _ = (device, sample_rate, period_frames, src_channels, callback);
        Err(not_applicable())
    }
}

/// One audio device as the operating system lists it: an output (speakers, an interface) or an
/// input (a microphone, a line in).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Endpoint {
    pub name: String,
    pub input: bool,
    /// The system's default device for its direction.
    pub default: bool,
    /// What the system runs the device at when programs share it: sample rate, channels and
    /// bits. On Windows this is the setting under Sound, Advanced. `None` if it would not say.
    pub rate: Option<u32>,
    pub channels: Option<u16>,
    pub bits: Option<u16>,
    /// Other programs with sound running on this device right now (Windows only; a program
    /// using an ASIO driver is invisible here).
    pub used_by: Vec<String>,
}

/// Every active audio device: quick, opens nothing, safe to call while audio runs.
pub fn endpoints() -> Result<Vec<Endpoint>, DawError> {
    #[cfg(windows)]
    {
        wasapi_endpoints::list()
    }
    #[cfg(not(windows))]
    {
        // PLATFORM: names and defaults through cpal until the ports add rates and listeners.
        let host = cpal::default_host();
        let (out, inp) = (host.default_output_device().and_then(|d| d.id().ok()), host.default_input_device().and_then(|d| d.id().ok()));
        let devices = host.devices().map_err(|e| DawError::new(crucible_diagnostics::Code::AudioDriverError, e.to_string()))?;
        let mut list = Vec::new();
        for d in devices {
            let name = d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| "<unnamed device>".to_string());
            let id = d.id().ok();
            for (input, default) in [(false, &out), (true, &inp)] {
                let has = if input { d.default_input_config().is_ok() } else { d.default_output_config().is_ok() };
                if has {
                    list.push(Endpoint { name: name.clone(), input, default: id.is_some() && id == *default, rate: None, channels: None, bits: None, used_by: Vec::new() });
                }
            }
        }
        Ok(list)
    }
}

/// Notices when devices are plugged in, unplugged, enabled, disabled, made the default, or have
/// a setting such as their sample rate changed. Ask `changes()` now and then: when the number
/// has moved, list the devices again.
pub struct DeviceWatch {
    #[cfg(windows)]
    inner: wasapi_endpoints::Watch,
}

impl DeviceWatch {
    pub fn start() -> DeviceWatch {
        DeviceWatch {
            #[cfg(windows)]
            inner: wasapi_endpoints::Watch::start(),
        }
    }

    /// How many changes the system has reported since `start`. Always 0 where there is no
    /// listener yet (macOS, Linux): list again on a timer there.
    pub fn changes(&self) -> u64 {
        #[cfg(windows)]
        {
            self.inner.changes()
        }
        #[cfg(not(windows))]
        {
            0
        }
    }
}

/// List exclusive-mode capabilities of every device. Only queries drivers; opens nothing.
pub fn exclusive_capabilities() -> Result<Vec<ExclusiveDeviceReport>, DawError> {
    #[cfg(windows)]
    {
        wasapi_exclusive::probe_all()
    }
    #[cfg(not(windows))]
    {
        Err(not_applicable())
    }
}

/// Hold an output device exclusively and play silence, measuring timing.
/// `device`: part of the device name, or `None` for the default output.
pub fn exclusive_stream_test(device: Option<String>, sample_rate: u32, seconds: f64) -> Result<ExclusiveStreamReport, DawError> {
    #[cfg(windows)]
    {
        Ok(wasapi_exclusive::stream_test(device, sample_rate, seconds))
    }
    #[cfg(not(windows))]
    {
        let _ = (device, sample_rate, seconds);
        Err(not_applicable())
    }
}

/// Sample rates worth reporting. A device's supported ranges are checked against these.
const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Every audio host (driver API) cpal can see on this machine, with its devices.
#[derive(Debug, Clone, Serialize)]
pub struct AudioReport {
    pub hosts: Vec<HostReport>,
    /// Facts about what this build can and cannot do, for the diagnostic log.
    pub build_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostReport {
    /// Driver API name as cpal reports it, e.g. "WASAPI" or "ASIO".
    pub host: String,
    pub devices: Vec<DeviceReport>,
    /// Set when the host exists but its devices could not be listed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceReport {
    pub name: String,
    /// Stable identifier, used later to remember a device per machine.
    pub id: Option<String>,
    pub manufacturer: Option<String>,
    pub driver: Option<String>,
    pub is_default_output: bool,
    pub is_default_input: bool,
    pub output: Option<DirectionReport>,
    pub input: Option<DirectionReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectionReport {
    pub default_sample_rate: Option<u32>,
    pub default_channels: Option<u16>,
    pub default_format: Option<String>,
    /// Which of the common sample rates the device advertises.
    pub common_rates: Vec<u32>,
    pub max_channels: u16,
    /// Driver buffer size range in frames, when the driver reports one.
    pub buffer_frames: Option<(u32, u32)>,
    /// Set when the device was listed but its formats could not be read.
    pub error: Option<String>,
}

/// List every audio host and device visible to this build.
///
/// Never panics on a misbehaving driver: failures are recorded in the report.
pub fn enumerate() -> AudioReport {
    let mut hosts = Vec::new();
    for host_id in cpal::available_hosts() {
        let name = host_id.name().to_string();
        let host = match cpal::host_from_id(host_id) {
            Ok(h) => h,
            Err(e) => {
                hosts.push(HostReport { host: name, devices: vec![], error: Some(e.to_string()) });
                continue;
            }
        };
        hosts.push(report_host(name, &host));
    }
    AudioReport { hosts, build_notes: build_notes() }
}

fn report_host(name: String, host: &cpal::Host) -> HostReport {
    let default_out = host.default_output_device().and_then(|d| d.id().ok());
    let default_in = host.default_input_device().and_then(|d| d.id().ok());

    let devices = match host.devices() {
        Ok(d) => d,
        Err(e) => return HostReport { host: name, devices: vec![], error: Some(e.to_string()) },
    };

    let mut out = Vec::new();
    for device in devices {
        let id = device.id().ok();
        let desc = device.description().ok();
        let display_name = desc
            .as_ref()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|| "<unnamed device>".to_string());

        out.push(DeviceReport {
            name: display_name,
            id: id.as_ref().map(|i| i.to_string()),
            manufacturer: desc.as_ref().and_then(|d| d.manufacturer()).map(str::to_string),
            driver: desc.as_ref().and_then(|d| d.driver()).map(str::to_string),
            is_default_output: id.is_some() && id == default_out,
            is_default_input: id.is_some() && id == default_in,
            output: direction(&device, Direction::Output),
            input: direction(&device, Direction::Input),
        });
    }
    HostReport { host: name, devices: out, error: None }
}

#[derive(Clone, Copy)]
enum Direction {
    Output,
    Input,
}

/// Returns `None` when the device has no channels in that direction.
fn direction(device: &cpal::Device, dir: Direction) -> Option<DirectionReport> {
    let ranges: Result<Vec<cpal::SupportedStreamConfigRange>, String> = match dir {
        Direction::Output => device.supported_output_configs().map(|i| i.collect()),
        Direction::Input => device.supported_input_configs().map(|i| i.collect()),
    }
    .map_err(|e| e.to_string());

    let default = match dir {
        Direction::Output => device.default_output_config(),
        Direction::Input => device.default_input_config(),
    }
    .ok();

    let ranges = match ranges {
        Ok(r) => r,
        Err(e) => {
            // A device with no channels in this direction is normal, not an error.
            return default.map(|d| DirectionReport {
                default_sample_rate: Some(d.sample_rate()),
                default_channels: Some(d.channels()),
                default_format: Some(d.sample_format().to_string()),
                common_rates: vec![],
                max_channels: d.channels(),
                buffer_frames: None,
                error: Some(e),
            });
        }
    };

    if ranges.is_empty() && default.is_none() {
        return None;
    }

    let mut common_rates: Vec<u32> = COMMON_RATES
        .iter()
        .copied()
        .filter(|r| ranges.iter().any(|c| c.min_sample_rate() <= *r && *r <= c.max_sample_rate()))
        .collect();
    common_rates.dedup();

    let max_channels = ranges.iter().map(|c| c.channels()).max().unwrap_or(0);

    let buffer_frames = ranges.iter().find_map(|c| match c.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    });

    Some(DirectionReport {
        default_sample_rate: default.as_ref().map(|d| d.sample_rate()),
        default_channels: default.as_ref().map(|d| d.channels()),
        default_format: default.as_ref().map(|d| d.sample_format().to_string()),
        common_rates,
        max_channels,
        buffer_frames,
        error: None,
    })
}

fn build_notes() -> Vec<String> {
    let mut notes = Vec::new();
    // PLATFORM: these notes describe the Windows build. Extend per platform when ports start.
    if cfg!(target_os = "windows") {
        notes.push(
            "WASAPI devices above are listed in shared mode. See the EXCLUSIVE MODE section for \
             what each device accepts when the app takes it over."
                .to_string(),
        );
        notes.push(
            "In shared mode Windows resamples to the device's fixed mix rate (the 'default' \
             rate). The WASAPI 'rates' list shows what Windows accepts, not what the hardware \
             runs at."
                .to_string(),
        );
        notes.push(
            if cfg!(feature = "asio") {
                "ASIO is compiled into this build. Installed ASIO drivers are listed under host \
                 ASIO; \"0 devices\" there means no ASIO driver is installed on this machine. The \
                 same hardware also appears through WASAPI."
            } else {
                "ASIO is not compiled into this build, so ASIO drivers are not listed. Their \
                 devices still appear through WASAPI."
            }
            .to_string(),
        );
    }
    notes
}

#[cfg(test)]
mod hardware_tests {
    //! Need a real output device that allows exclusive mode. Not run by default.
    //! Run with: cargo test -p crucible-audio-io -- --ignored
    //! Device is picked by the RUST_DAW_TEST_DEVICE environment variable (part of its name),
    //! or the Windows default output. Plays silence only.

    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    #[ignore = "needs an audio device that allows exclusive mode"]
    fn exclusive_output_calls_back_steadily() {
        let device = std::env::var("RUST_DAW_TEST_DEVICE").ok();
        let calls = Arc::new(AtomicU64::new(0));
        let frames = Arc::new(AtomicU64::new(0));
        let (c, f) = (calls.clone(), frames.clone());
        let out = open_exclusive_output(
            device,
            48_000,
            None,
            2,
            Box::new(move |buf: &mut [f32]| {
                c.fetch_add(1, Ordering::Relaxed);
                f.fetch_add((buf.len() / 2) as u64, Ordering::Relaxed);
                buf.fill(0.0);
            }),
        )
        .expect("open exclusive output");
        let info = out.info().clone();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let stats = out.stats();
        out.stop().expect("stream ended cleanly");

        let produced = frames.load(Ordering::Relaxed) as f64;
        println!("{info:?}\n{stats:?}\ncallbacks {} frames {produced}", calls.load(Ordering::Relaxed));
        assert_eq!(stats.timeouts, 0, "driver stopped signalling");
        // Two seconds of audio at 48 kHz, within 10 percent.
        assert!((86_400.0..=105_600.0).contains(&produced), "produced {produced} frames in 2 s");
    }
}
