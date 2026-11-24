//! Machine-checkable tests from `docs/13-reverb-and-delay.md`, section 9. What they cannot judge
//! (does a hall sound like a hall) is listed there under "Needs Dzyan's ears".

use super::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

// ---------------------------------------------------------------- allocation counter

thread_local! { static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Counting;
// SAFETY: forwards to the system allocator; counting touches only a const thread-local Cell.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| if let Some(n) = c.get() { c.set(Some(n + 1)) });
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    ALLOCS.with(|c| c.set(Some(0)));
    f();
    ALLOCS.with(|c| c.replace(None)).unwrap_or(0)
}

// ---------------------------------------------------------------- helpers

fn values(kind: &str, set: &[(&str, f64)]) -> Values {
    Values::new(super::kind(kind).unwrap(), |n| set.iter().find(|(k, _)| *k == n).map(|(_, v)| *v))
}

fn make(kind: &str, set: &[(&str, f64)], rate: f64, ir: Option<Vec<Vec<f32>>>) -> Box<dyn Effect> {
    build(super::kind(kind).unwrap(), &values(kind, set), rate, 120.0, ir).unwrap()
}

/// Run a stereo signal through in blocks of `block`.
fn run(e: &mut dyn Effect, mut l: Vec<f32>, mut r: Vec<f32>, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut pos = 0;
    while pos < l.len() {
        let n = block.min(l.len() - pos);
        e.process(pos as u64, &mut l[pos..pos + n], &mut r[pos..pos + n]);
        pos += n;
    }
    (l, r)
}

fn noise(n: usize, seed: u64) -> Vec<f32> {
    let mut rng = math::Rng(seed);
    (0..n).map(|_| (rng.unit() * 2.0 - 1.0) as f32 * 0.5).collect()
}

fn impulse(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
}

/// One-octave band-pass (RBJ, 0 dB peak), test-only, standard-library trig.
fn band(x: &[f32], f: f64, rate: f64) -> Vec<f64> {
    let w = 2.0 * std::f64::consts::PI * f / rate;
    let alpha = w.sin() * (std::f64::consts::LN_2 / 2.0 * 1.0 * w / w.sin()).sinh();
    let a0 = 1.0 + alpha;
    let (b0, b2, a1, a2) = (alpha / a0, -alpha / a0, -2.0 * w.cos() / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    x.iter()
        .map(|&v| {
            let v = f64::from(v);
            let y = b0 * v + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, v, y1, y);
            y
        })
        .collect()
}

/// Reverberation time from the Schroeder backward integral, extrapolated from -5 to -35 dB (T30).
fn t30(x: &[f64], rate: f64) -> f64 {
    let mut edc = vec![0.0; x.len()];
    let mut acc = 0.0;
    for i in (0..x.len()).rev() {
        acc += x[i] * x[i];
        edc[i] = acc;
    }
    let db = |i: usize| 10.0 * (edc[i] / edc[0]).log10();
    let at = |level: f64| (0..x.len()).find(|&i| db(i) <= level).unwrap() as f64 / rate;
    2.0 * (at(-35.0) - at(-5.0))
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa * bb).sqrt()
}

const RATE: f64 = 48_000.0;

fn all_effects(rate: f64) -> Vec<(String, Box<dyn Effect>)> {
    let ir = vec![noise(30_000, 9).iter().enumerate().map(|(i, v)| v * (-(i as f32) / 6000.0).exp()).collect()];
    let mut out: Vec<(String, Box<dyn Effect>)> = (0..3)
        .map(|c| (format!("reverb {c}"), make("reverb", &[("character", f64::from(c)), ("mix", 50.0)], rate, None)))
        .collect();
    out.push(("irreverb".into(), make("irreverb", &[("mix", 50.0)], rate, Some(ir))));
    out.push(("delay".into(), make("delay", &[("mix", 50.0)], rate, None)));
    out.push(("echo".into(), make("echo", &[("mix", 50.0)], rate, None)));
    out
}

// ---------------------------------------------------------------- tests

#[test]
fn mix_end_points_are_exact() {
    let x = noise(10_000, 1);
    for kind in ["reverb", "delay", "echo"] {
        let mut e = make(kind, &[("mix", 0.0)], RATE, None);
        let (l, r) = run(e.as_mut(), x.clone(), x.clone(), 256);
        assert!(l == x && r == x, "{kind}: mix 0 is exactly dry");
        // Mix 100 with no pre-delay: the first output sample of an impulse has no dry part.
        let mut e = make(kind, &[("mix", 100.0), ("predelay", 0.0)], RATE, None);
        let (l, _) = run(e.as_mut(), impulse(100), vec![0.0; 100], 100);
        assert!(l[0].abs() < 0.5, "{kind}: mix 100 removes the dry signal (got {})", l[0]);
    }
}

#[test]
fn process_never_allocates() {
    for (name, mut e) in all_effects(RATE) {
        let (mut l, mut r) = (noise(4096, 2), noise(4096, 3));
        // Long enough for every convolution level, the largest block being 16,384 samples.
        let n = allocations(|| {
            for k in 0..12u64 {
                e.process(k * 4096, &mut l, &mut r);
            }
        });
        assert_eq!(n, 0, "{name} allocated {n} times while processing");
    }
    assert!(allocations(|| drop(std::hint::black_box(vec![0u8; 8]))) > 0, "the counter works");
}

#[test]
fn finite_bounded_and_silent_after_the_tail() {
    for (name, mut e) in all_effects(RATE) {
        let n = RATE as usize * 4;
        let (l, r) = run(e.as_mut(), noise(n, 4).iter().map(|v| v * 2.0).collect(), noise(n, 5).iter().map(|v| v * 2.0).collect(), 512);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()), "{name}: finite");
        // -90 dB by the reported tail; exactly zero once under -150 dB, about 1.7 tails later.
        let tail = e.tail() as usize * 3 + 40_000;
        let (l, r) = run(e.as_mut(), vec![0.0; tail], vec![0.0; tail], 512);
        assert!(l[tail - 2048..].iter().chain(&r[tail - 2048..]).all(|&v| v == 0.0), "{name}: exactly silent after its reported tail");
    }
    // A pushed tape echo saturates instead of running away.
    let mut e = make("echo", &[("feedback", 110.0), ("wear", 100.0), ("mix", 100.0), ("sync", 5.0)], RATE, None);
    let x: Vec<f32> = noise(RATE as usize * 6, 6).iter().map(|v| v * 2.0).collect();
    let (l, _) = run(e.as_mut(), x.clone(), x, 512);
    let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak < 2.0, "tape echo at 110% stays under +6 dBFS (peak {peak})");
}

#[test]
fn same_output_whatever_the_block_size() {
    // A burst, then long enough silence for every effect to go idle: the moment an effect
    // stops working must not depend on the block size either (playback uses small blocks, an
    // export large ones, and the two must match bit for bit).
    let mut x = noise(30_000, 7);
    x.extend(std::iter::repeat_n(0.0, 400_000));
    x.extend(noise(2_000, 11));
    for kind in ["reverb", "echo", "irreverb"] {
        let ir = (kind == "irreverb").then(|| vec![noise(20_000, 8)]);
        let renders: Vec<_> = [1, 64, 256, 480, 4096]
            .iter()
            .map(|&b| run(make(kind, &[("mix", 60.0)], RATE, ir.clone()).as_mut(), x.clone(), x.clone(), b))
            .collect();
        for (n, w) in renders.windows(2).enumerate() {
            if let Some(i) = (0..x.len()).find(|&i| w[0].0[i] != w[1].0[i] || w[0].1[i] != w[1].1[i]) {
                panic!("{kind}: block sizes #{n} and #{} differ first at sample {i}: {:e} vs {:e}", n + 1, w[0].0[i], w[1].0[i]);
            }
        }
    }
}

#[test]
fn reverb_decay_matches_the_controls_per_band() {
    for rate in [48_000.0, 96_000.0] {
        let set = [("mix", 100.0), ("predelay", 0.0), ("lowcut", 0.0), ("decay", 2.0), ("damping", 50.0), ("size", 60.0)];
        for c in 0..3 {
            let mut set = set.to_vec();
            set.push(("character", f64::from(c)));
            let mut e = make("reverb", &set, rate, None);
            let n = (rate * 4.0) as usize;
            let (l, _) = run(e.as_mut(), impulse(n), vec![0.0; n], 1024);
            let low = t30(&band(&l, 500.0, rate), rate);
            let mid = t30(&band(&l, 1000.0, rate), rate);
            let high = t30(&band(&l, 8000.0, rate), rate);
            assert!((low / 2.0 - 1.0).abs() < 0.12, "character {c} at {rate}: T30 at 500 Hz {low:.2} s, set 2.0");
            assert!((mid / 2.0 - 1.0).abs() < 0.12, "character {c} at {rate}: T30 at 1 kHz {mid:.2} s, set 2.0");
            assert!((high / 1.0 - 1.0).abs() < 0.25, "character {c} at {rate}: T30 at 8 kHz {high:.2} s, set 1.0");
        }
    }
}

#[test]
fn decay_is_limited_by_size() {
    for c in 0..3 {
        for size in [0.0, 0.5, 1.0] {
            let max = reverb::max_decay(c, size);
            assert!(max > 0.5, "character {c} size {size}: max decay {max:.2}");
        }
        assert!(reverb::max_decay(c, 1.0) > reverb::max_decay(c, 0.0));
    }
    assert!(reverb::max_decay(1, 1.0) >= 10.0, "a full-size hall supports 10 s");
}

#[test]
fn reverb_tail_is_wide_and_level_steady() {
    let n = RATE as usize * 3;
    let x = noise(n, 11);
    let mut levels = Vec::new();
    for c in 0..3 {
        for decay in [0.3, 2.0, 8.0] {
            let mut e = make("reverb", &[("character", f64::from(c)), ("mix", 100.0), ("decay", decay), ("size", 100.0), ("lowcut", 0.0)], RATE, None);
            let (l, r) = run(e.as_mut(), x.clone(), x.clone(), 512);
            let (l, r) = (&l[n / 2..], &r[n / 2..]);
            let rho = correlation(l, r);
            assert!(rho.abs() < 0.3, "character {c} decay {decay}: left/right correlation {rho:.2}");
            levels.push(20.0 * (rms(l) / rms(&x)).log10());
        }
    }
    let (lo, hi) = levels.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!(hi - lo < 6.0 && hi.abs() < 6.0 && lo.abs() < 6.0, "wet level within +-3 dB of a common level: {levels:?}");
}

#[test]
fn convolution_is_exact_and_has_no_latency() {
    // Against a 64-bit direct convolution, with an IR long enough to use every level.
    let ir = noise(20_000, 12);
    let x = noise(24_000, 13);
    let set = [("mix", 100.0), ("normalise", 0.0)];
    let mut e = make("irreverb", &set, RATE, Some(vec![ir.clone()]));
    let (y, _) = run(e.as_mut(), x.clone(), x.clone(), 333);
    let mut worst = 0.0f64;
    let mut peak = 0.0f64;
    for n in (0..x.len()).step_by(7) {
        let direct: f64 = (0..=n.min(ir.len() - 1)).map(|k| f64::from(ir[k]) * f64::from(x[n - k])).sum();
        worst = worst.max((direct - f64::from(y[n])).abs());
        peak = peak.max(direct.abs());
    }
    assert!(worst / peak < 1e-5, "convolution error {:.1} dB of peak", 20.0 * (worst / peak).log10());

    // A single impulse at sample k comes out at exactly k, whatever the host buffer.
    for k in [0usize, 63, 64, 1000, 17_000] {
        let mut ir = vec![0.0; k + 1];
        ir[k] = 1.0;
        for block in [1, 32, 128, 512] {
            let mut e = make("irreverb", &set, RATE, Some(vec![ir.clone()]));
            let (y, _) = run(e.as_mut(), impulse(k + 2000), vec![0.0; k + 2000], block);
            let at = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
            assert_eq!(at, k, "impulse at {k}, buffer {block}");
            assert!((y[k] - 1.0).abs() < 1e-6);
        }
    }
}

#[test]
fn true_stereo_routes_each_path() {
    let one = |c: usize| {
        let mut ch = vec![vec![0.0f32; 100]; 4];
        ch[c][10] = 1.0;
        ch
    };
    let set = [("mix", 100.0), ("normalise", 0.0)];
    // Channel order: LL, LR, RL, RR. Feed the left input only.
    for (c, expect_left, expect_right) in [(0, true, false), (1, false, true), (2, false, false), (3, false, false)] {
        let mut e = make("irreverb", &set, RATE, Some(one(c)));
        let (l, r) = run(e.as_mut(), impulse(200), vec![0.0; 200], 64);
        let has = |x: &[f32]| x.iter().any(|v| v.abs() > 1e-6);
        assert_eq!((has(&l), has(&r)), (expect_left, expect_right), "path {c}");
    }
}

#[test]
fn delay_lands_on_the_sample_and_repeats_fall_by_the_feedback() {
    for (rate, at) in [(44_100.0, 22_050), (48_000.0, 24_000), (96_000.0, 48_000)] {
        let mut e = make("delay", &[("mix", 100.0), ("sync", 3.0), ("feedback", 0.0)], rate, None);
        let (l, _) = run(e.as_mut(), impulse(at + 100), vec![0.0; at + 100], 512);
        let first = l.iter().position(|v| v.abs() > 0.1).unwrap();
        assert_eq!(first, at, "a quarter note at 120 BPM and {rate} Hz");
    }
    let fb = 0.5;
    let mut e = make("delay", &[("mix", 100.0), ("sync", 0.0), ("time", 100.0), ("feedback", fb * 100.0), ("tone", 20_000.0)], RATE, None);
    let n = 4800 * 6;
    let (l, _) = run(e.as_mut(), impulse(n), vec![0.0; n], 512);
    let peaks: Vec<f64> = (1..5).map(|k| f64::from(l[k * 4800..k * 4800 + 400].iter().fold(0.0f32, |m, v| m.max(v.abs())))).collect();
    for w in peaks.windows(2) {
        let step = 20.0 * (w[1] / w[0]).log10();
        let want = 20.0 * fb.log10();
        assert!((step - want).abs() < 0.15, "repeat falls {step:.2} dB, feedback says {want:.2}");
    }
}

#[test]
fn ping_pong_alternates() {
    let mut e = make("delay", &[("mix", 100.0), ("sync", 0.0), ("time", 100.0), ("feedback", 50.0), ("pingpong", 1.0)], RATE, None);
    let n = 4800 * 4;
    let (l, r) = run(e.as_mut(), impulse(n), impulse(n), 512);
    let loud = |x: &[f32], k: usize| x[k * 4800..k * 4800 + 200].iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.05;
    assert!(loud(&l, 1) && !loud(&r, 1), "first repeat left");
    assert!(loud(&r, 2) && !loud(&l, 2), "second repeat right");
    assert!(loud(&l, 3) && !loud(&r, 3), "third repeat left");
}

#[test]
fn modulation_stays_below_two_cents() {
    // Pitch deviation of a moving delay read: 2 pi x rate x depth. Fastest reverb line: 0.53 Hz.
    for depth_ms in [0.2, 0.3, 0.25] {
        let cents = 1200.0 * (1.0 + 2.0 * std::f64::consts::PI * 0.53 * depth_ms / 1000.0).log2();
        assert!(cents <= 2.0, "{depth_ms} ms at 0.53 Hz: {cents:.2} cents");
    }
}

#[test]
fn parameters_parse_by_name_and_refuse_nonsense() {
    let k = kind("reverb").unwrap();
    let p = |n: &str| k.params.iter().find(|p| p.name == n).unwrap();
    assert_eq!(p("character").parse("plate"), Ok(2.0));
    assert_eq!(p("decay").parse("2.5 s"), Ok(2.5));
    assert!(p("decay").parse("40").is_err());
    assert!(p("character").parse("cathedral").is_err());
    assert_eq!(kind("irreverb").unwrap().params[0].parse("off"), Ok(0.0));
}

/// CPU per effect (doc 13, test 25). Release only: `cargo test --release -p crucible-dsp -- --ignored --nocapture`
#[test]
#[ignore]
fn cpu_per_effect() {
    let secs = 20.0;
    let n = (RATE * secs) as usize;
    let x = noise(n, 99);
    let ir3s = vec![noise((RATE * 3.0) as usize, 5), noise((RATE * 3.0) as usize, 6), noise((RATE * 3.0) as usize, 7), noise((RATE * 3.0) as usize, 8)];
    let cases: Vec<(&str, Box<dyn Effect>)> = vec![
        ("reverb hall", make("reverb", &[("mix", 50.0)], RATE, None)),
        ("reverb plate", make("reverb", &[("mix", 50.0), ("character", 2.0)], RATE, None)),
        ("delay clean", make("delay", &[("mix", 50.0)], RATE, None)),
        ("echo tape", make("echo", &[("mix", 50.0)], RATE, None)),
        ("ir reverb 3 s stereo", make("irreverb", &[("mix", 50.0)], RATE, Some(ir3s[..2].to_vec()))),
        ("ir reverb 3 s true stereo", make("irreverb", &[("mix", 50.0)], RATE, Some(ir3s.clone()))),
    ];
    for (name, mut e) in cases {
        let (mut l, mut r) = (x.clone(), x.clone());
        let t0 = std::time::Instant::now();
        let mut worst = 0.0f64;
        for (k, (cl, cr)) in l.chunks_mut(128).zip(r.chunks_mut(128)).enumerate() {
            let b0 = std::time::Instant::now();
            e.process((k * 128) as u64, cl, cr);
            worst = worst.max(b0.elapsed().as_secs_f64());
        }
        let used = t0.elapsed().as_secs_f64() / secs * 100.0;
        println!("{name:28} {used:6.2}% of one core, worst 128-frame callback {:5.1}% of its 2.67 ms", worst / (128.0 / RATE) * 100.0);
    }
}

/// Live playback rebuilds an effect when a setting changes and hands the old one's sound to the
/// new one. With the same settings the hand-over must be invisible: bit-identical to never having
/// swapped. With another Mix the tail must still be there, and the swap must not allocate.
#[test]
fn a_settings_change_keeps_the_tail() {
    let rate = 48_000.0;
    let ir = || Some(vec![noise(30_000, 9).iter().enumerate().map(|(i, x)| x * (-(i as f32) / 6_000.0).exp()).collect::<Vec<f32>>()]);
    for (kind, set, other) in [
        ("reverb", vec![("decay", 2.0), ("mix", 40.0)], ("mix", 60.0)),
        ("echo", vec![("feedback", 60.0), ("mix", 40.0)], ("mix", 60.0)),
        ("delay", vec![("feedback", 60.0), ("mix", 40.0)], ("tone", 4000.0)),
        ("irreverb", vec![("mix", 40.0)], ("mix", 60.0)),
    ] {
        let burst = noise(4_800, 3);
        let quiet = vec![0.0f32; 24_000];
        let input: Vec<f32> = burst.iter().chain(&quiet).copied().collect();
        // Reference: one instance, never swapped.
        let mut whole = make(kind, &set, rate, ir());
        let (want, _) = run(&mut *whole, input.clone(), input.clone(), 256);
        // Swapped after the burst, same settings.
        let mut a = make(kind, &set, rate, ir());
        let (head, _) = run(&mut *a, burst.clone(), burst.clone(), 256);
        let mut b = make(kind, &set, rate, ir());
        let n = allocations(|| b.adopt(&mut *a));
        assert_eq!(n, 0, "{kind}: adopt allocated");
        let (mut l, mut r) = (quiet.clone(), quiet.clone());
        for (k, (cl, cr)) in l.chunks_mut(256).zip(r.chunks_mut(256)).enumerate() {
            b.process((4_800 + k * 256) as u64, cl, cr);
        }
        let got: Vec<f32> = head.iter().chain(&l).copied().collect();
        assert!(got == want, "{kind}: hand-over with the same settings changed the sound");
        // Swapped with another setting: the tail carries on instead of starting from silence.
        let mut a = make(kind, &set, rate, ir());
        run(&mut *a, burst.clone(), burst.clone(), 256);
        let mut changed = set.clone();
        changed.push(other);
        let mut c = make(kind, &changed, rate, ir());
        c.adopt(&mut *a);
        let (mut l, mut r) = (vec![0.0f32; 36_000], vec![0.0f32; 36_000]); // past the first repeat
        c.process(4_800, &mut l, &mut r);
        assert!(rms(&l) > 1e-4, "{kind}: the tail was lost when {} changed (rms {})", other.0, rms(&l));
    }
}
