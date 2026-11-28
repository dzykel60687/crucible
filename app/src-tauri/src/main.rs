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
use crucible_engine::{Engine, ProbeCommand, Reply, command, trail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Shared(Arc<Mutex<Engine>>);

/// Whether the window is in front, for the once-a-second line.
static IN_FRONT: AtomicBool = AtomicBool::new(false);

impl Shared {
    /// A panic inside a command poisons the lock; the engine restores its own snapshot on
    /// errors, so carrying on with the inner value is safe.
    fn engine(&self) -> MutexGuard<'_, Engine> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The command running now and when it began, for the watchdog.
static RUNNING: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// True at most four times a second: for events that arrive in a stream.
fn not_too_often() -> bool {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if last.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

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

/// One thread, once a second, for two jobs:
/// - It says so in the log when a command has been running for 5, 30, 120 and 600 seconds. If
///   the window ever hangs, the log's last such line names the command that never came back.
/// - While the log records everything (test builds), it writes an `ALIVE` line every second:
///   what the engine is doing, how playback did in that second, memory and processor use. The
///   log then fills steadily while the program runs, and the last line before a gap is the last
///   moment the program was well.
fn start_watchdog(engine: Arc<Mutex<Engine>>) {
    const AFTER: [u64; 4] = [5, 30, 120, 600];
    let spawned = std::thread::Builder::new().name("crucible watchdog".into()).spawn(move || {
        let mut warned: Option<(Instant, usize)> = None;
        let mut process = diag::ProcessWatch::start();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let running = RUNNING.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if diag::everything() {
                // Never waits for the engine: a command that holds it is itself the news.
                let doing = match engine.try_lock() {
                    Ok(e) => e.alive(),
                    Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().alive(),
                    Err(std::sync::TryLockError::WouldBlock) => match &running {
                        Some((text, began)) => format!("busy for {:.1} s with: {text}", began.elapsed().as_secs_f64()),
                        None => "busy with a command".to_string(),
                    },
                };
                let (memory, cpu) = process.as_mut().map_or((0.0, 0.0), |p| p.sample());
                let front = if IN_FRONT.load(Ordering::Relaxed) { "in front" } else { "in the background" };
                diag::event("ALIVE", format!("{doing} | window {front} | memory {memory:.0} MB, processor {cpu:.0}%"));
            }
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
fn exec(line: String, since: Option<u64>, shared: tauri::State<'_, Shared>, window: tauri::WebviewWindow) -> Value {
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
                    // Waits for the person, not for the engine: no lock held, no watchdog.
                    "ui.pick_folder" | "ui.pick_files" => pick(&tokens, &window),
                    // Asking every plugin what it is can take minutes, and needs nothing from
                    // the engine: the window stays usable meanwhile.
                    "plugin.scan" | "plugin.folder" => {
                        let _busy = Busy::with(&text);
                        scan(&tokens)
                    }
                    "report" => {
                        let _busy = Busy::with(&text);
                        let out = tokens.iter().position(|t| t == "--out").and_then(|i| tokens.get(i + 1)).map(PathBuf::from);
                        report(&shared, out)
                    }
                    // The window asks for these many times a second: not worth the watchdog's lock.
                    "transport.status" | "log.ui" | "instrument.status" => shared.engine().execute(&tokens),
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

/// `ui.pick_folder` and `ui.pick_files`, both `[--title text] [--start folder]`: the system's own
/// picker (Explorer's on Windows), in front of the window, for Browse and Import (D-042).
/// Answers the folder chosen (or none), or the audio files chosen (or none).
/// The window has no file access of its own (D-022), so the picker is a command like any other
/// and lands in the log. It runs on a thread of its own, which the dialog needs for its COM
/// apartment on Windows, and it is not given the engine.
fn pick(tokens: &[String], window: &tauri::WebviewWindow) -> Result<Reply, DawError> {
    let value = |name: &str| tokens.iter().position(|t| t == name).and_then(|i| tokens.get(i + 1)).cloned();
    let files = tokens[0] == "ui.pick_files";
    let reply = |picked: Vec<PathBuf>, how: &str| {
        let text = match picked.as_slice() {
            [] => "Nothing chosen".to_string(),
            [one] => format!("Chose {}{how}", one.display()),
            many => format!("Chose {} files{how}", many.len()),
        };
        Reply::new(text, if files { json!({ "files": picked }) } else { json!({ "folder": picked.first() }) })
    };
    // The GUI test (tools/gui-test.ps1) cannot work the system's picker. It puts its answer in
    // the environment before it starts the window and gets it back here. With `--ask` the real
    // picker is shown all the same; the test checks that it appears, and closes it.
    let canned = std::env::var_os(if files { "CRUCIBLE_TEST_PICK_FILES" } else { "CRUCIBLE_TEST_PICK_FOLDER" });
    if let Some(c) = canned.filter(|_| !tokens.iter().any(|t| t == "--ask")) {
        return Ok(reply(std::env::split_paths(&c).collect(), " (the test's answer; no picker was shown)"));
    }
    let (title, start, window) = (value("--title"), value("--start"), window.clone());
    let picked = std::thread::Builder::new()
        .name("crucible picker".into())
        .spawn(move || {
            let mut d = rfd::FileDialog::new().set_title(title.as_deref().unwrap_or(if files { "Choose audio files" } else { "Choose a folder" })).set_parent(&window);
            if let Some(s) = start.map(PathBuf::from).filter(|s| s.is_dir()) {
                d = d.set_directory(s);
            }
            if files {
                d.add_filter("Audio (WAV, MP3, FLAC)", &["wav", "wave", "mp3", "flac"]).add_filter("All files", &["*"]).pick_files().unwrap_or_default()
            } else {
                d.pick_folder().into_iter().collect()
            }
        })
        .map_err(|e| DawError::new(Code::Uncategorised, format!("the picker could not start: {e}")))?
        .join()
        .map_err(|_| DawError::new(Code::Uncategorised, "the picker stopped unexpectedly"))?;
    Ok(reply(picked, ""))
}

/// The argument that makes this program ask one plugin what it is and end, instead of opening
/// a window. A scan starts one such child per plugin, so a plugin that crashes or hangs while
/// being asked takes only that child with it.
const PROBE_ARGUMENT: &str = "--probe-plugin";

fn probe_command() -> Result<ProbeCommand, DawError> {
    let exe = std::env::current_exe().map_err(|e| DawError::new(Code::FileRead, format!("cannot locate this program to ask plugins with: {e}")))?;
    Ok(ProbeCommand { exe, args_before_path: vec![PROBE_ARGUMENT.to_string()] })
}

/// `plugin.scan [--all] [--timeout n]` and `plugin.folder <folder>`, run without the engine.
fn scan(tokens: &[String]) -> Result<Reply, DawError> {
    let probe = probe_command()?;
    if tokens[0] == "plugin.folder" {
        let folder = tokens.get(1).ok_or_else(|| DawError::new(Code::BadArgument, "plugin.folder needs a folder"))?;
        return crucible_engine::add_plugin_folder(&probe, std::path::Path::new(folder));
    }
    let timeout = tokens.iter().position(|t| t == "--timeout").and_then(|i| tokens.get(i + 1)).and_then(|t| t.parse().ok());
    crucible_engine::scan_plugins(&probe, tokens.iter().any(|t| t == "--all"), timeout)
}

/// Plugins must be loaded, asked for their settings and shown on the main thread, and their
/// windows need a look-in from it a few times a second (D-044). Commands run on other threads,
/// so the engine is told how to reach this one; a small thread asks for the look-in.
fn serve_plugins(app: &tauri::AppHandle, engine: &Arc<Mutex<Engine>>) {
    let mut e = engine.lock().unwrap_or_else(|p| p.into_inner());
    let handle = app.clone();
    e.set_main_thread(Arc::new(move |job| {
        // If the event loop has ended the job is dropped, and whoever waits for it is told.
        let _ = handle.run_on_main_thread(job);
    }));
    match probe_command() {
        Ok(p) => e.set_probe(p),
        Err(err) => diag::warn(err.code, err.message),
    }
    // Plugins' own windows float above this program's window. The handle comes through the
    // portable window-handle interface; today only its Windows kind is put to use (the engine's
    // plugin host says what macOS and Linux would need).
    {
        use tauri::Manager;
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Some(main) = app.webview_windows().values().next()
            && let Ok(handle) = main.window_handle()
            && let RawWindowHandle::Win32(w) = handle.as_raw()
        {
            e.set_main_window(w.hwnd.get());
        }
    }
    let (service, handle) = (e.plugin_service(), app.clone());
    drop(e);
    let waiting = Arc::new(AtomicBool::new(false));
    let spawned = std::thread::Builder::new().name("crucible plugin windows".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(33));
            // One look-in at a time: while the main thread is busy (loading a big instrument),
            // they must not queue up behind it.
            if waiting.swap(true, Ordering::AcqRel) {
                continue;
            }
            let (service, done) = (service.clone(), waiting.clone());
            let posted = handle.run_on_main_thread(move || {
                service.tick();
                done.store(false, Ordering::Release);
            });
            if posted.is_err() {
                return; // the program is closing
            }
        }
    });
    if let Err(e) = spawned {
        diag::warn(Code::EditorEventError, format!("plugin windows will not be looked after: {e}"));
    }
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
    // plugin.scan and plugin.folder are in the engine's list; the window runs them itself so
    // that the engine stays free meanwhile.
    for (pick, what) in [("ui.pick_folder", "a folder"), ("ui.pick_files", "audio files")] {
        if name.is_none_or(|n| pick.starts_with(n)) {
            out.push_str(&format!("  {pick:<18} Choose {what} with the system's picker [--title text] [--start folder].\n"));
        }
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
    // As a scan's child: ask the one plugin and end. As little as possible happens before the
    // plugin is loaded; a native crash is reported on stderr, which the parent collects.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some(PROBE_ARGUMENT) {
        diag::start_child();
        let code = match args.next() {
            Some(bundle) => crucible_plugin_host::run_probe_child(std::path::Path::new(&bundle)),
            None => 2,
        };
        std::process::exit(code);
    }
    diag::start("crucible-app", "window");
    diag::info(format!("window: Tauri {}, web view {}", tauri::VERSION, tauri::webview_version().unwrap_or_else(|e| format!("unknown ({e})"))));
    let engine = Arc::new(Mutex::new(Engine::new()));
    start_watchdog(engine.clone());
    let for_plugins = engine.clone();
    let app = tauri::Builder::default()
        .setup(move |app| {
            serve_plugins(app.handle(), &for_plugins);
            Ok(())
        })
        .manage(Shared(engine))
        .invoke_handler(tauri::generate_handler![exec])
        .build(tauri::generate_context!());
    match app {
        // Tauri ends the process itself when the last window closes, so the log is finished on
        // its Exit event; code after `run` never runs. Before 2026-11-22 logs had no end line.
        Ok(app) => app.run(|app, event| match event {
            tauri::RunEvent::WindowEvent { event, .. } => match event {
                tauri::WindowEvent::Focused(on) => {
                    IN_FRONT.store(on, Ordering::Relaxed);
                    diag::event("UI", format!("did: window {}", if on { "came to the front" } else { "went to the background" }));
                }
                // A resize or a move arrives as a stream of events: a few a second is the whole story.
                tauri::WindowEvent::Resized(size) if diag::everything() && not_too_often() => diag::event("UI", format!("saw: window is {}x{} pixels", size.width, size.height)),
                tauri::WindowEvent::Moved(at) if diag::everything() && not_too_often() => diag::event("UI", format!("saw: window moved to {},{}", at.x, at.y)),
                // The page listens for this too: with unsaved changes it keeps the window open and
                // asks Save, Don't save or Cancel; otherwise it closes the window itself.
                tauri::WindowEvent::CloseRequested { .. } => diag::event("UI", "did: asked the window to close"),
                tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => diag::event("UI", format!("saw: display scale is now {scale_factor}")),
                _ => {}
            },
            tauri::RunEvent::Exit => {
                // Release the audio device and put playback's totals in the log, then remove an
                // unsaved project's folder (the window has already asked about saving it).
                use tauri::Manager;
                // A command that is waiting for this thread gives up, so the engine comes free.
                Engine::closing();
                let shared = app.state::<Shared>();
                let _ = shared.engine().execute(&["transport.stop".to_string()]);
                shared.engine().close();
                diag::finish(Ok(()));
            }
            _ => {}
        }),
        Err(e) => std::process::exit(diag::finish(Err(anyhow::anyhow!("window failed: {e}")))),
    }
}
