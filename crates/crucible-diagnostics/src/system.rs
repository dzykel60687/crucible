//! Facts about the machine and the build, written at the top of every log.
//!
//! Portable: the `sysinfo` crate covers Windows, macOS and Linux, so this file has no
//! OS-specific code.

use std::path::Path;
use sysinfo::{CpuRefreshKind, Pid, ProcessesToUpdate, Product, System};

/// Labelled lines describing this computer. Every lookup is best effort: a missing fact
/// shows as "?" rather than failing the program.
pub fn system_summary() -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let q = |r: Result<String, sysinfo::Error>| r.ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "?".into());

    let model = match (Product::vendor_name(), Product::name()) {
        (Ok(v), Ok(n)) => format!("{} {}", v.trim(), n.trim()),
        (_, Ok(n)) => n.trim().to_string(),
        _ => "?".into(),
    };
    out.push(("computer", format!("{} | {model}", q(System::host_name()))));
    out.push((
        "os",
        format!("{} | kernel {} | {}", q(System::long_os_version()), q(System::kernel_version()), System::cpu_arch()),
    ));

    match System::new() {
        Ok(mut sys) => {
            sys.refresh_cpu_list(CpuRefreshKind::nothing());
            sys.refresh_memory();
            let brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_else(|| "?".into());
            let physical = System::physical_core_count().map(|n| n.to_string()).unwrap_or_else(|_| "?".into());
            out.push(("cpu", format!("{brand} | {} logical / {physical} physical cores", sys.cpus().len())));
            out.push((
                "memory",
                format!("{:.1} GB total, {:.1} GB available", gb(sys.total_memory()), gb(sys.available_memory())),
            ));
        }
        Err(e) => out.push(("cpu/memory", format!("unavailable: {e:?}"))),
    }
    out
}

/// This process's resident memory in MB, for periodic heartbeat lines (leak spotting).
pub fn process_memory_mb() -> Option<f64> {
    let pid = Pid::from_u32(std::process::id());
    let mut sys = System::new().ok()?;
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
    sys.process(pid).map(|p| p.memory() as f64 / 1_048_576.0)
}

/// This process's memory and processor use, sampled again and again (the once-a-second line in
/// the log). Processor use is since the previous sample, as a share of one core: 100% is one
/// core fully busy, so several busy threads read above 100%.
pub struct ProcessWatch {
    sys: System,
    pid: Pid,
}

impl ProcessWatch {
    pub fn start() -> Option<ProcessWatch> {
        let mut w = ProcessWatch { sys: System::new().ok()?, pid: Pid::from_u32(std::process::id()) };
        w.sample();
        Some(w)
    }

    /// (memory in MB, processor use in percent of one core).
    pub fn sample(&mut self) -> (f64, f64) {
        self.sys.refresh_processes(ProcessesToUpdate::Some(&[self.pid]), false);
        self.sys.process(self.pid).map_or((0.0, 0.0), |p| (p.memory() as f64 / 1_048_576.0, f64::from(p.cpu_usage())))
    }
}

/// A short fingerprint of an executable file: FNV-1a 64 of its bytes.
///
/// The version number does not change between test builds, so this is how a log is matched
/// to the exact build that wrote it. `crucible version` prints the same value.
pub fn exe_fingerprint(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("{h:016x}"))
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn summary_has_the_basics() {
        let s = super::system_summary();
        let labels: Vec<&str> = s.iter().map(|(l, _)| *l).collect();
        assert!(labels.contains(&"computer") && labels.contains(&"os"), "{s:?}");
    }

    #[test]
    fn fingerprint_is_stable() {
        let exe = std::env::current_exe().unwrap();
        assert_eq!(super::exe_fingerprint(&exe), super::exe_fingerprint(&exe));
        assert!(super::process_memory_mb().is_some_and(|m| m > 0.0));
    }
}
