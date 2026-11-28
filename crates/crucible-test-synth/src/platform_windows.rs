// PLATFORM: Windows. The test plugin's editor: one label in a child window of the host's window.
// macOS would add an NSTextField to the host's NSView; Linux would map an X11 child window
// (VST3's X11EmbedWindowID). Until then the plugin has no editor on those systems.

use std::cell::Cell;
use std::ffi::{CStr, c_void};
use vst3::{Class, ComWrapper, Steinberg::*};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_CHILD, WS_VISIBLE};

pub const SIZE: (i32, i32) = (360, 120);

struct View {
    window: Cell<HWND>,
}

impl Class for View {
    type Interfaces = (IPlugView,);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, kind: FIDString) -> tresult {
        if !kind.is_null() && CStr::from_ptr(kind) == c"HWND" { kResultTrue } else { kResultFalse }
    }
    unsafe fn attached(&self, parent: *mut c_void, kind: FIDString) -> tresult {
        if self.isPlatformTypeSupported(kind) != kResultTrue || parent.is_null() {
            return kResultFalse;
        }
        let w = CreateWindowExW(0, wide("STATIC").as_ptr(), wide("Crucible Test Tone: the editor is open").as_ptr(), WS_CHILD | WS_VISIBLE, 0, 0, SIZE.0, SIZE.1, parent, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
        self.window.set(w);
        if w.is_null() { kResultFalse } else { kResultOk }
    }
    unsafe fn removed(&self) -> tresult {
        let w = self.window.replace(std::ptr::null_mut());
        if !w.is_null() {
            DestroyWindow(w);
        }
        kResultOk
    }
    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }
    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        *size = ViewRect { left: 0, top: 0, right: SIZE.0, bottom: SIZE.1 };
        kResultOk
    }
    unsafe fn onSize(&self, _size: *mut ViewRect) -> tresult {
        kResultOk
    }
    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, _frame: *mut IPlugFrame) -> tresult {
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultFalse
    }
    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        self.getSize(rect)
    }
}

pub fn view() -> *mut IPlugView {
    ComWrapper::new(View { window: Cell::new(std::ptr::null_mut()) }).to_com_ptr::<IPlugView>().map_or(std::ptr::null_mut(), |p| p.into_raw())
}
