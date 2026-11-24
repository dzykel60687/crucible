//! Reverb: a diffuser feeding a 16-line feedback delay network (FDN), with Room, Hall and Plate
//! characters. Design and sources: `docs/13-reverb-and-delay.md`, sections 3, 4 and 9.
//!
//! Signal path, per sample:
//! 1. Pre-delay.
//! 2. Early reflections (Room, Hall): seeded taps in the first 10 to 120 ms, alternating sides.
//! 3. Diffuser: four steps over 16 channels, each a per-channel delay, a polarity flip and a
//!    Hadamard mix, so echo density builds up within milliseconds (Signalsmith's design).
//! 4. FDN: 16 delay lines with prime lengths spread exponentially, a Householder feedback mix,
//!    and in every line Jot's absorption filter, sized from that line's own length, so lows decay
//!    in exactly `Decay` seconds and highs in `Decay x Damping`. Four lines are slowly modulated to
//!    break up ringing; their phase comes from the absolute sample position, so a render is the
//!    same whatever the block size.
//! 5. Left and right are different signed sums of the lines (an uncorrelated stereo tail), then
//!    the wet low cut, then the equal-power mix.

use crate::filter::{Biquad, Idle, Line, Shelf};
use crate::math::{Rng, exp, lfo, ln, mix_gains, next_prime};
use crate::{Effect, Values};

const N: usize = 16;
/// Where the damping shelf turns over. Below it lines decay at `Decay`, well above it at
/// `Decay x Damping`.
const DAMPING_CORNER_HZ: f64 = 3000.0;

#[derive(Clone, Copy)]
struct Character {
    /// Shortest and longest FDN line at Size 50%, in ms.
    lines_ms: (f64, f64),
    /// Total diffuser length, ms.
    diffusion_ms: f64,
    /// Early reflections: count, first and last tap in ms, level. Count 0 = none.
    er: (usize, f64, f64, f32),
    /// Modulation depth in ms on four lines.
    mod_ms: f64,
}

const CHARACTERS: [Character; 3] = [
    Character { lines_ms: (15.0, 45.0), diffusion_ms: 20.0, er: (10, 10.0, 80.0, 0.5), mod_ms: 0.2 }, // Room
    Character { lines_ms: (40.0, 120.0), diffusion_ms: 40.0, er: (12, 20.0, 120.0, 0.4), mod_ms: 0.3 }, // Hall
    Character { lines_ms: (20.0, 60.0), diffusion_ms: 60.0, er: (0, 0.0, 0.0, 0.0), mod_ms: 0.25 }, // Plate
];

/// Modulation rates for the four moving lines, Hz. Slow and unrelated, so no beat pattern forms.
const MOD_HZ: [f64; 4] = [0.13, 0.21, 0.37, 0.53];

/// The longest Decay a tank supports without audible ringing (Schroeder's mode-density rule):
/// the sum of the line lengths in seconds divided by 0.15.
pub fn max_decay(character: usize, size: f64) -> f64 {
    line_ms(character, size).iter().sum::<f64>() / 1000.0 / 0.15
}

fn line_ms(character: usize, size: f64) -> [f64; N] {
    let c = CHARACTERS[character.min(2)];
    let scale = 0.5 + size.clamp(0.0, 1.0); // 0.5x to 1.5x
    let (lo, hi) = (c.lines_ms.0 * scale, c.lines_ms.1 * scale);
    let step = ln(hi / lo) / (N - 1) as f64;
    std::array::from_fn(|i| lo * exp(step * i as f64))
}

struct Step {
    lines: Vec<Line>,
    delays: [usize; N],
    flip: [f32; N],
    order: [usize; N],
}

struct Tank {
    line: Line,
    len: usize,
    /// Low-frequency loss per pass, and the shelf that adds the extra high-frequency loss.
    gain: f32,
    shelf: Shelf,
    /// Input scaling that keeps the wet level the same whatever the decay.
    input: f32,
    /// (depth in samples, rate in Hz, phase) for the four modulated lines.
    modulation: Option<(f64, f64, f64)>,
}

pub struct Reverb {
    rate: f64,
    pre: [Line; 2],
    pre_len: usize,
    er_line: [Line; 2],
    er_taps: Vec<(usize, f32, usize)>,
    er_feed: f32,
    steps: Vec<Step>,
    tank: Vec<Tank>,
    out_sign: [[f32; N]; 2],
    low_cut: [Biquad; 2],
    dry: f32,
    wet: f32,
    tail: u64,
    idle: Idle,
    /// How long sound can sit in the pre-delay, reflections and diffuser before reaching the
    /// output. The reverb goes idle only after the input has been silent for longer than that.
    hold: u64,
}

/// Hadamard transform of 16 values, scaled to keep energy (1/4).
#[inline]
fn hadamard(x: &mut [f32; N]) {
    let mut h = 1;
    while h < N {
        for i in (0..N).step_by(h * 2) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
        }
        h *= 2;
    }
    for v in x.iter_mut() {
        *v *= 0.25;
    }
}

impl Reverb {
    pub fn new(v: &Values, rate: f64) -> Reverb {
        let character = v.get("character") as usize;
        let c = CHARACTERS[character.min(2)];
        let size = v.get("size") / 100.0;
        let decay = v.get("decay").clamp(0.2, max_decay(character, size));
        let decay_hf = decay * (v.get("damping") / 100.0).clamp(0.1, 1.0);
        let ms = |m: f64| (m * rate / 1000.0).round() as usize;
        let mut rng = Rng(0x00C0_FFEE ^ character as u64);

        // FDN lines: distinct primes at this rate.
        let mut used = Vec::new();
        let mut tank = Vec::with_capacity(N);
        for (i, m) in line_ms(character, size).iter().enumerate() {
            let mut len = next_prime(ms(*m).max(2));
            while used.contains(&len) {
                len = next_prime(len + 1);
            }
            used.push(len);
            let loss = |t60: f64| exp(-3.0 * std::f64::consts::LN_10 * len as f64 / (rate * t60));
            let (g_lo, g_hi) = (loss(decay), loss(decay_hf));
            let moving = [1, 6, 10, 13].iter().position(|&k| k == i);
            let depth = c.mod_ms * rate / 1000.0;
            tank.push(Tank {
                line: Line::new(len + depth.ceil() as usize + 4),
                len,
                gain: g_lo as f32,
                shelf: Shelf::new(g_hi / g_lo, DAMPING_CORNER_HZ, rate),
                input: (1.0 - g_lo * g_lo).sqrt() as f32,
                modulation: moving.map(|k| (depth, MOD_HZ[k], rng.unit())),
            });
        }

        // Diffuser: step lengths double up to the character's total; each channel's delay is
        // drawn from its own slice of the step, so they spread evenly.
        let mut steps = Vec::new();
        for s in 0..4 {
            let span = c.diffusion_ms * (0.5 + size) * f64::from(1u32 << s) / 8.0;
            let mut delays = [0usize; N];
            let mut order: [usize; N] = std::array::from_fn(|i| i);
            for (i, d) in delays.iter_mut().enumerate() {
                *d = ms(span * (i as f64 + rng.unit()) / N as f64).max(1);
            }
            for i in (1..N).rev() {
                order.swap(i, (rng.unit() * (i + 1) as f64) as usize);
            }
            let flip = std::array::from_fn(|_| if rng.unit() < 0.5 { -1.0 } else { 1.0 });
            steps.push(Step { lines: delays.iter().map(|&d| Line::new(d + 1)).collect(), delays, flip, order });
        }

        // Early reflections: seeded taps, alternating sides, quieter with time.
        let (count, first, last, level) = c.er;
        let scale = 0.5 + size;
        let er_taps: Vec<(usize, f32, usize)> = (0..count)
            .map(|k| {
                let t = first + (last - first) * (k as f64 + rng.unit() * 0.8) / count as f64;
                let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                (ms(t * scale).max(1), (sign * f64::from(level) * first / t) as f32, k % 2)
            })
            .collect();
        let er_max = er_taps.iter().map(|t| t.0).max().unwrap_or(1);

        // Output signs: two different Hadamard rows, so left and right share no common sum.
        let row = |r: usize| std::array::from_fn(|i| if (i & r).count_ones().is_multiple_of(2) { 0.25 } else { -0.25 });
        let pre_len = ms(v.get("predelay"));
        let (dry, wet) = mix_gains(v.get("mix") / 100.0);
        let tail_s = decay * 1.5 + v.get("predelay") / 1000.0 + last * scale / 1000.0;
        Reverb {
            rate,
            pre: [Line::new(pre_len + 1), Line::new(pre_len + 1)],
            pre_len,
            er_line: [Line::new(er_max + 1), Line::new(er_max + 1)],
            er_taps,
            er_feed: if count > 0 { 0.5 } else { 0.0 },
            steps,
            tank,
            out_sign: [row(0b0101), row(0b0011)],
            low_cut: [Biquad::high_pass(v.get("lowcut"), rate), Biquad::high_pass(v.get("lowcut"), rate)],
            dry,
            wet,
            tail: (tail_s * rate) as u64,
            idle: Idle::default(),
            hold: (pre_len + er_max + ms(c.diffusion_ms * 3.0) + 64) as u64,
        }
    }

    #[inline]
    fn sample(&mut self, pos: u64, l: f32, r: f32) -> (f32, f32) {
        // 1-2. pre-delay and early reflections
        self.pre[0].push(l);
        self.pre[1].push(r);
        let (pl, pr) = (self.pre[0].tap(self.pre_len), self.pre[1].tap(self.pre_len));
        self.er_line[0].push(pl);
        self.er_line[1].push(pr);
        let mut er = [0.0f32; 2];
        for &(d, g, side) in &self.er_taps {
            er[side] += self.er_line[side].tap(d) * g;
        }
        // 3. diffuser
        let mut ch: [f32; N] = std::array::from_fn(|i| if i % 2 == 0 { pl + er[0] * self.er_feed } else { pr + er[1] * self.er_feed });
        for s in &mut self.steps {
            let mut next = [0.0f32; N];
            for i in 0..N {
                s.lines[i].push(ch[i]);
                next[s.order[i]] = s.lines[i].tap(s.delays[i]) * s.flip[i];
            }
            ch = next;
            hadamard(&mut ch);
        }
        // 4. FDN
        let t = pos as f64 / self.rate;
        let mut out = [0.0f32; N];
        for (i, k) in self.tank.iter_mut().enumerate() {
            let y = match k.modulation {
                Some((depth, hz, phase)) => k.line.read(k.len as f64 + depth * (0.5 + 0.5 * lfo(hz * t + phase))),
                None => k.line.tap(k.len),
            };
            out[i] = y;
        }
        let mut fb: [f32; N] = std::array::from_fn(|i| self.tank[i].shelf.run(out[i]) * self.tank[i].gain);
        let sum = fb.iter().sum::<f32>() * (2.0 / N as f32);
        for (i, k) in self.tank.iter_mut().enumerate() {
            fb[i] -= sum; // Householder
            k.line.push(fb[i] + ch[i] * k.input);
        }
        // 5. outputs
        let mut w = [er[0], er[1]];
        for (side, sign) in self.out_sign.iter().enumerate() {
            for i in 0..N {
                w[side] += out[i] * sign[i];
            }
        }
        (self.low_cut[0].run(w[0]), self.low_cut[1].run(w[1]))
    }

    fn clear(&mut self) {
        self.pre.iter_mut().chain(self.er_line.iter_mut()).for_each(Line::clear);
        self.steps.iter_mut().flat_map(|s| s.lines.iter_mut()).for_each(Line::clear);
        for k in &mut self.tank {
            k.line.clear();
            k.shelf.clear();
        }
        self.low_cut.iter_mut().for_each(Biquad::clear);
    }
}

impl Effect for Reverb {
    fn adopt(&mut self, old: &mut dyn Effect) {
        let Some(o) = old.as_any().downcast_mut::<Reverb>() else { return };
        if o.rate != self.rate {
            return;
        }
        for (a, b) in self.pre.iter_mut().chain(&mut self.er_line).zip(o.pre.iter_mut().chain(&mut o.er_line)) {
            a.adopt(b);
        }
        for (a, b) in self.steps.iter_mut().zip(&mut o.steps) {
            for (x, y) in a.lines.iter_mut().zip(&mut b.lines) {
                x.adopt(y);
            }
        }
        for (a, b) in self.tank.iter_mut().zip(&mut o.tank) {
            a.line.adopt(&mut b.line);
            a.shelf.adopt(&b.shelf);
        }
        for (a, b) in self.low_cut.iter_mut().zip(&o.low_cut) {
            a.adopt(b);
        }
        self.idle = o.idle;
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn process(&mut self, pos: u64, l: &mut [f32], r: &mut [f32]) {
        if self.wet == 0.0 {
            return; // Mix 0: exactly the dry signal
        }
        if self.idle.idle && l.iter().chain(r.iter()).all(|&x| x == 0.0) {
            return; // silence in, silence out; the dry part of silence is silence
        }
        for k in 0..l.len() {
            let p = pos + k as u64;
            let quiet = l[k] == 0.0 && r[k] == 0.0;
            let (wl, wr) = self.sample(p, l[k], r[k]);
            l[k] = l[k] * self.dry + wl * self.wet;
            r[k] = r[k] * self.dry + wr * self.wet;
            if self.idle.step(p, quiet, wl.abs().max(wr.abs()), self.hold) {
                // The tail has died away: clear the state so it is exactly zero from here on.
                self.clear();
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
