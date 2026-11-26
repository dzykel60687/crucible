//! The session log: one plain-text file per program, beside the program.
//!
//! Design choices, each for diagnosing problems on machines no developer can see:
//! - **One file, where the program is.** `crucible-app.log` beside `crucible-app.exe`. Every run
//!   starts it afresh (Dzyan, 2026-11-25). The run before is kept once, as
//!   `crucible-app.previous.log`: after a crash the next start would otherwise erase the very
//!   log that explains it. Where the program's folder cannot be written (Program Files), the
//!   log goes to the per-user folder instead ([`data_dir`]).
//! - **Write-through.** Every line goes to the file as it happens, not when the program
//!   exits. If a plugin crashes the process, the log still holds everything up to the crash.
//! - **Header first.** Program build, command line, computer, OS, CPU and memory come before
//!   anything can go wrong.
//! - **Codes everywhere.** Warnings and errors carry a [`Code`] with help text.
//! - **Libraries included.** Records from crates that use the `log` facade (vst3-host,
//!   wasapi, midir) land in the same file.
//! - **Real-time safe by refusal.** Records from threads marked with [`mark_realtime_thread`]
//!   are dropped and counted, never written, so the audio thread never touches a file.
//!
//! Portability: no OS-specific code here. The log folder comes from `dirs`. Crash capture is
//! in the platform files.

use crate::codes::{Code, Severity, find_code};
use crate::platform;
use std::cell::Cell;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct State {
    file: Mutex<Option<File>>,
    path: Option<PathBuf>,
    start: Instant,
    /// Codes whose help text was already shown on the console this session.
    helped: Mutex<Vec<Code>>,
}

static STATE: OnceLock<State> = OnceLock::new();
static VERBOSE: AtomicBool = AtomicBool::new(false);
static DROPPED_RT: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static REALTIME: Cell<bool> = const { Cell::new(false) };
}

/// Crucible's per-user folder: `%LOCALAPPDATA%\Crucible` on Windows,
/// `~/Library/Application Support/Crucible` on macOS, `~/.local/share/Crucible` on Linux. Holds
/// this machine's settings, and the logs when the program's own folder cannot be written.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Crucible"))
}

/// The folders a log can be in, in the order they are tried: the program's own, then the
/// per-user one.
pub fn log_dirs() -> Vec<PathBuf> {
    let own = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let mut dirs: Vec<PathBuf> = own.into_iter().chain(data_dir()).collect();
    dirs.dedup();
    dirs
}

/// The current session's log file, if one could be created.
pub fn log_path() -> Option<PathBuf> {
    STATE.get().and_then(|s| s.path.clone())
}

/// Start this run's log. Call first thing in `main`. Later calls do nothing.
///
/// Creates `<program>.log` (see the module docs for where), writes the header, and installs the
/// panic hook, the native crash handler and the bridge for the `log` facade.
/// `label` is usually the command, e.g. `play-omnisphere`.
pub fn start(program: &str, label: &str) -> Option<PathBuf> {
    if STATE.get().is_some() {
        return log_path();
    }
    let opened = open_log_file(program);
    let (file, path, open_error) = match opened {
        Ok((f, p)) => (Some(f), Some(p), None),
        Err(e) => (None, None, Some(e)),
    };
    let crash_file = file.as_ref().and_then(|f| f.try_clone().ok());
    let _ = STATE.set(State { file: Mutex::new(file), path: path.clone(), start: Instant::now(), helped: Mutex::new(Vec::new()) });

    write_header(program, label, path.as_deref());
    install_panic_hook();
    platform::install_crash_handler(crash_file, path.as_ref().map(|p| p.with_extension("dmp")));
    if log::set_logger(&BRIDGE).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    if let Some(e) = open_error {
        warn(Code::LogUnavailable, e);
    }
    path
}

/// For short-lived helper processes, such as the plugin probe: no log file, but a native
/// crash is described on stderr, where the parent process collects it.
pub fn start_child() {
    platform::install_crash_handler(None, None);
}

/// Let `debug` records from libraries into the log. Turn on around steps worth tracing in
/// detail, such as loading a plugin; vst3-host then logs each loading step.
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
    log::set_max_level(if on { log::LevelFilter::Debug } else { log::LevelFilter::Info });
}

/// Mark the calling thread as real-time. Its log records are dropped and counted.
pub fn mark_realtime_thread() {
    REALTIME.with(|r| r.set(true));
}

/// How many library log records were dropped because they came from a real-time thread.
pub fn dropped_realtime_records() -> u64 {
    DROPPED_RT.load(Ordering::Relaxed)
}

/// Log file only.
pub fn info(msg: impl AsRef<str>) {
    write_line("INFO", msg.as_ref());
}

/// Log file only, under a tag of its own instead of a level, so one kind of line can be picked
/// out of a long log. Tags in use (five letters at most):
/// - `UI`: what the person did in the window (clicks, keys, drags) and what it showed them.
/// - `MARK`: a note the person dropped at the moment something went wrong (`mark` command).
/// - `AUDIO`: playback start, health while playing, and stop.
pub fn event(tag: &str, msg: impl AsRef<str>) {
    write_line(tag, msg.as_ref());
}

/// The clock time `seconds` ago, in the form the log's own lines use: `21:42:07.123`.
pub fn clock_before(seconds: f64) -> String {
    let then = chrono::Local::now() - chrono::Duration::milliseconds((seconds.max(0.0) * 1000.0) as i64);
    then.format("%H:%M:%S%.3f").to_string()
}

/// Which commit this program was built from, e.g. `commit 62b5c38cd830 of 2026-11-23`.
/// See `build.rs` for its one limit.
pub fn source() -> &'static str {
    env!("CRUCIBLE_SOURCE")
}

/// Where a problem report is written when nobody chose a folder: the Desktop, so the person
/// can find and send it; else Documents; else the log folder.
pub fn report_dir() -> Option<PathBuf> {
    [dirs::desktop_dir(), dirs::document_dir(), data_dir()].into_iter().flatten().find(|d| d.is_dir())
}

/// A block of text copied into the log as it is, under a heading. Log file only.
/// For whole reports, such as the device list, that are easier to read unindented.
pub fn raw(title: &str, text: &str) {
    write_line("INFO", &format!("---- {title} (copied below) ----"));
    if let Some(s) = STATE.get() {
        let mut g = s.file.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(f) = g.as_mut() {
            let body = if text.ends_with('\n') { text.to_string() } else { format!("{text}\n") };
            let _ = f.write_all(format!("{body}---- end of {title} ----\n").as_bytes());
        }
    }
}

/// Local time as `YYYY-MM-DD_HH-MM-SS`, safe in a file name on every system.
pub fn file_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Console (stdout) and log file.
pub fn note(msg: impl AsRef<str>) {
    println!("{}", msg.as_ref());
    write_line("INFO", msg.as_ref());
}

/// A problem that did not stop the program. Console (stderr) and log file.
pub fn warn(code: Code, detail: impl AsRef<str>) {
    report("WARN", code, detail.as_ref());
}

/// A failure. Console (stderr) and log file.
pub fn error(code: Code, detail: impl AsRef<str>) {
    report("ERROR", code, detail.as_ref());
}

fn report(level: &str, code: Code, detail: &str) {
    // The code is already in the heading; drop its repeat from the start of the detail.
    let repeated = format!("{}: ", code.id());
    let detail = detail.replace(&repeated, "");
    let detail = detail.as_str();
    let label = if code.severity() == Severity::Warning { "WARNING" } else { "ERROR" };
    let head = format!("{} {}", code.id(), code.title());
    let first_time = STATE.get().is_none_or(|s| {
        let mut h = s.helped.lock().unwrap_or_else(|p| p.into_inner());
        if h.contains(&code) {
            false
        } else {
            h.push(code);
            true
        }
    });
    let mut console = format!("{label} {head}");
    if !detail.is_empty() {
        console.push_str(&format!("\n  detail     : {detail}"));
    }
    if first_time {
        console.push_str(&format!("\n  what to try: {}", code.help()));
    }
    eprintln!("{console}");
    let mut line = head;
    if !detail.is_empty() {
        line.push_str(&format!(" | {detail}"));
    }
    if first_time {
        line.push_str(&format!("\nhelp: {}", code.help()));
    }
    write_line(level, &line);
}

/// End the run: log the outcome, tell the person where the log is if it failed, and return
/// the process exit code. Use as `std::process::exit(crucible_diagnostics::finish(run()))`.
pub fn finish(result: anyhow::Result<()>) -> i32 {
    let dropped = dropped_realtime_records();
    if dropped > 0 {
        info(format!("{dropped} library log records from the audio thread were dropped (real-time rule)"));
    }
    match result {
        Ok(()) => {
            info("finished: OK (exit code 0)");
            0
        }
        Err(e) => {
            let code = find_code(&e).unwrap_or(Code::Uncategorised);
            error(code, format!("{e:#}"));
            let exit = code.exit_code();
            info(format!("finished: FAILED with {} (exit code {exit})", code.id()));
            match log_path() {
                Some(p) => eprintln!("  log file   : {}", p.display()),
                None => eprintln!("  log file   : none (the log could not be created)"),
            }
            exit
        }
    }
}

// ---------------------------------------------------------------- internals

/// `<program>.log` in the first folder that can be written. What the last run left there moves
/// to `<program>.previous.log` (and its crash dump to `.previous.dmp`), replacing the one before.
fn open_log_file(program: &str) -> Result<(File, PathBuf), String> {
    let name: String = program.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let mut refused = Vec::new();
    for dir in log_dirs() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            refused.push(format!("{}: {e}", dir.display()));
            continue;
        }
        // Can this folder be written at all? Asked before the old log is touched.
        let probe = dir.join(format!("{name}.log.new"));
        if let Err(e) = File::create(&probe) {
            refused.push(format!("{}: {e}", dir.display()));
            continue;
        }
        let _ = std::fs::remove_file(&probe);
        for ext in ["log", "dmp"] {
            let (now, before) = (dir.join(format!("{name}.{ext}")), dir.join(format!("{name}.previous.{ext}")));
            if now.metadata().is_ok_and(|m| m.len() > 0) {
                let _ = std::fs::remove_file(&before);
                let _ = std::fs::rename(&now, &before);
            }
        }
        let path = dir.join(format!("{name}.log"));
        match File::create(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) => refused.push(format!("{}: {e}", path.display())),
        }
    }
    Err(if refused.is_empty() { "this system has no folder for it".to_string() } else { refused.join("; ") })
}

fn write_header(program: &str, label: &str, path: Option<&Path>) {
    let exe = std::env::current_exe().ok();
    let mut h = vec![format!("Crucible log | {program} {label}")];
    let fingerprint = exe.as_deref().and_then(crate::system::exe_fingerprint).unwrap_or_else(|| "?".into());
    h.push(format!(
        "program  : {program} {} ({}) | build fingerprint {fingerprint}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) { "debug build" } else { "release build" }
    ));
    h.push(format!("source   : {}", source()));
    if let Some(exe) = &exe {
        let meta = std::fs::metadata(exe).ok();
        let size = meta.as_ref().map(|m| format!("{:.1} MB", m.len() as f64 / 1_048_576.0)).unwrap_or_else(|| "?".into());
        let modified = meta
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "?".into());
        h.push(format!("exe      : {} ({size}, modified {modified})", exe.display()));
    }
    let args: Vec<String> = std::env::args().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a }).collect();
    h.push(format!("command  : {}", args.join(" ")));
    h.push(format!("folder   : {}", std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_else(|_| "?".into())));
    let now = chrono::Local::now();
    h.push(format!("started  : {} (UTC {})", now.format("%Y-%m-%d %H:%M:%S %:z"), now.to_utc().format("%Y-%m-%d %H:%M:%S")));
    for (label, value) in crate::system::system_summary() {
        h.push(format!("{label:<9}: {value}"));
    }
    if let Some(p) = path {
        h.push(format!("log file : {}", p.display()));
    }
    h.push("Times below are local: clock time, then seconds since start.".into());
    h.push(String::new());
    if let Some(s) = STATE.get() {
        let mut g = s.file.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(f) = g.as_mut() {
            let _ = f.write_all((h.join("\n") + "\n").as_bytes());
        }
    }
}

fn write_line(level: &str, msg: &str) {
    let Some(s) = STATE.get() else { return };
    let prefix = format!(
        "{} +{:.2}s {level:<5} ",
        chrono::Local::now().format("%H:%M:%S%.3f"),
        s.start.elapsed().as_secs_f64()
    );
    let indent = " ".repeat(prefix.len());
    let mut text = prefix;
    for (i, l) in msg.lines().enumerate() {
        if i > 0 {
            text.push('\n');
            text.push_str(&indent);
        }
        text.push_str(l);
    }
    text.push('\n');
    let mut g = s.file.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(f) = g.as_mut() {
        // Unbuffered: the bytes reach the operating system now and survive a process crash.
        let _ = f.write_all(text.as_bytes());
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "?".into());
        let trace = std::backtrace::Backtrace::force_capture();
        // Written even from a real-time thread: the thread is failing anyway.
        write_line(
            "PANIC",
            &format!(
                "{} {} | thread '{}' at {location}: {msg}\nhelp: {}\nstack:\n{trace}",
                Code::Panic.id(),
                Code::Panic.title(),
                thread.name().unwrap_or("unnamed"),
                Code::Panic.help()
            ),
        );
        previous(info);
    }));
}

/// Routes `log` facade records from libraries into the session log.
struct Bridge;
static BRIDGE: Bridge = Bridge;

impl log::Log for Bridge {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info || (VERBOSE.load(Ordering::Relaxed) && m.level() <= log::Level::Debug)
    }

    fn log(&self, r: &log::Record) {
        if REALTIME.with(Cell::get) {
            DROPPED_RT.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if !self.enabled(r.metadata()) {
            return;
        }
        let level = match r.level() {
            log::Level::Error => "ERROR",
            log::Level::Warn => "WARN",
            log::Level::Info => "INFO",
            _ => "DEBUG",
        };
        write_line(level, &format!("[{}] {}", r.target(), r.args()));
    }

    fn flush(&self) {}
}
