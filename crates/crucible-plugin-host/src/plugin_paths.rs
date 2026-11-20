//! Where VST3 plugins live on each operating system.

// PLATFORM: SEAM 2. This module is one of the few places allowed to branch on the OS.
//   Windows: %CommonProgramFiles%\VST3 (system-wide) and
//            %LOCALAPPDATA%\Programs\Common\VST3 (per-user, VST 3.7+ convention).
//   macOS:   /Library/Audio/Plug-Ins/VST3 and ~/Library/Audio/Plug-Ins/VST3
//   Linux:   ~/.vst3, /usr/lib/vst3, /usr/local/lib/vst3
// Only 64-bit plugins are relevant, so "Program Files (x86)" is ignored on Windows.

use std::path::PathBuf;

/// Standard VST3 folders for this OS, in search order. Folders that do not exist are
/// still returned so the diagnostic log can show where it looked.
pub fn standard_vst3_folders() -> Vec<PathBuf> {
    let mut v = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let common = std::env::var_os("CommonProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
        v.push(common.join("VST3"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join("Programs").join("Common").join("VST3"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        v.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        if let Some(home) = std::env::var_os("HOME") {
            v.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/VST3"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            v.push(PathBuf::from(home).join(".vst3"));
        }
        v.push(PathBuf::from("/usr/lib/vst3"));
        v.push(PathBuf::from("/usr/local/lib/vst3"));
    }

    v
}
