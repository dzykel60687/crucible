//! Crucible's headless engine: the project model, every command that edits it, undo, redo and
//! the crash journal (D-022).
//!
//! Today this is the editing core only. Transport, the render graph and the audio thread come
//! with the rest of M2 (docs/06-roadmap.md), after M0 has passed on real hardware.
//!
//! How an edit runs ([`Engine::run`]):
//! 1. Track arguments are resolved to stable ids (`#7`), so history and journal lines mean the
//!    same thing after tracks are renamed or reordered.
//! 2. The project is copied, the command runs, and on any error the copy is put back. A failed
//!    command never leaves a half-made change.
//! 3. If the project changed: the copy goes on the undo stack and the command's exact text goes
//!    to the journal.
//!
//! Undo keeps whole copies of the project model. The model is small plain data (audio lives in
//! files, never in it), so this is simpler and safer than writing an inverse for every command.
//!
//! Portability: no OS-specific code, no UI or Tauri dependency.

mod clips;
pub mod command;

pub use command::{Args, Effect, Kind, Reply, Spec, Value};
use command::{opt, req};
use crucible_diagnostics::{self as diag, Code, DawError};
use crucible_project::{self as store, Journal, Project, TrackKind};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const UNDO_LIMIT: usize = 1000;
const MAX_TRACKS: usize = 512;
const GAIN_RANGE: (f64, f64) = (-144.0, 24.0);

pub struct Engine {
    project: Project,
    /// Where the project was last saved or opened from. `None` until the first save.
    folder: Option<PathBuf>,
    /// The project as last saved or opened, to tell whether there are unsaved changes.
    saved: Project,
    undo: Vec<Step>,
    redo: Vec<Step>,
    journal: Option<Journal>,
    /// Every non-read command run this session, in canonical form.
    history: Vec<String>,
    /// Clips copied with edit.copy or edit.cut. Shared by every front end.
    clipboard: clips::Clipboard,
}

struct Step {
    command: String,
    before: Project,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            project: Project::default(),
            folder: None,
            saved: Project::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            journal: None,
            history: Vec::new(),
            clipboard: clips::Clipboard::default(),
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// Everything a front end needs to draw: the model, where it lives, and what undo/redo would do.
    pub fn state(&self) -> serde_json::Value {
        json!({
            "project": self.project,
            "folder": self.folder,
            "dirty": self.is_dirty(),
            "undo": self.undo.last().map(|s| &s.command),
            "redo": self.redo.last().map(|s| &s.command),
            "journal": self.journal.is_some(),
        })
    }

    pub fn specs() -> &'static [Spec<Engine>] {
        &ALL_SPECS
    }

    /// Parse and run one command given as tokens, name first.
    pub fn execute(&mut self, tokens: &[String]) -> Result<Reply, DawError> {
        let name = tokens.first().ok_or_else(|| DawError::new(Code::UnknownCommand, "empty command"))?;
        let spec = command::find(&ALL_SPECS, name).ok_or_else(|| command::unknown(name, ALL_SPECS.iter().map(|s| s.name)))?;
        let args = command::parse(spec, &tokens[1..])?;
        self.run(spec, args)
    }

    /// Run a parsed command. See the module docs for what happens around it.
    pub fn run(&mut self, spec: &Spec<Engine>, mut args: Args) -> Result<Reply, DawError> {
        for a in spec.args.iter().filter(|a| a.kind == Kind::Track) {
            if let Some(raw) = args.text(a.name) {
                let id = self.resolve_track(raw)?;
                args.set(a.name, Value::Text(format!("#{id}")));
            }
        }
        let line = command::canonical(spec, &args);
        let before = (spec.effect != Effect::Read).then(|| self.project.clone());
        let reply = match (spec.run)(self, &args) {
            Ok(r) => r,
            Err(e) => {
                if spec.effect == Effect::Edit
                    && let Some(b) = before
                {
                    self.project = b;
                }
                return Err(e);
            }
        };
        if let Some(before) = before {
            self.history.push(line.clone());
            let changed = self.project != before;
            if spec.effect == Effect::Edit && changed {
                self.undo.push(Step { command: line.clone(), before });
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
            if (matches!(spec.effect, Effect::Edit | Effect::History) && changed) || spec.effect == Effect::Clipboard {
                self.write_journal(&line);
            }
        }
        Ok(reply)
    }

    fn write_journal(&mut self, line: &str) {
        if let Some(j) = &mut self.journal
            && let Err(e) = j.append(line)
        {
            diag::warn(e.code, format!("{}. Crash recovery is off until the next save.", e.message));
            self.journal = None;
        }
    }

    /// A track by `#id`, by 1-based position, or by name (case-insensitive).
    fn resolve_track(&self, raw: &str) -> Result<u32, DawError> {
        let tracks = &self.project.tracks;
        let not_found = |what: String| DawError::new(Code::TrackNotFound, what);
        if let Some(id) = raw.strip_prefix('#') {
            let id: u32 = id.parse().map_err(|_| not_found(format!("'{raw}' is not a track id")))?;
            return self.project.track_index(id).map(|_| id).ok_or_else(|| not_found(format!("no track has id #{id}")));
        }
        if let Ok(pos) = raw.parse::<usize>() {
            return tracks
                .get(pos.wrapping_sub(1))
                .map(|t| t.id)
                .ok_or_else(|| not_found(format!("no track at position {pos}; there are {}", tracks.len())));
        }
        let hits: Vec<(usize, u32)> =
            tracks.iter().enumerate().filter(|(_, t)| t.name.eq_ignore_ascii_case(raw)).map(|(i, t)| (i + 1, t.id)).collect();
        match hits.as_slice() {
            [(_, id)] => Ok(*id),
            [] => {
                let names: Vec<&str> = tracks.iter().take(12).map(|t| t.name.as_str()).collect();
                Err(not_found(format!("no track is named '{raw}'. Tracks: {}", if names.is_empty() { "none".into() } else { names.join(", ") })))
            }
            many => Err(DawError::new(
                Code::TrackAmbiguous,
                format!("'{raw}' names the tracks at positions {}", many.iter().map(|(p, _)| p.to_string()).collect::<Vec<_>>().join(", ")),
            )),
        }
    }

    /// Index of a track argument that `run` already resolved to `#id`.
    fn track_at(&self, args: &Args, name: &str) -> Result<usize, DawError> {
        let id: u32 = args.need_text(name)?.trim_start_matches('#').parse().map_err(|_| DawError::new(Code::TrackNotFound, "bad track id"))?;
        self.project.track_index(id).ok_or_else(|| DawError::new(Code::TrackNotFound, format!("no track has id #{id}")))
    }

    fn reset(&mut self, project: Project, folder: Option<PathBuf>) {
        self.saved = project.clone();
        self.project = project;
        self.folder = folder;
        self.undo.clear();
        self.redo.clear();
        self.journal = None;
    }

    fn refuse_if_dirty(&self, args: &Args) -> Result<(), DawError> {
        if self.is_dirty() && args.flag("discard") != Some(true) {
            return Err(DawError::new(Code::UnsavedChanges, format!("'{}' has changes that are not saved", self.project.name)));
        }
        Ok(())
    }

    /// Replay a crashed session's journal. Returns how many commands were replayed.
    fn recover(&mut self, folder: &Path, lines: &[String]) -> usize {
        let mut done = 0;
        for (n, line) in lines.iter().enumerate() {
            let result = command::split_statements(line).and_then(|stmts| {
                stmts.iter().try_for_each(|tokens| {
                    let name = tokens.first().map(String::as_str).unwrap_or("");
                    let spec = command::find(&ALL_SPECS, name)
                        .filter(|s| matches!(s.effect, Effect::Edit | Effect::History | Effect::Clipboard))
                        .ok_or_else(|| DawError::new(Code::BadArgument, format!("'{name}' cannot appear in a journal")))?;
                    let args = command::parse(spec, &tokens[1..])?;
                    self.run(spec, args).map(|_| ())
                })
            });
            if let Err(e) = result {
                let kept = store::set_journal_aside(folder).map(|p| p.display().to_string()).unwrap_or_else(|e| e.to_string());
                diag::warn(Code::RecoveryIncomplete, format!("journal line {} '{line}' failed: {e}. Full journal kept as {kept}", n + 1));
                // Re-journal what was replayed, so the recovered state is still crash-safe.
                if let Ok(mut j) = Journal::open(folder) {
                    for l in &self.history {
                        let _ = j.append(l);
                    }
                    self.journal = Some(j);
                }
                return done;
            }
            done += 1;
        }
        self.journal = Journal::open(folder).ok();
        done
    }
}

// ---------------------------------------------------------------- the command table

const KINDS: &[&str] = &["audio", "instrument"];
const ON_OFF: &[&str] = &["on", "off"];
const DISCARD: command::Arg = opt("discard", Kind::Bool, "drop unsaved changes instead of refusing");
const TRACK: command::Arg = req("track", Kind::Track, "name, position (1, 2, ...) or id (#3)");

/// The engine's command table: the rows below plus the clip, note and clipboard rows in clips.rs.
static ALL_SPECS: LazyLock<Vec<Spec<Engine>>> = LazyLock::new(|| SPECS.iter().chain(clips::SPECS).map(|s| Spec { ..*s }).collect());

static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "project.new",
        args: &[opt("name", Kind::Text, "project name (default Untitled)"), DISCARD],
        help: "Start a new, empty project. Refuses if the current one has unsaved changes.",
        effect: Effect::File,
        run: project_new,
    },
    Spec {
        name: "project.open",
        args: &[
            req("folder", Kind::Text, "project folder (or its project.json)"),
            DISCARD,
            opt("no-recover", Kind::Bool, "set a crash journal aside instead of replaying it"),
        ],
        help: "Open a project. Unsaved changes from a crash are replayed from its journal.",
        effect: Effect::File,
        run: project_open,
    },
    Spec {
        name: "project.save",
        args: &[opt("folder", Kind::Text, "where to save; needed the first time, or to save a copy elsewhere")],
        help: "Save the project. Atomic: a crash mid-save keeps the previous save.",
        effect: Effect::File,
        run: project_save,
    },
    Spec { name: "project.info", args: &[], help: "Show the project's settings and state.", effect: Effect::Read, run: project_info },
    Spec {
        name: "project.state",
        args: &[],
        help: "The whole project model plus undo/redo state, as data. Front ends redraw from this.",
        effect: Effect::Read,
        run: project_state,
    },
    Spec {
        name: "project.rename",
        args: &[req("name", Kind::Text, "new project name")],
        help: "Rename the project. The folder keeps its name.",
        effect: Effect::Edit,
        run: project_rename,
    },
    Spec {
        name: "project.tempo",
        args: &[req("bpm", Kind::Number, "beats per minute, 20 to 999")],
        help: "Set the tempo.",
        effect: Effect::Edit,
        run: project_tempo,
    },
    Spec {
        name: "project.signature",
        args: &[req("beats", Kind::Int, "beats per bar, 1 to 32"), req("unit", Kind::Int, "beat note value: 1, 2, 4, 8, 16 or 32")],
        help: "Set the time signature, e.g. project.signature 6 8.",
        effect: Effect::Edit,
        run: project_signature,
    },
    Spec {
        name: "project.rate",
        args: &[req("hz", Kind::Int, "44100, 48000, 88200, 96000, 176400 or 192000")],
        help: "Set the project sample rate.",
        effect: Effect::Edit,
        run: project_rate,
    },
    Spec {
        name: "master.gain",
        args: &[req("db", Kind::Number, "gain in dB, -144 to +24")],
        help: "Set the master gain.",
        effect: Effect::Edit,
        run: master_gain,
    },
    Spec {
        name: "track.add",
        args: &[opt("name", Kind::Text, "track name (default: kind and number)"), opt("kind", Kind::Choice(KINDS), "audio (default) or instrument")],
        help: "Add a track at the end.",
        effect: Effect::Edit,
        run: track_add,
    },
    Spec { name: "track.remove", args: &[TRACK], help: "Remove a track.", effect: Effect::Edit, run: track_remove },
    Spec {
        name: "track.rename",
        args: &[TRACK, req("name", Kind::Text, "new name")],
        help: "Rename a track.",
        effect: Effect::Edit,
        run: track_rename,
    },
    Spec {
        name: "track.move",
        args: &[TRACK, req("position", Kind::Int, "new position, 1 = top")],
        help: "Move a track to another position.",
        effect: Effect::Edit,
        run: track_move,
    },
    Spec {
        name: "track.gain",
        args: &[TRACK, req("db", Kind::Number, "gain in dB, -144 to +24")],
        help: "Set a track's gain.",
        effect: Effect::Edit,
        run: track_gain,
    },
    Spec {
        name: "track.pan",
        args: &[TRACK, req("pan", Kind::Number, "-100 = full left, 0 = centre, 100 = full right")],
        help: "Set a track's pan.",
        effect: Effect::Edit,
        run: track_pan,
    },
    Spec {
        name: "track.mute",
        args: &[TRACK, opt("state", Kind::Choice(ON_OFF), "on (default) or off")],
        help: "Mute or unmute a track.",
        effect: Effect::Edit,
        run: track_mute,
    },
    Spec {
        name: "track.solo",
        args: &[TRACK, opt("state", Kind::Choice(ON_OFF), "on (default) or off")],
        help: "Solo or unsolo a track.",
        effect: Effect::Edit,
        run: track_solo,
    },
    Spec { name: "track.list", args: &[], help: "List tracks with position, id and mix settings.", effect: Effect::Read, run: track_list },
    Spec { name: "undo", args: &[], help: "Undo the last edit.", effect: Effect::History, run: undo },
    Spec { name: "redo", args: &[], help: "Redo the last undone edit.", effect: Effect::History, run: redo },
    Spec { name: "history", args: &[], help: "List this session's commands and the undo position.", effect: Effect::Read, run: history },
];

// ---------------------------------------------------------------- helpers

fn range(what: &str, v: f64, lo: f64, hi: f64, unit: &str) -> Result<f64, DawError> {
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(DawError::new(Code::ValueOutOfRange, format!("{what} {v}{unit} is outside {lo}{unit} to {hi}{unit}")))
    }
}

fn clean_name(raw: &str) -> Result<String, DawError> {
    let n = raw.trim();
    if n.is_empty() || n.chars().count() > 100 || n.chars().any(char::is_control) {
        return Err(DawError::new(Code::BadArgument, format!("'{raw}' is not a usable name: 1 to 100 characters, no control characters")));
    }
    Ok(n.to_string())
}

fn db(v: f64) -> String {
    format!("{v:+.1} dB")
}

fn pan_text(p: f64) -> String {
    let pct = (p * 100.0).round() as i64;
    match pct {
        0 => "C".into(),
        n if n < 0 => format!("L{}", -n),
        n => format!("R{n}"),
    }
}

fn track_json(p: &Project, i: usize) -> serde_json::Value {
    let t = &p.tracks[i];
    json!({ "position": i + 1, "id": t.id, "name": t.name, "kind": t.kind.as_str(),
            "gain_db": t.gain_db, "pan": (t.pan * 100.0).round(), "mute": t.mute, "solo": t.solo })
}

fn track_reply(e: &Engine, i: usize, text: String) -> Reply {
    Reply::new(text, track_json(&e.project, i))
}

// ---------------------------------------------------------------- project commands

fn project_new(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    e.refuse_if_dirty(a)?;
    let name = a.text("name").map(clean_name).transpose()?.unwrap_or_else(|| "Untitled".into());
    e.reset(Project { name: name.clone(), ..Default::default() }, None);
    Ok(Reply::new(format!("New project '{name}' (not saved yet)"), json!({ "name": name })))
}

fn project_open(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    e.refuse_if_dirty(a)?;
    let path = PathBuf::from(a.need_text("folder")?);
    let project = store::load(&path)?;
    let folder = store::project_folder(&path);
    let name = project.name.clone();
    e.reset(project, Some(folder.clone()));
    e.history.clear();

    let pending = store::pending_journal(&folder);
    let mut text = format!("Opened '{name}' from {}", folder.display());
    let mut recovered = 0;
    if !pending.is_empty() {
        if a.flag("no-recover") == Some(true) {
            let aside = store::set_journal_aside(&folder)?;
            text.push_str(&format!("\n{} unsaved changes from an earlier session were set aside in {}", pending.len(), aside.display()));
        } else {
            recovered = e.recover(&folder, &pending);
            text.push_str(&format!(
                "\nRecovered {recovered} of {} unsaved changes from an earlier session that did not save. Save to keep them, or undo them.",
                pending.len()
            ));
        }
    }
    if e.journal.is_none() {
        e.journal = Some(Journal::open(&folder)?);
    }
    Ok(Reply::new(text, json!({ "name": name, "folder": folder, "recovered": recovered, "pending": pending.len() })))
}

fn project_save(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let target = match a.text("folder") {
        Some(f) => store::project_folder(Path::new(f)),
        None => e.folder.clone().ok_or_else(|| {
            DawError::new(Code::BadArgument, "this project has never been saved; give a folder: project.save <folder>")
        })?,
    };
    // A new project takes the folder's name, so files and titles agree.
    if e.project.name == "Untitled"
        && let Some(n) = target.file_name()
    {
        e.project.name = n.to_string_lossy().into_owned();
    }
    store::save(&e.project, &target)?;
    store::clear_journal(&target)?;
    e.journal = Some(Journal::open(&target)?);
    e.saved = e.project.clone();
    e.folder = Some(target.clone());
    Ok(Reply::new(format!("Saved '{}' to {}", e.project.name, target.display()), json!({ "name": e.project.name, "folder": target })))
}

fn project_info(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let p = &e.project;
    let folder = e.folder.as_ref().map(|f| f.display().to_string()).unwrap_or_else(|| "not saved yet".into());
    let text = format!(
        "Project : {} ({folder}){}\nSettings: {} Hz, {} BPM, {}/{}, master {}\nTracks  : {}\nUndo    : {} steps, redo {}\nJournal : {}",
        p.name,
        if e.is_dirty() { "  [unsaved changes]" } else { "" },
        p.sample_rate,
        p.tempo_bpm,
        p.time_signature[0],
        p.time_signature[1],
        db(p.master_gain_db),
        p.tracks.len(),
        e.undo.len(),
        e.redo.len(),
        if e.journal.is_some() { "on (crash recovery)" } else { "off until the first save" },
    );
    let data = json!({ "name": p.name, "folder": e.folder, "dirty": e.is_dirty(), "sample_rate": p.sample_rate,
        "tempo_bpm": p.tempo_bpm, "time_signature": p.time_signature, "master_gain_db": p.master_gain_db,
        "tracks": p.tracks.len(), "undo": e.undo.len(), "redo": e.redo.len(), "journal": e.journal.is_some() });
    Ok(Reply::new(text, data))
}

fn project_rename(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    e.project.name = clean_name(a.need_text("name")?)?;
    Ok(Reply::new(format!("Project renamed to '{}'", e.project.name), json!({ "name": e.project.name })))
}

fn project_tempo(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    e.project.tempo_bpm = range("tempo", a.need_number("bpm")?, 20.0, 999.0, " BPM")?;
    Ok(Reply::new(format!("Tempo {} BPM", e.project.tempo_bpm), json!({ "tempo_bpm": e.project.tempo_bpm })))
}

fn project_signature(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let beats = range("beats per bar", a.need_int("beats")? as f64, 1.0, 32.0, "")? as u8;
    let unit = a.need_int("unit")?;
    if ![1, 2, 4, 8, 16, 32].contains(&unit) {
        return Err(DawError::new(Code::ValueOutOfRange, format!("beat unit {unit} must be 1, 2, 4, 8, 16 or 32")));
    }
    e.project.time_signature = [beats, unit as u8];
    Ok(Reply::new(format!("Time signature {beats}/{unit}"), json!({ "time_signature": e.project.time_signature })))
}

fn project_rate(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let hz = a.need_int("hz")?;
    if !store::SAMPLE_RATES.iter().any(|r| i64::from(*r) == hz) {
        return Err(DawError::new(Code::ValueOutOfRange, format!("{hz} Hz is not one of {:?}", store::SAMPLE_RATES)));
    }
    e.project.sample_rate = hz as u32;
    Ok(Reply::new(format!("Sample rate {hz} Hz"), json!({ "sample_rate": hz })))
}

fn master_gain(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    e.project.master_gain_db = range("gain", a.need_number("db")?, GAIN_RANGE.0, GAIN_RANGE.1, " dB")?;
    Ok(Reply::new(format!("Master {}", db(e.project.master_gain_db)), json!({ "master_gain_db": e.project.master_gain_db })))
}

// ---------------------------------------------------------------- track commands

fn track_add(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    if e.project.tracks.len() >= MAX_TRACKS {
        return Err(DawError::new(Code::ValueOutOfRange, format!("a project holds at most {MAX_TRACKS} tracks")));
    }
    let kind = if a.text("kind") == Some("instrument") { TrackKind::Instrument } else { TrackKind::Audio };
    let name = match a.text("name") {
        Some(n) => clean_name(n)?,
        None => {
            let n = e.project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
            format!("{} {n}", if kind == TrackKind::Audio { "Audio" } else { "Instrument" })
        }
    };
    let id = e.project.add_track(name, kind);
    let i = e.project.tracks.len() - 1;
    Ok(track_reply(e, i, format!("Added {} track '{}' at position {} (id #{id})", kind.as_str(), e.project.tracks[i].name, i + 1)))
}

fn track_remove(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    let data = track_json(&e.project, i);
    let t = e.project.tracks.remove(i);
    let before = e.project.clips.len();
    e.project.clips.retain(|c| c.track != t.id);
    let gone = before - e.project.clips.len();
    let clips = if gone > 0 { format!(" and its {gone} clip(s)") } else { String::new() };
    Ok(Reply::new(format!("Removed track '{}' (id #{}){clips}", t.name, t.id), data))
}

fn track_rename(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    e.project.tracks[i].name = clean_name(a.need_text("name")?)?;
    Ok(track_reply(e, i, format!("Track {} renamed to '{}'", i + 1, e.project.tracks[i].name)))
}

fn track_move(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    let count = e.project.tracks.len();
    let to = range("position", a.need_int("position")? as f64, 1.0, count as f64, "")? as usize - 1;
    let t = e.project.tracks.remove(i);
    e.project.tracks.insert(to, t);
    Ok(track_reply(e, to, format!("Moved '{}' to position {}", e.project.tracks[to].name, to + 1)))
}

fn track_gain(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    e.project.tracks[i].gain_db = range("gain", a.need_number("db")?, GAIN_RANGE.0, GAIN_RANGE.1, " dB")?;
    Ok(track_reply(e, i, format!("'{}' gain {}", e.project.tracks[i].name, db(e.project.tracks[i].gain_db))))
}

fn track_pan(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    e.project.tracks[i].pan = range("pan", a.need_number("pan")?, -100.0, 100.0, "")? / 100.0;
    Ok(track_reply(e, i, format!("'{}' pan {}", e.project.tracks[i].name, pan_text(e.project.tracks[i].pan))))
}

fn track_mute(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    e.project.tracks[i].mute = a.text("state") != Some("off");
    let t = &e.project.tracks[i];
    Ok(track_reply(e, i, format!("'{}' {}", t.name, if t.mute { "muted" } else { "unmuted" })))
}

fn track_solo(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = e.track_at(a, "track")?;
    e.project.tracks[i].solo = a.text("state") != Some("off");
    let t = &e.project.tracks[i];
    Ok(track_reply(e, i, format!("'{}' {}", t.name, if t.solo { "soloed" } else { "unsoloed" })))
}

fn project_state(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let text = format!("{} ({} tracks){}", e.project.name, e.project.tracks.len(), if e.is_dirty() { ", unsaved" } else { "" });
    Ok(Reply::new(text, e.state()))
}

fn track_list(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let p = &e.project;
    if p.tracks.is_empty() {
        return Ok(Reply::new("No tracks. Add one with track.add.", json!([])));
    }
    let mut text = format!("{:>3}  {:>4}  {:<24} {:<10} {:>9}  {:>4}  M S\n", "pos", "id", "name", "kind", "gain", "pan");
    for (i, t) in p.tracks.iter().enumerate() {
        let name: String = t.name.chars().take(24).collect();
        text.push_str(&format!(
            "{:>3}  {:>4}  {:<24} {:<10} {:>9}  {:>4}  {} {}\n",
            i + 1,
            format!("#{}", t.id),
            name,
            t.kind.as_str(),
            db(t.gain_db),
            pan_text(t.pan),
            if t.mute { "M" } else { "." },
            if t.solo { "S" } else { "." },
        ));
    }
    let data: Vec<_> = (0..p.tracks.len()).map(|i| track_json(p, i)).collect();
    Ok(Reply::new(text.trim_end().to_string(), json!(data)))
}

// ---------------------------------------------------------------- history

fn undo(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let Some(step) = e.undo.pop() else {
        return Ok(Reply::new("Nothing to undo", json!({ "undone": null })));
    };
    let current = std::mem::replace(&mut e.project, step.before);
    e.redo.push(Step { command: step.command.clone(), before: current });
    Ok(Reply::new(format!("Undid: {}", step.command), json!({ "undone": step.command })))
}

fn redo(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let Some(step) = e.redo.pop() else {
        return Ok(Reply::new("Nothing to redo", json!({ "redone": null })));
    };
    let current = std::mem::replace(&mut e.project, step.before);
    e.undo.push(Step { command: step.command.clone(), before: current });
    Ok(Reply::new(format!("Redid: {}", step.command), json!({ "redone": step.command })))
}

fn history(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let mut text = if e.history.is_empty() { "No commands yet this session.".to_string() } else { String::new() };
    for (i, l) in e.history.iter().enumerate() {
        text.push_str(&format!("{:>4}  {l}\n", i + 1));
    }
    text.push_str(&format!("Undo: {} steps available, redo: {}", e.undo.len(), e.redo.len()));
    Ok(Reply::new(text, json!({ "commands": e.history, "undo": e.undo.len(), "redo": e.redo.len() })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crucible_project::{Content, PPQ};

    fn run(e: &mut Engine, line: &str) -> Result<Reply, DawError> {
        let mut last = Reply::text("");
        for t in command::split_statements(line).unwrap() {
            last = e.execute(&t)?;
        }
        Ok(last)
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crucible-engine-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn edits_undo_redo_and_failed_commands_change_nothing() {
        let mut e = Engine::new();
        run(&mut e, "track.add Piano --kind instrument; track.add Bass; track.gain Bass -6; track.pan 1 -50").unwrap();
        assert_eq!(e.project().tracks[1].gain_db, -6.0);
        assert_eq!(e.project().tracks[0].pan, -0.5);
        let snapshot = e.project().clone();
        assert_eq!(run(&mut e, "track.gain Bass 99").unwrap_err().code, Code::ValueOutOfRange);
        assert_eq!(run(&mut e, "track.gain Drums 0").unwrap_err().code, Code::TrackNotFound);
        assert_eq!(e.project(), &snapshot, "failed commands must not change the project");
        run(&mut e, "undo; undo").unwrap();
        assert_eq!(e.project().tracks[1].gain_db, 0.0);
        assert_eq!(e.project().tracks[0].pan, 0.0);
        run(&mut e, "redo").unwrap(); // the last undone edit was the gain change
        assert_eq!((e.project().tracks[1].gain_db, e.project().tracks[0].pan), (-6.0, 0.0));
        run(&mut e, "track.mute 2; track.mute 2 off; track.mute 2").unwrap();
        assert!(e.project().tracks[1].mute);
        assert_eq!(run(&mut e, "redo").unwrap().text, "Nothing to redo", "a new edit clears redo");
    }

    #[test]
    fn tracks_resolve_by_name_position_and_id() {
        let mut e = Engine::new();
        run(&mut e, "track.add A; track.add B; track.add a").unwrap();
        assert_eq!(run(&mut e, "track.mute A").unwrap_err().code, Code::TrackAmbiguous);
        run(&mut e, "track.move #1 3; track.rename 1 First").unwrap();
        let names: Vec<_> = e.project().tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["First", "a", "A"]);
        assert_eq!(run(&mut e, "track.remove #99").unwrap_err().code, Code::TrackNotFound);
        assert!(e.history.iter().all(|l| !l.contains("--track A")), "history stores ids: {:?}", e.history);
    }

    #[test]
    fn save_open_and_dirty_guard() {
        let dir = temp("save");
        let mut e = Engine::new();
        assert_eq!(run(&mut e, "project.save").unwrap_err().code, Code::BadArgument);
        run(&mut e, &format!("track.add Keys; project.tempo 96; project.save \"{}\"", dir.display())).unwrap();
        assert!(!e.is_dirty());
        assert_eq!(e.project().name, dir.file_name().unwrap().to_string_lossy());
        run(&mut e, "track.add Extra").unwrap();
        assert_eq!(run(&mut e, "project.new").unwrap_err().code, Code::UnsavedChanges);
        run(&mut e, "project.new Fresh --discard").unwrap();
        run(&mut e, &format!("project.open \"{}\"", dir.display())).unwrap();
        assert_eq!(e.project().tempo_bpm, 96.0);
        // The unsaved "track.add Extra" was journaled and comes back after the reopen.
        assert_eq!(e.project().tracks.len(), 2, "journal replayed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_recovery_replays_the_journal() {
        let dir = temp("crash");
        let mut e = Engine::new();
        run(&mut e, &format!("track.add Drums; project.save \"{}\"", dir.display())).unwrap();
        run(&mut e, "track.add Vox; track.gain Vox -3; undo; track.solo Drums").unwrap();
        let expected = e.project().clone();
        drop(e); // "crash": no save

        let mut e2 = Engine::new();
        let r = run(&mut e2, &format!("project.open \"{}\"", dir.display())).unwrap();
        assert_eq!(r.data["recovered"], 4, "{}", r.text);
        assert_eq!(e2.project(), &expected);
        assert!(e2.is_dirty());
        run(&mut e2, "project.save").unwrap();
        assert!(store::pending_journal(&dir).is_empty());

        // A damaged journal line: replay stops there, keeps the journal, and reports it.
        std::fs::write(dir.join(store::JOURNAL_FILE), "track.add Ok\ntrack.gain --track #999 --db 1\ntrack.add Lost\n").unwrap();
        let mut e3 = Engine::new();
        let r = run(&mut e3, &format!("project.open \"{}\"", dir.display())).unwrap();
        assert_eq!(r.data["recovered"], 1);
        assert!(dir.join("journal-set-aside-1.log").exists());
        assert_eq!(store::pending_journal(&dir).len(), 1, "replayed part re-journaled");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_command_has_help_and_unique_name() {
        let mut names = std::collections::HashSet::new();
        for s in ALL_SPECS.iter() {
            assert!(names.insert(s.name), "{} twice", s.name);
            assert!(!s.help.is_empty() && s.args.iter().all(|a| !a.help.is_empty()), "{}", s.name);
        }
    }

    fn clip(e: &Engine, id: u32) -> &crucible_project::Clip {
        &e.project().clips[e.project().clip_index(id).expect("clip exists")]
    }

    #[test]
    fn clip_edits_are_single_undo_steps_and_refuse_bad_results() {
        let mut e = Engine::new();
        run(&mut e, "project.demo").unwrap();
        assert_eq!(e.project().clips.len(), 9);
        assert_eq!(run(&mut e, "project.demo").unwrap_err().code, Code::ClipEditRefused, "needs an empty project");

        // Two clips move together; one undo puts both back.
        let before = e.project().clone();
        run(&mut e, "clip.move #1,#2 --by 4").unwrap();
        assert_eq!((clip(&e, 1).start, clip(&e, 2).start), (4 * PPQ, 36 * PPQ));
        run(&mut e, "undo").unwrap();
        assert_eq!(e.project(), &before);

        // Audio may not land on an instrument track; nothing changes.
        assert_eq!(run(&mut e, "clip.move #1 --tracks 1").unwrap_err().code, Code::ClipEditRefused);
        assert_eq!(run(&mut e, "clip.move #1 --by -1").unwrap_err().code, Code::ClipEditRefused, "before bar 1");
        assert_eq!(e.project(), &before);

        // Trim: Vox take 3 (#8) starts at 16 and shows its source from beat 2, so 2 beats can be revealed.
        run(&mut e, "clip.trim #8 --start 15").unwrap();
        assert_eq!((clip(&e, 8).start, clip(&e, 8).offset, clip(&e, 8).length), (15 * PPQ, PPQ, 17 * PPQ));
        assert_eq!(run(&mut e, "clip.trim #8 --start 13").unwrap_err().code, Code::ClipEditRefused, "past the source start");
        assert_eq!(run(&mut e, "clip.trim #8 --end 15").unwrap_err().code, Code::ClipEditRefused, "no length left");

        // Split a MIDI clip: notes are shared out, none lost or duplicated.
        let notes = |e: &Engine, id: u32| match &clip(e, id).content {
            Content::Midi { notes } => notes.len(),
            Content::Audio { .. } => 0,
        };
        let total = notes(&e, 5);
        let r = run(&mut e, "clip.split #5 16").unwrap();
        let right = r.data["created"][0].as_u64().unwrap() as u32;
        assert_eq!(notes(&e, 5) + notes(&e, right), total);
        assert_eq!((clip(&e, 5).end(), clip(&e, right).start), (16 * PPQ, 16 * PPQ));

        // Copy with no position goes right after the original.
        let r = run(&mut e, "clip.copy #9").unwrap();
        let dup = r.data["created"][0].as_u64().unwrap() as u32;
        assert_eq!(clip(&e, dup).start, clip(&e, 9).end());

        // Removing a track removes its clips.
        run(&mut e, "track.remove Vox").unwrap();
        assert!(e.project().clip_index(9).is_none() && e.project().clip_index(dup).is_none());
        assert_eq!(run(&mut e, "clip.gain #9 -3").unwrap_err().code, Code::ClipNotFound);
    }

    #[test]
    fn notes_and_quantize() {
        let mut e = Engine::new();
        run(&mut e, "track.add Keys --kind instrument; clip.add Keys 8 4").unwrap();
        run(&mut e, "note.add #1 60 0.1 1; note.add #1 64 1.9 0.5 --velocity 90").unwrap();
        assert_eq!(run(&mut e, "note.add #1 200 0 1").unwrap_err().code, Code::ValueOutOfRange);
        run(&mut e, "note.edit #1 60 0.1 --to-pitch 62 --length 2").unwrap();
        assert_eq!(run(&mut e, "note.remove #1 60 0.1").unwrap_err().code, Code::NoteNotFound);
        run(&mut e, "clip.quantize #1 --grid 1").unwrap();
        let Content::Midi { notes } = &clip(&e, 1).content else { panic!() };
        assert_eq!(notes.iter().map(|n| (n.pitch, n.start)).collect::<Vec<_>>(), vec![(62, 0), (64, 2 * PPQ)]);
        run(&mut e, "note.remove #1 64 2").unwrap();
        assert_eq!(run(&mut e, "clip.add Keys 0 0").unwrap_err().code, Code::ClipEditRefused, "zero length");
    }

    #[test]
    fn clipboard_paste_survives_a_crash() {
        let dir = temp("clipboard");
        let mut e = Engine::new();
        assert_eq!(run(&mut e, "edit.paste 1 0").unwrap_err().code, Code::TrackNotFound);
        run(&mut e, "project.demo").unwrap();
        assert_eq!(run(&mut e, "edit.paste 1 0").unwrap_err().code, Code::ClipboardEmpty);
        run(&mut e, &format!("project.save \"{}\"", dir.display())).unwrap();
        // Copy two clips on different tracks; paste keeps their spacing in time and across tracks.
        run(&mut e, "edit.copy #3,#5; edit.paste Bass 64").unwrap();
        let pasted: Vec<_> = e.project().clips.iter().filter(|c| c.start >= 64 * PPQ).map(|c| (c.track, c.start)).collect();
        assert_eq!(pasted, vec![(2, 72 * PPQ), (3, 64 * PPQ)]);
        let expected = e.project().clone();
        drop(e); // "crash": nothing saved since the copy and paste

        let mut again = Engine::new();
        run(&mut again, &format!("project.open \"{}\"", dir.display())).unwrap();
        assert_eq!(again.project(), &expected, "journal replays the copy, then the paste");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
