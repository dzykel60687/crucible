// PLATFORM: Windows. Native crash capture and exit-status decoding.
//   How: SetUnhandledExceptionFilter catches an exception nothing else handled (a plugin's
//   access violation, an uncaught C++ exception), names the module it happened in, writes a
//   minidump with MiniDumpWriteDump (dbghelp.dll, part of Windows) and lets the process end.
//   macOS equivalent: sigaction for SIGSEGV, SIGBUS, SIGILL, SIGFPE and SIGABRT, or a Mach
//     exception port; dladdr names the faulting image; dumps need an out-of-process helper.
//   Linux equivalent: sigaction for the same signals and dladdr; minidumps via a
//     breakpad-compatible writer if ever needed.
//   Ledger row: docs/03-platform-notes.md, "Crash capture".
//
// Limits: a plugin may install its own filter and replace this one. A "fast fail"
// (exception 0xC0000409, which Rust's abort uses) skips filters entirely, so Rust-side failures
// are caught by the panic hook in session.rs instead.

use std::fs::File;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{HANDLE, HMODULE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MiniDumpWithIndirectlyReferencedMemory, MiniDumpWithThreadInfo,
    MiniDumpWithUnloadedModules, MiniDumpWriteDump, SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId};

struct Target {
    /// The session log (a second handle to it, so the handler never waits on a lock).
    log: Option<File>,
    dump: Option<PathBuf>,
}

static TARGET: OnceLock<Target> = OnceLock::new();
static IN_HANDLER: AtomicBool = AtomicBool::new(false);

/// Install the handler. `log`: where to describe the crash (also always stderr).
/// `dump`: where to write a minidump. Only the first call takes effect.
pub fn install_crash_handler(log: Option<File>, dump: Option<PathBuf>) {
    if TARGET.set(Target { log, dump }).is_ok() {
        // SAFETY: registering a valid `extern "system"` function as the top-level filter.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }
}

const EXCEPTION_EXECUTE_HANDLER: i32 = 1;

unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
    // A crash inside this handler must not loop.
    if IN_HANDLER.swap(true, Ordering::SeqCst) {
        return EXCEPTION_EXECUTE_HANDLER;
    }
    let Some(t) = TARGET.get() else { return EXCEPTION_EXECUTE_HANDLER };
    if info.is_null() {
        emit(t, "CRASH E9001 native exception with no details");
        return EXCEPTION_EXECUTE_HANDLER;
    }
    // SAFETY: Windows passes valid exception pointers to a top-level filter; checked non-null.
    let rec = unsafe { &*(*info).ExceptionRecord };
    let code = rec.ExceptionCode as u32;
    let addr = rec.ExceptionAddress as usize;

    let mut msg = format!(
        "CRASH E9001 The program crashed | native exception {code:#010X}: {}",
        exception_name(code).unwrap_or("unknown exception")
    );
    if code == 0xC000_0005 && rec.NumberParameters >= 2 {
        let op = match rec.ExceptionInformation[0] {
            0 => "reading",
            1 => "writing",
            8 => "executing",
            _ => "accessing",
        };
        msg.push_str(&format!(" ({op} address {:#x})", rec.ExceptionInformation[1]));
    }
    msg.push_str(&format!("\n  crashed in : {}", module_at(addr)));
    let thread = std::thread::current();
    // SAFETY: plain Win32 query with no arguments.
    let tid = unsafe { GetCurrentThreadId() };
    msg.push_str(&format!("\n  thread     : {} (id {tid})", thread.name().unwrap_or("unnamed")));
    emit(t, &msg);

    if let Some(dump) = &t.dump {
        match write_dump(dump, info) {
            Ok(()) => emit(t, &format!("  dump file  : {}", dump.display())),
            Err(e) => emit(t, &format!("  dump file  : could not be written: {e}")),
        }
    }
    emit(t, &format!("  help       : {}", crate::Code::NativeCrash.help()));
    // Best effort: symbols exist for Crucible's own code, plugins show as module + offset.
    let trace = std::backtrace::Backtrace::force_capture();
    emit(t, &format!("  stack (newest first, best effort):\n{trace}"));
    EXCEPTION_EXECUTE_HANDLER
}

fn emit(t: &Target, text: &str) {
    let line = format!("{} {text}\n", chrono::Local::now().format("%H:%M:%S%.3f"));
    if let Some(f) = &t.log {
        let mut f: &File = f;
        let _ = f.write_all(line.as_bytes());
    }
    let _ = std::io::stderr().write_all(line.as_bytes());
}

/// "C:\path\Plugin.vst3 + 0x1a2b3c" for an address inside a loaded module.
fn module_at(addr: usize) -> String {
    let mut module: HMODULE = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS treats the name argument as an address to look up; UNCHANGED_REFCOUNT
    // means nothing needs releasing.
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            addr as *const u16,
            &mut module,
        )
    };
    if ok == 0 || module.is_null() {
        return format!("{addr:#x} (not inside any loaded module)");
    }
    let mut buf = [0u16; 1024];
    // SAFETY: buffer and its length are correct.
    let n = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let name = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
    format!("{name} + {:#x}", addr.wrapping_sub(module as usize))
}

fn write_dump(path: &Path, info: *const EXCEPTION_POINTERS) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mei = MINIDUMP_EXCEPTION_INFORMATION {
        // SAFETY: plain Win32 query.
        ThreadId: unsafe { GetCurrentThreadId() },
        ExceptionPointers: info as *mut _,
        ClientPointers: 0,
    };
    let kind = MiniDumpWithThreadInfo | MiniDumpWithUnloadedModules | MiniDumpWithIndirectlyReferencedMemory;
    // SAFETY: valid process handle and id, an open writable file handle, and exception
    // information that lives for the duration of the call.
    let ok = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.as_raw_handle() as HANDLE,
            kind,
            &mei,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 { Err(std::io::Error::last_os_error().to_string()) } else { Ok(()) }
}

/// Plain-English names for the Windows exception and exit codes that matter here.
pub fn exception_name(code: u32) -> Option<&'static str> {
    Some(match code {
        0xC000_0005 => "access violation (bad memory access)",
        0xC000_0006 => "in-page error (a file on disk or the network could not be read)",
        0xC000_0017 => "out of memory",
        0xC000_001D => "illegal instruction (code built for a newer CPU?)",
        0xC000_008E => "floating-point divide by zero",
        0xC000_0090 => "invalid floating-point operation",
        0xC000_0094 => "integer divide by zero",
        0xC000_0095 => "integer overflow",
        0xC000_0096 => "privileged instruction",
        0xC000_00FD => "stack overflow",
        0xC000_0135 => "a required DLL was not found",
        0xC000_0139 => "a required function was missing from a DLL",
        0xC000_0142 => "a DLL failed to initialise",
        0xC000_013A => "stopped by Ctrl+C or the console window closing",
        0xC000_0374 => "heap corruption",
        0xC000_0409 => "deliberate fast exit (Rust abort, or a security check found memory corruption)",
        0xE06D_7363 => "unhandled C++ exception",
        0x8000_0003 => "breakpoint (a debug check in the code)",
        0x4001_0004 => "terminated by a debugger or another program",
        _ => return None,
    })
}

/// Describe how a child process ended, decoding Windows exception codes.
pub fn describe_exit(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(c) => {
            let u = c as u32;
            match exception_name(u) {
                Some(n) => format!("exit code {u:#010X}: {n}"),
                None if u >= 0xC000_0000 => format!("exit code {u:#010X} (a Windows error status)"),
                None => format!("exit code {c}"),
            }
        }
        None => "no exit code".into(),
    }
}

#[cfg(test)]
mod tests {
    //! The crash path is tested by re-running this test binary as a child that installs the
    //! handler and then raises an access violation.

    use super::*;
    use windows_sys::Win32::System::Diagnostics::Debug::RaiseException;

    const HELPER_ENV: &str = "RUST_DAW_CRASH_HELPER";

    #[test]
    #[ignore = "child-process helper, run by crash_is_logged_and_dumped"]
    fn helper_crash() {
        let Some(dir) = std::env::var_os(HELPER_ENV) else { return };
        let dir = PathBuf::from(dir);
        let log = File::create(dir.join("crash.txt")).unwrap();
        install_crash_handler(Some(log), Some(dir.join("crash.dmp")));
        let args = [1usize, 0x10];
        // SAFETY: raising a non-continuable exception on purpose; the filter ends the process.
        unsafe { RaiseException(0xC000_0005, 1, 2, args.as_ptr()) };
    }

    #[test]
    fn crash_is_logged_and_dumped() {
        let dir = std::env::temp_dir().join(format!("crucible-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["platform::tests::helper_crash", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env(HELPER_ENV, &dir)
            .output()
            .unwrap();

        assert_eq!(out.status.code().map(|c| c as u32), Some(0xC000_0005), "{out:?}");
        assert!(describe_exit(&out.status).contains("access violation"));
        let log = std::fs::read_to_string(dir.join("crash.txt")).unwrap();
        assert!(log.contains("CRASH E9001"), "{log}");
        assert!(log.contains("writing address 0x10"), "{log}");
        assert!(log.contains("crashed in : ") && log.to_lowercase().contains(".dll"), "{log}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("CRASH E9001"));
        let dump = std::fs::metadata(dir.join("crash.dmp")).unwrap();
        assert!(dump.len() > 1000, "dump is {} bytes", dump.len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
