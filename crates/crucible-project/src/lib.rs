//! Project store: the project model as plain data, and its files.
//!
//! A project is a folder (docs/02-architecture.md, "Project store"):
//!
//! ```text
//! MySong/
//!   project.json    the model, human-readable, with a format number
//!   journal.log     commands since the last save, one per line, for crash recovery
//! ```
//!
//! Rules this file keeps:
//! - **Plain data.** `Project` is a struct of values. No behaviour beyond small helpers; the
//!   engine's commands are the only code that changes it.
//! - **Atomic save.** Write a temporary file, flush it to disk, rename it over the old one. A
//!   crash mid-save leaves the previous save intact.
//! - **Validate on load.** A hand-edited or damaged file is refused with a reason, never half-used.
//! - **Nothing machine-specific.** No device names, buffer sizes or plugin paths.
//!
//! Portability: std file I/O only. `rename` replaces the target atomically on Windows, macOS
//! and Linux. No OS-specific code.

use crucible_diagnostics::{Code, DawError};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// File format version written by this build. Bump it when a change would confuse an older build.
/// 1: tracks only. 2: clips and notes (a format-1 file opens with no clips).
pub const FORMAT: u32 = 2;
pub const PROJECT_FILE: &str = "project.json";
pub const JOURNAL_FILE: &str = "journal.log";

/// Musical time resolution: ticks per quarter note. Positions are whole ticks, never floats, so
/// edits never drift. 960 divides by 2, 3, 4, 5, 6, 8, 10, 12, 15 and 16.
pub const PPQ: i64 = 960;

/// Sample rates a project may use.
pub const SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub name: String,
    pub sample_rate: u32,
    pub tempo_bpm: f64,
    /// Beats per bar and the note value of one beat, e.g. [3, 4].
    pub time_signature: [u8; 2],
    pub master_gain_db: f64,
    pub tracks: Vec<Track>,
    /// Ids are never reused, so a command journal stays valid after deletions.
    pub next_track_id: u32,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default = "first_id")]
    pub next_clip_id: u32,
}

fn first_id() -> u32 {
    1
}

/// A region on a track's timeline showing a window onto some content.
///
/// The content (audio file or MIDI notes) has its own time origin. The clip shows the part from
/// `offset` to `offset + length` of it, placed at `start` on the timeline. Trimming the left edge
/// moves `start` and `offset` together, so the content stays where it was in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u32,
    pub track: u32,
    pub name: String,
    /// Timeline position, in ticks.
    pub start: i64,
    /// Visible length, in ticks. Always above zero.
    pub length: i64,
    /// Where the visible part begins inside the content, in ticks. Never below zero.
    pub offset: i64,
    pub gain_db: f64,
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Content {
    /// `source` is a file name in the project's audio folder. Until audio import exists (M2) its
    /// length is kept in ticks; with real files it becomes a sample count.
    Audio { source: String, source_length: i64 },
    Midi { notes: Vec<Note> },
}

/// A MIDI note. `start` is in ticks from the content origin, not from the clip start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub start: i64,
    pub length: i64,
    pub velocity: u8,
}

impl Clip {
    pub fn end(&self) -> i64 {
        self.start + self.length
    }
    pub fn is_audio(&self) -> bool {
        matches!(self.content, Content::Audio { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: u32,
    pub name: String,
    pub kind: TrackKind,
    pub gain_db: f64,
    /// -1.0 is full left, 0.0 centre, 1.0 full right.
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Instrument,
}

impl TrackKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TrackKind::Audio => "audio",
            TrackKind::Instrument => "instrument",
        }
    }
}

impl Default for Project {
    fn default() -> Self {
        Project {
            format: FORMAT,
            name: "Untitled".into(),
            sample_rate: 48_000,
            tempo_bpm: 120.0,
            time_signature: [4, 4],
            master_gain_db: 0.0,
            tracks: Vec::new(),
            next_track_id: 1,
            clips: Vec::new(),
            next_clip_id: 1,
        }
    }
}

impl Project {
    /// Add a track at the end and return its id.
    pub fn add_track(&mut self, name: String, kind: TrackKind) -> u32 {
        let id = self.next_track_id;
        self.next_track_id += 1;
        self.tracks.push(Track { id, name, kind, gain_db: 0.0, pan: 0.0, mute: false, solo: false });
        id
    }

    pub fn track_index(&self, id: u32) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    pub fn clip_index(&self, id: u32) -> Option<usize> {
        self.clips.iter().position(|c| c.id == id)
    }

    /// Add a clip with the next free id and return the id. The caller has checked it with `check_clip`.
    pub fn add_clip(&mut self, mut clip: Clip) -> u32 {
        clip.id = self.next_clip_id;
        self.next_clip_id += 1;
        self.clips.push(clip);
        self.next_clip_id - 1
    }

    /// Why a clip would break the model, if it would. Used by every clip command and by `validate`.
    pub fn check_clip(&self, c: &Clip) -> Result<(), String> {
        let Some(t) = self.track_index(c.track).map(|i| &self.tracks[i]) else {
            return Err(format!("clip #{} is on track #{}, which does not exist", c.id, c.track));
        };
        if c.is_audio() != (t.kind == TrackKind::Audio) {
            let what = if c.is_audio() { "an audio clip" } else { "a MIDI clip" };
            return Err(format!("{what} cannot go on the {} track '{}'", t.kind.as_str(), t.name));
        }
        if c.start < 0 {
            return Err(format!("clip #{} would start before bar 1", c.id));
        }
        if c.length <= 0 {
            return Err(format!("clip #{} would have no length", c.id));
        }
        if c.offset < 0 {
            return Err(format!("clip #{} would start before its content begins", c.id));
        }
        if !c.gain_db.is_finite() {
            return Err(format!("clip #{} has an invalid gain", c.id));
        }
        match &c.content {
            Content::Audio { source_length, .. } if c.offset + c.length > *source_length => {
                Err(format!("clip #{} would run past the end of its audio", c.id))
            }
            Content::Midi { notes } => notes
                .iter()
                .find(|n| n.pitch > 127 || !(1..=127).contains(&n.velocity) || n.length <= 0 || n.start < 0)
                .map_or(Ok(()), |n| Err(format!("clip #{} has an invalid note (pitch {}, velocity {})", c.id, n.pitch, n.velocity))),
            _ => Ok(()),
        }
    }

    /// Check the invariants every command relies on. Used after loading a file.
    pub fn validate(&self) -> Result<(), String> {
        if !SAMPLE_RATES.contains(&self.sample_rate) {
            return Err(format!("sample rate {} is not one of {SAMPLE_RATES:?}", self.sample_rate));
        }
        if !(self.tempo_bpm.is_finite() && (20.0..=999.0).contains(&self.tempo_bpm)) {
            return Err(format!("tempo {} is outside 20 to 999 BPM", self.tempo_bpm));
        }
        let [beats, unit] = self.time_signature;
        if !(1..=32).contains(&beats) || ![1, 2, 4, 8, 16, 32].contains(&unit) {
            return Err(format!("time signature {beats}/{unit} is not valid"));
        }
        if !self.master_gain_db.is_finite() {
            return Err("master gain is not a number".into());
        }
        let mut ids = std::collections::HashSet::new();
        for t in &self.tracks {
            if !ids.insert(t.id) {
                return Err(format!("track id {} is used twice", t.id));
            }
            if t.id >= self.next_track_id {
                return Err(format!("track id {} is not below next_track_id {}", t.id, self.next_track_id));
            }
            if !t.gain_db.is_finite() || !(-1.0..=1.0).contains(&t.pan) {
                return Err(format!("track {} has an invalid gain or pan", t.id));
            }
        }
        let mut clip_ids = std::collections::HashSet::new();
        for c in &self.clips {
            if !clip_ids.insert(c.id) || c.id >= self.next_clip_id {
                return Err(format!("clip id {} is used twice or not below next_clip_id", c.id));
            }
            self.check_clip(c)?;
        }
        Ok(())
    }
}

/// The project file inside a folder. Also accepts a path to the file itself.
pub fn project_file(path: &Path) -> PathBuf {
    if path.file_name().is_some_and(|n| n == PROJECT_FILE) { path.to_path_buf() } else { path.join(PROJECT_FILE) }
}

/// The project folder for a path that may name the folder or the file inside it.
pub fn project_folder(path: &Path) -> PathBuf {
    if path.file_name().is_some_and(|n| n == PROJECT_FILE) {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        path.to_path_buf()
    }
}

pub fn load(path: &Path) -> Result<Project, DawError> {
    let file = project_file(path);
    let fail = |why: String| DawError::new(Code::ProjectReadFailed, format!("{}: {why}", file.display()));
    let text = std::fs::read_to_string(&file).map_err(|e| fail(e.to_string()))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| fail(format!("not valid JSON ({e})")))?;
    let format = value.get("format").and_then(|f| f.as_u64()).ok_or_else(|| fail("no format number".into()))?;
    if format > u64::from(FORMAT) {
        return Err(DawError::new(
            Code::ProjectTooNew,
            format!("{} is format {format}; this build reads up to format {FORMAT}", file.display()),
        ));
    }
    let project: Project = serde_json::from_value(value).map_err(|e| fail(e.to_string()))?;
    project.validate().map_err(fail)?;
    Ok(project)
}

/// Save atomically into `folder`, creating it if needed. Refuses a non-empty folder that is not
/// already a Crucible project, so a typo cannot scatter files into an unrelated folder.
pub fn save(project: &Project, folder: &Path) -> Result<(), DawError> {
    let fail = |why: String| DawError::new(Code::ProjectSaveFailed, format!("{}: {why}", folder.display()));
    if folder.exists() {
        if !folder.is_dir() {
            return Err(fail("exists and is not a folder".into()));
        }
        let is_project = folder.join(PROJECT_FILE).exists();
        let empty = std::fs::read_dir(folder).map_err(|e| fail(e.to_string()))?.next().is_none();
        if !is_project && !empty {
            return Err(fail("the folder already holds other files and is not a Crucible project".into()));
        }
    }
    std::fs::create_dir_all(folder).map_err(|e| fail(format!("creating the folder: {e}")))?;
    let json = serde_json::to_string_pretty(project).map_err(|e| fail(e.to_string()))?;
    let tmp = folder.join(format!("{PROJECT_FILE}.tmp"));
    let write = || -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        Ok(())
    };
    write().map_err(|e| fail(format!("writing {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, folder.join(PROJECT_FILE)).map_err(|e| fail(format!("replacing {PROJECT_FILE}: {e}")))?;
    Ok(())
}

/// Append-only list of the commands applied since the last save. Each line is flushed to the
/// operating system as it is written, so it survives a crash of the program.
pub struct Journal {
    file: File,
}

impl Journal {
    pub fn open(folder: &Path) -> Result<Journal, DawError> {
        let path = folder.join(JOURNAL_FILE);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map(|file| Journal { file })
            .map_err(|e| DawError::new(Code::ProjectSaveFailed, format!("opening {}: {e}", path.display())))
    }

    pub fn append(&mut self, command: &str) -> Result<(), DawError> {
        self.file
            .write_all(format!("{command}\n").as_bytes())
            .map_err(|e| DawError::new(Code::ProjectSaveFailed, format!("writing the journal: {e}")))
    }
}

/// Commands left in the journal by a session that did not save. Empty if there are none.
pub fn pending_journal(folder: &Path) -> Vec<String> {
    std::fs::read_to_string(folder.join(JOURNAL_FILE))
        .map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

/// Empty the journal after a successful save.
pub fn clear_journal(folder: &Path) -> Result<(), DawError> {
    let path = folder.join(JOURNAL_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(DawError::new(Code::ProjectSaveFailed, format!("clearing {}: {e}", path.display()))),
    }
}

/// Move the journal aside instead of replaying it. Never deletes it.
pub fn set_journal_aside(folder: &Path) -> Result<PathBuf, DawError> {
    let from = folder.join(JOURNAL_FILE);
    for n in 1..1000 {
        let to = folder.join(format!("journal-set-aside-{n}.log"));
        if !to.exists() {
            return std::fs::rename(&from, &to)
                .map(|()| to)
                .map_err(|e| DawError::new(Code::ProjectSaveFailed, format!("moving {}: {e}", from.display())));
        }
    }
    Err(DawError::new(Code::ProjectSaveFailed, "too many set-aside journals; remove some"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crucible-project-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = temp("roundtrip");
        let mut p = Project { name: "Song".into(), ..Default::default() };
        p.add_track("Piano".into(), TrackKind::Instrument);
        p.tracks[0].pan = -0.5;
        save(&p, &dir).unwrap();
        assert!(!dir.join("project.json.tmp").exists());
        assert_eq!(load(&dir).unwrap(), p);
        assert_eq!(load(&dir.join(PROJECT_FILE)).unwrap(), p);
        save(&p, &dir).unwrap(); // overwrite in place
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clips_round_trip_and_format_1_still_opens() {
        let dir = temp("clips");
        let mut p = Project::default();
        let t = p.add_track("Keys".into(), TrackKind::Instrument);
        let notes = vec![Note { pitch: 60, start: 0, length: PPQ, velocity: 100 }];
        let clip = Clip { id: 0, track: t, name: "A".into(), start: 4 * PPQ, length: 8 * PPQ, offset: 0, gain_db: 0.0, content: Content::Midi { notes } };
        p.check_clip(&clip).unwrap();
        p.add_clip(clip);
        save(&p, &dir).unwrap();
        assert_eq!(load(&dir).unwrap(), p);

        // An audio clip may not go on an instrument track.
        let audio = Content::Audio { source: "x.wav".into(), source_length: 10 };
        let bad = Clip { id: 9, track: t, name: "x".into(), start: 0, length: 10, offset: 0, gain_db: 0.0, content: audio };
        assert!(p.check_clip(&bad).unwrap_err().contains("audio clip"));

        let old = r#"{"format":1,"name":"Old","sample_rate":48000,"tempo_bpm":120.0,"time_signature":[4,4],"master_gain_db":0.0,"tracks":[],"next_track_id":1}"#;
        std::fs::write(dir.join(PROJECT_FILE), old).unwrap();
        let p1 = load(&dir).unwrap();
        assert!(p1.clips.is_empty() && p1.next_clip_id == 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_foreign_folder_newer_format_and_damage() {
        let dir = temp("foreign");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        assert_eq!(save(&Project::default(), &dir).unwrap_err().code, Code::ProjectSaveFailed);

        std::fs::write(dir.join(PROJECT_FILE), r#"{"format": 99}"#).unwrap();
        assert_eq!(load(&dir).unwrap_err().code, Code::ProjectTooNew);
        std::fs::write(dir.join(PROJECT_FILE), "{ not json").unwrap();
        assert_eq!(load(&dir).unwrap_err().code, Code::ProjectReadFailed);

        let mut bad = Project::default();
        bad.add_track("A".into(), TrackKind::Audio);
        bad.next_track_id = 1;
        std::fs::write(dir.join(PROJECT_FILE), serde_json::to_string(&bad).unwrap()).unwrap();
        let e = load(&dir).unwrap_err();
        assert!(e.code == Code::ProjectReadFailed && e.message.contains("next_track_id"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_appends_clears_and_sets_aside() {
        let dir = temp("journal");
        std::fs::create_dir_all(&dir).unwrap();
        let mut j = Journal::open(&dir).unwrap();
        j.append("track.add --name A").unwrap();
        j.append("track.gain --track #1 --db -6").unwrap();
        drop(j);
        assert_eq!(pending_journal(&dir).len(), 2);
        let aside = set_journal_aside(&dir).unwrap();
        assert!(aside.exists() && pending_journal(&dir).is_empty());
        Journal::open(&dir).unwrap().append("x").unwrap();
        clear_journal(&dir).unwrap();
        assert!(pending_journal(&dir).is_empty());
        clear_journal(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
