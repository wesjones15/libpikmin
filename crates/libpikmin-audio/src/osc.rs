//! Envelopes and LFOs (`Osc_`, `Oscbuf_`): `Bank_OscToOfs` (bankdrv.c) and the track presets
//! in jamosc.c.
//!
//! A table is (curve, time, value) triplets: move to `value / 32768` over `time` (in 1/600 s
//! steps scaled by the oscillator's rate), with curve 0 linear, 1 squared, 2 square-rooted.
//! 0xD jumps to entry `value`, 0xE holds (sustain), 0xF ends the note.

use std::sync::Arc;

use libpikmin_assets::sound::bank;

use crate::calc::SUBFRAME_RATE;

pub type Table = Arc<[[i16; 3]]>;

/// `Osc_`
#[derive(Debug, Clone)]
pub struct Osc {
    /// 0 volume, 1 pitch, 2 pan, 3 fx send, 4 dolby (`DoEffectOsc`, driverinterface.c).
    pub mode: u8,
    pub rate: f32,
    pub attack: Option<Table>,
    pub release: Option<Table>,
    pub width: f32,
    pub vertex: f32,
}

impl Osc {
    pub fn from_bank(o: &bank::Osc) -> Osc {
        Osc {
            mode: o.mode,
            rate: o.rate,
            attack: o.attack.as_ref().map(|t| t.as_slice().into()),
            release: o.release.as_ref().map(|t| t.as_slice().into()),
            width: o.width,
            vertex: o.vertex,
        }
    }

    fn same_tables(&self) -> bool {
        match (&self.attack, &self.release) {
            (Some(a), Some(r)) => Arc::ptr_eq(a, r),
            (None, None) => true,
            _ => false,
        }
    }
}

/// `Oscbuf_` states: 0 done, 1 start, 2 running, 3 sustain, 4 release requested, 5 release
/// table, 6 force-release requested, 7 force release, 8 timed release fade.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OscBuf {
    pub state: u8,
    pub curve: u8,
    pub index: u16,
    pub time: f32,
    pub value: f32,
    pub target: f32,
    pub delta: f32,
    pub release_param: u16,
}

/// Table time units to subframes: `x * (JAC_DAC_RATE / 80) / 600`.
fn to_subframes(t: f32) -> f32 {
    t * SUBFRAME_RATE / 600.0
}

/// `FORCE_RELEASE_TABLE` read as triplets: fade out over 5 units, then stop.
fn force_release() -> Table {
    Arc::from(vec![[0, 5, 0], [15, 0, 0]])
}

/// `Bank_OscToOfs`: advances the envelope one subframe and returns its output
/// (`value * width + vertex`).
pub fn osc_to_ofs(osc: Option<&Osc>, buf: &mut OscBuf) -> f32 {
    let Some(osc) = osc else {
        buf.value = 1.0;
        return 1.0;
    };
    if buf.state == 4 {
        if !osc.same_tables() {
            buf.index = 0;
            buf.time = 0.0;
            buf.target = buf.value;
        }
        if osc.release.is_none() && buf.release_param == 0 {
            buf.release_param = 0x10;
        }
        if buf.release_param != 0 {
            buf.state = 8;
            buf.curve = ((buf.release_param >> 14) & 3) as u8;
            buf.time = to_subframes((buf.release_param & 0x3FFF) as f32).max(1.0);
            buf.target = 0.0;
            buf.delta = (buf.target - buf.value) / buf.time;
        } else {
            buf.state = 5;
        }
    }
    if buf.state == 6 {
        buf.index = 0;
        buf.time = 0.0;
        buf.target = buf.value;
        buf.state = 7;
    }
    let table = match buf.state {
        5 => osc.release.clone(),
        7 => Some(force_release()),
        _ => osc.attack.clone(),
    };
    if table.is_none() && buf.state != 8 {
        buf.value = 1.0;
        return 1.0;
    }
    match buf.state {
        0 => return 1.0,
        3 => return buf.value * osc.width + osc.vertex,
        1 => {
            buf.state = 2;
            buf.index = 0;
            buf.time = 0.0;
            buf.target = 0.0;
            buf.release_param = 0;
        }
        _ => {}
    }
    buf.time -= if buf.state == 7 { 1.0 } else { osc.rate };
    // Guard against tables that jump in a loop without ever waiting.
    let mut steps = 0;
    while buf.time <= 0.0 {
        buf.value = buf.target;
        if buf.state == 8 {
            buf.state = 0;
            break;
        }
        steps += 1;
        let entry = table
            .as_ref()
            .and_then(|t| t.get(buf.index as usize))
            .copied()
            .filter(|_| steps < 256)
            .unwrap_or([0xF, 0, 0]);
        match entry[0] {
            0xD => {
                buf.index = entry[2] as u16;
                continue;
            }
            0xF => {
                buf.state = 0;
                break;
            }
            0xE => {
                buf.state = 3;
                return buf.value * osc.width + osc.vertex;
            }
            _ => {}
        }
        buf.curve = entry[0] as u8;
        buf.target = entry[2] as f32 / 32768.0;
        buf.index += 1;
        if entry[1] != 0 {
            buf.time = to_subframes(entry[1] as u16 as f32);
            buf.delta = (buf.target - buf.value) / buf.time;
        }
    }
    let v = -(buf.delta * buf.time - buf.target);
    buf.value = v;
    let shaped = match buf.curve {
        1 => v * v.abs(),
        2 => v.signum() * v.abs().sqrt(),
        _ => v,
    };
    shaped * osc.width + osc.vertex
}

/// Static presets from jamosc.c and oneshot.c.
pub mod presets {
    use super::*;

    fn t(v: &[i16]) -> Table {
        v.chunks(3)
            .map(|c| [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)])
            .collect::<Vec<_>>()
            .into()
    }

    /// `VIB_TABLE`: a triangle LFO.
    pub fn vibrato() -> Osc {
        let vib = t(&[
            0, 0, 0, 0, 20, 0x7FFF, 0, 20, 0, 0, 20, -0x4000, 0, 20, 0, 13, 0, 1,
        ]);
        Osc {
            mode: 1,
            rate: 0.8,
            attack: Some(vib.clone()),
            release: Some(vib),
            width: 0.0,
            vertex: 1.0,
        }
    }

    /// `TRE_TABLE`
    pub fn tremolo() -> Osc {
        let tre = t(&[
            0, 0, 0x7FFF, 0, 20, 0, 0, 20, -0x7FFF, 0, 20, 0, 0, 20, 0x7FFF, 13, 0, 1,
        ]);
        Osc {
            mode: 0,
            rate: 1.0,
            attack: Some(tre.clone()),
            release: Some(tre),
            width: 0.0,
            vertex: 1.0,
        }
    }

    /// `REL_TABLE`
    pub fn rel_table() -> Table {
        t(&[0, 10, 0, 15, 1, 0])
    }

    /// `ENVELOPE_DEF` (also `OSC_DEF`).
    pub fn envelope() -> Osc {
        Osc {
            mode: 0,
            rate: 1.0,
            attack: None,
            release: Some(rel_table()),
            width: 1.0,
            vertex: 0.0,
        }
    }

    /// `ADS_TABLE` with the four times/levels filled in, and `REL_TABLE` with its time
    /// (`Osc_Setup_ADSR`).
    pub fn adsr(args: [i16; 5]) -> Osc {
        let mut ads = [0, 0, 0x7FFF, 0, 0, 0x7FFF, 0, 0, 0, 14, 0, 0];
        ads[1] = args[0];
        ads[4] = args[1];
        ads[7] = args[2];
        ads[8] = args[3];
        let mut rel = [0, 10, 0, 15, 1, 0];
        rel[1] = args[4];
        Osc {
            mode: 0,
            rate: 1.0,
            attack: Some(t(&ads)),
            release: Some(t(&rel)),
            width: 1.0,
            vertex: 0.0,
        }
    }

    /// `PERC_ENV` (oneshot.c): flat, released by the percussion release table.
    pub fn perc_env() -> Osc {
        Osc {
            mode: 0,
            rate: 1.0,
            attack: None,
            release: None,
            width: 1.0,
            vertex: 0.0,
        }
    }

    /// `OSC_ENV`: release `OSC_REL`.
    pub fn osc_env() -> Osc {
        Osc {
            mode: 0,
            rate: 1.0,
            attack: None,
            release: Some(t(&[1, 10, 0, 15, 0, 0])),
            width: 1.0,
            vertex: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_attacks_holds_and_releases() {
        let osc = presets::adsr([0, 60, 60, 0x4000, 60]);
        let mut b = OscBuf {
            state: 1,
            ..Default::default()
        };
        let mut v = Vec::new();
        for _ in 0..200 {
            v.push(osc_to_ofs(Some(&osc), &mut b));
        }
        assert!((v[0] - 1.0).abs() < 0.05, "instant attack: {}", v[0]);
        assert_eq!(b.state, 3, "sustaining");
        assert!((v[199] - 0.5).abs() < 0.01, "sustain level: {}", v[199]);
        b.state = 4;
        for _ in 0..400 {
            osc_to_ofs(Some(&osc), &mut b);
        }
        assert_eq!(b.state, 0, "released");
    }

    #[test]
    fn vibrato_loops() {
        let osc = presets::vibrato();
        let mut b = OscBuf {
            state: 1,
            ..Default::default()
        };
        for _ in 0..2000 {
            osc_to_ofs(Some(&osc), &mut b);
        }
        assert_eq!(b.state, 2, "still running");
    }
}
