//! SEAM 1: MIDI devices.
//!
//! Everything that talks to the operating system's MIDI APIs lives in this crate.
//! Today it only lists input ports (milestone M0 step A). Receiving notes comes in M0 step B.
//!
//! Design rule from docs/07-hardware-and-testing.md: the DAW records from every connected
//! input by default and never binds to a keyboard by name, because each of the three
//! machines has a different generic keyboard.

// PLATFORM: portable. midir wraps
//   Windows: WinMM (the classic Windows MIDI API). A "winrt" feature exists for Windows MIDI
//            Services; not enabled, revisit if a keyboard fails to appear.
//   macOS:   CoreMIDI
//   Linux:   ALSA sequencer, or JACK behind a feature.

use crucible_diagnostics::{Code, DawError};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MidiReport {
    pub inputs: Vec<MidiPortReport>,
    /// Set when the MIDI system itself could not be opened. Starts with its code (E3001).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MidiPortReport {
    pub name: String,
    /// Backend-specific port id. Not stable across machines; never stored in a project.
    pub id: String,
}

/// List every MIDI input port visible to this build.
pub fn enumerate_inputs() -> MidiReport {
    let input = match midir::MidiInput::new("Crucible device scan") {
        Ok(i) => i,
        Err(e) => {
            let err = DawError::new(Code::MidiUnavailable, e.to_string());
            return MidiReport { inputs: vec![], error: Some(err.to_string()) };
        }
    };
    let inputs = input
        .ports()
        .iter()
        .map(|p| MidiPortReport {
            name: input.port_name(p).unwrap_or_else(|e| format!("<unreadable name: {e}>")),
            id: p.id(),
        })
        .collect();
    MidiReport { inputs, error: None }
}

/// One open MIDI input. Dropping it closes the port.
pub struct Connection {
    pub name: String,
    pub id: String,
    _conn: midir::MidiInputConnection<()>,
}

/// Open the input with this port id. `on` gets each channel message (note, controller, pitch
/// bend, pressure: one to three bytes) on the MIDI system's own thread, so it must be quick and
/// must not block. Clock, active sensing and system-exclusive messages are left out.
///
/// On Windows (WinMM) a port can be open in one program at a time: while another program holds
/// the keyboard this fails with E3002, and while Crucible holds it the other program's would.
pub fn connect(id: &str, mut on: impl FnMut(&[u8]) + Send + 'static) -> Result<Connection, DawError> {
    let mut input = midir::MidiInput::new("Crucible").map_err(|e| DawError::new(Code::MidiUnavailable, e.to_string()))?;
    input.ignore(midir::Ignore::All);
    let port = input.find_port_by_id(id).ok_or_else(|| DawError::new(Code::MidiPortBusy, format!("the MIDI input with id {id} is no longer there")))?;
    let name = input.port_name(&port).unwrap_or_else(|_| id.to_string());
    let conn = input
        .connect(&port, "crucible-in", move |_stamp, bytes, _| if bytes.first().is_some_and(|s| (0x80..0xF0).contains(s)) { on(bytes) }, ())
        .map_err(|e| DawError::new(Code::MidiPortBusy, format!("{name}: {e}")))?;
    Ok(Connection { name, id: id.to_string(), _conn: conn })
}
