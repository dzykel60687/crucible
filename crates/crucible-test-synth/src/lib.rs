//! "Crucible Test Tone": a minimal VST3 instrument, used only by tests.
//!
//! The dev machine has no plugins and none may be downloaded without a yes (CLAUDE.md, install
//! rule), so plugin hosting had nothing to load. This is that something: a real VST3 module that
//! goes through the same loader, the same COM calls and the same editor window as any other.
//!
//! What it does, so tests can predict its output exactly:
//! - One sine per held key, at the key's pitch, `velocity / 127 * 0.25 * level` loud, no envelope.
//! - Parameter 0 "Level" (0 to 1, default 1) is its whole saved state: 8 bytes.
//! - Parameter 1 "Sustain" is what MIDI controller 64 maps to (VST3 has no controller events;
//!   a host that sends only notes leaves the sustain pedal dead: `12-known-daw-problems.md` P10).
//!   While it is on, released keys keep sounding.
//! - An editor: one label in a child window (Windows only).
//!
//! It shares its author's reading of the VST3 rules with the host it tests, so it proves the
//! plumbing, not compatibility with other people's plugins. That takes real plugins.
//!
//! Portability: the audio and COM parts are portable. The editor window is in
//! `platform_windows.rs`; elsewhere the plugin has no editor.

#![allow(non_snake_case, non_upper_case_globals, unsafe_op_in_unsafe_fn, clippy::missing_safety_doc)]

#[cfg(windows)]
mod platform_windows;

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use vst3::{Class, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*, uid};

pub const NAME: &str = "Crucible Test Tone";
pub const VENDOR: &str = "Crucible";
/// The bundle's folder name.
pub const BUNDLE: &str = "Crucible Test Tone.vst3";
pub const PARAM_LEVEL: u32 = 0;
pub const PARAM_SUSTAIN: u32 = 1;

const PROCESSOR_CID: TUID = uid(0x43727563, 0x69626C65, 0x54657374, 0x546F6E65);
const CONTROLLER_CID: TUID = uid(0x43727563, 0x69626C65, 0x54657374, 0x4374726C);

/// Lay the built plugin out as a VST3 bundle under `dir` and return the bundle's path.
/// The plugin's file is looked for beside the running program, where cargo puts it.
pub fn bundle_into(dir: &Path) -> std::io::Result<PathBuf> {
    let file = format!("{}crucible_test_synth{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX);
    let here = std::env::current_exe()?.parent().map(Path::to_path_buf).unwrap_or_default();
    // Cargo keeps each package's output in a folder of its own: beside the program (a plain
    // build), in `deps` (older cargo), or in `build/crucible-test-synth/<hash>/out` next to the
    // running test's `build/<package>/<hash>/out`. Several builds can be lying about; the
    // newest is the one this run was built with.
    let mut places = vec![here.join(&file), here.join("deps").join(&file), here.join("..").join(&file)];
    for builds in [here.join("../../../crucible-test-synth"), here.join("build/crucible-test-synth")] {
        places.extend(std::fs::read_dir(builds).into_iter().flatten().flatten().map(|d| d.path().join("out").join(&file)));
    }
    let modified = |p: &PathBuf| p.metadata().and_then(|m| m.modified()).ok();
    let built = places.iter().filter(|p| p.is_file()).max_by_key(|p| modified(p)).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, format!("the test plugin is not built: looked for {}", places.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
    })?;
    // PLATFORM: the folder inside a bundle that holds the binary, per the VST3 bundle format.
    let (arch, ext) = if cfg!(windows) {
        ("x86_64-win", "vst3")
    } else if cfg!(target_os = "macos") {
        ("MacOS", "")
    } else {
        ("x86_64-linux", "so")
    };
    let inner = dir.join(BUNDLE).join("Contents").join(arch);
    std::fs::create_dir_all(&inner)?;
    let name = if ext.is_empty() { NAME.to_string() } else { format!("{NAME}.{ext}") };
    std::fs::copy(built, inner.join(name))?;
    Ok(dir.join(BUNDLE))
}

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    dst.fill(0);
    for (s, d) in src.bytes().zip(dst.iter_mut().rev().skip(1).rev()) {
        *d = s as c_char;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    dst.fill(0);
    for (s, d) in src.encode_utf16().zip(dst.iter_mut().rev().skip(1).rev()) {
        *d = s as TChar;
    }
}

/// Read or write the 8-byte state. A short read leaves the level alone.
unsafe fn read_level(stream: *mut IBStream) -> Option<f64> {
    let s = ComRef::from_raw(stream)?;
    let (mut bytes, mut got) = ([0u8; 8], 0i32);
    (s.read(bytes.as_mut_ptr().cast(), 8, &mut got) == kResultOk && got == 8).then(|| f64::from_le_bytes(bytes)).filter(|v| (0.0..=1.0).contains(v))
}

unsafe fn write_level(stream: *mut IBStream, level: f64) -> tresult {
    let Some(s) = ComRef::from_raw(stream) else { return kInvalidArgument };
    let (mut bytes, mut put) = (level.to_le_bytes(), 0i32);
    s.write(bytes.as_mut_ptr().cast(), 8, &mut put)
}

// ---------------------------------------------------------------- the sound

struct Voices {
    rate: f64,
    level: f64,
    sustain: bool,
    /// Per key: loudness while the key is down (0 = silent), whether the pedal alone holds it,
    /// and where its sine is.
    gain: [f32; 128],
    pedal_held: [bool; 128],
    phase: [f64; 128],
}

impl Voices {
    fn note(&mut self, pitch: i16, velocity: f32) {
        let Some(k) = usize::try_from(pitch).ok().filter(|k| *k < 128) else { return };
        if velocity > 0.0 {
            (self.gain[k], self.pedal_held[k], self.phase[k]) = (velocity * 0.25, false, 0.0);
        } else if self.sustain {
            self.pedal_held[k] = self.gain[k] > 0.0;
        } else {
            self.gain[k] = 0.0;
        }
    }

    fn pedal(&mut self, down: bool) {
        self.sustain = down;
        if !down {
            for k in 0..128 {
                if std::mem::take(&mut self.pedal_held[k]) {
                    self.gain[k] = 0.0;
                }
            }
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        for k in 0..128 {
            if self.gain[k] == 0.0 {
                continue;
            }
            let step = 440.0 * 2f64.powf((k as f64 - 69.0) / 12.0) / self.rate;
            let g = f64::from(self.gain[k]) * self.level;
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                let s = (g * (self.phase[k] * std::f64::consts::TAU).sin()) as f32;
                *a += s;
                *b += s;
                self.phase[k] = (self.phase[k] + step).fract();
            }
        }
    }
}

struct Processor {
    v: RefCell<Voices>,
}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor);
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        *class_id = CONTROLLER_CID;
        kResultOk
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        let audio_out = media == MediaTypes_::kAudio as MediaType && dir == BusDirections_::kOutput as BusDirection;
        let event_in = media == MediaTypes_::kEvent as MediaType && dir == BusDirections_::kInput as BusDirection;
        i32::from(audio_out || event_in)
    }
    unsafe fn getBusInfo(&self, media: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        if index != 0 || self.getBusCount(media, dir) == 0 {
            return kInvalidArgument;
        }
        let bus = &mut *bus;
        let audio = media == MediaTypes_::kAudio as MediaType;
        bus.mediaType = media;
        bus.direction = dir;
        bus.channelCount = if audio { 2 } else { 16 };
        copy_wstring(if audio { "Output" } else { "MIDI In" }, &mut bus.name);
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(&self, _media: MediaType, _dir: BusDirection, _index: i32, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        if let Some(level) = read_level(state) {
            self.v.borrow_mut().level = level;
        }
        kResultOk
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        write_level(state, self.v.borrow().level)
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(&self, _inputs: *mut SpeakerArrangement, num_ins: i32, outputs: *mut SpeakerArrangement, num_outs: i32) -> tresult {
        if num_ins == 0 && num_outs == 1 && *outputs == SpeakerArr::kStereo { kResultTrue } else { kResultFalse }
    }
    unsafe fn getBusArrangement(&self, dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        if dir == BusDirections_::kOutput as BusDirection && index == 0 {
            *arr = SpeakerArr::kStereo;
            kResultOk
        } else {
            kInvalidArgument
        }
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 { kResultOk } else { kResultFalse }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        self.v.borrow_mut().rate = (*setup).sampleRate;
        kResultOk
    }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = &*data;
        let mut v = self.v.borrow_mut();
        // Parameters first: the last point of each queue is the value from now on.
        if let Some(changes) = ComRef::from_raw(data.inputParameterChanges) {
            for i in 0..changes.getParameterCount() {
                let Some(q) = ComRef::from_raw(changes.getParameterData(i)) else { continue };
                let (mut offset, mut value) = (0, 0.0);
                if q.getPointCount() < 1 || q.getPoint(q.getPointCount() - 1, &mut offset, &mut value) != kResultOk {
                    continue;
                }
                match q.getParameterId() {
                    PARAM_LEVEL => v.level = value.clamp(0.0, 1.0),
                    PARAM_SUSTAIN => v.pedal(value >= 0.5),
                    _ => {}
                }
            }
        }
        let n = data.numSamples.max(0) as usize;
        let out = if data.numOutputs >= 1 && !data.outputs.is_null() && (*data.outputs).numChannels >= 2 && n > 0 {
            let ch = std::slice::from_raw_parts((*data.outputs).__field0.channelBuffers32, 2);
            (!ch[0].is_null() && !ch[1].is_null()).then(|| (std::slice::from_raw_parts_mut(ch[0], n), std::slice::from_raw_parts_mut(ch[1], n)))
        } else {
            None
        };
        let Some((l, r)) = out else {
            // A call with no audio (a parameter flush): notes still count.
            if let Some(events) = ComRef::from_raw(data.inputEvents) {
                for i in 0..events.getEventCount() {
                    let mut e: Event = std::mem::zeroed();
                    if events.getEvent(i, &mut e) == kResultOk {
                        apply(&mut v, &e);
                    }
                }
            }
            return kResultOk;
        };
        l.fill(0.0);
        r.fill(0.0);
        // Render up to each event's sample, then apply it: notes start on the sample they ask for.
        let mut at = 0usize;
        if let Some(events) = ComRef::from_raw(data.inputEvents) {
            for i in 0..events.getEventCount() {
                let mut e: Event = std::mem::zeroed();
                if events.getEvent(i, &mut e) != kResultOk {
                    continue;
                }
                let to = (e.sampleOffset.max(0) as usize).clamp(at, n);
                v.render(&mut l[at..to], &mut r[at..to]);
                at = to;
                apply(&mut v, &e);
            }
        }
        v.render(&mut l[at..], &mut r[at..]);
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

unsafe fn apply(v: &mut Voices, e: &Event) {
    if e.r#type == Event_::EventTypes_::kNoteOnEvent as u16 {
        v.note(e.__field0.noteOn.pitch, e.__field0.noteOn.velocity);
    } else if e.r#type == Event_::EventTypes_::kNoteOffEvent as u16 {
        v.note(e.__field0.noteOff.pitch, 0.0);
    }
}

// ---------------------------------------------------------------- the controller

struct Controller {
    level: Cell<f64>,
    sustain: Cell<f64>,
}

impl Class for Controller {
    type Interfaces = (IEditController, IMidiMapping);
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        if let Some(level) = read_level(state) {
            self.level.set(level);
        }
        kResultOk
    }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        2
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let (id, title, steps, default) = match index {
            0 => (PARAM_LEVEL, "Level", 0, 1.0),
            1 => (PARAM_SUSTAIN, "Sustain", 1, 0.0),
            _ => return kInvalidArgument,
        };
        let info = &mut *info;
        info.id = id;
        copy_wstring(title, &mut info.title);
        copy_wstring(title, &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = steps;
        info.defaultNormalizedValue = default;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, _id: u32, value: f64, string: *mut String128) -> tresult {
        copy_wstring(&format!("{value:.2}"), &mut *string);
        kResultOk
    }
    unsafe fn getParamValueByString(&self, _id: u32, _string: *mut TChar, _value: *mut f64) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, _id: u32, value: f64) -> f64 {
        value
    }
    unsafe fn plainParamToNormalized(&self, _id: u32, plain: f64) -> f64 {
        plain
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        match id {
            PARAM_LEVEL => self.level.get(),
            PARAM_SUSTAIN => self.sustain.get(),
            _ => 0.0,
        }
    }
    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        match id {
            PARAM_LEVEL => self.level.set(value),
            PARAM_SUSTAIN => self.sustain.set(value),
            _ => return kInvalidArgument,
        }
        kResultOk
    }
    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }
    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        #[cfg(windows)]
        {
            platform_windows::view()
        }
        #[cfg(not(windows))]
        {
            std::ptr::null_mut()
        }
    }
}

impl IMidiMappingTrait for Controller {
    unsafe fn getMidiControllerAssignment(&self, bus: i32, _channel: i16, controller: CtrlNumber, id: *mut ParamID) -> tresult {
        if bus == 0 && controller == ControllerNumbers_::kCtrlSustainOnOff as CtrlNumber {
            *id = PARAM_SUSTAIN;
            kResultTrue
        } else {
            kResultFalse
        }
    }
}

// ---------------------------------------------------------------- the factory

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

const CLASSES: [(TUID, &str); 2] = [(PROCESSOR_CID, "Audio Module Class"), (CONTROLLER_CID, "Component Controller Class")];

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = &mut *info;
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring("", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        CLASSES.len() as i32
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let Some((cid, category)) = usize::try_from(index).ok().and_then(|i| CLASSES.get(i)) else { return kInvalidArgument };
        let info = &mut *info;
        info.cid = *cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(NAME, &mut info.name);
        kResultOk
    }
    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        let cid = *(cid as *const TUID);
        let instance = if cid == PROCESSOR_CID {
            ComWrapper::new(Processor { v: RefCell::new(Voices { rate: 48_000.0, level: 1.0, sustain: false, gain: [0.0; 128], pedal_held: [false; 128], phase: [0.0; 128] }) }).to_com_ptr::<FUnknown>()
        } else if cid == CONTROLLER_CID {
            ComWrapper::new(Controller { level: Cell::new(1.0), sustain: Cell::new(0.0) }).to_com_ptr::<FUnknown>()
        } else {
            None
        };
        match instance {
            Some(i) => {
                let ptr = i.as_ptr();
                ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj)
            }
            None => kInvalidArgument,
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let Some((cid, category)) = usize::try_from(index).ok().and_then(|i| CLASSES.get(i)) else { return kInvalidArgument };
        let info = &mut *info;
        info.cid = *cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring(if index == 0 { "Instrument|Synth" } else { "" }, &mut info.subCategories);
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(env!("CARGO_PKG_VERSION"), &mut info.version);
        copy_cstring("VST 3.7.0", &mut info.sdkVersion);
        kResultOk
    }
}

// PLATFORM: each system's VST3 module has its own entry and exit names.
#[cfg(windows)]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_bundle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().map_or(std::ptr::null_mut(), |p| p.into_raw())
}
