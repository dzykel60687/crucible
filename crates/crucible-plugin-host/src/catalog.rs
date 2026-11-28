//! The list of plugins installed on this machine, kept between runs.
//!
//! Finding out what a plugin is means loading it, which for a big sampled instrument takes
//! seconds, so the answer is kept in a file (`plugins.json` in the per-user data folder) and a
//! plugin is asked again only when its file on disk has changed. Each plugin is asked in a child
//! process (`probe_isolated`), so one that crashes or hangs costs only its own row.
//!
//! A project names a plugin by its class id, never by a path (`02-architecture.md`); this list
//! is how each machine turns the id into the file to load.
//!
//! Portability: no OS code. Which folders to look in comes from `plugin_paths` (seam 2).

use crate::{ProbeCommand, ProbeOutcome, find_bundles, probe_isolated};
use crucible_diagnostics::{self as diag, Code, DawError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

/// Folders to look in instead of the system's own, separated like `PATH`. For tests, and for a
/// machine whose plugins live somewhere unusual.
pub const FOLDERS_ENV: &str = "CRUCIBLE_VST3_PATH";
/// Where to keep the list instead of the per-user data folder. For tests.
pub const FILE_ENV: &str = "CRUCIBLE_PLUGIN_CATALOG";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub plugins: Vec<Item>,
    /// Bundles that could not be asked, and why.
    #[serde(default)]
    pub failed: Vec<Failed>,
    /// Plugin files, and folders of them, outside the usual folders that the person pointed at.
    #[serde(default)]
    pub extra: Vec<PathBuf>,
    /// Folders the last scan looked in, and whether each exists.
    #[serde(default)]
    pub folders: Vec<(PathBuf, bool)>,
    /// When the last scan ran, in seconds since 1970. `None`: never.
    #[serde(default)]
    pub scanned: Option<u64>,
}

/// One plugin. A bundle holding several plugins gives several items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// VST3 sub-categories, e.g. "Instrument|Synth". Empty for the second and later plugins of
    /// a bundle: the probe only describes the first in full.
    pub category: String,
    pub class_id: String,
    pub is_instrument: bool,
    pub has_editor: bool,
    pub path: PathBuf,
    /// The bundle's file as it was when asked: size and modified time.
    pub stamp: (u64, u64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failed {
    pub path: PathBuf,
    pub why: String,
    pub stamp: (u64, u64),
}

/// What a scan did, for its reply.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Scanned {
    pub bundles: usize,
    pub asked: usize,
    pub kept: usize,
    pub failed: usize,
    pub seconds: f64,
}

pub fn file() -> Option<PathBuf> {
    std::env::var_os(FILE_ENV).map(PathBuf::from).or_else(|| Some(diag::data_dir()?.join("plugins.json")))
}

/// The list as last saved. Empty if there is none yet or it cannot be read.
pub fn load() -> Catalog {
    file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(c: &Catalog) -> Result<(), DawError> {
    let file = file().ok_or_else(|| DawError::new(Code::FileWrite, "no per-user folder to keep the plugin list in"))?;
    let fail = |e: std::io::Error| DawError::new(Code::FileWrite, format!("{}: {e}", file.display()));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = file.with_extension("json.part");
    std::fs::write(&tmp, serde_json::to_string_pretty(c).unwrap_or_default()).map_err(fail)?;
    std::fs::rename(&tmp, &file).map_err(fail)
}

/// The folders a scan looks in.
pub fn folders() -> Vec<PathBuf> {
    match std::env::var_os(FOLDERS_ENV) {
        Some(v) => std::env::split_paths(&v).collect(),
        None => crate::plugin_paths::standard_vst3_folders(),
    }
}

/// Size and modified time of the plugin's binary: of the bundle's newest file, two levels in
/// (`Contents/<system>/<binary>`), or of the bundle itself when it is a single file.
fn stamp(bundle: &Path) -> (u64, u64) {
    fn of(p: &Path) -> (u64, u64) {
        p.metadata().map_or((0, 0), |m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())))
    }
    fn walk(dir: &Path, depth: u8, best: &mut (u64, u64)) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth > 0 {
                    walk(&p, depth - 1, best);
                }
            } else {
                let s = of(&p);
                if s.1 > best.1 || (s.1 == best.1 && s.0 > best.0) {
                    *best = s;
                }
            }
        }
    }
    if bundle.is_file() {
        return of(bundle);
    }
    let mut best = (0, 0);
    walk(bundle, 3, &mut best);
    best
}

fn items(s: &crate::PluginSummary, stamp: (u64, u64)) -> Vec<Item> {
    let first = Item { name: s.name.clone(), vendor: s.vendor.clone(), version: s.version.clone(), category: s.category.clone(), class_id: s.class_id.clone(), is_instrument: s.is_instrument, has_editor: s.has_editor, path: s.path.clone(), stamp };
    let mut out = vec![first.clone()];
    for c in s.classes.iter().filter(|c| !c.class_id.eq_ignore_ascii_case(&s.class_id)) {
        out.push(Item { name: c.name.clone(), version: c.version.clone(), category: String::new(), class_id: c.class_id.clone(), is_instrument: false, ..first.clone() });
    }
    out
}

/// Look for plugins and ask each new or changed one what it is. `all` asks every one again.
/// `progress(done, total, bundle)` is called before each bundle is asked.
pub fn scan(probe: &ProbeCommand, timeout: Duration, all: bool, progress: &mut dyn FnMut(usize, usize, &Path)) -> Result<(Catalog, Scanned), DawError> {
    let t0 = Instant::now();
    let old = load();
    let is_bundle = |p: &PathBuf| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vst3"));
    let mut look_in = folders();
    look_in.extend(old.extra.iter().filter(|p| !is_bundle(p)).cloned());
    let (reports, mut bundles) = find_bundles(&look_in);
    bundles.extend(old.extra.iter().filter(|p| is_bundle(p) && p.exists()).cloned());
    bundles.sort();
    bundles.dedup();
    let mut new = Catalog { extra: old.extra.clone(), folders: reports.iter().map(|r| (r.folder.clone(), r.exists)).collect(), ..Catalog::default() };
    let mut done = Scanned { bundles: bundles.len(), ..Scanned::default() };
    for (i, b) in bundles.iter().enumerate() {
        let st = stamp(b);
        let known: Vec<Item> = old.plugins.iter().filter(|p| p.path == *b && p.stamp == st).cloned().collect();
        let known_bad = old.failed.iter().find(|f| f.path == *b && f.stamp == st);
        if !all && !known.is_empty() {
            done.kept += known.len();
            new.plugins.extend(known);
            continue;
        }
        if !all && let Some(f) = known_bad {
            new.failed.push(f.clone());
            done.failed += 1;
            continue;
        }
        progress(i, bundles.len(), b);
        done.asked += 1;
        let t = Instant::now();
        match probe_isolated(b, probe, timeout) {
            ProbeOutcome::Ok(s) => {
                diag::info(format!("plugin scan: {} is {} by {} ({}), asked in {:.1} s", b.display(), s.name, s.vendor, s.category, t.elapsed().as_secs_f64()));
                new.plugins.extend(items(&s, st));
            }
            other => {
                let (code, why) = match other {
                    ProbeOutcome::Failed { detail, .. } => (Code::PluginLoadFailed, detail),
                    ProbeOutcome::Crashed { detail, .. } => (Code::PluginCrashed, detail),
                    ProbeOutcome::TimedOut { after_secs, .. } => (Code::PluginTimedOut, format!("no answer after {after_secs} s")),
                    ProbeOutcome::Ok(_) => unreachable!(),
                };
                diag::warn(code, format!("plugin scan: {}: {why}", b.display()));
                new.failed.push(Failed { path: b.clone(), why: format!("{} {why}", code.id()), stamp: st });
                done.failed += 1;
            }
        }
    }
    new.plugins.sort_by(|a, b| (a.name.to_lowercase(), &a.vendor).cmp(&(b.name.to_lowercase(), &b.vendor)));
    new.scanned = Some(std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    save(&new)?;
    done.seconds = t0.elapsed().as_secs_f64();
    Ok((new, done))
}

/// Remember a folder of plugins (or one plugin's file) outside the usual folders, for every
/// scan from now on. The caller scans afterwards.
pub fn add_place(place: &Path) -> Result<(), DawError> {
    if !place.exists() {
        return Err(DawError::new(Code::FileRead, format!("{} does not exist", place.display())));
    }
    let mut c = load();
    if !c.extra.iter().any(|e| e == place) && !folders().iter().any(|f| f == place) {
        c.extra.push(place.to_path_buf());
        save(&c)?;
    }
    Ok(())
}

/// A bundle the person pointed at: ask it, add it to the list, and remember where it is.
pub fn add(bundle: &Path, probe: &ProbeCommand, timeout: Duration) -> Result<Vec<Item>, DawError> {
    add_probed(bundle, probe_isolated(bundle, probe, timeout))
}

/// `add`, for a bundle that has been asked already.
pub fn add_probed(bundle: &Path, outcome: ProbeOutcome) -> Result<Vec<Item>, DawError> {
    let found = match outcome {
        ProbeOutcome::Ok(s) => items(&s, stamp(bundle)),
        ProbeOutcome::Failed { detail, .. } => return Err(DawError::new(Code::PluginLoadFailed, format!("{}: {detail}", bundle.display()))),
        ProbeOutcome::Crashed { detail, .. } => return Err(DawError::new(Code::PluginCrashed, format!("{}: {detail}", bundle.display()))),
        ProbeOutcome::TimedOut { after_secs, .. } => return Err(DawError::new(Code::PluginTimedOut, format!("{}: no answer after {after_secs} s", bundle.display()))),
    };
    let mut c = load();
    c.plugins.retain(|p| p.path != bundle);
    c.failed.retain(|f| f.path != bundle);
    c.plugins.extend(found.iter().cloned());
    c.plugins.sort_by(|a, b| (a.name.to_lowercase(), &a.vendor).cmp(&(b.name.to_lowercase(), &b.vendor)));
    if !folders().iter().any(|f| bundle.starts_with(f)) && !c.extra.iter().any(|e| e == bundle) {
        c.extra.push(bundle.to_path_buf());
    }
    save(&c)?;
    Ok(found)
}

impl Catalog {
    /// The plugin a person or a project means: by class id, else by exact name, else by the one
    /// name that contains the text.
    pub fn find(&self, what: &str) -> Result<&Item, DawError> {
        if let Some(p) = self.plugins.iter().find(|p| p.class_id.eq_ignore_ascii_case(what)) {
            return Ok(p);
        }
        let lower = what.to_lowercase();
        let exact: Vec<&Item> = self.plugins.iter().filter(|p| p.name.to_lowercase() == lower).collect();
        let hits = if exact.is_empty() { self.plugins.iter().filter(|p| p.name.to_lowercase().contains(&lower)).collect() } else { exact };
        match hits.as_slice() {
            [one] => Ok(one),
            [] => Err(DawError::new(Code::PluginNotFound, format!("'{what}' is not among the {} plugins found on this machine{}", self.plugins.len(), if self.scanned.is_none() { " (it has not been scanned yet: plugin.scan)" } else { "" }))),
            many => Err(DawError::new(Code::PluginAmbiguous, format!("'{what}' matches: {}", many.iter().take(8).map(|p| format!("{} by {}", p.name, p.vendor)).collect::<Vec<_>>().join("; ")))),
        }
    }
}
