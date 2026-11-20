//! Crucible's window. A thin Tauri 2 shell: the web UI sends command text, this runs it through
//! the same engine the `crucible` CLI uses and returns the replies plus the new project state.
//! No behaviour lives here or in the UI (D-022): every change is a command.
//!
//! Portability: no OS-specific code. Tauri uses WebView2 on Windows, WebKit on macOS and Linux.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crucible_diagnostics::{self as diag};
use crucible_engine::{Engine, command};
use serde_json::{Value, json};
use std::sync::Mutex;

struct Shared(Mutex<Engine>);

/// Run one line of command text (statements split by `;`). Stops at the first error, like
/// `crucible -c`. Always returns the state afterwards so the UI can redraw from the truth.
#[tauri::command]
fn exec(line: String, shared: tauri::State<'_, Shared>) -> Value {
    // A panic inside a command poisons the lock; the engine restores its own snapshot on
    // errors, so carrying on with the inner value is safe.
    let mut engine = shared.0.lock().unwrap_or_else(|p| p.into_inner());
    let mut replies = Vec::new();
    match command::split_statements(&line) {
        Err(e) => replies.push(error_json(&line, &e)),
        Ok(statements) => {
            for tokens in statements {
                let text = tokens.join(" ");
                if tokens[0] == "help" {
                    replies.push(json!({ "ok": true, "command": text, "text": help(tokens.get(1).map(String::as_str)) }));
                    continue;
                }
                diag::info(format!("> {text}"));
                match engine.execute(&tokens) {
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
    json!({ "replies": replies, "state": engine.state() })
}

fn error_json(command: &str, e: &crucible_diagnostics::DawError) -> Value {
    json!({ "ok": false, "command": command, "code": e.code.id(), "message": e.message,
            "title": e.code.title(), "help": e.code.help() })
}

/// The engine's commands. The CLI's machine commands (diag, audio.devices, ...) are not in the
/// window yet; run them with crucible.exe.
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
    out.trim_end().to_string()
}

fn main() {
    diag::start("crucible-app", "window");
    let result = tauri::Builder::default()
        .manage(Shared(Mutex::new(Engine::new())))
        .invoke_handler(tauri::generate_handler![exec])
        .run(tauri::generate_context!())
        .map_err(|e| anyhow::anyhow!("window failed: {e}"));
    std::process::exit(diag::finish(result));
}
