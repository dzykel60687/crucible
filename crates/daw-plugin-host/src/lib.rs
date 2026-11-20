//! VST3 plugin discovery and, later, hosting.
//!
//! Milestone M0 step A: find plugin bundles and read their metadata safely.
//!
//! Safety model: reading a plugin's metadata means loading its code, and a broken plugin
//! can crash or hang whatever process loads it. So [`scan`] never loads a plugin itself.
//! It starts one child process per plugin (see [`ProbeCommand`]) and kills it after a
//! timeout. The child calls [`probe_in_process`] and prints one marked JSON line.
//!
//! Portability: only `plugin_paths` (and later `editor_window` and `module_loader`) may
//! contain OS-specific code. See docs/03-platform-notes.md.

pub mod plugin_paths;

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Prefix of the one stdout line the probe child writes. Plugins sometimes print their
/// own noise to stdout, so the parent ignores every line without this prefix.
pub const PROBE_MARKER: &str = "@@RUST_DAW_PROBE@@";

/// Default time allowed for one plugin to load and report. Large sample-based
/// instruments can be slow to instantiate, so this is generous.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// What the DAW needs to know about one plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummary {
    pub path: PathBuf,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// VST3 sub-categories, e.g. "Instrument|Synth" or "Fx|EQ".
    pub category: String,
    /// The VST3 class ID. This, not the path, is how a project refers to a plugin.
    pub class_id: String,
    pub is_instrument: bool,
    pub audio_inputs: u32,
    pub audio_outputs: u32,
    pub has_midi_input: bool,
    pub has_editor: bool,
    /// Every audio-processor class in the bundle. Most bundles have exactly one.
    pub classes: Vec<ClassSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassSummary {
    pub name: String,
    pub category: String,
    pub class_id: String,
    pub version: String,
}

/// Outcome of probing one bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProbeOutcome {
    Ok(PluginSummary),
    /// The plugin loaded far enough to report an error.
    Failed { path: PathBuf, detail: String },
    /// The child process died without a result: the plugin crashed while loading.
    Crashed { path: PathBuf, detail: String },
    /// The plugin took longer than the timeout and the child was killed.
    TimedOut { path: PathBuf, after_secs: u64 },
}

impl ProbeOutcome {
    pub fn path(&self) -> &Path {
        match self {
            ProbeOutcome::Ok(s) => &s.path,
            ProbeOutcome::Failed { path, .. }
            | ProbeOutcome::Crashed { path, .. }
            | ProbeOutcome::TimedOut { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub folders: Vec<FolderReport>,
    pub results: Vec<ProbeOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderReport {
    pub folder: PathBuf,
    pub exists: bool,
    pub bundles_found: usize,
}

/// How to start a probe child: an executable plus the arguments that come before the
/// bundle path. The CLI passes itself, e.g. `crucible.exe probe-plugin <bundle>`.
#[derive(Debug, Clone)]
pub struct ProbeCommand {
    pub exe: PathBuf,
    pub args_before_path: Vec<String>,
}

/// Find `.vst3` bundles under the given folders. Loads nothing.
pub fn find_bundles(folders: &[PathBuf]) -> (Vec<FolderReport>, Vec<PathBuf>) {
    let mut reports = Vec::new();
    let mut all = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for folder in folders {
        let exists = folder.is_dir();
        let mut found = Vec::new();
        if exists {
            walk(folder, &mut found, &mut visited);
        }
        reports.push(FolderReport { folder: folder.clone(), exists, bundles_found: found.len() });
        all.extend(found);
    }
    all.sort();
    all.dedup();
    (reports, all)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, visited: &mut std::collections::HashSet<PathBuf>) {
    // Guard against symlink loops and folders listed twice.
    let Ok(real) = dir.canonicalize() else { return };
    if !visited.insert(real) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_vst3 = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("vst3"));
        if is_vst3 {
            // A bundle is a folder on all three systems (or a single file on old Windows
            // installs). Either way it is one plugin; do not descend into it.
            out.push(path);
        } else if path.is_dir() {
            walk(&path, out, visited);
        }
    }
}

/// Find bundles in `folders` and probe each one in its own child process.
pub fn scan(folders: &[PathBuf], probe: &ProbeCommand, timeout: Duration) -> ScanReport {
    let (folder_reports, bundles) = find_bundles(folders);
    let results = bundles.iter().map(|b| probe_isolated(b, probe, timeout)).collect();
    ScanReport { folders: folder_reports, results }
}

/// Probe one bundle in a child process, with a timeout.
pub fn probe_isolated(bundle: &Path, probe: &ProbeCommand, timeout: Duration) -> ProbeOutcome {
    let path = bundle.to_path_buf();
    let mut child = match Command::new(&probe.exe)
        .args(&probe.args_before_path)
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return ProbeOutcome::Failed { path, detail: format!("could not start probe: {e}") };
        }
    };

    // Drain both pipes on their own threads, so a chatty plugin cannot fill a pipe
    // buffer and leave the child blocked forever.
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeOutcome::Failed { path, detail: format!("waiting on probe: {e}") };
            }
        }
    };

    // Note: if the plugin spawned its own long-lived helper process that inherited the
    // pipes, these joins wait for it. Acceptable for M0; revisit if a plugin does that.
    let out = out_thread.join().unwrap_or_default();
    let err = err_thread.join().unwrap_or_default();

    let Some(status) = status else {
        return ProbeOutcome::TimedOut { path, after_secs: timeout.as_secs() };
    };

    if let Some(line) = out.lines().find_map(|l| l.strip_prefix(PROBE_MARKER)) {
        return match serde_json::from_str::<ProbeOutcome>(line.trim()) {
            Ok(outcome) => outcome,
            Err(e) => {
                ProbeOutcome::Failed { path, detail: format!("unreadable probe output: {e}") }
            }
        };
    }

    ProbeOutcome::Crashed {
        path,
        detail: format!(
            "the probe ended with {} and no result. {}",
            daw_diagnostics::describe_exit(&status),
            crash_summary(&err)
        ),
    }
}

/// The useful part of a dead probe's stderr. The probe installs the crash handler, so a native
/// crash prints a "CRASH E9001" block naming the module; keep that block's first lines. Without
/// one, keep the last few lines of whatever the plugin printed.
fn crash_summary(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let picked: Vec<&str> = match lines.iter().position(|l| l.contains("CRASH E9001")) {
        Some(i) => lines[i..].iter().take(3).copied().collect(),
        None => lines.iter().rev().take(5).rev().copied().collect(),
    };
    if picked.is_empty() { "The plugin printed nothing.".into() } else { format!("Its last words: {}", picked.join(" | ")) }
}

/// Load one bundle in *this* process and summarise it. Only the probe child should call
/// this: a crash in here takes the calling process down, which is the point of the child.
pub fn probe_in_process(bundle: &Path) -> ProbeOutcome {
    let path = bundle.to_path_buf();
    match vst3_host::get_detailed_plugin_info(bundle) {
        Ok(detailed) => {
            let info = detailed.info;
            let classes = detailed
                .classes
                .iter()
                .filter(|c| c.category.contains("Audio Module Class"))
                .map(|c| ClassSummary {
                    name: c.name.clone(),
                    category: c.category.clone(),
                    class_id: c.class_id.clone(),
                    version: c.version.clone(),
                })
                .collect();
            let is_instrument =
                info.category.split('|').any(|s| s.trim().eq_ignore_ascii_case("Instrument"));
            ProbeOutcome::Ok(PluginSummary {
                path,
                name: info.name,
                vendor: info.vendor,
                version: info.version,
                category: info.category,
                class_id: info.uid,
                is_instrument,
                audio_inputs: info.audio_inputs,
                audio_outputs: info.audio_outputs,
                has_midi_input: info.has_midi_input,
                has_editor: info.has_gui,
                classes,
            })
        }
        Err(e) => ProbeOutcome::Failed { path, detail: e.to_string() },
    }
}

/// Probe and print the marked result line. Used by the CLI's hidden probe subcommand.
/// Returns the process exit code.
pub fn run_probe_child(bundle: &Path) -> i32 {
    let outcome = probe_in_process(bundle);
    match serde_json::to_string(&outcome) {
        Ok(json) => {
            println!("{PROBE_MARKER}{json}");
            0
        }
        Err(e) => {
            eprintln!("could not serialise probe result: {e}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bundles_and_ignores_other_files() {
        let root = std::env::temp_dir().join(format!("rust-daw-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Vendor/Synth.vst3/Contents")).unwrap();
        std::fs::create_dir_all(root.join("Other.VST3")).unwrap();
        std::fs::write(root.join("readme.txt"), "x").unwrap();
        // A file inside a bundle must not be reported as a second plugin.
        std::fs::create_dir_all(root.join("Vendor/Synth.vst3/Contents/Inner.vst3")).unwrap();

        let (reports, bundles) = find_bundles(&[root.clone(), root.join("missing")]);
        let names: Vec<String> =
            bundles.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();

        assert_eq!(names, vec!["Other.VST3", "Synth.vst3"]);
        assert!(reports[0].exists);
        assert_eq!(reports[0].bundles_found, 2);
        assert!(!reports[1].exists);
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- Child-process behaviour ----
    //
    // The scanner's crash and timeout handling cannot be tested with real plugins on a
    // machine that has none. Instead the test binary re-runs itself as the probe child,
    // selecting one of the `helper_*` tests below, which pretend to be a plugin that
    // crashes, hangs, or prints noise before its result.

    const HELPER_ENV: &str = "RUST_DAW_PROBE_HELPER";

    fn helper_command(helper: &str) -> ProbeCommand {
        // SAFETY: set before any child is spawned; no other test reads this variable.
        unsafe { std::env::set_var(HELPER_ENV, "1") };
        ProbeCommand {
            exe: std::env::current_exe().unwrap(),
            args_before_path: vec![
                format!("tests::{helper}"),
                "--exact".into(),
                "--ignored".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ],
        }
    }

    fn helper_active() -> bool {
        std::env::var_os(HELPER_ENV).is_some()
    }

    #[test]
    #[ignore = "child-process helper, run by probe tests"]
    fn helper_crash() {
        if helper_active() {
            std::process::abort();
        }
    }

    #[test]
    #[ignore = "child-process helper, run by probe tests"]
    fn helper_hang() {
        if helper_active() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    #[ignore = "child-process helper, run by probe tests"]
    fn helper_noisy_result() {
        if helper_active() {
            println!("some plugin printing to stdout");
            let o = ProbeOutcome::Failed { path: PathBuf::from("noisy.vst3"), detail: "fine".into() };
            println!("{PROBE_MARKER}{}", serde_json::to_string(&o).unwrap());
        }
    }

    #[test]
    fn crashing_plugin_is_reported_not_fatal() {
        let out = probe_isolated(Path::new("fake.vst3"), &helper_command("helper_crash"), Duration::from_secs(20));
        assert!(matches!(out, ProbeOutcome::Crashed { .. }), "got {out:?}");
    }

    #[test]
    fn hanging_plugin_is_killed_at_timeout() {
        let start = Instant::now();
        let out = probe_isolated(Path::new("fake.vst3"), &helper_command("helper_hang"), Duration::from_secs(2));
        assert!(matches!(out, ProbeOutcome::TimedOut { after_secs: 2, .. }), "got {out:?}");
        assert!(start.elapsed() < Duration::from_secs(15), "kill took too long");
    }

    #[test]
    fn result_line_is_found_among_plugin_noise() {
        let out = probe_isolated(Path::new("fake.vst3"), &helper_command("helper_noisy_result"), Duration::from_secs(20));
        match out {
            ProbeOutcome::Failed { path, detail } => {
                assert_eq!(path, PathBuf::from("noisy.vst3"));
                assert_eq!(detail, "fine");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn crash_summary_prefers_the_crash_block() {
        let s = "noise\n12:00:00.000 CRASH E9001 The program crashed | native exception 0xC0000005\n  \
                 crashed in : C:\\x\\Synth.vst3 + 0x10\n  thread     : main (id 1)\n  stack ...\n";
        let out = crash_summary(s);
        assert!(out.contains("CRASH E9001") && out.contains("Synth.vst3") && !out.contains("stack"), "{out}");
        assert_eq!(crash_summary(""), "The plugin printed nothing.");
    }

    #[test]
    fn probe_outcome_round_trips_through_json() {
        let o = ProbeOutcome::TimedOut { path: PathBuf::from("a/b.vst3"), after_secs: 60 };
        let json = serde_json::to_string(&o).unwrap();
        let back: ProbeOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path(), Path::new("a/b.vst3"));
    }
}
