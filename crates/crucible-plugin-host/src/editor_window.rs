//! A plugin's own window, as the program's main window sees it.

// PLATFORM: SEAM 2, Windows today. The window itself is made by the host library (vst3-host's
// `PluginWindow`), as a top-level window that belongs to nobody. Here it is given an owner, the
// program's main window, which on Windows is what makes a window float above another one, stay
// out of the taskbar and minimise with it: how every other audio program shows a plugin.
//   macOS: `NSWindow.addChildWindow(_:ordered:)` on the main window, or a floating panel level.
//   Linux/X11: set `WM_TRANSIENT_FOR` to the main window.
// Until those are written a plugin's window on those systems is an ordinary separate window.

/// Make every plugin window on this thread that has no owner yet float above `owner`, the main
/// window's native handle. Call it on the thread that opened the plugin's window, straight after.
/// Returns how many windows it took.
#[cfg(windows)]
pub fn float_above(owner: isize) -> usize {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GW_OWNER, GWLP_HWNDPARENT, GetClassNameW, GetWindow, SW_HIDE, SW_SHOW, SetWindowLongPtrW, ShowWindow};

    /// The class vst3-host registers for the windows it makes (its `window.rs`).
    const CLASS: &str = "VST3PluginWindow";

    struct Job {
        owner: isize,
        taken: usize,
    }

    unsafe extern "system" fn each(window: HWND, job: LPARAM) -> BOOL {
        // SAFETY: `job` is the `Job` passed to EnumThreadWindows below, alive for the whole call;
        // `window` is a live window of this thread, handed over by the system.
        unsafe {
            let job = &mut *(job as *mut Job);
            let mut name = [0u16; 64];
            let n = GetClassNameW(window, name.as_mut_ptr(), name.len() as i32).max(0) as usize;
            if String::from_utf16_lossy(&name[..n]) == CLASS && GetWindow(window, GW_OWNER).is_null() {
                // An owner given to a window that is already showing takes effect in the taskbar
                // only when the window is shown again.
                ShowWindow(window, SW_HIDE);
                SetWindowLongPtrW(window, GWLP_HWNDPARENT, job.owner);
                ShowWindow(window, SW_SHOW);
                job.taken += 1;
            }
        }
        1
    }

    let mut job = Job { owner, taken: 0 };
    // SAFETY: `each` matches the callback's signature and `job` outlives the call.
    unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(each), &mut job as *mut Job as LPARAM) };
    job.taken
}

#[cfg(not(windows))]
pub fn float_above(_owner: isize) -> usize {
    0
}
