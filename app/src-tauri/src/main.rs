//! Crucible's window. A thin Tauri 2 shell: the web UI sends command text, this runs it through
//! the same engine the `crucible` CLI uses and returns the replies plus the new project state.
//! No behaviour lives here or in the UI (D-022): every change is a command.
//!
//! The session log (D-037) gets every command with its outcome and timing, what the person did
//! and saw in the window (the UI sends `log.ui` commands), window events, and a warning when a
//! command runs for a long time, so a log sent from another machine tells the whole session.
//!
//! Portability: no OS-specific code. Tauri uses WebView2 on Windows, WebKit on macOS and Linux.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crucible_diagnostics::{self as diag, Code, DawError};
use crucible_engine::{Engine, Reply, command, trail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Shared(Mutex<Engine>);

impl Shared {
    /// A panic inside a command poisons the lock; the engine restores its own snapshot on
    /// errors, so carrying on with the inner value is safe.
    fn engine(&self) -> MutexGuard<'_, Engine> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The command running now and when it began, for the watchdog.
static RUNNING: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Marks a command as running until dropped, also when the command panics.
struct Busy;

impl Busy {
    fn with(text: &str) -> Busy {
        *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = Some((text.to_string(), Instant::now()));
        Busy
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// Says so in the log when a command has been running for 5, 30, 120 and 600 seconds. If the
/// window ever hangs, the log's last such line names the command that never came back.
fn start_watchdog() {
    const AFTER: [u64; 4] = [5, 30, 120, 600];
    let spawned = std::thread::Builder::new().name("crucible watchdog".into()).spawn(|| {
        let mut warned: Option<(Instant, usize)> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let running = RUNNING.lock().unwrap_or_else(|p| p.into_inner()).clone();
            let Some((text, began)) = running else {
                warned = None;
                continue;
            };
            let step = warned.filter(|w| w.0 == began).map_or(0, |w| w.1);
            if let Some(&limit) = AFTER.get(step)
                && began.elapsed() >= Duration::from_secs(limit)
            {
                diag::warn(Code::SlowCommand, format!("still running after {limit} s: {text}"));
                warned = Some((began, step + 1));
            }
        }
    });
    if let Err(e) = spawned {
        diag::info(format!("no watchdog for slow commands: {e}"));
    }
}

/// Run one line of command text (statements split by `;`). Stops at the first error, like
/// `crucible -c`. Always returns the state afterwards so the UI can redraw from the truth: only
/// the clips that changed since revision `since`, or everything if the UI holds an older one.
///
/// `async` puts it on Tauri's thread pool. A plain command runs on the main thread, which also
/// runs the window, so a slow command would freeze scrolling and painting until it finished.
#[tauri::command(async)]
fn exec(line: String, since: Option<u64>, shared: tauri::State<'_, Shared>) -> Value {
    let mut replies = Vec::new();
    match command::split_statements(&line) {
        Err(e) => {
            diag::error(e.code, format!("{} | the line was: {line}", e.message));
            replies.push(error_json(&line, &e));
        }
        Ok(statements) => {
            for tokens in statements {
                let text = tokens.join(" ");
                let stamp = trail::begin(&tokens);
                let result = match tokens[0].as_str() {
                    "help" => Ok(Reply::text(help(tokens.get(1).map(String::as_str)))),
                    "report" => {
                        let _busy = Busy::with(&text);
                        let out = tokens.iter().position(|t| t == "--out").and_then(|i| tokens.get(i + 1)).map(PathBuf::from);
                        report(&shared, out)
                    }
                    // The window asks for these many times a second: not worth the watchdog's lock.
                    "transport.status" | "log.ui" => shared.engine().execute(&tokens),
                    _ => {
                        let mut engine = shared.engine();
                        let _busy = Busy::with(&text);
                        engine.execute(&tokens)
                    }
                };
                trail::end(stamp, &result);
                match result {
                    Ok(r) => replies.push(json!({ "ok": true, "command": text, "text": r.text, "data": r.data })),
                    Err(e) => {
                        diag::error(e.code, e.message.clone());
                        replies.push(error_json(&text, &e));
                        break;
                    }
                }
            }
        }
    }
    json!({ "replies": replies, "state": shared.engine().sync(since) })
}

fn error_json(command: &str, e: &DawError) -> Value {
    json!({ "ok": false, "command": command, "code": e.code.id(), "message": e.message,
            "title": e.code.title(), "help": e.code.help() })
}

/// The engine's commands, plus the two the window itself answers. The CLI's machine commands
/// (diag, audio.devices, ...) are not in the window yet; run them with crucible.exe.
fn help(name: Option<&str>) -> String {
    let specs = Engine::specs();
    if let Some(n) = name
        && let Some(s) = command::find(specs, n)
    {
        return command::describe(s);
    }
    let mut out = String::from("Engine commands (machine checks such as diag are in crucible.exe):\n");
    for s in specs.iter().filter(|s| name.is_none_or(|n| s.name.starts_with(n))) {
        out.push_str(&format!("  {:<18} {}\n", s.name, s.help));
    }
    if name.is_none_or(|n| "report".starts_with(n)) {
        out.push_str(&format!("  {:<18} {}\n", "report [--out dir]", "Write ONE file to send when something went wrong (to the Desktop): the machine check and the recent logs."));
    }
    out.trim_end().to_string()
}

/// The `crucible` command-line tool that ships with the window: beside the window program in
/// a build folder, in `tools` when installed, in `..\m0` in the staged test folder.
fn cli_tool() -> Result<PathBuf, DawError> {
    let name = format!("crucible{}", std::env::consts::EXE_SUFFIX);
    let here = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).unwrap_or_default();
    let places = [here.join(&name), here.join("tools").join(&name), here.join("..").join("m0").join(&name)];
    places.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        DawError::new(Code::ReportHelperMissing, format!("looked for {}", places.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
    })
}

/// `report [--out <folder>]`: one file to send. The engine's state goes into this run's log
/// first; then crucible.exe, which owns the machine check, gathers that check and the recent
/// logs (this one included) into a file on the Desktop, or in the folder asked for. The engine
/// is not held while it works.
fn report(shared: &Shared, out: Option<PathBuf>) -> Result<Reply, DawError> {
    shared.engine().execute(&["log.snapshot".to_string()])?;
    let tool = cli_tool()?;
    let mut cmd = diag::quiet_command(&tool);
    cmd.args(["--json", "report", "--no-plugins"]);
    if let Some(dir) = out.or_else(diag::report_dir) {
        cmd.arg("--out").arg(dir);
    }
    if let Some(log) = diag::log_path() {
        cmd.arg("--include").arg(log);
    }
    let failed = |why: String| DawError::new(Code::ReportHelperMissing, format!("{}: {why}", tool.display()));
    let out = cmd.output().map_err(|e| failed(format!("could not be started: {e}")))?;
    // With --json the tool prints one JSON line per command; anything else is progress text.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let reply = stdout.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok().filter(|v| v["ok"].is_boolean()));
    match reply {
        Some(v) if v["ok"] == true => Ok(Reply::new(v["text"].as_str().unwrap_or("Report written").to_string(), v["data"].clone())),
        Some(v) => {
            let code = v["code"].as_str().and_then(Code::from_id).unwrap_or(Code::Uncategorised);
            Err(DawError::new(code, v["message"].as_str().unwrap_or("the report failed").to_string()))
        }
        None => Err(failed(format!("ended with {} and no answer; its last words: {}", diag::describe_exit(&out.status), String::from_utf8_lossy(&out.stderr).trim()))),
    }
}

fn main() {
    diag::start("crucible-app", "window");
    diag::info(format!("window: Tauri {}, web view {}", tauri::VERSION, tauri::webview_version().unwrap_or_else(|e| format!("unknown ({e})"))));
    start_watchdog();
    let app = tauri::Builder::default()
        .manage(Shared(Mutex::new(Engine::new())))
        .invoke_handler(tauri::generate_handler![exec])
        .build(tauri::generate_context!());
    match app {
        // Tauri ends the process itself when the last window closes, so the log is finished on
        // its Exit event; code after `run` never runs. Before 2026-11-22 logs had no end line.
        Ok(app) => app.run(|app, event| match event {
            tauri::RunEvent::WindowEvent { event, .. } => match event {
                tauri::WindowEvent::Focused(on) => diag::event("UI", format!("did: window {}", if on { "came to the front" } else { "went to the background" })),
                tauri::WindowEvent::CloseRequested { .. } => diag::event("UI", "did: asked the window to close"),
                tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => diag::event("UI", format!("saw: display scale is now {scale_factor}")),
                _ => {}
            },
            tauri::RunEvent::Exit => {
                // Release the audio device and put playback's totals in the log.
                use tauri::Manager;
                let _ = app.state::<Shared>().engine().execute(&["transport.stop".to_string()]);
                diag::finish(Ok(()));
            }
            _ => {}
        }),
        Err(e) => std::process::exit(diag::finish(Err(anyhow::anyhow!("window failed: {e}")))),
    }
}
