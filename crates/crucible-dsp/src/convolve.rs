//! IR Reverb: zero-latency partitioned convolution with an impulse-response file
//! (`docs/13-reverb-and-delay.md`, section 5). Same arithmetic as ReaVerb's convolution, so the
//! same IR gives the same result to within float rounding.
//!
//! How it stays cheap and has no latency: the first 64 IR samples are applied directly, sample by
//! sample. The rest is split into partitions handled by FFT in three block sizes (64, 1,024 and
//! 8,192 samples). A level with block size B only handles IR parts that start at least B samples
//! in, so its answer is always ready before it is due (Gardner 1995, uniform partitions per level).
//! A 3 s IR costs roughly 100 multiply-adds per sample instead of 144,000.
//!
//! Determinism: RustFFT picks AVX, SSE or NEON code at run time, so IR Reverb output may differ in
//! the last bit between machines (doc 13, section 5). The algorithmic effects do not.

use crate::filter::{Biquad, Idle};
use crate::math::{cos_turns, mix_gains};
use crate::{Effect, Values};
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

const HEAD: usize = 64;
/// (block size, last partition index + 1) per level. Level k covers IR samples [B, B x end).
/// The largest block was 16,384 at first; its once-a-block FFT took half of a 128-frame callback
/// (measured), so it is 8,192: half the burst for about 4 more multiply-adds per sample.
const LEVELS: [(usize, usize); 3] = [(64, 16), (1024, 8), (8192, usize::MAX)];

struct Level {
    b: usize,
    parts: Vec<Vec<Complex<f32>>>,
    fdl: Vec<Vec<Complex<f32>>>,
    newest: usize,
    input: Vec<f32>,
    fill: usize,
    out: Vec<f32>,
    out_pos: usize,
    fft: Arc<dyn RealToComplex<f32>>,
    ifft: Arc<dyn ComplexToReal<f32>>,
    time: Vec<f32>,
    spec: Vec<Complex<f32>>,
    acc: Vec<Complex<f32>>,
    scratch_f: Vec<Complex<f32>>,
    scratch_i: Vec<Complex<f32>>,
}

impl Level {
    fn new(ir: &[f32], b: usize, end: usize, planner: &mut RealFftPlanner<f32>) -> Option<Level> {
        let count = ir.len().div_ceil(b).min(end).saturating_sub(1); // partitions q = 1 ..= count
        if count == 0 {
            return None;
        }
        let (fft, ifft) = (planner.plan_fft_forward(2 * b), planner.plan_fft_inverse(2 * b));
        let mut time = fft.make_input_vec();
        let mut scratch_f = fft.make_scratch_vec();
        let scale = 1.0 / (2 * b) as f32; // the inverse FFT is unnormalised
        let parts = (1..=count)
            .map(|q| {
                time.fill(0.0);
                let seg = &ir[q * b..((q + 1) * b).min(ir.len())];
                for (t, h) in time.iter_mut().zip(seg) {
                    *t = h * scale;
                }
                let mut spec = fft.make_output_vec();
                fft.process_with_scratch(&mut time, &mut spec, &mut scratch_f).expect("buffer sizes come from the plan");
                spec
            })
            .collect();
        Some(Level {
            b,
            parts,
            fdl: (0..count).map(|_| fft.make_output_vec()).collect(),
            newest: 0,
            input: vec![0.0; 2 * b],
            fill: 0,
            out: vec![0.0; b],
            out_pos: 0,
            time,
            spec: fft.make_output_vec(),
            acc: fft.make_output_vec(),
            scratch_f,
            scratch_i: ifft.make_scratch_vec(),
            fft,
            ifft,
        })
    }

    #[inline]
    fn push(&mut self, x: f32) -> f32 {
        let y = self.out[self.out_pos];
        self.out_pos += 1;
        self.input[self.b + self.fill] = x;
        self.fill += 1;
        if self.fill == self.b {
            self.block();
            self.fill = 0;
            self.out_pos = 0;
        }
        y
    }

    /// The block just completed: its spectrum joins the delay line, and every partition's share of
    /// the NEXT block's output is summed in the frequency domain and transformed back once.
    fn block(&mut self) {
        let n = self.parts.len();
        self.time.copy_from_slice(&self.input);
        self.fft.process_with_scratch(&mut self.time, &mut self.spec, &mut self.scratch_f).expect("sizes from plan");
        self.newest = (self.newest + 1) % n;
        self.fdl[self.newest].copy_from_slice(&self.spec);
        self.acc.fill(Complex::new(0.0, 0.0));
        for (q, h) in self.parts.iter().enumerate() {
            let s = &self.fdl[(self.newest + n - q) % n];
            for ((a, x), y) in self.acc.iter_mut().zip(s).zip(h) {
                *a += x * y;
            }
        }
        let last = self.acc.len() - 1;
        self.acc[0].im = 0.0;
        self.acc[last].im = 0.0;
        self.ifft.process_with_scratch(&mut self.acc, &mut self.time, &mut self.scratch_i).expect("sizes from plan");
        self.out.copy_from_slice(&self.time[self.b..]);
        self.input.copy_within(self.b.., 0);
    }

    fn clear(&mut self) {
        self.fdl.iter_mut().for_each(|s| s.fill(Complex::new(0.0, 0.0)));
        self.input.fill(0.0);
        self.out.fill(0.0);
        (self.fill, self.out_pos) = (0, 0);
    }
}

/// One impulse response applied to one input, with no latency.
pub struct Convolver {
    head: Vec<f32>,
    hist: Vec<f32>,
    pos: usize,
    levels: Vec<Level>,
}

impl Convolver {
    pub fn new(ir: &[f32]) -> Convolver {
        let mut planner = RealFftPlanner::<f32>::new();
        let head = ir[..ir.len().min(HEAD)].to_vec();
        let levels = LEVELS.iter().filter_map(|&(b, end)| Level::new(ir, b, end, &mut planner)).collect();
        Convolver { hist: vec![0.0; 2 * head.len().max(1)], head, pos: 0, levels }
    }

    #[inline]
    pub fn push(&mut self, x: f32) -> f32 {
        let h = self.head.len();
        let mut y = 0.0;
        if h > 0 {
            self.pos = if self.pos == 0 { h - 1 } else { self.pos - 1 };
            self.hist[self.pos] = x;
            self.hist[self.pos + h] = x;
            let past = &self.hist[self.pos..self.pos + h]; // past[k] = x[n - k]
            for (c, v) in self.head.iter().zip(past) {
                y += c * v;
            }
        }
        for level in &mut self.levels {
            y += level.push(x);
        }
        y
    }

    pub fn clear(&mut self) {
        self.hist.fill(0.0);
        self.levels.iter_mut().for_each(Level::clear);
    }

    /// Only when cleared: put every level where it would be had silence been pushed up to
    /// timeline frame `pos`. Which samples share an FFT block decides the last bit of the
    /// rounding, so an effect that skipped silent blocks must rejoin on the same block grid as
    /// one that worked through them; otherwise output would depend on the host's block size.
    pub fn align(&mut self, pos: u64) {
        for l in &mut self.levels {
            l.fill = (pos % l.b as u64) as usize;
            l.out_pos = l.fill;
        }
    }
}

/// Prepare an impulse response (one, two or four channels at the project rate), off the audio
/// thread. With `normalise`: trim the silence before the first sound (the same amount on every
/// channel, so their timing is kept) and even out the energy. Without it the file is used exactly
/// as recorded, which is what a null test against ReaVerb needs. Then shorten to `length` (0..1)
/// with a smooth fade, and add the pre-delay.
pub fn prepare(mut ch: Vec<Vec<f32>>, normalise: bool, length: f64, predelay: usize, rate: f64) -> Vec<Vec<f32>> {
    let peak = ch.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak == 0.0 {
        return ch;
    }
    let first = if normalise { ch.iter().filter_map(|c| c.iter().position(|v| v.abs() > peak * 1e-3)).min().unwrap_or(0) } else { 0 };
    let keep = (((ch[0].len() - first) as f64 * length.clamp(0.01, 1.0)) as usize).max(1);
    // Fade only when shortening: at 100% the IR is used exactly as recorded (the ReaVerb null test).
    let fade = if length < 1.0 { (keep / 10).min((0.1 * rate) as usize).max(1) } else { 0 };
    for c in &mut ch {
        c.drain(..first);
        c.truncate(keep);
        let n = c.len();
        for k in 0..fade.min(n) {
            c[n - 1 - k] *= (0.5 - 0.5 * cos_turns(k as f64 / (2.0 * fade as f64))) as f32;
        }
    }
    if normalise {
        // Energy reaching each output; scale so the loudest output gets as much as the dry signal.
        let energy = |c: &Vec<f32>| c.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>();
        let e: Vec<f64> = ch.iter().map(energy).collect();
        let worst = match e.len() {
            4 => (e[0] + e[2]).max(e[1] + e[3]), // LL + RL into left, LR + RR into right
            _ => e.iter().copied().fold(0.0, f64::max),
        };
        if worst > 0.0 {
            let g = (1.0 / worst.sqrt()) as f32;
            ch.iter_mut().flatten().for_each(|v| *v *= g);
        }
    }
    for c in &mut ch {
        c.splice(0..0, std::iter::repeat_n(0.0, predelay));
    }
    ch
}

pub struct IrReverb {
    /// (input side, output side, convolver).
    paths: Vec<(usize, usize, Convolver)>,
    low_cut: [Biquad; 2],
    high_cut: [Biquad; 2],
    dry: f32,
    wet: f32,
    tail: u64,
    idle: Idle,
    /// Fingerprint of the prepared impulse response, to tell whether a new instance convolves
    /// with the same one.
    ir_id: u64,
}

impl IrReverb {
    /// `ir`: one, two or four channels at the project rate. Four is true stereo, in the order
    /// left-to-left, left-to-right, right-to-left, right-to-right.
    pub fn new(v: &Values, rate: f64, ir: Vec<Vec<f32>>) -> Result<IrReverb, String> {
        if ![1, 2, 4].contains(&ir.len()) {
            return Err(format!("{} channels: an impulse response has 1, 2 or 4 (true stereo)", ir.len()));
        }
        let predelay = (v.get("predelay") * rate / 1000.0).round() as usize;
        let ir = prepare(ir, v.get("normalise") >= 0.5, v.get("length") / 100.0, predelay, rate);
        let len = ir[0].len() as u64;
        // FNV-1a over every sample's bits.
        let ir_id = ir.iter().flatten().fold(0xcbf2_9ce4_8422_2325u64 ^ ir.len() as u64, |h, x| (h ^ u64::from(x.to_bits())).wrapping_mul(0x0100_0000_01b3));
        let routes: &[(usize, usize, usize)] = match ir.len() {
            1 => &[(0, 0, 0), (1, 1, 0)],
            2 => &[(0, 0, 0), (1, 1, 1)],
            _ => &[(0, 0, 0), (0, 1, 1), (1, 0, 2), (1, 1, 3)],
        };
        let (dry, wet) = mix_gains(v.get("mix") / 100.0);
        Ok(IrReverb {
            paths: routes.iter().map(|&(i, o, c)| (i, o, Convolver::new(&ir[c]))).collect(),
            low_cut: [Biquad::high_pass(v.get("lowcut"), rate), Biquad::high_pass(v.get("lowcut"), rate)],
            high_cut: [Biquad::low_pass(v.get("highcut"), rate), Biquad::low_pass(v.get("highcut"), rate)],
            dry,
            wet,
            tail: len,
            idle: Idle::default(),
            ir_id,
        })
    }
}


impl Effect for IrReverb {
    fn adopt(&mut self, old: &mut dyn Effect) {
        let Some(o) = old.as_any().downcast_mut::<IrReverb>() else { return };
        if o.ir_id != self.ir_id {
            return; // another impulse response: the old tail does not belong to it
        }
        std::mem::swap(&mut self.paths, &mut o.paths);
        for k in 0..2 {
            self.low_cut[k].adopt(&o.low_cut[k]);
            self.high_cut[k].adopt(&o.high_cut[k]);
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
        if self.idle.idle {
            if l.iter().chain(r.iter()).all(|&x| x == 0.0) {
                return;
            }
            self.paths.iter_mut().for_each(|p| p.2.align(pos));
        }
        for k in 0..l.len() {
            let quiet = l[k] == 0.0 && r[k] == 0.0;
            let x = [l[k], r[k]];
            let mut w = [0.0f32; 2];
            for (i, o, c) in &mut self.paths {
                w[*o] += c.push(x[*i]);
            }
            let wl = self.high_cut[0].run(self.low_cut[0].run(w[0]));
            let wr = self.high_cut[1].run(self.low_cut[1].run(w[1]));
            l[k] = l[k] * self.dry + wl * self.wet;
            r[k] = r[k] * self.dry + wr * self.wet;
            if self.idle.step(pos + k as u64, quiet, wl.abs().max(wr.abs()), self.tail + 16_384) {
                self.paths.iter_mut().for_each(|p| {
                    p.2.clear();
                    p.2.align(pos + k as u64 + 1);
                });
                self.low_cut.iter_mut().chain(self.high_cut.iter_mut()).for_each(Biquad::clear);
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
