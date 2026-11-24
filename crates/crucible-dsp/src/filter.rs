//! Small filters and a delay line shared by the effects. Coefficients come from `math`, so they
//! are identical on every machine. State is plain fields; nothing allocates after `new`.

use crate::math::{PI, cos_turns, exp, sin_turns};

/// Second-order section (RBJ cookbook), Butterworth Q. Used for the wet low and high cuts.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    pub on: bool,
}

impl Biquad {
    /// Keep the new coefficients, continue from `old`'s signal history.
    pub fn adopt(&mut self, old: &Biquad) {
        (self.x1, self.x2, self.y1, self.y2) = (old.x1, old.x2, old.y1, old.y2);
    }
    fn design(f: f64, rate: f64, high_pass: bool) -> Biquad {
        let t = f / rate;
        let (sn, cs) = (sin_turns(t), cos_turns(t));
        let alpha = sn / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high_pass { ((1.0 + cs) / 2.0, -(1.0 + cs)) } else { ((1.0 - cs) / 2.0, 1.0 - cs) };
        Biquad {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b0 / a0) as f32,
            a1: (-2.0 * cs / a0) as f32,
            a2: ((1.0 - alpha) / a0) as f32,
            on: true,
            ..Biquad::default()
        }
    }

    /// High-pass at `f` Hz, or a pass-through when `f` is below 20 Hz ("Off").
    pub fn high_pass(f: f64, rate: f64) -> Biquad {
        if f < 20.0 { Biquad::default() } else { Biquad::design(f, rate, true) }
    }

    /// Low-pass at `f` Hz, or a pass-through when `f` is at 20 kHz or above, or past 45% of the rate.
    pub fn low_pass(f: f64, rate: f64) -> Biquad {
        if f >= 20_000.0 || f >= 0.45 * rate { Biquad::default() } else { Biquad::design(f, rate, false) }
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        if !self.on {
            return x;
        }
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn clear(&mut self) {
        (self.x1, self.x2, self.y1, self.y2) = (0.0, 0.0, 0.0, 0.0);
    }
}

/// One-pole low-pass or high-pass, used inside delay feedback loops.
#[derive(Clone, Copy, Default)]
pub struct OnePole {
    a: f32,
    z: f32,
    high_pass: bool,
    pub on: bool,
}

impl OnePole {
    pub fn adopt(&mut self, old: &OnePole) {
        self.z = old.z;
    }
    fn new(f: f64, rate: f64, high_pass: bool) -> OnePole {
        OnePole { a: (1.0 - exp(-2.0 * PI * f / rate)) as f32, z: 0.0, high_pass, on: true }
    }
    /// Low-pass at `f`, or a pass-through at 20 kHz and above.
    pub fn low_pass(f: f64, rate: f64) -> OnePole {
        if f >= 20_000.0 || f >= 0.45 * rate { OnePole::default() } else { OnePole::new(f, rate, false) }
    }
    pub fn high_pass(f: f64, rate: f64) -> OnePole {
        if f < 1.0 { OnePole::default() } else { OnePole::new(f, rate, true) }
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        if !self.on {
            return x;
        }
        self.z += self.a * (x - self.z);
        if self.high_pass { x - self.z } else { self.z }
    }
    pub fn clear(&mut self) {
        self.z = 0.0;
    }
}

/// Second-order high shelf (RBJ, slope 1): gain 1 below `corner` Hz and `hf` well above it.
/// Jot's absorption filter: inside a reverb line it makes high frequencies die sooner. A
/// first-order shelf was tried first and measured: its slow transition shortened the 1 kHz decay
/// by 12%, so the mids no longer matched the Decay control.
#[derive(Clone, Copy, Default)]
pub struct Shelf(Biquad);

impl Shelf {
    pub fn adopt(&mut self, old: &Shelf) {
        self.0.adopt(&old.0);
    }
    pub fn new(hf: f64, corner: f64, rate: f64) -> Shelf {
        let a = hf.sqrt(); // RBJ's A: the shelf's gain is A squared
        let t = corner / rate;
        let (sn, cs) = (sin_turns(t), cos_turns(t));
        let alpha = sn / 2.0 * std::f64::consts::SQRT_2;
        let ra = a.sqrt();
        let a0 = (a + 1.0) - (a - 1.0) * cs + 2.0 * ra * alpha;
        let q = |v: f64| (v / a0) as f32;
        Shelf(Biquad {
            b0: q(a * ((a + 1.0) + (a - 1.0) * cs + 2.0 * ra * alpha)),
            b1: q(-2.0 * a * ((a - 1.0) + (a + 1.0) * cs)),
            b2: q(a * ((a + 1.0) + (a - 1.0) * cs - 2.0 * ra * alpha)),
            a1: q(2.0 * ((a - 1.0) - (a + 1.0) * cs)),
            a2: q((a + 1.0) - (a - 1.0) * cs - 2.0 * ra * alpha),
            on: true,
            ..Biquad::default()
        })
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        self.0.run(x)
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// A circular delay line with whole-sample and cubic fractional reads.
#[derive(Clone)]
pub struct Line {
    buf: Vec<f32>,
    w: usize,
}

impl Line {
    /// Take `old`'s contents if it is the same length; otherwise stay empty.
    pub fn adopt(&mut self, old: &mut Line) {
        if self.buf.len() == old.buf.len() {
            std::mem::swap(self, old);
        }
    }
    /// Room for delays up to `max` samples, plus the four points a cubic read needs.
    pub fn new(max: usize) -> Line {
        Line { buf: vec![0.0; max + 4], w: 0 }
    }
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.w = if self.w + 1 == self.buf.len() { 0 } else { self.w + 1 };
        self.buf[self.w] = x;
    }
    /// The sample written `d` pushes ago (0 = the last one pushed). `d` is always shorter than
    /// the line, so one compare replaces the two integer divisions a `%` would cost (measured:
    /// they made the reverb three times slower).
    #[inline]
    pub fn tap(&self, d: usize) -> f32 {
        let i = if d <= self.w { self.w - d } else { self.w + self.buf.len() - d };
        self.buf[i]
    }
    /// A fractional delay of `d` samples (d >= 1), cubic Hermite interpolation.
    #[inline]
    pub fn read(&self, d: f64) -> f32 {
        let i = d.floor();
        let f = (d - i) as f32;
        let i = i as usize;
        let (y0, y1, y2, y3) = (self.tap(i - 1), self.tap(i), self.tap(i + 1), self.tap(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * f + c2) * f + c1) * f + y1
    }
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// Wet output below this (about -150 dBFS) counts as silence.
pub const SILENT: f32 = 3.2e-8;

/// Decides when an effect's tail has died away, so it can clear its state and stop working.
///
/// The decision is taken only at fixed points of the timeline (every 1,024 samples) from counts
/// kept sample by sample, so it does not depend on how the audio is cut into blocks. Playback in
/// small blocks and an export in large ones go idle on the same sample, which keeps what is
/// heard and what is exported identical. (Until 2026-11-23 the decision was per block, and the
/// two differed by up to -150 dBFS near the end of a tail.)
#[derive(Clone, Copy, Default)]
pub struct Idle {
    pub idle: bool,
    quiet_for: u64,
    loudest: f32,
}

impl Idle {
    /// One sample at timeline frame `pos`. `quiet`: nothing came in and nothing audible is being
    /// fed back. `wet`: the size of the effect's output. `hold`: how many quiet samples must
    /// pass before no sound can still be on its way out. Returns true when the effect should
    /// clear its state now.
    #[inline]
    pub fn step(&mut self, pos: u64, quiet: bool, wet: f32, hold: u64) -> bool {
        if quiet {
            self.quiet_for += 1;
        } else {
            self.quiet_for = 0;
            self.idle = false;
        }
        self.loudest = self.loudest.max(wet);
        if (pos + 1) & 1023 != 0 {
            return false;
        }
        let done = self.quiet_for > hold && self.loudest < SILENT;
        self.loudest = 0.0;
        if done && !self.idle {
            self.idle = true;
            return true;
        }
        false
    }
}
