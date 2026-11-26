// PLATFORM: macOS and Linux placeholder. The Windows version is platform_windows.rs.
//   Still to do when the ports start: a native crash handler via sigaction for SIGSEGV, SIGBUS,
//   SIGILL, SIGFPE and SIGABRT, naming the faulting image with dladdr. Until then a plugin
//   crash on these systems ends the process with only the panic hook and the shell's report.
//   Ledger row: docs/03-platform-notes.md, "Crash capture".

use std::fs::File;
use std::path::PathBuf;

pub fn install_crash_handler(_log: Option<File>, _dump: Option<PathBuf>) {}

/// A command for a helper program that must not show a window of its own. Nothing special is
/// needed here: only Windows opens a console window for a console program started by a GUI one.
pub fn quiet_command(exe: &std::path::Path) -> std::process::Command {
    std::process::Command::new(exe)
}

pub fn exception_name(_code: u32) -> Option<&'static str> {
    None
}

/// Describe how a child process ended.
pub fn describe_exit(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            let name = match sig {
                4 => " (illegal instruction)",
                6 => " (abort)",
                7 | 10 => " (bus error)",
                8 => " (floating-point error)",
                9 => " (killed)",
                11 => " (segmentation fault: bad memory access)",
                _ => "",
            };
            return format!("killed by signal {sig}{name}");
        }
    }
    match status.code() {
        Some(c) => format!("exit code {c}"),
        None => "no exit code".into(),
    }
}
