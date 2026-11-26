//! The paper trail (D-037): what the session log needs beyond errors, so that a log sent from a
//! machine no developer can see tells the whole story of a session.
//!
//! - Every command is logged with its outcome and how long it took ([`begin`], [`end`]). Both
//!   front ends call these, so the command line and the window leave the same trail.
//! - `mark` drops the person's own note into the log at the moment something goes wrong.
//! - `log.ui` is how the window records what the person did and saw: clicks, keys, messages on
//!   screen, its own errors and stalls. The window has no file access of its own (D-022).
//! - `log.snapshot` copies the project and the playback state into the log, for a report.
//!
//! Playback's own lines (start, health, dropouts, stop) are written by `play.rs`.
//!
//! Portability: no OS code.

use crate::command::{Args, Kind, Reply, Spec, opt, req};
use crate::{Effect, Engine, play};
use crucible_diagnostics::{self as diag, Code, DawError};
use serde_json::json;
use std::time::{Duration, Instant};

/// How much of a command's answer goes into the log.
const REPLY_LINES: usize = 12;
const REPLY_CHARS: usize = 1500;
/// A command slower than this is called slow in its outcome line.
const SLOW: Duration = Duration::from_millis(500);
const UI_KINDS: &[&str] = &["did", "saw", "slow", "error", "info"];

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "mark",
        args: &[
            opt("note", Kind::Words, "what you noticed, in your own words; quotes are optional"),
            opt("ago", Kind::Number, "seconds since it happened (the window fills this in)"),
        ],
        help: "Drop a note into the log at this moment, e.g. mark heard a click. Reports show it in place.",
        effect: Effect::Read,
        run: mark,
    },
    Spec {
        name: "log.ui",
        args: &[req("kind", Kind::Choice(UI_KINDS), "did, saw, slow, error or info"), req("text", Kind::Words, "the line to record")],
        help: "Used by the window: record what the person did or saw in the session log.",
        effect: Effect::Read,
        run: log_ui,
    },
    Spec { name: "log.path", args: &[], help: "Where this run's log file is.", effect: Effect::Read, run: log_path },
    Spec {
        name: "log.snapshot",
        args: &[],
        help: "Copy the project, the recent commands and the playback state into the log, for a report.",
        effect: Effect::Read,
        run: log_snapshot,
    },
];

/// Commands that run many times a second, or exist only to write the log. Their own lines are
/// all the trail they need. `project.sync` is the development bridge fetching the state after
/// every command; the window proper does that without a command.
fn quiet(name: &str) -> bool {
    matches!(name, "transport.status" | "log.ui" | "project.sync" | "audio.changes")
}

/// Returned by [`begin`], handed to [`end`].
pub struct Stamp {
    quiet: bool,
    started: Instant,
}

/// Call before running a command: logs `> the command`.
pub fn begin(tokens: &[String]) -> Stamp {
    let quiet = tokens.first().is_none_or(|n| quiet(n));
    if !quiet {
        diag::info(format!("> {}", tokens.join(" ")));
    }
    Stamp { quiet, started: Instant::now() }
}

/// Call with the command's result: logs `< ok in 1.2 ms: what it answered`, or `< FAILED E5017`.
/// The error's own detail is logged by whoever reports it to the person.
pub fn end(stamp: Stamp, result: &Result<Reply, DawError>) {
    if !stamp.quiet {
        diag::info(outcome(result, stamp.started.elapsed()));
    }
}

fn took(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    let text = if ms < 100.0 { format!("{ms:.1} ms") } else if ms < 10_000.0 { format!("{ms:.0} ms") } else { format!("{:.1} s", ms / 1000.0) };
    if d >= SLOW { format!("{text} (slow)") } else { text }
}

fn outcome(result: &Result<Reply, DawError>, d: Duration) -> String {
    match result {
        Err(e) => format!("< FAILED {} in {}", e.code.id(), took(d)),
        Ok(r) => {
            let text = r.text.trim_end();
            if text.is_empty() {
                return format!("< ok in {}", took(d));
            }
            let mut out = format!("< ok in {}: ", took(d));
            let (mut lines, mut chars) = (0, 0);
            for l in text.lines() {
                if lines == REPLY_LINES || chars + l.len() > REPLY_CHARS {
                    out.push_str(&format!("\n(+{} more lines, not logged)", text.lines().count() - lines));
                    break;
                }
                if lines > 0 {
                    out.push('\n');
                }
                out.push_str(l);
                lines += 1;
                chars += l.len();
            }
            out
        }
    }
}

fn mark(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let note = a.text("note").map(str::trim).filter(|n| !n.is_empty());
    let ago = a.number("ago").unwrap_or(0.0).clamp(0.0, 3600.0);
    let said = note.map_or("(no note)".to_string(), |n| format!("\"{n}\""));
    let mut line = said.clone();
    if ago >= 0.05 {
        line.push_str(&format!(" | noticed {ago:.1} s before this line, at {}", diag::clock_before(ago)));
    }
    line.push_str(&format!(" | {}", play::where_now(e, ago)));
    diag::event("MARK", &line);
    Ok(Reply::new(format!("Marked in the log: {said}"), json!({ "log": diag::log_path(), "line": line })))
}

fn log_ui(_: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let kind = a.need_text("kind")?;
    let mut text: String = a.need_text("text")?.chars().take(800).collect();
    text = text.replace(['\r', '\n'], " / ");
    match kind {
        "error" => diag::error(Code::UiError, &text),
        _ => diag::event("UI", format!("{kind}: {text}")),
    }
    Ok(Reply::text(""))
}

fn log_path(_: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let path = diag::log_path();
    Ok(Reply::new(path.as_ref().map_or("No log: the file could not be created".to_string(), |p| p.display().to_string()), json!({ "log": path })))
}

fn log_snapshot(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let mut text = String::new();
    text.push_str(&format!("playback : {}\n", play::where_now(e, 0.0)));
    text.push_str(&format!("output   : {}\n", play::output_text(e)));
    text.push_str(&format!("folder   : {}\n", e.folder.as_ref().map_or("not saved yet".to_string(), |f| f.display().to_string())));
    text.push_str(&format!("unsaved  : {}\n", if e.is_dirty() { "yes" } else { "no" }));
    text.push_str(&format!("undo     : {} steps, redo {} steps\n", e.undo.len(), e.redo.len()));
    let recent = e.history.len().saturating_sub(60);
    text.push_str(&format!("commands : {} this session; the last {}:\n", e.history.len(), e.history.len() - recent));
    for line in &e.history[recent..] {
        text.push_str(&format!("  {line}\n"));
    }
    // The project itself, without audio: enough to rebuild the arrangement and every setting.
    let mut project = serde_json::to_string_pretty(&e.project).unwrap_or_else(|err| format!("(could not be written: {err})"));
    const LIMIT: usize = 400 * 1024;
    if project.len() > LIMIT {
        let mut cut = LIMIT;
        while !project.is_char_boundary(cut) {
            cut -= 1;
        }
        project.truncate(cut);
        project.push_str("\n(cut: the project is larger than 400 KB)");
    }
    text.push_str("project  :\n");
    text.push_str(&project);
    diag::raw("snapshot", &text);
    Ok(Reply::new("The project and the playback state were copied into the log", json!({ "log": diag::log_path() })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command;

    fn run(e: &mut Engine, line: &str) -> Result<Reply, DawError> {
        let mut last = Reply::text("");
        for t in command::split_statements(line).unwrap() {
            last = e.execute(&t)?;
        }
        Ok(last)
    }

    #[test]
    fn outcome_lines_say_what_happened_and_how_long_it_took() {
        let ms = Duration::from_micros;
        assert_eq!(outcome(&Ok(Reply::text("")), ms(1200)), "< ok in 1.2 ms");
        assert_eq!(outcome(&Ok(Reply::text("Added track 'Piano'\n")), ms(300)), "< ok in 0.3 ms: Added track 'Piano'");
        assert_eq!(outcome(&Ok(Reply::text("a\nb")), ms(2_315_000)), "< ok in 2315 ms (slow): a\nb");
        assert_eq!(outcome(&Err(DawError::new(Code::SampleRateMismatch, "x")), ms(900)), "< FAILED E5017 in 0.9 ms");
        let long = (0..40).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let o = outcome(&Ok(Reply::text(long)), ms(10));
        assert!(o.contains("line 11") && !o.contains("line 12\n") && o.ends_with("(+28 more lines, not logged)"), "{o}");
        assert_eq!(took(Duration::from_secs(12)), "12.0 s (slow)");
    }

    #[test]
    fn a_mark_takes_free_text_with_or_without_quotes() {
        let mut e = Engine::new();
        let r = run(&mut e, "mark heard a click on the snare").unwrap();
        assert_eq!(r.text, "Marked in the log: \"heard a click on the snare\"");
        assert!(r.data["line"].as_str().unwrap().ends_with("| not playing"), "the log line says where playback was");
        let r = run(&mut e, "mark \"two words\" --ago 2.5").unwrap();
        assert!(r.text.contains("\"two words\"") && r.data["line"].as_str().unwrap().contains("noticed 2.5 s before this line"), "{}", r.data);
        assert!(run(&mut e, "mark").unwrap().text.contains("(no note)"));
        // Marks, window lines and snapshots are not edits: no undo step, nothing unsaved.
        run(&mut e, "log.ui did click button \"Export\"; log.snapshot").unwrap();
        assert!(e.undo.is_empty() && e.history.is_empty() && !e.is_dirty());
        assert!(run(&mut e, "log.ui shouted text").is_err(), "the kind is checked");
    }
}
