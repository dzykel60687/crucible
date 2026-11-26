//! Records which commit the programs were built from, for log headers and `crucible version`.
//!
//! Portability: needs only `git` on the build machine. Without git, or outside a git checkout
//! (a source zip), the text says so and the build carries on.
//!
//! Limit: this runs again when the checked-out commit changes, not on every edit, so a test
//! build made from uncommitted work can still name the last commit alone. The build
//! fingerprint in the same log header is what tells two builds apart. Release builds are made
//! right after their commit, so for them the commit is exact.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = git(&["rev-parse", "--short=12", "HEAD"]).filter(|s| !s.is_empty());
    let date = git(&["log", "-1", "--format=%cs"]).filter(|s| !s.is_empty());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let text = match (commit, date) {
        (Some(c), Some(d)) => format!("commit {c} of {d}{}", if dirty { ", plus uncommitted changes" } else { "" }),
        _ => "unknown (built without git)".to_string(),
    };
    println!("cargo:rustc-env=CRUCIBLE_SOURCE={text}");
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(dir) = git(&["rev-parse", "--git-dir"]).filter(|s| !s.is_empty()) {
        for f in ["HEAD", "refs/heads", "packed-refs"] {
            println!("cargo:rerun-if-changed={dir}/{f}");
        }
    }
}
