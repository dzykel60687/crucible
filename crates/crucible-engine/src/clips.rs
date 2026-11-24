//! Clip, note and clipboard commands. Every mouse edit in the timeline and piano roll ends up
//! as one of these, so undo, the crash journal and the CLI work for them without extra code.
//!
//! Positions in arguments are beats (quarter notes) from the song start, or from the clip start
//! for notes; inside the model they are whole ticks (`PPQ` per beat). A command that touches
//! several clips (`#3,#5`) is one undo step.
//!
//! Portability: no OS-specific code.

use super::{Engine, clean_name, db, range};
use crate::command::{Args, Kind, Reply, Spec, opt, req};
use crate::Effect;
use crucible_diagnostics::{Code, DawError};
use crucible_project::{Clip, Content, Note, PPQ, TrackKind};
use serde_json::json;

const MAX_NOTES: usize = 100_000;
const CLIPS: crate::command::Arg = req("clips", Kind::Text, "clip id (#4) or several (#4,#7)");
const CLIP: crate::command::Arg = req("clip", Kind::Text, "clip id, e.g. #4");
const TRACK: crate::command::Arg = req("track", Kind::Track, "name, position (1, 2, ...) or id (#3)");

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "clip.add",
        args: &[
            TRACK,
            req("start", Kind::Number, "position in beats from the song start (0 = bar 1)"),
            req("length", Kind::Number, "length in beats"),
            opt("name", Kind::Text, "clip name"),
        ],
        help: "Add an empty clip: MIDI on an instrument track, audio (placeholder until import exists) on an audio track.",
        effect: Effect::Edit,
        run: clip_add,
    },
    Spec { name: "clip.remove", args: &[CLIPS], help: "Remove clips.", effect: Effect::Edit, run: clip_remove },
    Spec {
        name: "clip.move",
        args: &[
            CLIPS,
            opt("by", Kind::Number, "beats to move, negative moves earlier"),
            opt("to", Kind::Number, "move so the earliest clip starts at this beat"),
            opt("tracks", Kind::Int, "tracks to move down (negative: up)"),
        ],
        help: "Move clips in time and/or to other tracks, keeping their spacing.",
        effect: Effect::Edit,
        run: clip_move,
    },
    Spec {
        name: "clip.copy",
        args: &[
            CLIPS,
            opt("by", Kind::Number, "beats from the originals (default: right after them)"),
            opt("to", Kind::Number, "place the earliest copy at this beat"),
            opt("tracks", Kind::Int, "tracks down (negative: up)"),
        ],
        help: "Duplicate clips. With no position the copies go right after the originals.",
        effect: Effect::Edit,
        run: clip_copy,
    },
    Spec {
        name: "clip.trim",
        args: &[CLIP, opt("start", Kind::Number, "new left edge, in beats"), opt("end", Kind::Number, "new right edge, in beats")],
        help: "Move a clip's left or right edge. The content stays where it is in time.",
        effect: Effect::Edit,
        run: clip_trim,
    },
    Spec {
        name: "clip.split",
        args: &[CLIPS, req("at", Kind::Number, "position in beats from the song start")],
        help: "Split clips at a position. Clips that do not cross it are left alone.",
        effect: Effect::Edit,
        run: clip_split,
    },
    Spec { name: "clip.rename", args: &[CLIP, req("name", Kind::Text, "new name")], help: "Rename a clip.", effect: Effect::Edit, run: clip_rename },
    Spec {
        name: "clip.gain",
        args: &[CLIP, req("db", Kind::Number, "gain in dB, -144 to +24")],
        help: "Set a clip's gain.",
        effect: Effect::Edit,
        run: clip_gain,
    },
    Spec {
        name: "clip.quantize",
        args: &[CLIP, opt("grid", Kind::Number, "grid in beats (default 0.25 = sixteenths)")],
        help: "Snap a MIDI clip's note starts to the grid.",
        effect: Effect::Edit,
        run: clip_quantize,
    },
    Spec {
        name: "clip.list",
        args: &[opt("track", Kind::Track, "only this track")],
        help: "List clips with id, track, position and length.",
        effect: Effect::Read,
        run: clip_list,
    },
    Spec { name: "clip.notes", args: &[CLIP], help: "List a MIDI clip's notes.", effect: Effect::Read, run: clip_notes },
    Spec {
        name: "note.add",
        args: &[
            CLIP,
            req("pitch", Kind::Int, "MIDI note number, 60 = middle C"),
            req("start", Kind::Number, "beats from the clip start"),
            req("length", Kind::Number, "length in beats"),
            opt("velocity", Kind::Int, "1 to 127 (default 100)"),
        ],
        help: "Add a note to a MIDI clip.",
        effect: Effect::Edit,
        run: note_add,
    },
    Spec {
        name: "note.remove",
        args: &[CLIP, req("pitch", Kind::Int, "the note's pitch"), req("start", Kind::Number, "the note's start, beats from the clip start")],
        help: "Remove a note, found by pitch and start.",
        effect: Effect::Edit,
        run: note_remove,
    },
    Spec {
        name: "note.edit",
        args: &[
            CLIP,
            req("pitch", Kind::Int, "the note's pitch"),
            req("start", Kind::Number, "the note's start, beats from the clip start"),
            opt("to-pitch", Kind::Int, "new pitch"),
            opt("to-start", Kind::Number, "new start, beats from the clip start"),
            opt("length", Kind::Number, "new length in beats"),
            opt("velocity", Kind::Int, "new velocity, 1 to 127"),
        ],
        help: "Move, resize or change the velocity of a note.",
        effect: Effect::Edit,
        run: note_edit,
    },
    Spec {
        name: "edit.copy",
        args: &[CLIPS],
        help: "Copy clips to Crucible's clipboard.",
        effect: Effect::Clipboard,
        run: edit_copy,
    },
    Spec { name: "edit.cut", args: &[CLIPS], help: "Copy clips to the clipboard and remove them.", effect: Effect::Edit, run: edit_cut },
    Spec {
        name: "edit.paste",
        args: &[TRACK, req("at", Kind::Number, "beats from the song start for the earliest clip")],
        help: "Paste the clipboard. Clips keep their spacing in time and across tracks.",
        effect: Effect::Edit,
        run: edit_paste,
    },
    Spec {
        name: "project.demo",
        args: &[],
        help: "Fill an empty project with a short demo song (tracks, clips, notes).",
        effect: Effect::Edit,
        run: project_demo,
    },
];

/// What `edit.copy` keeps: each clip with its start relative to the earliest one and its track
/// relative to the topmost one.
#[derive(Clone, Default)]
pub(crate) struct Clipboard {
    items: Vec<(i64, i64, Clip)>,
}

// ---------------------------------------------------------------- helpers

pub(crate) fn ticks(beats: f64) -> Result<i64, DawError> {
    range("position", beats, -1.0e6, 1.0e6, " beats")?;
    Ok((beats * PPQ as f64).round() as i64)
}

pub(crate) fn beats(t: i64) -> f64 {
    t as f64 / PPQ as f64
}

fn refuse(msg: impl Into<String>) -> DawError {
    DawError::new(Code::ClipEditRefused, msg)
}

/// Indices of the clips named by `#4,#7` (or `4,7`). Order kept, duplicates dropped.
fn clip_indices(e: &Engine, raw: &str) -> Result<Vec<usize>, DawError> {
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let id: u32 = part.trim_start_matches('#').parse().map_err(|_| DawError::new(Code::ClipNotFound, format!("'{part}' is not a clip id")))?;
        let i = e.project.clip_index(id).ok_or_else(|| DawError::new(Code::ClipNotFound, format!("no clip has id #{id}")))?;
        if !out.contains(&i) {
            out.push(i);
        }
    }
    if out.is_empty() {
        return Err(DawError::new(Code::ClipNotFound, "no clip ids given"));
    }
    Ok(out)
}

fn one_clip(e: &Engine, a: &Args) -> Result<usize, DawError> {
    match clip_indices(e, a.need_text("clip")?)?.as_slice() {
        [i] => Ok(*i),
        _ => Err(DawError::new(Code::BadArgument, "this command takes one clip")),
    }
}

fn check(e: &Engine, i: usize) -> Result<(), DawError> {
    e.project.check_clip(&e.project.clips[i]).map_err(refuse)
}

fn track_id(e: &Engine, a: &Args) -> Result<u32, DawError> {
    let i = e.track_at(a, "track")?;
    Ok(e.project.tracks[i].id)
}

/// Track id `n` tracks below (or above, if negative) the track holding clip `i`.
fn shifted_track(e: &Engine, track: u32, n: i64) -> Result<u32, DawError> {
    let from = e.project.track_index(track).unwrap_or(0) as i64;
    let to = from + n;
    e.project.tracks.get(usize::try_from(to).unwrap_or(usize::MAX)).map(|t| t.id).ok_or_else(|| refuse(format!("there is no track {} places from track {}", n, from + 1)))
}

fn clip_json(c: &Clip) -> serde_json::Value {
    let (kind, extra) = match &c.content {
        Content::Audio { source, .. } => ("audio", json!({ "source": source })),
        Content::Midi { notes } => ("midi", json!({ "notes": notes.len() })),
    };
    json!({ "id": c.id, "track": c.track, "name": c.name, "kind": kind, "start": beats(c.start), "length": beats(c.length),
            "offset": beats(c.offset), "gain_db": c.gain_db, "content": extra })
}

/// A beat count for people: at most three decimals, no trailing zeros (31.921, not 31.920833333333334).
fn short(b: f64) -> String {
    let t = format!("{b:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub(crate) fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// The delta (ticks) a move or copy applies, from `--by` or `--to`.
fn delta(e: &Engine, a: &Args, idx: &[usize], default: i64) -> Result<i64, DawError> {
    let earliest = idx.iter().map(|&i| e.project.clips[i].start).min().unwrap_or(0);
    match (a.number("by"), a.number("to")) {
        (Some(_), Some(_)) => Err(DawError::new(Code::BadArgument, "give --by or --to, not both")),
        (Some(b), None) => ticks(b),
        (None, Some(t)) => Ok(ticks(t)? - earliest),
        (None, None) => Ok(default),
    }
}

// ---------------------------------------------------------------- clip commands

fn clip_add(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let track = track_id(e, a)?;
    let kind = e.project.tracks[e.project.track_index(track).unwrap_or(0)].kind;
    let (start, length) = (ticks(a.need_number("start")?)?, ticks(a.need_number("length")?)?);
    let n = e.project.clips.iter().filter(|c| c.track == track).count() + 1;
    let name = match a.text("name") {
        Some(n) => clean_name(n)?,
        None => format!("{} {n}", if kind == TrackKind::Audio { "Audio" } else { "MIDI" }),
    };
    let content = match kind {
        TrackKind::Audio => Content::Audio { source: format!("{name}.wav"), source_length: length, frames: 0, rate: 0 },
        TrackKind::Instrument => Content::Midi { notes: Vec::new() },
    };
    let clip = Clip { id: e.project.next_clip_id, track, name, start, length, offset: 0, gain_db: 0.0, content };
    e.project.check_clip(&clip).map_err(refuse)?;
    let id = e.project.add_clip(clip);
    let c = &e.project.clips[e.project.clips.len() - 1];
    Ok(Reply::new(format!("Added clip #{id} '{}' at beat {}", c.name, beats(c.start)), clip_json(c)))
}

fn clip_remove(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let idx = clip_indices(e, a.need_text("clips")?)?;
    let ids: Vec<u32> = idx.iter().map(|&i| e.project.clips[i].id).collect();
    let gone: std::collections::HashSet<u32> = ids.iter().copied().collect();
    e.project.clips.retain(|c| !gone.contains(&c.id));
    Ok(Reply::new(format!("Removed {}", plural(ids.len(), "clip")), json!({ "removed": ids })))
}

fn clip_move(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let idx = clip_indices(e, a.need_text("clips")?)?;
    let d = delta(e, a, &idx, 0)?;
    let n = a.int("tracks").unwrap_or(0);
    for &i in &idx {
        let to_track = if n != 0 { shifted_track(e, e.project.clips[i].track, n)? } else { e.project.clips[i].track };
        let c = e.project.clip_mut(i);
        c.start += d;
        c.track = to_track;
    }
    for &i in &idx {
        check(e, i)?;
    }
    Ok(Reply::new(
        format!("Moved {} by {:+} beats{}", plural(idx.len(), "clip"), beats(d), if n != 0 { format!(", {n:+} tracks") } else { String::new() }),
        json!({ "moved": idx.iter().map(|&i| e.project.clips[i].id).collect::<Vec<_>>() }),
    ))
}

fn clip_copy(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let idx = clip_indices(e, a.need_text("clips")?)?;
    let first = idx.iter().map(|&i| e.project.clips[i].start).min().unwrap_or(0);
    let last = idx.iter().map(|&i| e.project.clips[i].end()).max().unwrap_or(0);
    let d = delta(e, a, &idx, last - first)?;
    let n = a.int("tracks").unwrap_or(0);
    let mut new_ids = Vec::new();
    for &i in &idx {
        let mut c = Clip::clone(&e.project.clips[i]);
        c.start += d;
        if n != 0 {
            c.track = shifted_track(e, c.track, n)?;
        }
        e.project.check_clip(&c).map_err(refuse)?;
        new_ids.push(e.project.add_clip(c));
    }
    Ok(Reply::new(format!("Copied {} (new: {})", plural(idx.len(), "clip"), ids_text(&new_ids)), json!({ "created": new_ids })))
}

fn ids_text(ids: &[u32]) -> String {
    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
}

fn clip_trim(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    if a.number("start").is_none() && a.number("end").is_none() {
        return Err(DawError::new(Code::BadArgument, "give --start, --end or both"));
    }
    if let Some(s) = a.number("start") {
        let s = ticks(s)?;
        let c = e.project.clip_mut(i);
        let d = s - c.start;
        c.start = s;
        c.offset += d;
        c.length -= d;
    }
    if let Some(end) = a.number("end") {
        let end = ticks(end)?;
        let c = e.project.clip_mut(i);
        c.length = end - c.start;
    }
    check(e, i)?;
    let c = &e.project.clips[i];
    Ok(Reply::new(format!("Clip #{} now {} to {} beats", c.id, beats(c.start), beats(c.end())), clip_json(c)))
}

fn clip_split(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let idx = clip_indices(e, a.need_text("clips")?)?;
    let at = ticks(a.need_number("at")?)?;
    let mut created = Vec::new();
    for i in idx {
        let c = Clip::clone(&e.project.clips[i]);
        if at <= c.start || at >= c.end() {
            continue;
        }
        let cut = at - c.start;
        let mut right = c.clone();
        right.start = at;
        right.offset = c.offset + cut;
        right.length = c.length - cut;
        e.project.clip_mut(i).length = cut;
        // MIDI: each side keeps only the notes that start inside it, shortened to its edge.
        if let Content::Midi { notes } = &c.content {
            let keep = |from: i64, to: i64| -> Vec<Note> {
                notes.iter().filter(|n| n.start >= from && n.start < to).map(|n| Note { length: n.length.min(to - n.start), ..*n }).collect()
            };
            let split_point = c.offset + cut;
            let left_notes: Vec<Note> = notes.iter().filter(|n| n.start < split_point).map(|n| Note { length: n.length.min(split_point - n.start), ..*n }).collect();
            e.project.clip_mut(i).content = Content::Midi { notes: left_notes };
            right.content = Content::Midi { notes: keep(split_point, i64::MAX) };
        }
        created.push(e.project.add_clip(right));
    }
    if created.is_empty() {
        return Err(refuse(format!("no given clip crosses beat {}", beats(at))));
    }
    Ok(Reply::new(format!("Split {} at beat {}", plural(created.len(), "clip"), beats(at)), json!({ "created": created })))
}

fn clip_rename(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    e.project.clip_mut(i).name = clean_name(a.need_text("name")?)?;
    let c = &e.project.clips[i];
    Ok(Reply::new(format!("Clip #{} renamed to '{}'", c.id, c.name), clip_json(c)))
}

fn clip_gain(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    e.project.clip_mut(i).gain_db = range("gain", a.need_number("db")?, -144.0, 24.0, " dB")?;
    let c = &e.project.clips[i];
    Ok(Reply::new(format!("Clip #{} gain {}", c.id, db(c.gain_db)), clip_json(c)))
}

fn clip_quantize(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    let grid = ticks(range("grid", a.number("grid").unwrap_or(0.25), 1.0 / 64.0, 16.0, " beats")?)?.max(1);
    let c = e.project.clip_mut(i);
    let origin = c.start - c.offset; // timeline tick of content tick 0
    let Content::Midi { notes } = &mut c.content else {
        return Err(refuse("only MIDI clips can be quantized"));
    };
    let mut moved = 0;
    for n in notes.iter_mut() {
        let abs = origin + n.start;
        let snapped = ((abs as f64 / grid as f64).round() as i64) * grid;
        let new = (snapped - origin).max(0);
        if new != n.start {
            n.start = new;
            moved += 1;
        }
    }
    let id = c.id;
    Ok(Reply::new(format!("Quantized clip #{id}: {} moved", plural(moved, "note")), json!({ "moved": moved })))
}

fn clip_list(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let only = if a.text("track").is_some() { Some(track_id(e, a)?) } else { None };
    let list: Vec<&Clip> = e.project.clips.iter().map(|c| &**c).filter(|c| only.is_none_or(|t| c.track == t)).collect();
    if list.is_empty() {
        return Ok(Reply::new("No clips.", json!([])));
    }
    let mut text = format!("{:>5}  {:<20} {:<16} {:>9} {:>9}  kind\n", "id", "name", "track", "start", "length");
    for c in &list {
        let track = e.project.track_index(c.track).map(|i| e.project.tracks[i].name.as_str()).unwrap_or("?");
        let name: String = c.name.chars().take(20).collect();
        let track: String = track.chars().take(16).collect();
        text.push_str(&format!("{:>5}  {name:<20} {track:<16} {:>9} {:>9}  {}\n", format!("#{}", c.id), short(beats(c.start)), short(beats(c.length)), if c.is_audio() { "audio" } else { "midi" }));
    }
    Ok(Reply::new(text.trim_end().to_string(), json!(list.iter().map(|c| clip_json(c)).collect::<Vec<_>>())))
}

/// Notes as the user sees them: start in beats from the clip start.
fn clip_notes(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    let c = &e.project.clips[i];
    let Content::Midi { notes } = &c.content else { return Err(refuse("only MIDI clips have notes")) };
    let mut text = format!("Clip #{} '{}': {}\n", c.id, c.name, plural(notes.len(), "note"));
    let mut data = Vec::new();
    for n in notes {
        let s = beats(n.start - c.offset);
        text.push_str(&format!("  pitch {:>3}  start {:>8}  length {:>6}  velocity {:>3}\n", n.pitch, s, beats(n.length), n.velocity));
        data.push(json!({ "pitch": n.pitch, "start": s, "length": beats(n.length), "velocity": n.velocity }));
    }
    Ok(Reply::new(text.trim_end().to_string(), json!(data)))
}

// ---------------------------------------------------------------- notes

fn midi_notes(e: &mut Engine, i: usize) -> Result<(i64, &mut Vec<Note>), DawError> {
    let c = e.project.clip_mut(i);
    let offset = c.offset;
    match &mut c.content {
        Content::Midi { notes } => Ok((offset, notes)),
        Content::Audio { .. } => Err(refuse("only MIDI clips have notes")),
    }
}

fn pitch(v: i64) -> Result<u8, DawError> {
    Ok(range("pitch", v as f64, 0.0, 127.0, "")? as u8)
}

fn velocity(v: i64) -> Result<u8, DawError> {
    Ok(range("velocity", v as f64, 1.0, 127.0, "")? as u8)
}

fn find_note(notes: &[Note], p: u8, start: i64) -> Result<usize, DawError> {
    notes
        .iter()
        .position(|n| n.pitch == p && n.start == start)
        .ok_or_else(|| DawError::new(Code::NoteNotFound, format!("no note with pitch {p} starts at that position")))
}

fn note_add(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    let p = pitch(a.need_int("pitch")?)?;
    let v = velocity(a.int("velocity").unwrap_or(100))?;
    let (start, length) = (ticks(a.need_number("start")?)?, ticks(a.need_number("length")?)?);
    let (offset, notes) = midi_notes(e, i)?;
    if notes.len() >= MAX_NOTES {
        return Err(refuse(format!("a clip holds at most {MAX_NOTES} notes")));
    }
    notes.push(Note { pitch: p, start: start + offset, length, velocity: v });
    check(e, i)?;
    Ok(Reply::new(format!("Added note {p} at beat {} of clip #{}", beats(start), e.project.clips[i].id), json!({ "pitch": p, "start": beats(start) })))
}

fn note_remove(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    let p = pitch(a.need_int("pitch")?)?;
    let start = ticks(a.need_number("start")?)?;
    let (offset, notes) = midi_notes(e, i)?;
    let k = find_note(notes, p, start + offset)?;
    notes.remove(k);
    Ok(Reply::new(format!("Removed note {p} at beat {}", beats(start)), json!({ "pitch": p, "start": beats(start) })))
}

fn note_edit(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let i = one_clip(e, a)?;
    let p = pitch(a.need_int("pitch")?)?;
    let start = ticks(a.need_number("start")?)?;
    let to_pitch = a.int("to-pitch").map(pitch).transpose()?;
    let to_start = a.number("to-start").map(ticks).transpose()?;
    let length = a.number("length").map(ticks).transpose()?;
    let vel = a.int("velocity").map(velocity).transpose()?;
    let (offset, notes) = midi_notes(e, i)?;
    let k = find_note(notes, p, start + offset)?;
    let n = &mut notes[k];
    if let Some(v) = to_pitch {
        n.pitch = v;
    }
    if let Some(v) = to_start {
        n.start = v + offset;
    }
    if let Some(v) = length {
        n.length = v;
    }
    if let Some(v) = vel {
        n.velocity = v;
    }
    let n = *n;
    check(e, i)?;
    Ok(Reply::new(
        format!("Note now pitch {} at beat {}, length {}, velocity {}", n.pitch, beats(n.start - offset), beats(n.length), n.velocity),
        json!({ "pitch": n.pitch, "start": beats(n.start - offset), "length": beats(n.length), "velocity": n.velocity }),
    ))
}

// ---------------------------------------------------------------- clipboard
//
// The clipboard lives in the engine, not the project, so the CLI and the window share it.
// `edit.copy` is journaled (Effect::Clipboard) so that crash recovery can replay a later paste.

fn fill_clipboard(e: &mut Engine, idx: &[usize]) {
    let first = idx.iter().map(|&i| e.project.clips[i].start).min().unwrap_or(0);
    let top = idx.iter().filter_map(|&i| e.project.track_index(e.project.clips[i].track)).min().unwrap_or(0) as i64;
    e.clipboard.items = idx
        .iter()
        .map(|&i| {
            let c = &e.project.clips[i];
            (c.start - first, e.project.track_index(c.track).unwrap_or(0) as i64 - top, Clip::clone(c))
        })
        .collect();
}

fn edit_copy(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let idx = clip_indices(e, a.need_text("clips")?)?;
    fill_clipboard(e, &idx);
    Ok(Reply::new(format!("Copied {}", plural(idx.len(), "clip")), json!({ "clipboard": idx.len() })))
}

fn edit_cut(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let idx = clip_indices(e, a.need_text("clips")?)?;
    fill_clipboard(e, &idx);
    let ids: Vec<u32> = idx.iter().map(|&i| e.project.clips[i].id).collect();
    let gone: std::collections::HashSet<u32> = ids.iter().copied().collect();
    e.project.clips.retain(|c| !gone.contains(&c.id));
    Ok(Reply::new(format!("Cut {}", plural(ids.len(), "clip")), json!({ "removed": ids })))
}

fn edit_paste(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    if e.clipboard.items.is_empty() {
        return Err(DawError::new(Code::ClipboardEmpty, "the clipboard is empty"));
    }
    let base_track = e.track_at(a, "track")? as i64;
    let at = ticks(a.need_number("at")?)?;
    let mut created = Vec::new();
    for (dt, dtrack, clip) in e.clipboard.items.clone() {
        let ti = usize::try_from(base_track + dtrack).ok().filter(|&t| t < e.project.tracks.len());
        let Some(ti) = ti else { return Err(refuse("the pasted clips would run past the last track")) };
        let c = Clip { track: e.project.tracks[ti].id, start: at + dt, ..clip };
        e.project.check_clip(&c).map_err(refuse)?;
        created.push(e.project.add_clip(c));
    }
    Ok(Reply::new(format!("Pasted {} ({})", plural(created.len(), "clip"), ids_text(&created)), json!({ "created": created })))
}

// ---------------------------------------------------------------- demo song

/// A short song so the window has something to show. Deterministic, so tests and screenshots
/// match. One undo step.
fn project_demo(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    if !e.project.tracks.is_empty() {
        return Err(refuse("project.demo needs an empty project; run project.new first"));
    }
    let p = &mut e.project;
    p.name = "Demo song".into();
    p.tempo_bpm = 96.0;
    let b = |beats: i64| beats * PPQ;
    let mut seed: u64 = 7;
    let mut rnd = move || {
        seed = seed * 16807 % 2_147_483_647;
        seed as f64 / 2_147_483_647.0
    };
    let add_track = |p: &mut crucible_project::Project, name: &str, kind: TrackKind, gain: f64, pan: f64| {
        let id = p.add_track(name.into(), kind);
        let t = p.tracks.last_mut().expect("just added");
        t.gain_db = gain;
        t.pan = pan;
        id
    };
    let drums = add_track(p, "Drums", TrackKind::Audio, 0.0, 0.0);
    let bass = add_track(p, "Bass", TrackKind::Instrument, 0.0, 0.0);
    let keys = add_track(p, "Keys", TrackKind::Instrument, 0.0, -0.2);
    let pad = add_track(p, "Pad", TrackKind::Instrument, -4.5, 0.0);
    let vox = add_track(p, "Vox", TrackKind::Audio, -2.0, 0.0);

    // Audio clips show 2 beats into their source with 2 spare beats at the end, so both edges trim.
    let audio = |track: u32, name: &str, start: i64, len: i64| Clip {
        id: 0,
        track,
        name: name.into(),
        start: b(start),
        length: b(len),
        offset: b(2),
        gain_db: 0.0,
        content: Content::Audio { source: format!("{name}.wav"), source_length: b(len + 4), frames: 0, rate: 0 },
    };
    let midi = |track: u32, name: &str, start: i64, len: i64, notes: Vec<Note>| Clip {
        id: 0,
        track,
        name: name.into(),
        start: b(start),
        length: b(len),
        offset: 0,
        gain_db: 0.0,
        content: Content::Midi { notes },
    };
    let chords = |bars: i64, rnd: &mut dyn FnMut() -> f64, long_only: bool| -> Vec<Note> {
        let prog = [[57u8, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
        let mut v = Vec::new();
        for bar in 0..bars {
            let ch = prog[(bar % 4) as usize];
            for p in ch {
                v.push(Note { pitch: p, start: b(bar * 4), length: b(4) - PPQ / 4, velocity: 80 + (rnd() * 20.0) as u8 });
            }
            if !long_only {
                for k in 0..4 {
                    v.push(Note { pitch: ch[k % 3] + 12, start: b(bar * 4 + k as i64) + PPQ / 2, length: PPQ * 45 / 100, velocity: 60 + (rnd() * 40.0) as u8 });
                }
            }
        }
        v
    };
    let bassline = |beats: i64, rnd: &mut dyn FnMut() -> f64| -> Vec<Note> {
        let roots = [45u8, 41, 36, 43];
        let mut v = Vec::new();
        for half in 0..beats * 2 {
            if rnd() < 0.18 {
                continue;
            }
            let up = if rnd() < 0.2 { 12 } else { 0 };
            v.push(Note { pitch: roots[((half / 8) % 4) as usize] + up, start: half * PPQ / 2, length: PPQ * 4 / 10, velocity: 70 + (rnd() * 40.0) as u8 });
        }
        v
    };
    let clips = vec![
        audio(drums, "Drums verse", 0, 32),
        audio(drums, "Drums chorus", 32, 32),
        midi(bass, "Bass line", 8, 24, bassline(24, &mut rnd)),
        midi(bass, "Bass chorus", 32, 32, bassline(32, &mut rnd)),
        midi(keys, "Keyscape chords", 0, 32, chords(8, &mut rnd, false)),
        midi(keys, "Chords B", 40, 24, chords(6, &mut rnd, false)),
        midi(pad, "Omnisphere pad", 16, 48, chords(12, &mut rnd, true)),
        audio(vox, "Vox take 3", 16, 16),
        audio(vox, "Vox take 5", 36, 24),
    ];
    for c in clips {
        p.check_clip(&c).map_err(refuse)?;
        p.add_clip(c);
    }
    Ok(Reply::new(format!("Demo song: {} tracks, {} clips", p.tracks.len(), p.clips.len()), json!({ "tracks": p.tracks.len(), "clips": p.clips.len() })))
}
