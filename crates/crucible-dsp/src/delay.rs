//! Delay and Echo: one engine with two characters (`docs/13-reverb-and-delay.md`, section 7).
//!
//! - **Clean** (shown as Delay): whole-sample timing, so a quarter note at 120 BPM lands exactly
//!   on sample 24,000 at 48 kHz. The loop has a gentle low cut (80 Hz) and the Tone high cut, so
//!   repeats do not pile up mud. Feedback stops at 98%.
//! - **Tape** (shown as Echo): the read point moves with slow wow and faster flutter (cubic
//!   interpolation keeps it clean), the loop is darker (150 Hz low cut) and saturates softly, so
//!   feedback can pass 100% and still stay bounded, as a tape echo does when pushed.
//!
//! Ping-pong feeds the input to the left line and crosses the repeats left, right, left.

use crate::filter::{Idle, Line, OnePole, SILENT};
use crate::math::{lfo, mix_gains};
use crate::{Effect, Values};

/// Note values for the Sync control, in beats. Index 0 is Off (use Time in ms).
pub const NOTE_BEATS: [f64; 12] = [0.0, 4.0, 2.0, 1.0, 0.5, 0.25, 1.5, 0.75, 0.375, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0];
pub const NOTE_NAMES: &[&str] = &["Off", "1/1", "1/2", "1/4", "1/8", "1/16", "1/4 dotted", "1/8 dotted", "1/16 dotted", "1/4 triplet", "1/8 triplet", "1/16 triplet"];

const MAX_SECONDS: f64 = 2.0;

pub struct Delay {
    rate: f64,
    lines: [Line; 2],
    /// Delay in samples.
    d: f64,
    tape: bool,
    feedback: f32,
    tone: [OnePole; 2],
    low_cut: [OnePole; 2],
    ping_pong: bool,
    /// Tape only: (depth in samples, rate in Hz) for wow and flutter, and saturation drive.
    wow: (f64, f64),
    flutter: (f64, f64),
    drive: f32,
    dry: f32,
    wet: f32,
    tail: u64,
    /// Repeats keep circulating after the input stops, so the effect only goes idle once nothing
    /// audible has been written into a line for a whole delay time.
    idle: Idle,
}

/// Delay time in samples for these settings: the note value at the tempo, or the time in ms.
pub fn delay_samples(v: &Values, rate: f64, tempo: f64) -> f64 {
    let note = NOTE_BEATS[(v.get("sync") as usize).min(NOTE_BEATS.len() - 1)];
    let seconds = if note > 0.0 { note * 60.0 / tempo } else { v.get("time") / 1000.0 };
    (seconds * rate).clamp(2.0, MAX_SECONDS * rate)
}

/// Gentle saturation: slope 1 near zero, approaching +-1. A rational fit to tanh, exact arithmetic.
#[inline]
fn soft(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

impl Delay {
    pub fn new(v: &Values, rate: f64, tempo: f64) -> Delay {
        let tape = v.get("character") >= 1.0;
        let wear = (v.get("wear") / 100.0).clamp(0.0, 1.0);
        let mut d = delay_samples(v, rate, tempo);
        if !tape {
            d = d.round();
        }
        let fb = (v.get("feedback") / 100.0).clamp(0.0, if tape { 1.1 } else { 0.98 });
        let (dry, wet) = mix_gains(v.get("mix") / 100.0);
        let wow = (wear * 1.5e-3 * rate, 0.6);
        let flutter = (wear * 0.1e-3 * rate, 6.3);
        // Room for the longest time plus the deepest wobble (Wear 100), whatever the settings, so
        // a settings change during playback can keep the repeats (`adopt`).
        let room = (MAX_SECONDS * rate) as usize + (1.6e-3 * rate).ceil() as usize + 8;
        // Repeats until -90 dB, capped at 30 s (a self-oscillating tape echo never stops).
        let repeats = if fb <= 0.0 { 1.0 } else if fb >= 1.0 { f64::INFINITY } else { 1.0 - 4.5 * std::f64::consts::LN_10 / crate::math::ln(fb) };
        let tail = (d * repeats.min(30.0 * rate / d)).min(30.0 * rate) as u64;
        let (lc, tone) = (if tape { 150.0 } else { 80.0 }, v.get("tone"));
        Delay {
            rate,
            lines: [Line::new(room), Line::new(room)],
            d,
            tape,
            feedback: fb as f32,
            tone: [OnePole::low_pass(tone, rate), OnePole::low_pass(tone, rate)],
            low_cut: [OnePole::high_pass(lc, rate), OnePole::high_pass(lc, rate)],
            ping_pong: v.get("pingpong") >= 0.5,
            wow,
            flutter,
            drive: (1.0 + 3.0 * wear) as f32,
            dry,
            wet,
            tail,
            idle: Idle::default(),
        }
    }

    /// Extra delay from wow and flutter at time `t` (seconds), the same for both sides.
    #[inline]
    fn wobble(&self, t: f64) -> f64 {
        if self.tape { self.wow.0 * (0.5 + 0.5 * lfo(self.wow.1 * t)) + self.flutter.0 * (0.5 + 0.5 * lfo(self.flutter.1 * t + 0.31)) } else { 0.0 }
    }

    #[inline]
    fn read(&self, side: usize, m: f64) -> f32 {
        if self.tape { self.lines[side].read(self.d + m - 1.0) } else { self.lines[side].tap(self.d as usize - 1) }
    }

    #[inline]
    fn loop_filter(&mut self, side: usize, y: f32) -> f32 {
        let y = self.low_cut[side].run(self.tone[side].run(y * self.feedback));
        if self.tape { soft(y * self.drive) / self.drive } else { y }
    }
}


impl Effect for Delay {
    fn adopt(&mut self, old: &mut dyn Effect) {
        let Some(o) = old.as_any().downcast_mut::<Delay>() else { return };
        if o.rate != self.rate {
            return;
        }
        for k in 0..2 {
            self.lines[k].adopt(&mut o.lines[k]);
            self.tone[k].adopt(&o.tone[k]);
            self.low_cut[k].adopt(&o.low_cut[k]);
        }
        self.idle = o.idle;
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn process(&mut self, pos: u64, l: &mut [f32], r: &mut [f32]) {
        if self.wet == 0.0 {
            return;
        }
        if self.idle.idle && l.iter().chain(r.iter()).all(|&x| x == 0.0) {
            return;
        }
        let hold = (self.d + self.wow.0 + self.flutter.0 + 8.0) as u64;
        for k in 0..l.len() {
            let p = pos + k as u64;
            let m = self.wobble(p as f64 / self.rate);
            let (yl, yr) = (self.read(0, m), self.read(1, m));
            let (fl, fr) = (self.loop_filter(0, yl), self.loop_filter(1, yr));
            let (wl, wr) = if self.ping_pong { (0.5 * (l[k] + r[k]) + fr, fl) } else { (l[k] + fl, r[k] + fr) };
            self.lines[0].push(wl);
            self.lines[1].push(wr);
            let quiet = l[k] == 0.0 && r[k] == 0.0 && wl.abs() <= SILENT && wr.abs() <= SILENT;
            l[k] = l[k] * self.dry + yl * self.wet;
            r[k] = r[k] * self.dry + yr * self.wet;
            if self.idle.step(p, quiet, yl.abs().max(yr.abs()), hold) {
                self.lines.iter_mut().for_each(Line::clear);
                self.tone.iter_mut().chain(self.low_cut.iter_mut()).for_each(OnePole::clear);
            }
        }
    }
    fn tail(&self) -> u64 {
        self.tail
    }
    fn latency(&self) -> u64 {
        0
    }
}
