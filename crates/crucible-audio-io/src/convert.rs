//! Sample conversion from the engine's 32-bit float to device formats. Portable.
//!
//! Rules (docs/04-audio-quality.md): no dither at 24 bits or more, clamp instead of wrap.
//! 16-bit output is not dithered here either. That is acceptable for monitoring, which is the
//! only use of these functions. Export has its own path with dither.
//!
//! Scale: a float is multiplied by 2^(bits-1), the same power of two import divides by and
//! export multiplies by. So a 16 or 24-bit file played at unity gain reaches the device with
//! exactly the bits it has on disk. Until 2026-11-23 this used 2^(bits-1) - 1, which changed
//! the last bit of loud samples. +1.0 itself has no integer and clamps one step lower.

/// Device sample formats this crate can write. All little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSampleFormat {
    I16,
    /// 24 valid bits stored in a 32-bit container, left-justified.
    I24In32,
    /// 24 valid bits stored in a 32-bit container, right-justified (cpal's I24).
    I24Low,
    /// 24 valid bits packed in 3 bytes.
    I24Packed,
    /// 32-bit integers. A float carries 24 bits, so the low byte is zero.
    I32,
    F32,
    F64,
}

impl DeviceSampleFormat {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            DeviceSampleFormat::I16 => 2,
            DeviceSampleFormat::I24Packed => 3,
            DeviceSampleFormat::I24In32 | DeviceSampleFormat::I24Low | DeviceSampleFormat::I32 | DeviceSampleFormat::F32 => 4,
            DeviceSampleFormat::F64 => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DeviceSampleFormat::I16 => "16-bit int",
            DeviceSampleFormat::I24In32 | DeviceSampleFormat::I24Low => "24-bit int in 32",
            DeviceSampleFormat::I24Packed => "24-bit int packed",
            DeviceSampleFormat::I32 => "32-bit int",
            DeviceSampleFormat::F32 => "32-bit float",
            DeviceSampleFormat::F64 => "64-bit float",
        }
    }
}

#[inline]
fn to_i24(x: f32) -> i32 {
    (x * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32
}

/// Convert interleaved `src` (source channel count `src_ch`) into the device's interleaved
/// byte layout with `dev_ch` channels. Extra device channels get silence; extra source
/// channels are dropped. `dst` must hold exactly `frames * dev_ch * bytes_per_sample` bytes.
///
/// Allocation-free: safe to call from the audio thread.
pub fn write_interleaved(src: &[f32], src_ch: usize, dst: &mut [u8], dev_ch: usize, fmt: DeviceSampleFormat) {
    let bps = fmt.bytes_per_sample();
    let frames = dst.len() / (dev_ch * bps);
    for f in 0..frames {
        for c in 0..dev_ch {
            let x = if c < src_ch { src.get(f * src_ch + c).copied().unwrap_or(0.0) } else { 0.0 };
            let x = if x.is_finite() { x } else { 0.0 };
            let at = (f * dev_ch + c) * bps;
            let out = &mut dst[at..at + bps];
            match fmt {
                DeviceSampleFormat::F32 => out.copy_from_slice(&x.to_le_bytes()),
                DeviceSampleFormat::F64 => out.copy_from_slice(&f64::from(x).to_le_bytes()),
                DeviceSampleFormat::I16 => {
                    let v = (x * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16;
                    out.copy_from_slice(&v.to_le_bytes());
                }
                DeviceSampleFormat::I24In32 => out.copy_from_slice(&(to_i24(x) << 8).to_le_bytes()),
                DeviceSampleFormat::I24Low => out.copy_from_slice(&to_i24(x).to_le_bytes()),
                DeviceSampleFormat::I24Packed => out.copy_from_slice(&to_i24(x).to_le_bytes()[..3]),
                DeviceSampleFormat::I32 => {
                    let v = (f64::from(x) * 2_147_483_648.0).round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                    out.copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(src: &[f32], src_ch: usize, dev_ch: usize, fmt: DeviceSampleFormat) -> Vec<u8> {
        let frames = src.len() / src_ch;
        let mut dst = vec![0xAAu8; frames * dev_ch * fmt.bytes_per_sample()];
        write_interleaved(src, src_ch, &mut dst, dev_ch, fmt);
        dst
    }

    #[test]
    fn float_passes_through_bit_exact() {
        let src = [0.123_456_78f32, -0.987_654_3];
        let out = conv(&src, 2, 2, DeviceSampleFormat::F32);
        assert_eq!(f32::from_le_bytes(out[0..4].try_into().unwrap()), src[0]);
        assert_eq!(f32::from_le_bytes(out[4..8].try_into().unwrap()), src[1]);
    }

    #[test]
    fn i16_full_scale_and_clamp() {
        let out = conv(&[1.0, -1.0, 2.0, -2.0], 2, 2, DeviceSampleFormat::I16);
        let v: Vec<i16> = out.chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        assert_eq!(v, vec![32_767, -32_768, 32_767, -32_768]);
    }

    #[test]
    fn i24_in_32_is_left_justified() {
        let out = conv(&[1.0, 0.0], 2, 2, DeviceSampleFormat::I24In32);
        let v = i32::from_le_bytes(out[0..4].try_into().unwrap());
        assert_eq!(v, 8_388_607 << 8);
        assert_eq!(v & 0xFF, 0, "low byte must be zero padding");
        assert_eq!(i32::from_le_bytes(out[4..8].try_into().unwrap()), 0);
    }

    #[test]
    fn i24_packed_round_trips_small_values() {
        let x = 1000.0 / 8_388_608.0;
        let out = conv(&[x, -x], 2, 2, DeviceSampleFormat::I24Packed);
        let decode = |b: &[u8]| (i32::from_le_bytes([b[0], b[1], b[2], 0]) << 8) >> 8;
        assert_eq!(decode(&out[0..3]), 1000);
        assert_eq!(decode(&out[3..6]), -1000);
    }

    /// What import produces for every 16-bit value (v / 32768) must come back as the same value,
    /// and likewise for 24-bit: playback at unity gain is bit-exact (priority 2).
    #[test]
    fn integers_from_files_come_back_exactly() {
        for v in i16::MIN..=i16::MAX {
            let out = conv(&[f32::from(v) / 32_768.0], 1, 1, DeviceSampleFormat::I16);
            assert_eq!(i16::from_le_bytes([out[0], out[1]]), v);
        }
        for v in (-8_388_608i32..=8_388_607).step_by(127).chain([8_388_607, 8_388_606, -8_388_608]) {
            let x = v as f32 / 8_388_608.0;
            let low = conv(&[x], 1, 1, DeviceSampleFormat::I24Low);
            assert_eq!(i32::from_le_bytes(low[..].try_into().unwrap()), v);
            let wide = conv(&[x], 1, 1, DeviceSampleFormat::I32);
            assert_eq!(i32::from_le_bytes(wide[..].try_into().unwrap()), v << 8);
        }
    }

    #[test]
    fn stereo_into_four_channel_device_pads_with_silence() {
        let out = conv(&[0.5, -0.5], 2, 4, DeviceSampleFormat::F32);
        let v: Vec<f32> = out.chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
        assert_eq!(v, vec![0.5, -0.5, 0.0, 0.0]);
    }

    #[test]
    fn non_finite_becomes_silence() {
        let out = conv(&[f32::NAN, f32::INFINITY], 2, 2, DeviceSampleFormat::I16);
        assert!(out.iter().all(|&b| b == 0));
    }
}
