//! Main-thread event pump for plugin editor windows.

// PLATFORM: Windows: drain the thread's Win32 message queue (PeekMessage / Translate /
//           Dispatch). Plugin editors are child windows that only repaint and react to
//           input while someone pumps messages on the thread that created them.
//   macOS:  the AppKit run loop must be driven instead (NSApp / NSRunLoop).
//   Linux:  vst3-host polls the X11 connection inside service_platform_events, and the
//           plugin's own run-loop requests need servicing.
// In the real app Tauri owns the main event loop, so this module disappears; see
// docs/02-architecture.md, "Known hard problems".

#[cfg(target_os = "windows")]
pub fn pump_messages() {
    use winapi::um::winuser::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
    // SAFETY: plain Win32 calls on the current thread with a zeroed, stack-owned MSG.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        // Bounded so a flood of messages cannot starve the rest of the loop.
        for _ in 0..256 {
            if PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn pump_messages() {
    // Not implemented: this spike is Windows-only. See the PLATFORM note above.
}
