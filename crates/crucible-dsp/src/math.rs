//! Deterministic maths for coefficients and oscillators.
//!
//! The standard library's `sin`, `exp` and `powf` call the platform's maths library, which does
//! not promise identical results everywhere and on Windows may pick a different code path per CPU
//! (`docs/13-reverb-and-delay.md`, Platform notes). These versions use only `+ - * /`, `floor`,
//! `round`, `sqrt` and bit operations, which IEEE 754 defines exactly, so one project renders the
//! same bits on every machine (priority 5). Accuracy is about 1e-15 relative, far below what a
//! 32-bit float sample can show.

pub const PI: f64 = std::f64::consts::PI;
const LN2: f64 = std::f64::consts::LN_2;
const LN10: f64 = std::f64::consts::LN_10;
// ln 2 split so k * LN2_HI is exact for the k used here (Cody and Waite).
const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000); // fdlibm's split of ln 2
const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);

/// e^x.
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -745.0 {
        return 0.0;
    }
    let k = (x / LN2).round();
    let r = x - k * LN2_HI - k * LN2_LO; // |r| <= ln2 / 2
    let mut p = 1.0;
    for n in (1..=14).rev() {
        p = 1.0 + r * p / f64::from(n);
    }
    scale2(p, k as i32)
}

/// x * 2^k for the k `exp` produces, including results that fall into the subnormal range.
fn scale2(x: f64, k: i32) -> f64 {
    let pow2 = |e: i32| f64::from_bits(((e + 1023) as u64) << 52);
    if k > 1023 {
        x * pow2(1023) * pow2(k - 1023)
    } else if k < -1022 {
        x * pow2(-1022) * pow2(k + 1022)
    } else {
        x * pow2(k)
    }
}

/// Natural logarithm of a positive, normal number.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000); // [1, 2)
    if m > std::f64::consts::SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    // ln m = 2 atanh z, z = (m - 1) / (m + 1), |z| <= 0.172
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let (mut sum, mut term) = (0.0, z);
    for k in 0..14 {
        sum += term / f64::from(2 * k + 1);
        term *= z2;
    }
    2.0 * sum + e as f64 * LN2
}

/// 10^(db/20): the linear gain of a level in decibels. 0 dB is exactly 1.0.
pub fn db_to_gain(db: f64) -> f64 {
    if db == 0.0 { 1.0 } else { exp(db * LN10 / 20.0) }
}

/// 20 log10(g).
pub fn gain_to_db(g: f64) -> f64 {
    20.0 * ln(g) / LN10
}

/// sin(2 pi t): `t` in turns (cycles), so oscillator phases never need a large-angle reduction.
pub fn sin_turns(t: f64) -> f64 {
    let mut u = t - t.floor(); // [0, 1)
    if u >= 0.5 {
        u -= 1.0; // [-0.5, 0.5)
    }
    // Fold into [-0.25, 0.25]: sin(pi - x) = sin(x).
    if u > 0.25 {
        u = 0.5 - u;
    } else if u < -0.25 {
        u = -0.5 - u;
    }
    let x = 2.0 * PI * u; // [-pi/2, pi/2]
    let x2 = x * x;
    let (mut sum, mut term) = (0.0, x);
    for k in 0..12 {
        sum += term;
        term *= -x2 / f64::from((2 * k + 2) * (2 * k + 3));
    }
    sum
}

/// sin(2 pi t) to about 1e-7: enough for slow modulation, at half the cost of `sin_turns`.
/// Same exact operations on every machine.
#[inline]
pub fn lfo(t: f64) -> f64 {
    let mut u = t - t.floor();
    if u >= 0.5 {
        u -= 1.0;
    }
    if u > 0.25 {
        u = 0.5 - u;
    } else if u < -0.25 {
        u = -0.5 - u;
    }
    let x = 2.0 * PI * u;
    let x2 = x * x;
    // Taylor series to x^13, Horner form.
    x * (1.0 + x2 * (-1.0 / 6.0 + x2 * (1.0 / 120.0 + x2 * (-1.0 / 5040.0 + x2 * (1.0 / 362_880.0 + x2 * (-1.0 / 39_916_800.0 + x2 / 6_227_020_800.0))))))
}

/// cos(2 pi t).
pub fn cos_turns(t: f64) -> f64 {
    sin_turns(t + 0.25)
}

/// tan(pi * f / rate): the pre-warped frequency of the bilinear transform.
pub fn prewarp(f: f64, rate: f64) -> f64 {
    let t = f / (2.0 * rate);
    sin_turns(t) / cos_turns(t)
}

/// Equal-power mix gains (dry, wet) for a mix of 0..1, with exact end points: 0 is exactly dry,
/// 1 has exactly no dry signal.
pub fn mix_gains(mix: f64) -> (f32, f32) {
    if mix <= 0.0 {
        (1.0, 0.0)
    } else if mix >= 1.0 {
        (0.0, 1.0)
    } else {
        (cos_turns(mix / 4.0) as f32, sin_turns(mix / 4.0) as f32)
    }
}

/// Smallest prime at or above n.
pub fn next_prime(n: usize) -> usize {
    let is_prime = |p: usize| p >= 2 && (2..).take_while(|d| d * d <= p).all(|d| !p.is_multiple_of(d));
    (n.max(2)..).find(|&p| is_prime(p)).expect("primes are unbounded")
}

/// A small seeded random source, so every reverb tank is built the same way on every machine.
pub struct Rng(pub u64);

impl Rng {
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_library_closely() {
        for i in -400..=400 {
            let x = f64::from(i) * 0.137;
            assert!((exp(x) / x.exp() - 1.0).abs() < 1e-14, "exp {x}");
        }
        for i in 1..=2000 {
            let x = f64::from(i) * 0.731;
            assert!((ln(x) - x.ln()).abs() < 1e-13, "ln {x}");
        }
        for i in -1000..=1000 {
            let t = f64::from(i) * 0.00731;
            assert!((lfo(t) - (2.0 * PI * t).sin()).abs() < 1e-7, "lfo {t}");
            assert!((sin_turns(t) - (2.0 * PI * t).sin()).abs() < 1e-14, "sin {t}");
            assert!((cos_turns(t) - (2.0 * PI * t).cos()).abs() < 1e-14, "cos {t}");
        }
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((db_to_gain(-6.0) - 10f64.powf(-0.3)).abs() < 1e-15);
        assert_eq!(mix_gains(0.0), (1.0, 0.0));
        assert_eq!(mix_gains(1.0), (0.0, 1.0));
        assert_eq!(next_prime(1000), 1009);
    }
}
