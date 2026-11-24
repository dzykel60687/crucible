//! `crucible`: the command-line front end. One of three front ends over the same command
//! tables (D-022); the GUI and its console are the other two.
//!
//! Two tables are joined here: the engine's (project and track editing, undo) and this file's
//! `HOST` table (devices, plugins, diagnostics, help, scripts). Every run that does work writes a
//! session log through `crucible-diagnostics`, and every command is recorded in it.
//!
//! Portability: no OS-specific code. Folders come from library calls and the seam crates.

use anyhow::{Context, Result};
use crucible_diagnostics::{self as diag, Code, DawError};
use crucible_engine::command::{self, opt, req};
use crucible_engine::{Args, Effect, Engine, Kind, Reply, Spec};
use serde_json::json;
use std::fmt::Write as _;
use std::io::{BufRead, IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

const USAGE: &str = "\
Crucible: command line

  crucible <command> [arguments]     run one command
  crucible -c \"cmd; cmd; cmd\"        run several in order, stop at the first error
  crucible run <script.txt>          run a script: one command per line, # for comments
  crucible shell                     interactive console (type help, exit to leave)
  crucible help [command]            list every command, or explain one

Global options, before the command:
  --json               print one JSON object per command on stdout (for scripts and tests)
  --project <folder>   open this project first, and save it afterwards if anything changed

Examples:
  crucible diag
  crucible -c \"project.new Demo; track.add Piano --kind instrument; track.list; project.save C:\\Music\\Demo\"
  crucible --project C:\\Music\\Demo track.gain Piano -3

Problems print a code such as E2004 and what to try (`crucible codes` lists them all).
Every run writes a log; `crucible report` bundles the recent ones into one file to send.
Exit codes: 0 OK, 2 command line, 3 file, 10 audio, 11 MIDI, 12 plugin, 13 project, 70 bug.
";

/// Hidden subcommand the scanner uses to probe one plugin in a child process.
const PROBE_SUBCOMMAND: &str = "probe-plugin";
/// Scripts may run scripts, but not forever.
const MAX_SCRIPT_DEPTH: u8 = 8;
/// How many recent logs `crucible report` includes, and how big each may be in it.
const REPORT_LOGS: usize = 8;
const REPORT_MAX_BYTES: usize = 300 * 1024;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // The probe child must do as little as possible before loading the plugin. It reports a
    // native crash on stderr, which the parent collects.
    if args.first().map(String::as_str) == Some(PROBE_SUBCOMMAND) {
        diag::start_child();
        let Some(bundle) = args.get(1) else {
            eprintln!("{PROBE_SUBCOMMAND} needs a bundle path");
            std::process::exit(2);
        };
        std::process::exit(crucible_plugin_host::run_probe_child(Path::new(bundle)));
    }

    let (mut json, mut project, mut script) = (false, None, None);
    let globals = (|| -> Result<()> {
        while let Some(first) = args.first().cloned() {
            match first.as_str() {
                "--json" => json = true,
                "--project" | "-c" => {
                    let v = args.get(1).cloned().ok_or_else(|| usage_error(format!("{first} needs a value")))?;
                    if first == "-c" { script = Some(v) } else { project = Some(v) }
                    args.remove(0);
                }
                _ => break,
            }
            args.remove(0);
        }
        Ok(())
    })();

    let command = args.first().map(|c| alias(c).to_string()).unwrap_or_default();
    let quiet = matches!(command.as_str(), "help" | "version" | "codes" | "logs") || (command.is_empty() && script.is_none());
    if !quiet {
        let label = if script.is_some() { "script" } else { command.as_str() };
        diag::start("crucible", label);
    }

    let mut cli = Cli { engine: Engine::new(), json, depth: 0 };
    let result = globals.and_then(|()| {
        if let Some(p) = &project {
            cli.exec_print(&["project.open".into(), p.clone()])?;
        }
        if let Some(text) = &script {
            cli.run_text(text, "-c")?;
        } else if command == "shell" {
            cli.shell()?;
        } else if command.is_empty() {
            print!("{USAGE}");
        } else {
            args[0] = command.clone();
            cli.exec_print(&args)?;
        }
        if project.is_some() && cli.engine.is_dirty() {
            cli.exec_print(&["project.save".into()])?;
        }
        Ok(())
    });
    std::process::exit(diag::finish(result));
}

fn alias(c: &str) -> &str {
    match c {
        "-h" | "--help" => "help",
        "-V" | "--version" => "version",
        other => other,
    }
}

fn usage_error(msg: impl Into<String>) -> anyhow::Error {
    DawError::new(Code::Usage, msg).into()
}

struct Cli {
    engine: Engine,
    json: bool,
    depth: u8,
}

impl Cli {
    /// Run one command from either table.
    fn exec(&mut self, tokens: &[String]) -> Result<Reply, DawError> {
        let name = alias(tokens.first().map(String::as_str).unwrap_or(""));
        // A window polls transport.status many times a second while playing.
        if name != "transport.status" {
            diag::info(format!("> {}", tokens.join(" ")));
        }
        if let Some(spec) = command::find(HOST, name) {
            let args = command::parse(spec, &tokens[1..])?;
            return (spec.run)(self, &args);
        }
        if let Some(spec) = command::find(Engine::specs(), name) {
            let args = command::parse(spec, &tokens[1..])?;
            return self.engine.run(spec, args);
        }
        Err(command::unknown(name, HOST.iter().map(|s| s.name).chain(Engine::specs().iter().map(|s| s.name))))
    }

    /// Run one command and print its result. Errors are returned for the caller to report.
    fn exec_print(&mut self, tokens: &[String]) -> Result<()> {
        match self.exec(tokens) {
            Ok(reply) => {
                if self.json {
                    println!("{}", json!({ "ok": true, "command": tokens.join(" "), "text": reply.text, "data": reply.data }));
                } else if !reply.text.is_empty() {
                    println!("{}", reply.text);
                }
                Ok(())
            }
            Err(e) => {
                if self.json {
                    println!("{}", json!({ "ok": false, "command": tokens.join(" "), "code": e.code.id(), "message": e.message }));
                }
                Err(e.into())
            }
        }
    }

    /// Run statements in order and stop at the first error.
    fn run_text(&mut self, text: &str, source: &str) -> Result<()> {
        let statements = command::split_statements(text).map_err(|e| DawError::new(e.code, format!("{source}: {}", e.message)))?;
        for (n, tokens) in statements.iter().enumerate() {
            self.exec_print(tokens).with_context(|| format!("{source}, command {} ({})", n + 1, tokens.join(" ")))?;
        }
        Ok(())
    }

    /// Interactive console. Errors are reported and the console carries on.
    fn shell(&mut self) -> Result<()> {
        let interactive = std::io::stdin().is_terminal();
        if interactive {
            println!("Crucible shell. Type help for commands, exit to leave.");
        }
        let mut exit_warned = false;
        let mut lines = std::io::stdin().lock().lines();
        loop {
            if interactive {
                let p = self.engine.project();
                print!("{}{}> ", p.name, if self.engine.is_dirty() { "*" } else { "" });
                let _ = std::io::stdout().flush();
            }
            let Some(Ok(line)) = lines.next() else { break };
            let trimmed = line.trim();
            if trimmed == "exit" || trimmed == "quit" {
                if self.engine.is_dirty() && !exit_warned {
                    exit_warned = true;
                    diag::warn(
                        Code::UnsavedChanges,
                        "type exit again to leave without saving, or project.save first. Saved projects keep a crash journal.",
                    );
                    continue;
                }
                break;
            }
            exit_warned = false;
            let statements = match command::split_statements(&line) {
                Ok(s) => s,
                Err(e) => {
                    diag::error(e.code, e.message);
                    continue;
                }
            };
            for tokens in &statements {
                if tokens[0] == "shell" {
                    println!("Already in the shell.");
                    continue;
                }
                if let Err(e) = self.exec_print(tokens) {
                    let code = diag::find_code(&e).unwrap_or(Code::Uncategorised);
                    diag::error(code, format!("{e:#}"));
                    break;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- host command table

static HOST: &[Spec<Cli>] = &[
    Spec {
        name: "help",
        args: &[opt("command", Kind::Text, "a command name, or a group such as track")],
        help: "List every command, or explain one.",
        effect: Effect::Read,
        run: help,
    },
    Spec { name: "version", args: &[], help: "Program version and build fingerprint.", effect: Effect::Read, run: version },
    Spec {
        name: "codes",
        args: &[opt("markdown", Kind::Bool, "print a Markdown table")],
        help: "List every error and warning code with what to try.",
        effect: Effect::Read,
        run: codes,
    },
    Spec { name: "logs", args: &[], help: "Show where logs are kept and list the recent ones.", effect: Effect::Read, run: logs },
    Spec {
        name: "report",
        args: &[opt("no-plugins", Kind::Bool, "skip the plugin scan")],
        help: "Run diag, then gather it and the recent logs into ONE file to send.",
        effect: Effect::System,
        run: report,
    },
    Spec {
        name: "diag",
        args: &[opt("no-plugins", Kind::Bool, "skip the plugin scan"), opt("timeout", Kind::Int, "seconds allowed per plugin (default 60)")],
        help: "Machine check: audio devices, exclusive mode, MIDI inputs and plugins, written to the log.",
        effect: Effect::System,
        run: diag_cmd,
    },
    Spec {
        name: "audio.devices",
        args: &[],
        help: "List audio devices, including what each accepts in exclusive mode.",
        effect: Effect::System,
        run: audio_devices,
    },
    Spec {
        name: "audio.test",
        args: &[
            opt("device", Kind::Text, "part of the output's name (default: the system default output)"),
            opt("rate", Kind::Int, "sample rate in Hz (default 48000)"),
            opt("seconds", Kind::Number, "how long to hold the device, 0.5 to 600 (default 5)"),
        ],
        help: "Take an output exclusively, play SILENCE, and measure the driver's timing.",
        effect: Effect::System,
        run: audio_test,
    },
    Spec { name: "midi.inputs", args: &[], help: "List MIDI input ports.", effect: Effect::System, run: midi_inputs },
    Spec {
        name: "plugin.scan",
        args: &[
            opt("folder", Kind::Text, "scan this folder instead of the standard VST3 folders"),
            opt("timeout", Kind::Int, "seconds allowed per plugin (default 60)"),
        ],
        help: "Find VST3 plugins and probe each one in its own process.",
        effect: Effect::System,
        run: plugin_scan,
    },
    Spec {
        name: "run",
        args: &[req("file", Kind::Text, "a text file of commands, one per line")],
        help: "Run a script file. Stops at the first error.",
        effect: Effect::System,
        run: run_script,
    },
];

// ---------------------------------------------------------------- help and info

fn help(_: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    let host = HOST.iter().map(|s| (s.name, command::usage(s), command::describe(s), s.help));
    let engine = Engine::specs().iter().map(|s| (s.name, command::usage(s), command::describe(s), s.help));
    let all: Vec<_> = host.chain(engine).collect();
    if let Some(topic) = a.text("command") {
        if let Some((_, _, desc, _)) = all.iter().find(|(n, ..)| *n == topic) {
            return Ok(Reply::new(desc.trim_end().to_string(), json!({ "command": topic })));
        }
        let group: Vec<_> = all.iter().filter(|(n, ..)| n.starts_with(&format!("{topic}."))).collect();
        if group.is_empty() {
            return Err(command::unknown(topic, all.iter().map(|(n, ..)| *n)));
        }
        let text: String = group.iter().map(|(_, _, d, _)| d.as_str()).collect::<Vec<_>>().join("\n");
        return Ok(Reply::new(text.trim_end().to_string(), json!(group.iter().map(|(n, ..)| *n).collect::<Vec<_>>())));
    }
    let mut text = String::from("Commands (`crucible help <command>` explains one; `crucible help track` a group):\n");
    let mut group = "";
    for (name, usage, _, help) in &all {
        let g = if name.contains('.') { name.split('.').next().unwrap_or("") } else { "" };
        if g != group {
            text.push('\n');
            group = g;
        }
        let _ = writeln!(text, "  {usage:<46} {help}");
    }
    text.push_str("\nRun `crucible` with no arguments for modes (-c, run, shell) and global options.");
    Ok(Reply::new(text, json!(all.iter().map(|(n, ..)| *n).collect::<Vec<_>>())))
}

fn version(_: &mut Cli, _: &Args) -> Result<Reply, DawError> {
    let fp = std::env::current_exe().ok().and_then(|e| diag::exe_fingerprint(&e)).unwrap_or_else(|| "?".into());
    let kind = if cfg!(debug_assertions) { "debug build" } else { "release build" };
    let v = env!("CARGO_PKG_VERSION");
    Ok(Reply::new(format!("Crucible {v} ({kind}), build fingerprint {fp}"), json!({ "version": v, "build": kind, "fingerprint": fp })))
}

fn codes(_: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    if a.flag("markdown") == Some(true) {
        return Ok(Reply::text(diag::codes::markdown_table().trim_end()));
    }
    let mut text = String::new();
    for c in Code::ALL {
        let _ = writeln!(text, "{}  {}\n       what to try: {}\n", c.id(), c.title(), c.help());
    }
    let data: Vec<_> = Code::ALL.iter().map(|c| json!({ "code": c.id(), "title": c.title(), "help": c.help() })).collect();
    Ok(Reply::new(text.trim_end().to_string(), json!(data)))
}

fn with_log_location(mut text: String) -> String {
    if let Some(p) = diag::log_path() {
        let _ = write!(text, "\nLog written to: {}", p.display());
    }
    text
}

// ---------------------------------------------------------------- devices and plugins

fn audio_devices(_: &mut Cli, _: &Args) -> Result<Reply, DawError> {
    let report = crucible_audio_io::enumerate();
    let exclusive = crucible_audio_io::exclusive_capabilities();
    let text = format!("{}\n{}", format_audio(&report), format_exclusive(&exclusive));
    diag::raw("audio devices", &text);
    let exclusive = exclusive.map_err(|e| e.to_string());
    Ok(Reply::new(text.trim_end().to_string(), json!({ "audio": report, "exclusive": exclusive })))
}

fn audio_test(_: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    let rate = a.int("rate").unwrap_or(48_000);
    let rate = u32::try_from(rate).map_err(|_| DawError::new(Code::ValueOutOfRange, format!("rate {rate} Hz is not usable")))?;
    let seconds = a.number("seconds").unwrap_or(5.0).clamp(0.5, 600.0);
    let report = crucible_audio_io::exclusive_stream_test(a.text("device").map(String::from), rate, seconds)?;
    let text = format_stream_test(&report);
    diag::raw("exclusive-mode timing test", &text);
    if let Some(e) = &report.error {
        print!("{text}");
        return Err(DawError::parse(e).unwrap_or_else(|| DawError::new(Code::StreamStartFailed, e.clone())));
    }
    if report.stats.timeouts > 0 {
        diag::warn(Code::Dropout, format!("{} driver timeouts in {:.1}s", report.stats.timeouts, report.seconds));
    } else if report.stats.late_wakeups > 0 {
        diag::warn(Code::LateWakeup, format!("{} late wake-ups, worst gap {:.2} ms", report.stats.late_wakeups, report.stats.worst_gap_ms));
    }
    Ok(Reply::new(with_log_location(text.trim_end().to_string()), json!(report)))
}

fn midi_inputs(_: &mut Cli, _: &Args) -> Result<Reply, DawError> {
    let report = crucible_midi_io::enumerate_inputs();
    let text = format_midi(&report);
    diag::raw("MIDI inputs", &text);
    if let Some(e) = report.error.as_deref().and_then(DawError::parse) {
        return Err(e);
    }
    Ok(Reply::new(text.trim_end().to_string(), json!(report)))
}

fn plugin_scan(_: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    let report = scan_plugins(a)?;
    let text = format_plugins(&report);
    diag::raw("plugins", &text);
    Ok(Reply::new(text.trim_end().to_string(), json!(report)))
}

fn scan_plugins(a: &Args) -> Result<crucible_plugin_host::ScanReport, DawError> {
    let folders = match a.text("folder") {
        Some(f) => vec![PathBuf::from(f)],
        None => crucible_plugin_host::plugin_paths::standard_vst3_folders(),
    };
    let timeout = a.int("timeout").map(|s| Duration::from_secs(s.max(1) as u64)).unwrap_or(crucible_plugin_host::DEFAULT_PROBE_TIMEOUT);
    let exe = std::env::current_exe().map_err(|e| DawError::new(Code::FileRead, format!("cannot locate own executable for the probe: {e}")))?;
    let probe = crucible_plugin_host::ProbeCommand { exe, args_before_path: vec![PROBE_SUBCOMMAND.to_string()] };
    diag::info(format!("scanning {} folder(s), {}s timeout per plugin", folders.len(), timeout.as_secs()));
    let report = crucible_plugin_host::scan(&folders, &probe, timeout);
    for r in &report.results {
        use crucible_plugin_host::ProbeOutcome as P;
        match r {
            P::Ok(_) => {}
            P::Failed { path, detail } => diag::warn(Code::PluginLoadFailed, format!("{}: {detail}", path.display())),
            P::Crashed { path, detail } => diag::warn(Code::PluginCrashed, format!("{}: {detail}", path.display())),
            P::TimedOut { path, after_secs } => diag::warn(Code::PluginTimedOut, format!("{} after {after_secs}s", path.display())),
        }
    }
    Ok(report)
}

/// The whole machine check, also copied into the session log.
fn diag_text(a: &Args) -> Result<(String, serde_json::Value), DawError> {
    let audio = crucible_audio_io::enumerate();
    let exclusive = crucible_audio_io::exclusive_capabilities();
    let midi = crucible_midi_io::enumerate_inputs();
    let plugins = if a.flag("no-plugins") == Some(true) { None } else { Some(scan_plugins(a)?) };

    let mut text = format!("{}\n{}\n{}\n", format_audio(&audio), format_exclusive(&exclusive), format_midi(&midi));
    match &plugins {
        Some(p) => text.push_str(&format_plugins(p)),
        None => text.push_str("VST3 PLUGINS\n  skipped (--no-plugins)\n"),
    }
    diag::raw("diag", &text);
    if midi.inputs.is_empty() && midi.error.is_none() {
        diag::info(format!("{} {}", Code::NoMidiInputs.id(), Code::NoMidiInputs.title()));
    }
    let exclusive = exclusive.map_err(|e| e.to_string());
    let data = json!({ "log_file": diag::log_path(), "audio": audio, "exclusive": exclusive, "midi": midi, "plugins": plugins });
    Ok((text, data))
}

fn diag_cmd(_: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    let (text, data) = diag_text(a)?;
    Ok(Reply::new(with_log_location(text.trim_end().to_string()), data))
}

fn run_script(cli: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    if cli.depth >= MAX_SCRIPT_DEPTH {
        return Err(DawError::new(Code::BadArgument, format!("scripts nested more than {MAX_SCRIPT_DEPTH} deep")));
    }
    let file = a.need_text("file")?;
    let text = std::fs::read_to_string(file).map_err(|e| DawError::new(Code::FileRead, format!("{file}: {e}")))?;
    cli.depth += 1;
    let result = cli.run_text(&text, file);
    cli.depth -= 1;
    match result {
        Ok(()) => Ok(Reply::new(format!("Script {file} finished"), json!({ "script": file }))),
        Err(e) => {
            let code = diag::find_code(&e).unwrap_or(Code::Uncategorised);
            Err(DawError::new(code, format!("{e:#}")))
        }
    }
}

// ---------------------------------------------------------------- logs and report

/// Files in the log folder, newest first.
fn recent_logs(dir: &Path) -> Vec<(std::time::SystemTime, PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt" || x == "dmp"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, e.path(), m.len()))
        })
        .collect();
    v.sort_by_key(|f| std::cmp::Reverse(f.0));
    v
}

/// First line in a log that shows trouble, for the `logs` listing.
fn first_problem(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|l| {
        let at = ["CRASH E9001", " PANIC ", " ERROR "].iter().find_map(|m| l.find(m))?;
        Some(l[at..].trim().chars().take(110).collect())
    })
}

fn logs(_: &mut Cli, _: &Args) -> Result<Reply, DawError> {
    let dir = diag::log_dir().ok_or_else(|| DawError::new(Code::LogUnavailable, "this system has no per-user data folder"))?;
    let mut text = format!("Log folder: {}\n", dir.display());
    let files = recent_logs(&dir);
    if files.is_empty() {
        text.push_str("No logs yet.");
    }
    for (t, p, len) in files.iter().take(20) {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let _ = writeln!(text, "  {:>9}  {:>8}  {name}", age(*t), human_size(*len));
        if p.extension().is_some_and(|x| x == "txt")
            && let Some(problem) = first_problem(p)
        {
            let _ = writeln!(text, "                       -> {problem}");
        }
    }
    if !files.is_empty() {
        text.push_str("To send everything in one file: crucible report");
    }
    let data: Vec<_> = files.iter().take(20).map(|(_, p, len)| json!({ "file": p, "bytes": len })).collect();
    Ok(Reply::new(text.trim_end().to_string(), json!({ "folder": dir, "files": data })))
}

fn report(_: &mut Cli, a: &Args) -> Result<Reply, DawError> {
    println!("Collecting a machine report. The plugin scan can take a minute...");
    diag_text(a)?;
    let dir = diag::log_dir().ok_or_else(|| DawError::new(Code::LogUnavailable, "this system has no per-user data folder"))?;
    let files = recent_logs(&dir);

    let mut out = String::from("Crucible problem report\n");
    let _ = writeln!(out, "Contains this run's machine check, then the {REPORT_LOGS} most recent logs, newest first.\n");
    let _ = writeln!(out, "Crash dumps in the log folder (send the newest one too, if it matches a crash below):");
    let dumps: Vec<_> = files.iter().filter(|f| f.1.extension().is_some_and(|x| x == "dmp")).collect();
    if dumps.is_empty() {
        let _ = writeln!(out, "  none");
    }
    for (t, p, len) in &dumps {
        let _ = writeln!(out, "  {}  {}  {}", age(*t), human_size(*len), p.display());
    }
    for (_, p, len) in files.iter().filter(|f| f.1.extension().is_some_and(|x| x == "txt")).take(REPORT_LOGS) {
        let _ = writeln!(out, "\n\n==================== {} ({}) ====================", p.display(), human_size(*len));
        match std::fs::read(p) {
            Ok(bytes) => out.push_str(&clip(&String::from_utf8_lossy(&bytes))),
            Err(e) => {
                let _ = writeln!(out, "(could not read: {e})");
            }
        }
    }

    let name = format!("crucible-report-{}.txt", diag::file_stamp());
    let here = std::env::current_dir().map(|d| d.join(&name)).unwrap_or_else(|_| PathBuf::from(&name));
    let path = match std::fs::write(&here, &out) {
        Ok(()) => here,
        Err(e) => {
            diag::info(format!("could not write the report to {}: {e}; using the log folder", here.display()));
            let fallback = dir.join(&name);
            std::fs::write(&fallback, &out).map_err(|e| DawError::new(Code::FileWrite, format!("{}: {e}", fallback.display())))?;
            fallback
        }
    };
    diag::info(format!("report written to {}", path.display()));
    let mut text = format!("Report written to:\n  {}\nSend that one file.", path.display());
    if let Some((_, newest, _)) = dumps.first() {
        let _ = write!(text, "\nA crash dump exists as well. Send it too if the report shows a crash:\n  {}", newest.display());
    }
    Ok(Reply::new(text, json!({ "report": path, "dump": dumps.first().map(|d| &d.1) })))
}

/// Keep the head (the header) and the tail (where things went wrong) of a very long log.
fn clip(text: &str) -> String {
    if text.len() <= REPORT_MAX_BYTES {
        return text.to_string();
    }
    let head_end = floor_char(text, 40 * 1024);
    let tail_start = floor_char(text, text.len() - (REPORT_MAX_BYTES - 40 * 1024));
    format!("{}\n\n[... {} bytes cut from the middle ...]\n\n{}", &text[..head_end], tail_start - head_end, &text[tail_start..])
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1_048_576 { format!("{:.1} MB", bytes as f64 / 1_048_576.0) } else { format!("{:.0} KB", (bytes as f64 / 1024.0).ceil()) }
}

/// "5m ago" style age, so this crate needs no date library.
fn age(t: std::time::SystemTime) -> String {
    let secs = std::time::SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

// ---------------------------------------------------------------- text formatting

fn format_audio(r: &crucible_audio_io::AudioReport) -> String {
    let mut s = String::from("AUDIO\n");
    if r.hosts.is_empty() {
        s.push_str("  no audio hosts available\n");
    }
    for h in &r.hosts {
        let _ = writeln!(s, "  host: {}  ({} devices)", h.host, h.devices.len());
        if let Some(e) = &h.error {
            let _ = writeln!(s, "    ERROR: {e}");
        }
        for d in &h.devices {
            let mut tags = Vec::new();
            if d.is_default_output {
                tags.push("default output");
            }
            if d.is_default_input {
                tags.push("default input");
            }
            let tags = if tags.is_empty() { String::new() } else { format!("  [{}]", tags.join(", ")) };
            let _ = writeln!(s, "    - {}{}", d.name, tags);
            if let Some(m) = &d.manufacturer {
                let _ = writeln!(s, "        manufacturer: {m}");
            }
            if let Some(dr) = &d.driver {
                let _ = writeln!(s, "        driver: {dr}");
            }
            for (label, dir) in [("output", &d.output), ("input", &d.input)] {
                let Some(dir) = dir else { continue };
                let rates = if dir.common_rates.is_empty() {
                    "none of the common rates".to_string()
                } else {
                    dir.common_rates.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
                };
                let _ = writeln!(
                    s,
                    "        {label:<6}: default {} Hz, {} ch, {} | max {} ch | rates: {} | buffer: {}",
                    opt_str(dir.default_sample_rate),
                    opt_str(dir.default_channels),
                    dir.default_format.as_deref().unwrap_or("?"),
                    dir.max_channels,
                    rates,
                    dir.buffer_frames.map(|(a, b)| format!("{a}-{b} frames")).unwrap_or_else(|| "not reported".into()),
                );
                if let Some(e) = &dir.error {
                    let _ = writeln!(s, "        {label:<6}  note: {e}");
                }
            }
        }
    }
    for n in &r.build_notes {
        let _ = writeln!(s, "  note: {n}");
    }
    s
}

fn format_exclusive(r: &Result<Vec<crucible_audio_io::ExclusiveDeviceReport>, DawError>) -> String {
    let mut s = String::from("EXCLUSIVE MODE (what each device accepts when the app takes it over)\n");
    let devices = match r {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(s, "  {e}");
            return s;
        }
    };
    if devices.is_empty() {
        s.push_str("  no active devices\n");
    }
    for d in devices {
        let dir = match d.direction {
            crucible_audio_io::StreamDirection::Output => "output",
            crucible_audio_io::StreamDirection::Input => "input",
        };
        let _ = writeln!(
            s,
            "  - {} ({dir}, {} ch) | period: min {} ms, default {} ms",
            d.name,
            opt_str(d.native_channels),
            d.min_period_ms.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into()),
            d.default_period_ms.map(|v| format!("{v:.2}")).unwrap_or_else(|| "?".into()),
        );
        if let Some(e) = &d.error {
            let _ = writeln!(s, "      ERROR {e}");
        }
        if d.formats.is_empty() && d.error.is_none() {
            let _ = writeln!(s, "      accepts none of the tested formats exclusively");
        }
        for f in &d.formats {
            let rates: Vec<String> = f.sample_rates.iter().map(|r| r.to_string()).collect();
            let _ = writeln!(s, "      {:<17} {}", f.format, rates.join(", "));
        }
    }
    s
}

fn format_stream_test(r: &crucible_audio_io::ExclusiveStreamReport) -> String {
    let mut s = String::from("EXCLUSIVE-MODE TIMING TEST (output was silence)\n");
    let (Some(o), None) = (&r.output, &r.error) else {
        let _ = writeln!(s, "  RESULT         : FAILED. {}", r.error.as_deref().unwrap_or("unknown error"));
        return s;
    };
    let st = &r.stats;
    let _ = writeln!(s, "  device         : {}", o.device);
    let _ = writeln!(s, "  format         : {}, {} ch at {} Hz", o.format, o.device_channels, o.sample_rate);
    let _ = writeln!(s, "  buffer granted : {} frames = {:.2} ms (requested period {:.2} ms)", o.buffer_frames, o.buffer_ms, o.requested_period_ms);
    let _ = writeln!(s, "  wake-ups       : {} of about {} expected in {:.1} s", st.wakeups, r.expected_wakeups, r.seconds);
    let _ = writeln!(s, "  timeouts       : {}  (any above 0 means dropouts)", st.timeouts);
    let _ = writeln!(s, "  late wake-ups  : {}  (more than 1.5 periods apart)", st.late_wakeups);
    let _ = writeln!(s, "  worst gap      : {:.2} ms", st.worst_gap_ms);
    let verdict = if st.timeouts > 0 {
        "FAIL: the driver stopped signalling (W2101)"
    } else if st.late_wakeups == 0 {
        "PASS: steady timing"
    } else if (st.late_wakeups as f64) < (st.wakeups as f64 * 0.001) {
        "PASS with rare late wake-ups"
    } else {
        "MARGINAL: frequent late wake-ups; a larger buffer may be needed (W2102)"
    };
    let _ = writeln!(s, "  verdict        : {verdict}");
    s
}

fn format_midi(r: &crucible_midi_io::MidiReport) -> String {
    let mut s = String::from("MIDI INPUTS\n");
    if let Some(e) = &r.error {
        let _ = writeln!(s, "  ERROR {e}");
    }
    if r.inputs.is_empty() {
        let _ = writeln!(s, "  none found ({})", Code::NoMidiInputs.id());
    }
    for p in &r.inputs {
        let _ = writeln!(s, "  - {}", p.name);
    }
    s
}

fn format_plugins(r: &crucible_plugin_host::ScanReport) -> String {
    use crucible_plugin_host::ProbeOutcome as P;
    let mut s = String::from("VST3 PLUGINS\n");
    for f in &r.folders {
        let state = if f.exists { format!("{} bundles", f.bundles_found) } else { "not present".into() };
        let _ = writeln!(s, "  folder: {}  ({state})", f.folder.display());
    }
    let (mut ok, mut bad) = (0, 0);
    for o in &r.results {
        match o {
            P::Ok(p) => {
                ok += 1;
                let kind = if p.is_instrument { "instrument" } else { "effect" };
                let _ = writeln!(
                    s,
                    "  OK      {} | {} | v{} | {kind} | {} | in {} / out {} | midi in: {} | editor: {}",
                    p.name, p.vendor, p.version, p.category, p.audio_inputs, p.audio_outputs,
                    yes(p.has_midi_input), yes(p.has_editor),
                );
                let _ = writeln!(s, "          class id {}  ({})", p.class_id, p.path.display());
                if p.classes.len() > 1 {
                    let _ = writeln!(s, "          bundle holds {} classes:", p.classes.len());
                    for c in &p.classes {
                        let _ = writeln!(s, "            - {} [{}] {}", c.name, c.category, c.class_id);
                    }
                }
            }
            P::Failed { path, detail } => {
                bad += 1;
                let _ = writeln!(s, "  FAILED  {} {}: {detail}", Code::PluginLoadFailed.id(), path.display());
            }
            P::Crashed { path, detail } => {
                bad += 1;
                let _ = writeln!(s, "  CRASHED {} {}: {detail}", Code::PluginCrashed.id(), path.display());
            }
            P::TimedOut { path, after_secs } => {
                bad += 1;
                let _ = writeln!(s, "  TIMEOUT {} {} after {after_secs}s", Code::PluginTimedOut.id(), path.display());
            }
        }
    }
    let _ = writeln!(s, "  summary: {ok} loaded, {bad} problems");
    s
}

fn opt_str<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "?".into())
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_head_and_tail() {
        let long = format!("HEADER\n{}\nTHE-END", "x".repeat(REPORT_MAX_BYTES * 2));
        let c = clip(&long);
        assert!(c.starts_with("HEADER") && c.ends_with("THE-END") && c.contains("cut from the middle"));
        assert!(c.len() < REPORT_MAX_BYTES + 200);
        assert_eq!(clip("short"), "short");
    }

    #[test]
    fn host_and_engine_names_do_not_collide_and_have_help() {
        let mut names = std::collections::HashSet::new();
        for (n, h) in HOST.iter().map(|s| (s.name, s.help)).chain(Engine::specs().iter().map(|s| (s.name, s.help))) {
            assert!(names.insert(n), "{n} defined twice");
            assert!(!h.is_empty());
        }
    }

    #[test]
    fn scripts_stop_at_the_first_error_with_its_code() {
        let mut cli = Cli { engine: Engine::new(), json: false, depth: 0 };
        let e = cli.run_text("track.add A\ntrack.gain A 500\ntrack.add B", "test").unwrap_err();
        assert_eq!(diag::find_code(&e), Some(Code::ValueOutOfRange));
        assert!(format!("{e:#}").contains("command 2"), "{e:#}");
        assert_eq!(cli.engine.project().tracks.len(), 1, "stopped before the third command");
        let e = cli.run_text("trak.add X", "test").unwrap_err();
        assert!(format!("{e:#}").contains("track.add"), "suggests the right name: {e:#}");
    }
}
