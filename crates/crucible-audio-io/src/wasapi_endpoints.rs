//! The audio devices Windows knows about: what is plugged in, the rate each runs at, which other
//! programs have sound running on it, and a signal when any of that changes.

// PLATFORM: Windows only (the MMDevice and audio-session APIs, through the `wasapi` crate).
//   macOS equivalent: kAudioHardwarePropertyDevices with property listeners (CoreAudio), and
//     kAudioDevicePropertyNominalSampleRate for the rate. CoreAudio mixes every program, so
//     "who else is using it" has no meaning there.
//   Linux equivalent: PipeWire's registry (nodes appear and vanish as events) or udev with ALSA
//     hardware parameters; `pw-dump` shows which clients are linked to a node.
//   Ledger rows: "Device list and plug detection", "Who else is using a device".
// Callers use `endpoints()` and `DeviceWatch` in lib.rs and never see this module.
//
// Everything here only asks. It opens no stream, so it cannot disturb running audio.
// What it cannot see: a program holding a device through an ASIO driver. ASIO bypasses Windows'
// audio system, so Windows has no record of it.

use crate::Endpoint;
use crate::wasapi_exclusive::{explain, on_com_thread};
use crucible_diagnostics::DawError;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use wasapi::{DeviceEnumerator, DeviceEventCallbacks, Direction, SessionState};

pub fn list() -> Result<Vec<Endpoint>, DawError> {
    on_com_thread(|| {
        let enumerator = DeviceEnumerator::new().map_err(|e| explain(&e))?;
        let me = std::process::id();
        let mut out = Vec::new();
        for (direction, input) in [(Direction::Render, false), (Direction::Capture, true)] {
            let default = enumerator.get_default_device(direction).ok().and_then(|d| d.get_id().ok());
            let collection = enumerator.get_device_collection(direction).map_err(|e| explain(&e))?;
            for i in 0..collection.get_nbr_devices().map_err(|e| explain(&e))? {
                let Ok(device) = collection.get_device_at_index(i) else { continue };
                // The format Windows runs the device at for shared use (Sound settings, Advanced).
                let format = device.get_device_format().ok();
                out.push(Endpoint {
                    name: device.get_friendlyname().unwrap_or_else(|_| "<unnamed device>".to_string()),
                    input,
                    default: default.is_some() && device.get_id().ok() == default,
                    rate: format.as_ref().map(|f| f.get_samplespersec()),
                    channels: format.as_ref().map(|f| f.get_nchannels()),
                    bits: format.as_ref().map(|f| f.get_validbitspersample()),
                    used_by: users(&device, me),
                });
            }
        }
        Ok(out)
    })
}

/// Other programs with sound running on this device right now. Empty if Windows will not say.
fn users(device: &wasapi::Device, me: u32) -> Vec<String> {
    let Ok(sessions) = device.get_iaudiosessionmanager().and_then(|m| m.get_audiosessionenumerator()) else { return Vec::new() };
    let mut names = Vec::new();
    for i in 0..sessions.get_count().unwrap_or(0) {
        let Ok(session) = sessions.get_session(i) else { continue };
        if !matches!(session.get_state(), Ok(SessionState::Active)) {
            continue;
        }
        // Process 0 is Windows' own notification sounds.
        let Ok(pid) = session.get_process_id() else { continue };
        if pid == 0 || pid == me {
            continue;
        }
        let name = process_name(pid).unwrap_or_else(|| format!("process {pid}"));
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The file name of a running program, e.g. `chrome.exe`.
fn process_name(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: plain Win32 calls. The buffer is as long as `len` says, and the handle is closed.
    let ok = unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        ok
    };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    Some(path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string())
}

/// Counts every change Windows reports to its audio devices: plugged in, unplugged, enabled,
/// disabled, another default chosen, a property (such as the sample rate) changed.
pub struct Watch {
    changes: Arc<AtomicU64>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watch {
    pub fn start() -> Watch {
        let changes = Arc::new(AtomicU64::new(0));
        let (stop, stopped) = mpsc::channel::<()>();
        let counter = changes.clone();
        // The registration lives on its own thread: it has to stay alive, on a thread that keeps
        // COM set up, for as long as changes should be reported.
        let thread = std::thread::Builder::new()
            .name("crucible device watch".into())
            .spawn(move || {
                let _ = wasapi::initialize_mta();
                // Each report from Windows is counted, and written down as it arrives while the
                // log records everything. Names are looked up later, by whoever lists the devices:
                // a notification callback should do as little as it can.
                let bump = move |what: &str, id: &str| {
                    counter.fetch_add(1, Ordering::Relaxed);
                    if crucible_diagnostics::everything() {
                        crucible_diagnostics::event("AUDIO", format!("Windows reports: {what} {id}"));
                    }
                };
                let mut callbacks = DeviceEventCallbacks::new();
                let b = bump.clone();
                callbacks.set_device_added_callback(move |id| b("device added", &id));
                let b = bump.clone();
                callbacks.set_device_removed_callback(move |id| b("device removed", &id));
                let b = bump.clone();
                callbacks.set_device_state_callback(move |id, state| b(&format!("device is now {state:?}"), &id));
                let b = bump.clone();
                callbacks.set_default_device_callback(move |direction, role, id| b(&format!("default {direction:?} device for {role:?} is now"), id.as_deref().unwrap_or("none")));
                callbacks.set_property_value_callback(move |id, _| bump("a setting changed on device", &id));
                let registration = DeviceEnumerator::new().and_then(|e| e.register_notification_callback(callbacks));
                if let Err(e) = &registration {
                    crucible_diagnostics::info(format!("device changes will not be noticed: {e}"));
                }
                // Until the Watch is dropped.
                let _ = stopped.recv();
                drop(registration);
                wasapi::deinitialize();
            })
            .ok();
        Watch { changes, stop: Some(stop), thread }
    }

    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
