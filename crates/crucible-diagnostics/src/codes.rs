//! Stable error and warning codes.
//!
//! Every problem the programs report carries a code such as `E2004`. A published code never
//! changes meaning, so a code in an old log still means the same thing. Retire a code by
//! leaving it in place with "(retired)" in its title; never reuse the number.
//!
//! - Letter: `E` = error (the action failed), `W` = warning (it worked, but something is off).
//! - First digit = area: 1 general and files, 2 audio devices, 3 MIDI, 4 plugins,
//!   5 projects and commands, 9 crashes and bugs.
//!
//! `crucible codes` prints the full list. `docs/11-error-codes.md` is generated from it, and a test
//! fails if the doc is missing a code.
//!
//! Portability: codes are platform-neutral. Help text names the Windows fix first because
//! Windows is the first platform; add the macOS and Linux wording when the ports start.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

macro_rules! codes {
    ($( $variant:ident = $id:literal, $title:literal, $help:literal; )*) => {
        /// One reportable problem. See the module docs for the numbering scheme.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Code { $( $variant, )* }

        impl Code {
            pub const ALL: &'static [Code] = &[ $( Code::$variant, )* ];

            /// The stable identifier, e.g. `"E2004"`.
            pub fn id(self) -> &'static str {
                match self { $( Code::$variant => $id, )* }
            }

            /// One line saying what went wrong.
            pub fn title(self) -> &'static str {
                match self { $( Code::$variant => $title, )* }
            }

            /// What the person at the machine should try.
            pub fn help(self) -> &'static str {
                match self { $( Code::$variant => $help, )* }
            }
        }
    };
}

codes! {
    // ---- 1: general and files
    Usage = "E1001", "The command line was not understood",
        "Run the program with no arguments, or with `help`, to see the commands and options.";
    FileRead = "E1002", "A file could not be read",
        "Check that the path exists and that no other program has the file locked.";
    FileWrite = "E1003", "A file could not be written",
        "Check free disk space, that the folder exists and that you may write there. OneDrive folders can lock files while syncing.";
    LogUnavailable = "W1004", "The log file could not be created",
        "The program still runs but records nothing. Check that the folder shown by `crucible logs` is writable.";
    SlowCommand = "W1005", "A command is taking a long time",
        "Wait if it is an export or a large import. If the window stays stuck, close it and send a report: the log names the command that never finished.";
    ReportHelperMissing = "E1006", "The report could not be made: crucible.exe was not found",
        "The window uses the crucible.exe command-line tool to gather the report. It is installed in the tools folder beside the window program. Reinstall Crucible, or run `crucible.exe report` yourself.";

    // ---- 2: audio devices
    NoOutputDevice = "E2001", "No audio output device was found",
        "Connect or enable an output in the system sound settings, then run `crucible audio.devices`.";
    DeviceNotFound = "E2002", "No audio device matches the name given",
        "Run `crucible audio.devices` and use part of a device name exactly as listed.";
    DeviceAmbiguous = "E2003", "The device name matches more than one device",
        "Use more of the name, so that only one device matches.";
    DeviceInUse = "E2004", "Another program is using the audio device exclusively",
        "Close other audio programs (other DAWs, standalone plugins, browsers, Teams, Zoom), then retry.";
    ExclusiveNotAllowed = "E2005", "Exclusive mode is switched off for this device",
        "Windows: Sound settings > the device > Properties/Advanced > tick 'Allow applications to take exclusive control of this device'.";
    FormatNotSupported = "E2006", "The device does not accept this sample rate or format",
        "Try --rate 48000 or --rate 44100. `crucible audio.devices` lists what each device accepts in exclusive mode.";
    BufferRejected = "E2007", "The driver rejected the buffer size",
        "Try a larger buffer, for example --block 256 or --block 512.";
    StreamStartFailed = "E2008", "The audio stream could not start",
        "Retry once. If it repeats, run `crucible audio.test` and send the log.";
    StreamStopped = "E2009", "Audio stopped unexpectedly",
        "The device was unplugged, changed or reset by its driver. Sleep, or changing the device's format in Windows, also does this. Reconnect and restart.";
    AudioServiceDown = "E2010", "The system audio service is not running",
        "Windows: start the 'Windows Audio' service in services.msc, or reboot.";
    AudioCpuExceeded = "E2011", "Windows stopped the audio stream because the audio thread used too much CPU",
        "Use a larger buffer, close CPU-heavy programs, and use the High performance power plan.";
    ExclusiveNotApplicable = "E2012", "Exclusive mode does not exist on this system",
        "Only Windows has WASAPI exclusive mode. Other systems use their normal low-latency audio path.";
    OutputChoiceNotSaved = "E2013", "This machine's audio output choice could not be saved",
        "The choice is kept beside the logs, in the per-user Crucible folder. Check that the folder can be written; the message names the file.";
    AudioDriverError = "E2099", "The audio driver reported an error",
        "The log line includes the driver's own code (HRESULT on Windows). Send the log.";
    Dropout = "W2101", "An audible dropout: the sound was interrupted",
        "Use a larger buffer (`audio.output --buffer 256` or 512; `--block` in m0-play), close CPU-heavy programs, use the High performance power plan and keep a laptop on mains power.";
    LateWakeup = "W2102", "The audio thread woke up late: a risk of dropouts",
        "Harmless if rare. If it is frequent, follow the advice for W2101.";

    // ---- 3: MIDI
    MidiUnavailable = "E3001", "The MIDI system could not be opened",
        "Restart the program. If it repeats, reconnect the keyboard and send the log.";
    MidiPortBusy = "E3002", "A MIDI input could not be opened",
        "On Windows a MIDI input can be open in only one program at a time. Close other music programs (DAWs, standalone Omnisphere, browser MIDI pages, the keyboard's own editor).";
    NoMidiInputs = "W3003", "No MIDI inputs were found",
        "Connect the keyboard before starting the program and check that `crucible midi.inputs` lists it. Unplugging and replugging it can help.";

    // ---- 4: plugins
    PluginNotFound = "E4001", "No installed plugin matches that name",
        "Run `m0-play list` or `crucible plugin.scan` to see what is installed, or give the full path to the .vst3 bundle.";
    PluginAmbiguous = "E4002", "The plugin name matches more than one plugin",
        "Use more of the name, or give the full path to the .vst3 bundle.";
    PluginLoadFailed = "E4003", "The plugin could not be loaded",
        "Check that the plugin works in another host. Spectrasonics plugins need their STEAM folder present and authorised. The log lists each loading step.";
    PluginCrashed = "E4004", "The plugin crashed while being scanned",
        "The plugin is broken, incompatible or needs an update. Other plugins are not affected.";
    PluginTimedOut = "E4005", "The plugin took too long to load and was stopped",
        "Large libraries can be slow the first time. Retry with a longer --timeout, for example 180.";
    StateRestoreFailed = "W4006", "Saved plugin settings could not be restored",
        "The plugin starts with its default sound. The saved file may come from a different plugin version.";
    StateSaveFailed = "E4007", "Plugin settings could not be saved",
        "Check that the folder is writable. The plugin's own error is in the log.";
    EditorFailed = "E4008", "The plugin window could not be opened",
        "Sound and MIDI may still work. Send the log.";
    EditorEventError = "W4009", "The plugin window reported an error while running",
        "Usually harmless. Send the log if the window misbehaves.";
    OfflineRefused = "W4010", "The plugin refused offline rendering mode",
        "The render still runs, in real-time mode. Usually harmless.";
    RenderSilent = "W4011", "The render is digital silence",
        "Check that a patch is loaded. Large sample libraries may need a longer --preroll, for example 10.";
    RenderNonFinite = "E4012", "The render contains invalid samples (NaN or infinity)",
        "A plugin bug. Note the patch and send the log.";
    PluginProcessFailed = "E4013", "The plugin failed while processing audio",
        "Note the patch and send the log.";
    HostInitFailed = "E4014", "The plugin host could not start",
        "Send the log.";

    // ---- 5: projects and commands
    UnknownCommand = "E5001", "Unknown command",
        "Run `crucible help` for the list of commands.";
    BadArgument = "E5002", "A command argument is missing or wrong",
        "Run `crucible help <command>` to see its arguments.";
    TrackNotFound = "E5003", "No track matches",
        "Refer to a track by its name, its position (1, 2, ...) or its id (#3). `track.list` shows all three.";
    TrackAmbiguous = "E5004", "More than one track has that name",
        "Use the track's position or id instead. `track.list` shows both.";
    ValueOutOfRange = "E5005", "A value is outside the allowed range",
        "The message gives the allowed range.";
    UnsavedChanges = "E5006", "The project has unsaved changes",
        "Save with `project.save`, or repeat the command with --discard to drop the changes.";
    ProjectReadFailed = "E5007", "The project could not be opened",
        "Check that the folder holds a project.json written by Crucible. The message says what was wrong with it.";
    ProjectTooNew = "E5008", "The project was saved by a newer version of Crucible",
        "Update Crucible on this machine to the version used on the other machine.";
    ProjectSaveFailed = "E5009", "The project could not be saved",
        "Check free disk space and that you may write to the folder. OneDrive folders can lock files while syncing. The previous save is untouched.";
    RecoveryIncomplete = "W5010", "Crash recovery could not replay every unsaved change",
        "The project opened with the changes up to the one named. The full journal was kept beside the project.";
    ClipNotFound = "E5011", "No clip matches",
        "Refer to clips by id: #4, or several as #4,#7. `clip.list` shows them.";
    ClipEditRefused = "E5012", "That clip edit is not possible",
        "The message says why, for example a clip would start before bar 1, end before it starts, or land on a track of the wrong kind.";
    ClipboardEmpty = "E5013", "Nothing has been copied",
        "Copy or cut clips first with `edit.copy` or `edit.cut`.";
    NoteNotFound = "E5014", "No note matches",
        "Notes are found by pitch and start, in beats from the clip's start. `clip.notes <clip>` lists them.";
    ProjectNotSaved = "E5015", "The project needs a folder first",
        "Imported audio is copied into the project's folder, so save the project once (`project.save <folder>`), then import.";
    AudioFileUnreadable = "E5016", "The audio file could not be read",
        "Crucible reads WAV (16, 24 or 32-bit integer, or 32-bit float), MP3 and FLAC, mono or stereo. The message gives the file and the reason.";
    SampleRateMismatch = "E5017", "The audio is at another sample rate than the project",
        "The message names both rates. Convert it (the window asks; with commands, add --convert): the file is kept as it is and converted while it plays. Or set the project to the file's rate with project.rate.";
    AudioMissing = "E5018", "An audio file the project uses is missing",
        "The file should be in the project's `audio` folder. Copy it back there; the message names it.";
    ExportFailed = "E5019", "The export could not be written",
        "Check the folder exists and is writable, and that the file is not open in another program.";
    ExportClipped = "W5020", "The export has samples above full scale",
        "Fixed-point files cannot hold them, so they were clipped. Lower the master gain by at least the reported amount, or export 32-bit float.";
    EffectNotFound = "E5021", "No effect in that slot",
        "Effects are numbered from 1 on each track, top to bottom. `fx.list <track>` shows them.";
    EffectRefused = "E5022", "That effect setting is not possible",
        "The message says why: an unknown effect or setting, a value out of range, or an impulse-response file Crucible cannot use. `fx.kinds` lists every effect and setting.";
    ConvertFailed = "E5023", "Audio could not be converted to the project's sample rate",
        "The message says why. Send a report (Settings, Diagnostics, Create report file), and use the file at its own rate meanwhile.";

    // ---- 9: crashes and bugs
    NativeCrash = "E9001", "The program crashed",
        "The log names the file the crash happened in. A plugin file there means the plugin crashed. Send the log and the .dmp file next to it.";
    Panic = "E9002", "Internal error: a bug in Crucible",
        "Send the log. It contains the source location of the bug.";
    UiError = "E9003", "Internal error in the window: a bug in Crucible",
        "The window may behave oddly from here on. Save under a new name, restart Crucible and send a report (Settings, Diagnostics, Create report file).";
    Uncategorised = "E9099", "Unexpected error",
        "Send the log.";
}

impl Code {
    /// Look a code up by its identifier, e.g. `"E2004"`.
    pub fn from_id(id: &str) -> Option<Code> {
        Code::ALL.iter().copied().find(|c| c.id() == id)
    }

    pub fn severity(self) -> Severity {
        if self.id().starts_with('W') { Severity::Warning } else { Severity::Error }
    }

    /// Process exit code for a run that ended with this error. Grouped by area, so a script
    /// can tell an audio failure from a plugin failure without parsing text.
    pub fn exit_code(self) -> i32 {
        match self {
            Code::Usage => 2,
            Code::Panic => 70,
            _ => match self.id().as_bytes()[1] {
                b'1' => 3,
                b'2' => 10,
                b'3' => 11,
                b'4' => 12,
                b'5' => 13,
                _ => 1,
            },
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// An error that carries a [`Code`]. Display: `E2004: <message>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DawError {
    pub code: Code,
    pub message: String,
}

impl DawError {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl DawError {
    /// Read back an error from its text form (`E2004: message`), e.g. from a report field
    /// that had to be stored as a string.
    pub fn parse(text: &str) -> Option<DawError> {
        let (id, message) = text.split_once(": ")?;
        Some(DawError::new(Code::from_id(id)?, message))
    }
}

impl fmt::Display for DawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.id(), self.message)
    }
}

impl std::error::Error for DawError {}

/// Attach a code to any error: `thing().code(Code::FileRead)?`.
pub trait CodeExt<T> {
    fn code(self, code: Code) -> Result<T, DawError>;
}

impl<T, E: fmt::Display> CodeExt<T> for Result<T, E> {
    fn code(self, code: Code) -> Result<T, DawError> {
        self.map_err(|e| DawError::new(code, format!("{e:#}")))
    }
}

/// The first code found anywhere in an error's chain of causes.
pub fn find_code(e: &anyhow::Error) -> Option<Code> {
    e.chain().find_map(|c| c.downcast_ref::<DawError>()).map(|d| d.code)
}

/// The codes table as Markdown, for `docs/11-error-codes.md`.
pub fn markdown_table() -> String {
    let mut s = String::from("| Code | Meaning | What to try |\n|---|---|---|\n");
    for c in Code::ALL {
        s.push_str(&format!("| `{}` | {} | {} |\n", c.id(), c.title(), c.help().replace('|', "\\|")));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn ids_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for c in Code::ALL {
            let id = c.id();
            assert!(seen.insert(&id[1..]), "number of {id} used twice");
            assert_eq!(id.len(), 5, "{id}");
            assert!(id.starts_with('E') || id.starts_with('W'), "{id}");
            assert!(id[1..].chars().all(|ch| ch.is_ascii_digit()), "{id}");
            assert!(!c.title().is_empty() && !c.help().is_empty(), "{id}");
        }
    }

    #[test]
    fn code_survives_added_context() {
        let e = anyhow::Error::from(DawError::new(Code::DeviceInUse, "busy")).context("starting audio").context("play");
        assert_eq!(find_code(&e), Some(Code::DeviceInUse));
        assert_eq!(find_code(&anyhow::anyhow!("plain")), None);
        let r2: anyhow::Result<()> = Err(std::io::Error::other("disk")).code(Code::FileWrite).context("saving");
        assert_eq!(find_code(&r2.unwrap_err()), Some(Code::FileWrite));
    }

    #[test]
    fn text_form_round_trips() {
        let e = DawError::new(Code::BufferRejected, "too small: 0x88890019");
        assert_eq!(DawError::parse(&e.to_string()), Some(e));
        assert_eq!(DawError::parse("no code here"), None);
    }

    #[test]
    fn exit_codes_follow_area() {
        assert_eq!(Code::Usage.exit_code(), 2);
        assert_eq!(Code::FileRead.exit_code(), 3);
        assert_eq!(Code::DeviceInUse.exit_code(), 10);
        assert_eq!(Code::MidiPortBusy.exit_code(), 11);
        assert_eq!(Code::PluginLoadFailed.exit_code(), 12);
        assert_eq!(Code::TrackNotFound.exit_code(), 13);
        assert_eq!(Code::Panic.exit_code(), 70);
        assert_eq!(Code::NativeCrash.exit_code(), 1);
    }

    #[test]
    fn every_code_is_documented() {
        let doc = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/11-error-codes.md");
        let text = std::fs::read_to_string(&doc).expect("docs/11-error-codes.md exists");
        for c in Code::ALL {
            assert!(text.contains(&format!("`{}`", c.id())), "{} missing from {}. Regenerate with `crucible codes --markdown`.", c.id(), doc.display());
        }
    }
}
