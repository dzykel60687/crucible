//! The machine's audio devices as the window shows them: what is plugged in, the rate each runs
//! at, which other programs have sound on it, and what changed since the last look.
//!
//! Portability: no OS code. Devices come from `crucible-audio-io` (seam 1).

use crate::command::{Args, Kind, Reply, Spec, opt};
use crate::{Effect, Engine, play};
use crucible_audio_io as io;
use crucible_diagnostics::{self as diag, DawError};
use serde_json::json;

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "audio.devices",
        args: &[opt("probe", Kind::Bool, "also ask each device which rates it accepts in exclusive mode, and list ASIO drivers (a second or two)")],
        help: "List audio outputs and inputs (the default, the rate each runs at, who else is using it) and MIDI inputs.",
        effect: Effect::Read,
        run: audio_devices,
    },
    Spec {
        name: "audio.changes",
        args: &[],
        help: "Used by the window: a number that moves when an audio or MIDI device is plugged in, unplugged or changed.",
        effect: Effect::Read,
        run: audio_changes,
    },
];

/// What the engine remembers about the devices between two looks.
pub(crate) struct Devices {
    watch: io::DeviceWatch,
    /// The list at the last look, or `None` before the first.
    last: Option<Vec<io::Endpoint>>,
    /// ASIO driver names from the last `--probe`. Asking again loads the drivers, which must not
    /// happen while one of them is playing.
    asio: Vec<String>,
    /// MIDI inputs at the last look, and how many times that list has changed. Windows has no
    /// notice for MIDI devices that this program receives, so the list is compared each time.
    midi: Option<Vec<String>>,
    midi_changes: u64,
}

/// MIDI input names, and an error if the MIDI system could not be asked.
fn midi_inputs() -> (Vec<String>, Option<String>) {
    let r = crucible_midi_io::enumerate_inputs();
    (r.inputs.into_iter().map(|p| p.name).collect(), r.error)
}

/// Compare the MIDI inputs with the last look: what was connected or disconnected.
fn midi_diff(d: &mut Devices, now: &[String]) -> Vec<String> {
    let Some(old) = d.midi.replace(now.to_vec()) else { return Vec::new() };
    let mut out: Vec<String> = now.iter().filter(|n| !old.contains(n)).map(|n| format!("Connected: {n} (MIDI input)")).collect();
    out.extend(old.iter().filter(|o| !now.contains(o)).map(|o| format!("Disconnected: {o} (MIDI input)")));
    if !out.is_empty() {
        d.midi_changes += 1;
    }
    out
}

fn devices(e: &mut Engine) -> &mut Devices {
    e.devices.get_or_insert_with(|| Devices { watch: io::DeviceWatch::start(), last: None, asio: Vec::new(), midi: None, midi_changes: 0 })
}

fn describe(d: &io::Endpoint) -> String {
    let mut s = d.name.clone();
    if let Some(rate) = d.rate {
        s.push_str(&format!(", {rate} Hz"));
        if let (Some(bits), Some(ch)) = (d.bits, d.channels) {
            s.push_str(&format!(" {bits}-bit {ch} ch"));
        }
    }
    if d.default {
        s.push_str(" [default]");
    }
    if !d.used_by.is_empty() {
        s.push_str(&format!(" | in use by {}", d.used_by.join(", ")));
    }
    s
}

/// What is different, in words for the person and the log.
fn diff(old: &[io::Endpoint], new: &[io::Endpoint]) -> Vec<String> {
    let kind = |d: &io::Endpoint| if d.input { "input" } else { "output" };
    let same = |a: &io::Endpoint, b: &io::Endpoint| a.name == b.name && a.input == b.input;
    let mut out = Vec::new();
    for d in new.iter().filter(|d| !old.iter().any(|o| same(o, d))) {
        out.push(format!("Connected: {} ({})", d.name, kind(d)));
    }
    for d in old.iter().filter(|d| !new.iter().any(|n| same(n, d))) {
        out.push(format!("Disconnected: {} ({})", d.name, kind(d)));
    }
    for d in new {
        let Some(o) = old.iter().find(|o| same(o, d)) else { continue };
        if d.rate != o.rate
            && let (Some(now), Some(was)) = (d.rate, o.rate)
        {
            out.push(format!("{} now runs at {now} Hz (was {was} Hz)", d.name));
        }
        if d.default && !o.default {
            out.push(format!("The default {} is now {}", kind(d), d.name));
        }
    }
    out
}

fn audio_devices(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let probe = a.flag("probe") == Some(true);
    let now = io::endpoints()?;
    let playing = e.player.is_some();
    let rate = e.project.sample_rate;
    let choice = play::choice_json(e);
    let (midi, midi_error) = midi_inputs();
    let d = devices(e);
    let mut changes = d.last.as_deref().map(|old| diff(old, &now)).unwrap_or_default();
    changes.extend(midi_diff(d, &midi));
    // The list itself goes into the log with this command's answer; what changed gets its own line.
    for c in &changes {
        diag::event("AUDIO", format!("device change: {c}"));
    }
    d.last = Some(now.clone());
    let exclusive = if probe { io::exclusive_capabilities().ok() } else { None };
    if probe && !playing {
        d.asio = io::asio_drivers();
    }
    let (asio, generation) = (d.asio.clone(), d.watch.changes() + d.midi_changes);

    let entry = |x: &io::Endpoint| {
        let mut v = json!(x);
        if let Some(caps) = exclusive.as_ref().and_then(|all| all.iter().find(|c| c.name == x.name && (c.direction == io::StreamDirection::Input) == x.input)) {
            v["exclusive"] = json!({ "formats": caps.formats, "min_period_ms": caps.min_period_ms, "error": caps.error });
        }
        v
    };
    let mut text = String::new();
    for (title, input) in [("Outputs", false), ("Inputs", true)] {
        text.push_str(&format!("{title}\n"));
        for x in now.iter().filter(|x| x.input == input) {
            text.push_str(&format!("  {}\n", describe(x)));
        }
        if !now.iter().any(|x| x.input == input) {
            text.push_str("  none\n");
        }
    }
    text.push_str("MIDI inputs\n");
    for m in &midi {
        text.push_str(&format!("  {m}\n"));
    }
    match (&midi_error, midi.is_empty()) {
        (Some(err), _) => text.push_str(&format!("  could not be listed: {err}\n")),
        (None, true) => text.push_str("  none\n"),
        _ => {}
    }
    if probe {
        text.push_str(&format!("ASIO drivers: {}\n", if asio.is_empty() { "none installed".to_string() } else { asio.join("; ") }));
    }
    for c in &changes {
        text.push_str(&format!("Changed: {c}\n"));
    }
    Ok(Reply::new(
        text.trim_end().to_string(),
        json!({
            "outputs": now.iter().filter(|x| !x.input).map(entry).collect::<Vec<_>>(),
            "inputs": now.iter().filter(|x| x.input).map(entry).collect::<Vec<_>>(),
            "midi": midi, "midi_error": midi_error,
            "asio": asio, "probed": probe, "changes": changes, "generation": generation,
            "choice": choice, "project_rate": rate, "playing": playing,
        }),
    ))
}

fn audio_changes(e: &mut Engine, _: &Args) -> Result<Reply, DawError> {
    let (midi, _) = midi_inputs();
    let d = devices(e);
    // Not logged here: the window lists the devices again when this number moves, and that
    // listing writes what changed.
    let before = d.midi.clone();
    if midi_diff(d, &midi).is_empty() {
        // nothing new
    } else {
        // The listing that follows must see the change again, to name it.
        d.midi = before;
    }
    let n = d.watch.changes() + d.midi_changes;
    Ok(Reply::new(format!("{n} device changes since Crucible started"), json!({ "generation": n })))
}

/// Other programs with sound on the output whose name contains `device`, or on the default
/// output. For the notes shown when playback starts or cannot start.
pub(crate) fn others_on(device: Option<&str>) -> Vec<String> {
    let Ok(list) = io::endpoints() else { return Vec::new() };
    let needle = device.map(str::to_lowercase);
    list.into_iter()
        .filter(|d| !d.input)
        .find(|d| match &needle {
            Some(n) => d.name.to_lowercase().contains(n),
            None => d.default,
        })
        .map(|d| d.used_by)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, input: bool, default: bool, rate: u32) -> io::Endpoint {
        io::Endpoint { name: name.into(), input, default, rate: Some(rate), channels: Some(2), bits: Some(24), used_by: Vec::new() }
    }

    #[test]
    fn changes_are_named_in_plain_words() {
        let old = vec![dev("Speakers", false, true, 48_000), dev("Mic", true, true, 48_000)];
        let new = vec![dev("Speakers", false, false, 44_100), dev("USB Interface", false, true, 96_000)];
        assert_eq!(
            diff(&old, &new),
            ["Connected: USB Interface (output)", "Disconnected: Mic (input)", "Speakers now runs at 44100 Hz (was 48000 Hz)"]
        );
        assert_eq!(diff(&new, &new), Vec::<String>::new());
        let back = vec![dev("Speakers", false, true, 44_100)];
        assert_eq!(diff(&new, &back), ["Disconnected: USB Interface (output)", "The default output is now Speakers"]);
        assert_eq!(describe(&io::Endpoint { used_by: vec!["chrome.exe".into()], ..dev("Speakers", false, true, 48_000) }), "Speakers, 48000 Hz 24-bit 2 ch [default] | in use by chrome.exe");
    }
}
