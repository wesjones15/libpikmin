//! JAudio's numeric helpers: the output rate, pitch tables, the pan curve and the RNG.

/// `JAC_DAC_RATE` (rate.c): the DSP's output rate.
pub const DAC_RATE: f32 = 32028.5;
/// Samples per DSP subframe. `UpdateDSP` (dspbuf.c) runs the sequencers and channel updates
/// once per subframe.
pub const SUBFRAME: usize = 80;
/// Subframes per second.
pub const SUBFRAME_RATE: f32 = DAC_RATE / SUBFRAME as f32;

/// `C5BASE_PITCHTABLE` (tables.c): equal temperament around key 60.
pub fn c5base(key: i32) -> f32 {
    2f32.powf((key.clamp(0, 127) - 60) as f32 / 12.0)
}

/// `Jam_PitchToCent` (centcalc.c): a track's pitch parameter (scaled by `pitchScale`, in
/// quarter units) to a frequency ratio. `KEY_TABLE` is 2^(n/64/12).
pub fn pitch_to_cent(pitch: f32, scale: f32) -> f32 {
    let scaled = 4.0 * pitch * scale;
    let mut index = scaled as i32;
    let mut frac = scaled - index as f32;
    if scaled < 0.0 && frac != 0.0 {
        frac += 1.0;
        index -= 1;
    }
    if frac >= 1.0 {
        frac -= 1.0;
        index += 1;
    }
    let fine = ((frac * 64.0) as u32).min(63);
    c5base(index + 60) * 2f32.powf(fine as f32 / 64.0 / 12.0)
}

/// `sinf3` (ja_calc.c): a 256-step quarter sine for equal-power panning.
pub fn sinf3(x: f32) -> f32 {
    let i = ((256.0 * x) as i32).clamp(0, 256);
    (i as f32 / 256.0 * std::f32::consts::FRAC_PI_2).sin()
}

/// `GetRandom_s32` (random.c).
#[derive(Debug, Clone)]
pub struct Random {
    v0: i32,
    v1: i32,
}

impl Default for Random {
    fn default() -> Self {
        Random {
            v0: 0x1000,
            v1: 0x5555,
        }
    }
}

impl Random {
    pub fn s32(&mut self) -> i32 {
        let t = self
            .v1
            .wrapping_mul(0x98765432u32 as i32)
            .wrapping_add(self.v0.wrapping_mul(0x13579BDE) >> 4);
        self.v0 = self.v1;
        self.v1 = t.wrapping_add(1);
        self.v1
    }

    pub fn u32(&mut self) -> u32 {
        (self.s32() as u32) & !0x8000_0000
    }

    /// `GetRandom_ulimit`
    pub fn ulimit(&mut self, limit: u32) -> u32 {
        self.u32() % limit.max(1)
    }

    /// `GetRandom_sf32`: -1..1.
    pub fn sf32(&mut self) -> f32 {
        let r = self.s32();
        let f = (r & 0xFF_FFFF) as f32 / 16_777_215.0;
        if r < 0 { -f } else { f }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_table_matches_semitones() {
        assert_eq!(c5base(60), 1.0);
        assert!((c5base(72) - 2.0).abs() < 1e-6);
        assert!((pitch_to_cent(0.0, 12.0) - 1.0).abs() < 1e-6);
        // 1/48 of a full-scale bend at scale 12 is one semitone.
        assert!((pitch_to_cent(1.0 / 48.0, 12.0) - c5base(61)).abs() < 1e-5);
    }

    #[test]
    fn pan_curve_is_quarter_sine() {
        assert_eq!(sinf3(0.0), 0.0);
        assert!((sinf3(1.0) - 1.0).abs() < 1e-6);
        assert!((sinf3(0.5) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    }
}
