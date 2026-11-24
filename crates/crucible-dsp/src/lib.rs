//! Crucible's built-in effects: Reverb, IR Reverb, Delay and Echo.
//!
//! Plain arithmetic, no OS code, no allocation while processing. Each effect is described by a
//! table of parameters (`KINDS`), the same way commands are (D-022): the engine validates and
//! stores values by name, and the window draws its controls from the same table, so adding a
//! parameter is one table row. Research and sources: `docs/13-reverb-and-delay.md`.
//!
//! Portability: platform-neutral. Coefficients use `math`, not the standard library's `sin`/`exp`,
//! so algorithmic effects render the same bits on every machine.

pub mod convolve;
pub mod delay;
pub mod filter;
pub mod math;
pub mod reverb;

/// A running effect. Created off the audio thread (it allocates there); `process` never allocates.
pub trait Effect: Send {
    /// Process one stereo block in place. `pos` is the timeline frame of its first sample, so
    /// modulation follows song time and renders do not depend on block size.
    fn process(&mut self, pos: u64, l: &mut [f32], r: &mut [f32]);
    /// How many samples the effect keeps sounding after its input stops, so an export can include
    /// the reverb or echo tail.
    fn tail(&self) -> u64;
    /// Delay the effect adds to the signal. Every built-in effect is zero.
    fn latency(&self) -> u64;
    /// Take over the sound still inside `old`, the same effect as it was before a settings
    /// change, so a knob moved during playback does not cut the reverb or echo tail. Only pointer
    /// swaps and copies: no allocation. Parts whose size changed (a new room size, another
    /// impulse response) start empty, as they must.
    fn adopt(&mut self, old: &mut dyn Effect);
    fn as_any(&mut self) -> &mut dyn std::any::Any;
}

/// Stands in for an effect that the running mixer already holds (see `Effect::adopt`): playback
/// swaps the real one in. Passes sound through untouched.
pub struct Hole;

impl Effect for Hole {
    fn process(&mut self, _pos: u64, _l: &mut [f32], _r: &mut [f32]) {}
    fn tail(&self) -> u64 {
        0
    }
    fn latency(&self) -> u64 {
        0
    }
    fn adopt(&mut self, _old: &mut dyn Effect) {}
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// A number between `min` and `max`, shown with `unit`.
    Number { min: f64, max: f64, unit: &'static str },
    /// One of the named options, stored as its index.
    Choice(&'static [&'static str]),
    /// Off (0) or on (1).
    Switch,
}

#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub label: &'static str,
    pub control: Control,
    pub default: f64,
    pub help: &'static str,
}

#[derive(Debug)]
pub struct Kind {
    pub name: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    /// Needs an impulse-response file (IR Reverb).
    pub needs_file: bool,
    pub params: &'static [Param],
}

const fn num(name: &'static str, label: &'static str, min: f64, max: f64, unit: &'static str, default: f64, help: &'static str) -> Param {
    Param { name, label, control: Control::Number { min, max, unit }, default, help }
}

const MIX: Param = num("mix", "Mix", 0.0, 100.0, "%", 25.0, "Wet share. 0 is exactly the dry signal, 100 has no dry at all. Use 100 on a reverb send track");

static REVERB: &[Param] = &[
    Param { name: "character", label: "Character", control: Control::Choice(&["Room", "Hall", "Plate"]), default: 1.0, help: "The kind of space: small room, concert hall, or a dense bright plate" },
    num("size", "Size", 0.0, 100.0, "%", 50.0, "Scales the space. Long decays need a larger size; Decay is limited to what the size supports without ringing"),
    num("decay", "Decay", 0.2, 12.0, "s", 2.0, "Time for the reverb to fall 60 dB, in the mid frequencies"),
    num("damping", "Damping", 10.0, 100.0, "%", 50.0, "How long the highs last, as a share of Decay. Lower is darker"),
    num("predelay", "Pre-delay", 0.0, 200.0, "ms", 20.0, "Gap before the reverb starts. Keeps the dry sound clear"),
    num("lowcut", "Low cut", 0.0, 500.0, "Hz", 120.0, "High-pass on the reverb only, so it does not muddy the low end. Below 20 is off"),
    MIX,
];

static IR_REVERB: &[Param] = &[
    Param { name: "normalise", label: "Normalise", control: Control::Switch, default: 1.0, help: "Trim silence before the first sound and even out loudness between impulse responses. Off uses the file exactly as recorded" },
    num("length", "Length", 10.0, 100.0, "%", 100.0, "Shorten the tail, with a smooth fade"),
    num("predelay", "Pre-delay", 0.0, 200.0, "ms", 0.0, "Gap before the reverb starts"),
    num("lowcut", "Low cut", 0.0, 500.0, "Hz", 0.0, "High-pass on the reverb only. Below 20 is off, so the IR is heard as recorded"),
    num("highcut", "High cut", 2000.0, 20_000.0, "Hz", 20_000.0, "Low-pass on the reverb only. 20 kHz is off"),
    MIX,
];

const fn delay_params(character: f64, sync: f64, feedback: f64, tone: f64, wear: f64) -> [Param; 8] {
    [
        Param { name: "character", label: "Character", control: Control::Choice(&["Clean", "Tape"]), default: character, help: "Clean: exact repeats. Tape: darker repeats that wobble and saturate like a tape echo" },
        Param { name: "sync", label: "Sync", control: Control::Choice(delay::NOTE_NAMES), default: sync, help: "Lock the time to a note value at the project tempo. Off uses Time" },
        num("time", "Time", 1.0, 2000.0, "ms", 375.0, "Delay time when Sync is off"),
        num("feedback", "Feedback", 0.0, 110.0, "%", feedback, "How much of each repeat comes back. Clean stops at 98%; Tape can pass 100% and saturates"),
        num("tone", "Tone", 1000.0, 20_000.0, "Hz", tone, "High cut inside the loop: each repeat gets darker. 20 kHz is open"),
        Param { name: "pingpong", label: "Ping-pong", control: Control::Switch, default: 0.0, help: "Repeats bounce between left and right" },
        num("wear", "Wear", 0.0, 100.0, "%", wear, "Tape only: wow, flutter and saturation together"),
        MIX,
    ]
}
static DELAY: [Param; 8] = delay_params(0.0, 3.0, 30.0, 12_000.0, 0.0);
static ECHO: [Param; 8] = delay_params(1.0, 7.0, 40.0, 4500.0, 30.0);

pub static KINDS: &[Kind] = &[
    Kind { name: "reverb", label: "Reverb", help: "Algorithmic reverb: room, hall or plate. Adjustable and cheap", needs_file: false, params: REVERB },
    Kind { name: "irreverb", label: "IR Reverb", help: "Convolution with a recorded space (an impulse-response WAV or FLAC), like ReaVerb", needs_file: true, params: IR_REVERB },
    Kind { name: "delay", label: "Delay", help: "Clean, exact repeats, synced to the tempo", needs_file: false, params: &DELAY },
    Kind { name: "echo", label: "Echo", help: "Tape echo: warm repeats that wobble and saturate", needs_file: false, params: &ECHO },
];

pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.name == name)
}

/// An effect's parameter values: the table's defaults with any stored values on top.
pub struct Values {
    values: Vec<(&'static str, f64)>,
}

impl Values {
    pub fn new(kind: &Kind, stored: impl Fn(&str) -> Option<f64>) -> Values {
        Values { values: kind.params.iter().map(|p| (p.name, stored(p.name).unwrap_or(p.default))).collect() }
    }
    pub fn get(&self, name: &str) -> f64 {
        self.values.iter().find(|(n, _)| *n == name).map_or(0.0, |(_, v)| *v)
    }
}

impl Param {
    /// Check a value for this parameter. Choices and switches take their index or name.
    pub fn parse(&self, raw: &str) -> Result<f64, String> {
        let raw = raw.trim();
        match self.control {
            Control::Number { min, max, unit } => {
                let v: f64 = raw.trim_end_matches(unit).trim().parse().map_err(|_| format!("{} takes a number, not '{raw}'", self.label))?;
                if !(min..=max).contains(&v) {
                    return Err(format!("{} {v} is outside {min} to {max} {unit}", self.label));
                }
                Ok(v)
            }
            Control::Choice(names) => names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(raw))
                .map(|i| i as f64)
                .or_else(|| raw.parse::<usize>().ok().filter(|i| *i < names.len()).map(|i| i as f64))
                .ok_or_else(|| format!("{} is one of: {}", self.label, names.join(", "))),
            Control::Switch => match raw.to_ascii_lowercase().as_str() {
                "on" | "1" | "true" => Ok(1.0),
                "off" | "0" | "false" => Ok(0.0),
                _ => Err(format!("{} is on or off", self.label)),
            },
        }
    }
}

/// Build a running effect at the project's sample rate and tempo. `ir` is required for IR Reverb:
/// one, two or four channels at `rate`.
pub fn build(kind: &Kind, v: &Values, rate: f64, tempo: f64, ir: Option<Vec<Vec<f32>>>) -> Result<Box<dyn Effect>, String> {
    Ok(match kind.name {
        "reverb" => Box::new(reverb::Reverb::new(v, rate)),
        "irreverb" => Box::new(convolve::IrReverb::new(v, rate, ir.ok_or("IR Reverb has no impulse-response file yet")?)?),
        "delay" | "echo" => Box::new(delay::Delay::new(v, rate, tempo)),
        other => return Err(format!("unknown effect '{other}'")),
    })
}

#[cfg(test)]
mod tests;
