//! Effect commands: insert effects on tracks (D-033). The effects themselves and their parameter
//! tables live in `crucible-dsp`; this file validates against those tables and edits the model,
//! so every knob turn is an undoable, journaled command like any other edit.
//!
//! Effects are addressed by slot: 1 is the first effect on the track, top to bottom.
//!
//! Portability: no OS-specific code.

use super::Engine;
use crate::command::{Args, Kind, Reply, Spec, opt, req};
use crucible_diagnostics::{Code, DawError};
use crucible_dsp::{Control, KINDS};
use crucible_project::{Effect as Insert, Project};
use serde_json::{Value as Json, json};
use std::path::Path;

const MAX_EFFECTS: usize = 8;
const TRACK: crate::command::Arg = req("track", Kind::Track, "name, position (1, 2, ...) or id (#3)");
const SLOT: crate::command::Arg = req("slot", Kind::Int, "effect position on the track, 1 for the first");

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec { name: "fx.kinds", args: &[], help: "The built-in effects and every setting each one has, with ranges and defaults.", effect: crate::Effect::Read, run: fx_kinds },
    Spec { name: "fx.list", args: &[opt("track", Kind::Track, "only this track")], help: "List the effects on each track.", effect: crate::Effect::Read, run: fx_list },
    Spec {
        name: "fx.add",
        args: &[TRACK, req("kind", Kind::Text, "reverb, irreverb, delay or echo"), opt("at", Kind::Int, "slot to insert at (default: the end)")],
        help: "Add an effect to a track, with its default settings.",
        effect: crate::Effect::Edit,
        run: fx_add,
    },
    Spec { name: "fx.remove", args: &[TRACK, SLOT], help: "Remove an effect from a track.", effect: crate::Effect::Edit, run: fx_remove },
    Spec {
        name: "fx.set",
        args: &[TRACK, SLOT, req("setting", Kind::Text, "setting name, e.g. decay"), req("value", Kind::Text, "a number, or an option name such as hall")],
        help: "Change one setting of an effect.",
        effect: crate::Effect::Edit,
        run: fx_set,
    },
    Spec {
        name: "fx.bypass",
        args: &[TRACK, SLOT, opt("state", Kind::Choice(&["on", "off"]), "on (default) bypasses, off brings it back")],
        help: "Bypass an effect, or bring it back. A bypassed effect passes the sound untouched.",
        effect: crate::Effect::Edit,
        run: fx_bypass,
    },
    Spec {
        name: "fx.file",
        args: &[TRACK, SLOT, req("source", Kind::Text, "impulse response in the project's audio folder, as audio.import named it")],
        help: "Choose the impulse-response file for an IR Reverb. Import it first with audio.import.",
        effect: crate::Effect::Edit,
        run: fx_file,
    },
    Spec { name: "fx.move", args: &[TRACK, SLOT, req("to", Kind::Int, "new slot")], help: "Move an effect to another slot on the same track.", effect: crate::Effect::Edit, run: fx_move },
];

fn refuse(msg: impl Into<String>) -> DawError {
    DawError::new(Code::EffectRefused, msg)
}

fn kind_of(name: &str) -> Result<&'static crucible_dsp::Kind, DawError> {
    crucible_dsp::kind(name).ok_or_else(|| refuse(format!("unknown effect '{name}'; the effects are {}", KINDS.iter().map(|k| k.name).collect::<Vec<_>>().join(", "))))
}

/// Track index and effect index for the `track` and `slot` arguments.
fn slot(e: &Engine, a: &Args) -> Result<(usize, usize), DawError> {
    let t = e.track_at(a, "track")?;
    let s = a.need_int("slot")?;
    let n = e.project.tracks[t].effects.len();
    if s < 1 || s as usize > n {
        return Err(DawError::new(Code::EffectNotFound, format!("track '{}' has {n} effect{}; there is no slot {s}", e.project.tracks[t].name, if n == 1 { "" } else { "s" })));
    }
    Ok((t, s as usize - 1))
}

fn describe(f: &Insert) -> String {
    let k = crucible_dsp::kind(&f.kind);
    let label = k.map_or(f.kind.as_str(), |k| k.label);
    let mut s = format!("{label}{}", if f.bypass { " (bypassed)" } else { "" });
    if let Some(file) = &f.file {
        s.push_str(&format!(", file {file}"));
    }
    if let Some(k) = k {
        let shown: Vec<String> = k.params.iter().map(|p| format!("{} {}", p.name, show(p, f.params.get(p.name).copied().unwrap_or(p.default)))).collect();
        s.push_str(&format!(": {}", shown.join(", ")));
    }
    s
}

fn show(p: &crucible_dsp::Param, v: f64) -> String {
    match p.control {
        Control::Choice(names) => names.get(v as usize).copied().unwrap_or("?").to_string(),
        Control::Switch => (if v >= 0.5 { "on" } else { "off" }).to_string(),
        Control::Number { unit, .. } => format!("{} {unit}", (v * 1000.0).round() / 1000.0),
    }
}

fn effect_json(f: &Insert, slot: usize) -> Json {
    json!({ "slot": slot + 1, "id": f.id, "kind": f.kind, "bypass": f.bypass, "params": f.params, "file": f.file })
}

// ---------------------------------------------------------------- commands

fn fx_kinds(_: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let kinds: Vec<Json> = KINDS
        .iter()
        .map(|k| {
            let params: Vec<Json> = k
                .params
                .iter()
                .map(|p| {
                    let control = match p.control {
                        Control::Number { min, max, unit } => json!({ "type": "number", "min": min, "max": max, "unit": unit }),
                        Control::Choice(names) => json!({ "type": "choice", "options": names }),
                        Control::Switch => json!({ "type": "switch" }),
                    };
                    json!({ "name": p.name, "label": p.label, "default": p.default, "help": p.help, "control": control })
                })
                .collect();
            json!({ "name": k.name, "label": k.label, "help": k.help, "needs_file": k.needs_file, "params": params })
        })
        .collect();
    let text = KINDS.iter().map(|k| format!("  {:<9} {}", k.name, k.help)).collect::<Vec<_>>().join("\n");
    Ok(Reply::new(format!("Built-in effects:\n{text}"), json!(kinds)))
}

fn fx_list(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let only = if a.text("track").is_some() { Some(e.track_at(a, "track")?) } else { None };
    let (mut text, mut data) = (String::new(), Vec::new());
    for (i, t) in e.project.tracks.iter().enumerate().filter(|(i, _)| only.is_none_or(|o| o == *i)) {
        if t.effects.is_empty() && only.is_none() {
            continue;
        }
        text.push_str(&format!("{} '{}':{}\n", i + 1, t.name, if t.effects.is_empty() { " no effects" } else { "" }));
        for (s, f) in t.effects.iter().enumerate() {
            text.push_str(&format!("  {}  {}\n", s + 1, describe(f)));
        }
        data.push(json!({ "track": t.id, "effects": t.effects.iter().enumerate().map(|(s, f)| effect_json(f, s)).collect::<Vec<_>>() }));
    }
    if text.is_empty() {
        text = "No effects on any track.".into();
    }
    Ok(Reply::new(text.trim_end().to_string(), json!(data)))
}

fn fx_add(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let t = e.track_at(a, "track")?;
    let kind = kind_of(&a.need_text("kind")?.to_ascii_lowercase())?;
    let n = e.project.tracks[t].effects.len();
    if n >= MAX_EFFECTS {
        return Err(refuse(format!("a track holds at most {MAX_EFFECTS} effects")));
    }
    let at = match a.int("at") {
        Some(s) if s < 1 || s as usize > n + 1 => return Err(refuse(format!("slot {s} is not between 1 and {}", n + 1))),
        Some(s) => s as usize - 1,
        None => n,
    };
    let id = e.project.next_effect_id;
    e.project.next_effect_id += 1;
    // Echo is the delay engine in Tape character: store the character so the setting shows.
    let mut params = std::collections::BTreeMap::new();
    if kind.name == "echo" {
        params.insert("character".to_string(), 1.0);
    }
    let f = Insert { id, kind: kind.name.to_string(), bypass: false, params, file: None };
    e.project.tracks[t].effects.insert(at, f);
    let tr = &e.project.tracks[t];
    let hint = if kind.needs_file { ". Choose its impulse response with fx.file" } else { "" };
    Ok(Reply::new(format!("Added {} to '{}' in slot {}{hint}", kind.label, tr.name, at + 1), effect_json(&tr.effects[at], at)))
}

fn fx_remove(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (t, s) = slot(e, a)?;
    let f = e.project.tracks[t].effects.remove(s);
    Ok(Reply::new(format!("Removed {} from '{}'", crucible_dsp::kind(&f.kind).map_or(f.kind.as_str(), |k| k.label), e.project.tracks[t].name), json!({ "removed": f.id })))
}

fn fx_set(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (t, s) = slot(e, a)?;
    let name = a.need_text("setting")?.to_ascii_lowercase();
    let f = &e.project.tracks[t].effects[s];
    let kind = kind_of(&f.kind)?;
    let p = kind
        .params
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| refuse(format!("{} has no setting '{name}'; it has {}", kind.label, kind.params.iter().map(|p| p.name).collect::<Vec<_>>().join(", "))))?;
    let v = p.parse(a.need_text("value")?).map_err(refuse)?;
    let f = &mut e.project.tracks[t].effects[s];
    f.params.insert(p.name.to_string(), v);
    Ok(Reply::new(format!("{} {} on '{}' is now {}", kind.label, p.label, e.project.tracks[t].name, show(p, v)), effect_json(&e.project.tracks[t].effects[s], s)))
}

fn fx_bypass(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (t, s) = slot(e, a)?;
    let f = &mut e.project.tracks[t].effects[s];
    f.bypass = a.text("state") != Some("off");
    let state = if f.bypass { "bypassed" } else { "active" };
    Ok(Reply::new(format!("Slot {} on '{}' is {state}", s + 1, e.project.tracks[t].name), effect_json(&e.project.tracks[t].effects[s], s)))
}

fn fx_file(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (t, s) = slot(e, a)?;
    if !kind_of(&e.project.tracks[t].effects[s].kind)?.needs_file {
        return Err(refuse("only IR Reverb takes a file"));
    }
    let source = a.need_text("source")?.to_string();
    if source.contains(['/', '\\']) || source.starts_with('.') {
        return Err(DawError::new(Code::BadArgument, format!("'{source}' should be a file name in the audio folder, as audio.import reported it")));
    }
    let folder = e.folder.clone().ok_or_else(|| DawError::new(Code::ProjectNotSaved, "the project has no folder yet"))?;
    let i = check_ir(&folder.join(super::audio::AUDIO_DIR).join(&source), e.project.sample_rate)?;
    e.project.tracks[t].effects[s].file = Some(source.clone());
    let what = match i.channels { 1 => "mono", 2 => "stereo", _ => "true stereo" };
    Ok(Reply::new(
        format!("IR Reverb on '{}' now uses {source} ({what}, {:.2} s)", e.project.tracks[t].name, i.frames as f64 / f64::from(i.rate)),
        effect_json(&e.project.tracks[t].effects[s], s),
    ))
}

fn fx_move(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let (t, s) = slot(e, a)?;
    let n = e.project.tracks[t].effects.len();
    let to = a.need_int("to")?;
    if to < 1 || to as usize > n {
        return Err(refuse(format!("slot {to} is not between 1 and {n}")));
    }
    let f = e.project.tracks[t].effects.remove(s);
    e.project.tracks[t].effects.insert(to as usize - 1, f);
    Ok(Reply::new(format!("Moved slot {} to {to} on '{}'", s + 1, e.project.tracks[t].name), json!({ "to": to })))
}

/// An impulse-response file Crucible can use: 1, 2 or 4 channels, the project's rate, 30 s at most.
fn check_ir(path: &Path, rate: u32) -> Result<super::audio::Info, DawError> {
    let i = super::audio::info(path)?;
    if i.rate != rate {
        return Err(DawError::new(Code::SampleRateMismatch, format!("{} is {} Hz; the project is {rate} Hz", path.display(), i.rate)));
    }
    if i.frames as f64 / f64::from(i.rate) > 30.0 {
        return Err(refuse(format!("{} is longer than 30 s, too long for an impulse response", path.display())));
    }
    Ok(i)
}

/// A running effect for the mixer, from a track's stored settings.
pub(crate) fn build(f: &Insert, p: &Project, folder: Option<&Path>, rate: f64) -> Result<Box<dyn crucible_dsp::Effect>, DawError> {
    let kind = kind_of(&f.kind)?;
    let values = crucible_dsp::Values::new(kind, |n| f.params.get(n).copied());
    let ir = if kind.needs_file {
        let file = f.file.as_ref().ok_or_else(|| refuse(format!("an IR Reverb (effect {}) has no impulse-response file; choose one with fx.file", f.id)))?;
        let folder = folder.ok_or_else(|| DawError::new(Code::ProjectNotSaved, "the project has no folder"))?;
        let path = folder.join(super::audio::AUDIO_DIR).join(file);
        check_ir(&path, p.sample_rate)?;
        Some(super::audio::read_ir(&path)?.0)
    } else {
        None
    };
    crucible_dsp::build(kind, &values, rate, p.tempo_bpm, ir).map_err(refuse)
}
