//! Diagnostics: error codes, the session log, crash capture and system facts.
//!
//! Goal: when something goes wrong on a machine no developer can see, the person at that
//! machine sends one log file, and it says what happened, where, on which build and on what
//! system. `crucible report` bundles the recent logs into one file to send.
//!
//! Real-time rule: nothing here may be called from the audio callback. Library log records
//! from threads marked with [`mark_realtime_thread`] are dropped, not written.
//!
//! Portability: this is a seam crate for one thing only, native crash capture, which lives in
//! `platform_windows.rs` and `platform_other.rs`. Everything else is portable.

pub mod codes;
mod session;
mod system;

#[cfg(windows)]
#[path = "platform_windows.rs"]
mod platform;
#[cfg(not(windows))]
#[path = "platform_other.rs"]
mod platform;

pub use codes::{Code, CodeExt, DawError, Severity, find_code};
pub use platform::{describe_exit, exception_name, quiet_command};
pub use session::{
    clock_before, dropped_realtime_records, error, data_dir, event, everything, file_stamp, finish, info, log_dirs, log_path, mark_realtime_thread, note, raw, report_dir,
    set_verbose, source, start, start_child, warn,
};
pub use system::{ProcessWatch, exe_fingerprint, process_memory_mb, system_summary};
