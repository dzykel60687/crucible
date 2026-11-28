//! Where a project that was never saved keeps its audio (D-042).
//!
//! Most programs let you work first and ask where to save later. Crucible copies every imported
//! file into the project's own `audio` folder (D-039), so a project needs a folder from its first
//! import. Until the first save it gets one of its own, `<per-user folder>\unsaved\<process>-<n>`,
//! made on that first import. Saving copies its audio into the folder chosen; this one is then
//! removed, as it is when the project is replaced or discarded and when the program closes.
//!
//! A lock file is held open while the folder is in use. A later start can then tell a folder
//! left by a session that crashed (removed, the next time one is made) from one that belongs to
//! another Crucible still running (kept). Everything in it is a copy of a file that still exists
//! where it was imported from, so removing a crashed session's folder loses nothing.
//!
//! Portability: std only (file locks have been in std since Rust 1.89).

use crucible_diagnostics::{self as diag, Code, DawError};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const LOCK: &str = "in-use.lock";

pub(crate) struct Scratch {
    pub(crate) dir: PathBuf,
    lock: Option<File>,
}

impl Scratch {
    pub(crate) fn new() -> Result<Scratch, DawError> {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let root = diag::data_dir()
            .ok_or_else(|| DawError::new(Code::ProjectSaveFailed, "this system has no per-user folder to hold an unsaved project's audio"))?
            .join("unsaved");
        sweep(&root);
        let dir = root.join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let fail = |e: String| DawError::new(Code::ProjectSaveFailed, format!("the unsaved project's folder {}: {e}", dir.display()));
        std::fs::create_dir_all(dir.join(crate::audio::AUDIO_DIR)).map_err(|e| fail(e.to_string()))?;
        let lock = File::create(dir.join(LOCK)).map_err(|e| fail(e.to_string()))?;
        lock.try_lock().map_err(|e| fail(e.to_string()))?;
        diag::info(format!("the project is not saved yet: its audio is kept in {} until it is", dir.display()));
        Ok(Scratch { dir, lock: Some(lock) })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Windows removes no folder while a file in it is open.
        drop(self.lock.take());
        match std::fs::remove_dir_all(&self.dir) {
            Ok(()) => diag::info(format!("removed the unsaved project's folder {}", self.dir.display())),
            Err(e) => diag::info(format!("the unsaved project's folder {} could not be removed yet ({e}); it is removed the next time one is needed", self.dir.display())),
        }
    }
}

/// Remove folders left by sessions that ended without closing: those whose lock nobody holds.
fn sweep(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let free = match OpenOptions::new().write(true).open(dir.join(LOCK)) {
            // The handle closes at the end of this arm, before the folder is removed.
            Ok(f) => f.try_lock().is_ok(),
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
        if free && std::fs::remove_dir_all(&dir).is_ok() {
            diag::info(format!("removed {}, an unsaved project's folder left by a session that did not close", dir.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_folder_is_kept_and_a_left_one_is_removed() {
        let held = Scratch::new().unwrap();
        // A folder as a crashed session leaves it: lock file present, nobody holding it.
        let left = held.dir.with_file_name(format!("{}-left-{}", std::process::id(), line!()));
        std::fs::create_dir_all(left.join("audio")).unwrap();
        File::create(left.join(LOCK)).unwrap();
        let other = Scratch::new().unwrap();
        assert!(held.dir.is_dir(), "a folder still in use is kept");
        assert!(!left.exists(), "a folder nobody holds is removed");
        let (a, b) = (held.dir.clone(), other.dir.clone());
        drop(held);
        drop(other);
        assert!(!a.exists() && !b.exists(), "dropping removes the folder");
    }
}
