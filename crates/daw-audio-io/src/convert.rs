//! Sample conversion from the engine's 32-bit float to device formats. Portable.
//!
//! Rules (docs/04-audio-quality.md): no dither at 24 bits or more, clamp instead of wrap.
//! 16-bit output is not dithered here either. That is acceptable for monitoring, which is the
//! only use of these functions. Export has its own path with dither.

/// Device sample formats this crate can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSampleFormat {
    I16,
    /// 24 valid bits stored in a 32-bit little-endian container, left-justified.
    I24In32,
    /// 24 valid bits packed in 3 bytes.
    I24Packed,
    F32,
}

impl DeviceSampleFormat {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            DeviceSampleFormat::I16 => 2,
            DeviceSampleFormat::I24Packed => 3,
            DeviceSampleFormat::I24In32 | DeviceSampleFormat::F32 => 4,
        }
    }
}

#[inline]
fn to_i24(x: f32) -> i32 {
    // Scale by 2^23 - 1 so +1.0 maps to the largest positive value and the scale is symmetric.
    (x.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32
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
                DeviceSampleFormat::I16 => {
                    let v = (x.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
                    out.copy_from_slice(&v.to_le_bytes());
                }
                DeviceSampleFormat::I24In32 => out.copy_from_slice(&(to_i24(x) << 8).to_le_bytes()),
                DeviceSampleFormat::I24Packed => out.copy_from_slice(&to_i24(x).to_le_bytes()[..3]),
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
        assert_eq!(v, vec![32_767, -32_767, 32_767, -32_767]);
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
        let x = 1000.0 / 8_388_607.0;
        let out = conv(&[x, -x], 2, 2, DeviceSampleFormat::I24Packed);
        let decode = |b: &[u8]| (i32::from_le_bytes([b[0], b[1], b[2], 0]) << 8) >> 8;
        assert_eq!(decode(&out[0..3]), 1000);
        assert_eq!(decode(&out[3..6]), -1000);
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
