//! `SndData/Banks/pikibank.bx`: every wave system (WSYS) and instrument bank (IBNK) in one file.
//!
//! - `Bank_Setup` (bankloader.c): a 16-byte header of {offset, count} for the WSYS and IBNK
//!   tables, each an array of {addr, size}; a zero size is an empty slot. The slot index is the
//!   *physical* id; each chunk also carries a *virtual* id (`Wsys_::globalID`, `Ibnk_::_08`) that
//!   sequences use, mapped through `Jac_WsConnectTableSet` / `Jac_BnkConnectTableSet` (connect.c).
//! - Pointers inside a chunk are offsets from the chunk's start (`PTconvert`, bankread.c and
//!   waveread.c); zero is null. Layouts are in `include/jaudio/bx.h`.

use crate::Error;

fn u8_at(d: &[u8], at: usize) -> Result<u8, Error> {
    d.get(at).copied().ok_or(Error::Truncated(at))
}

fn u16_at(d: &[u8], at: usize) -> Result<u16, Error> {
    let b = d.get(at..at + 2).ok_or(Error::Truncated(at))?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32, Error> {
    let b = d.get(at..at + 4).ok_or(Error::Truncated(at))?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32_at(d: &[u8], at: usize) -> Result<f32, Error> {
    Ok(f32::from_bits(u32_at(d, at)?))
}

/// A chunk-relative pointer at `at`, resolved to an index into `d` (`None` for null).
fn ptr_at(d: &[u8], at: usize) -> Result<Option<usize>, Error> {
    let p = u32_at(d, at)? as usize;
    if p == 0 {
        return Ok(None);
    }
    if p >= d.len() {
        return Err(Error::Bad(format!("pointer {p:#x} at {at:#x} outside chunk")));
    }
    Ok(Some(p))
}

fn magic(d: &[u8], at: usize, want: &[u8; 4]) -> Result<(), Error> {
    match d.get(at..at + 4) {
        Some(m) if m == want => Ok(()),
        _ => Err(Error::Bad(format!(
            "expected {} at {at:#x}",
            String::from_utf8_lossy(want)
        ))),
    }
}

/// Everything in `pikibank.bx`, indexed by physical id.
#[derive(Debug, Clone)]
pub struct SoundBank {
    pub wsys: Vec<Option<Wsys>>,
    pub banks: Vec<Option<Ibnk>>,
}

impl SoundBank {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let table = |off: usize, count: usize| -> Result<Vec<Option<&[u8]>>, Error> {
            (0..count)
                .map(|i| {
                    let addr = u32_at(data, off + i * 8)? as usize;
                    let size = u32_at(data, off + i * 8 + 4)? as usize;
                    if size == 0 {
                        return Ok(None);
                    }
                    data.get(addr..addr + size).map(Some).ok_or(Error::Truncated(addr))
                })
                .collect()
        };
        let wsys = table(u32_at(data, 0)? as usize, u32_at(data, 4)? as usize)?
            .into_iter()
            .map(|c| c.map(Wsys::parse).transpose())
            .collect::<Result<_, _>>()?;
        let banks = table(u32_at(data, 8)? as usize, u32_at(data, 12)? as usize)?
            .into_iter()
            .map(|c| c.map(Ibnk::parse).transpose())
            .collect::<Result<_, _>>()?;
        Ok(SoundBank { wsys, banks })
    }

    /// `Jac_WsVirtualToPhysical`: the first slot registered with that virtual id.
    pub fn wsys_physical(&self, virtual_id: u16) -> Option<usize> {
        self.wsys
            .iter()
            .position(|w| w.as_ref().is_some_and(|w| w.virtual_id == virtual_id as u32))
    }

    /// `Jac_BnkVirtualToPhysical`.
    pub fn bank_physical(&self, virtual_id: u16) -> Option<usize> {
        self.banks
            .iter()
            .position(|b| b.as_ref().is_some_and(|b| b.virtual_id == virtual_id as u32))
    }
}

// ---------------------------------------------------------------- WSYS

/// A wave system: wave archives (`.aw` files) and the scenes that say which waves are loaded.
#[derive(Debug, Clone)]
pub struct Wsys {
    /// `Wsys_::globalID`
    pub virtual_id: u32,
    /// `WINF`: one archive per scene.
    pub archives: Vec<WaveArchive>,
    /// `WBCT`
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Clone)]
pub struct WaveArchive {
    /// Path under `SndData/`, e.g. `Banks/pikise_0.aw`.
    pub file: String,
    pub waves: Vec<Wave>,
}

/// `Wave_` (bx.h).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wave {
    /// `compBlockIdx`: 0 = 4-bit ADPCM, 1 = 2-bit ADPCM, 2 = PCM8, 3 = PCM16
    /// (`DSP_SetWaveInfo` block tables, dspinterface.c).
    pub format: u8,
    /// Root key the wave was recorded at.
    pub key: u8,
    pub sample_rate: f32,
    /// Byte offset and length inside the archive file.
    pub offset: u32,
    pub length: u32,
    pub looping: bool,
    /// Loop start in samples, and the decoder history there (`loopYN1/2`).
    pub loop_start: u32,
    pub loop_hist: [i16; 2],
    /// `_1C`: loop end, or the sample count when not looping.
    pub end: u32,
}

/// `SCNE_`: which wave ids a scene loads. `cdf[i]` is the archive's wave `i`; `cex` continues
/// the numbering after it, or (`external`) points into other scenes' `cdf` lists by
/// `scene << 16 | wave` (`UpdateWave` / `UpdateWave_Extern`, connect.c).
#[derive(Debug, Clone)]
pub struct Scene {
    pub external: bool,
    pub dependencies: Vec<u32>,
    pub cdf: Vec<u32>,
    pub cex: Vec<u32>,
    pub cst: Vec<u32>,
}

impl Wsys {
    fn parse(d: &[u8]) -> Result<Self, Error> {
        magic(d, 0, b"WSYS")?;
        let virtual_id = u32_at(d, 8)?;
        let winf = ptr_at(d, 0x10)?.ok_or(Error::Bad("WSYS without WINF".into()))?;
        let wbct = ptr_at(d, 0x14)?.ok_or(Error::Bad("WSYS without WBCT".into()))?;
        magic(d, winf, b"WINF")?;
        magic(d, wbct, b"WBCT")?;

        let mut archives = Vec::new();
        for i in 0..u32_at(d, winf + 4)? as usize {
            let Some(a) = ptr_at(d, winf + 8 + i * 4)? else {
                continue;
            };
            let name = d.get(a..a + 0x40).ok_or(Error::Truncated(a))?;
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            let file = String::from_utf8_lossy(&name[..end]).into_owned();
            let mut waves = Vec::new();
            for j in 0..u32_at(d, a + 0x70)? as usize {
                let w = ptr_at(d, a + 0x74 + j * 4)?.ok_or(Error::Bad("null wave".into()))?;
                waves.push(Wave {
                    format: u8_at(d, w + 1)?,
                    key: u8_at(d, w + 2)?,
                    sample_rate: f32_at(d, w + 4)?,
                    offset: u32_at(d, w + 8)?,
                    length: u32_at(d, w + 0xC)?,
                    looping: u32_at(d, w + 0x10)? != 0,
                    loop_start: u32_at(d, w + 0x18)?,
                    end: u32_at(d, w + 0x1C)?,
                    loop_hist: [u16_at(d, w + 0x20)? as i16, u16_at(d, w + 0x22)? as i16],
                });
            }
            archives.push(WaveArchive { file, waves });
        }

        let ctrl = |at: Option<usize>| -> Result<Vec<u32>, Error> {
            let Some(c) = at else {
                return Ok(Vec::new());
            };
            (0..u32_at(d, c + 4)? as usize)
                .map(|i| {
                    let id = ptr_at(d, c + 8 + i * 4)?.ok_or(Error::Bad("null wave id".into()))?;
                    u32_at(d, id)
                })
                .collect()
        };
        let mut scenes = Vec::new();
        for i in 0..u32_at(d, wbct + 8)? as usize {
            let s = ptr_at(d, wbct + 0xC + i * 4)?.ok_or(Error::Bad("null scene".into()))?;
            magic(d, s, b"SCNE")?;
            let deps = u32_at(d, s + 8)? as usize;
            scenes.push(Scene {
                external: u32_at(d, s + 4)? != 0,
                dependencies: (0..deps).map(|k| u32_at(d, s + 0x18 + k * 4)).collect::<Result<_, _>>()?,
                cdf: ctrl(ptr_at(d, s + 0xC)?)?,
                cex: ctrl(ptr_at(d, s + 0x10)?)?,
                cst: ctrl(ptr_at(d, s + 0x14)?)?,
            });
        }
        Ok(Wsys {
            virtual_id,
            archives,
            scenes,
        })
    }

    /// `GetSoundHandle` / `__GetSoundHandle` (connect.c): the wave a 16-bit wave id names in
    /// `scene`, searching its `cdf`, then `cex`, then its dependencies. Returns
    /// (archive index, wave index).
    pub fn find_wave(&self, scene: usize, id: u16) -> Option<(usize, usize)> {
        self.find_wave_depth(scene, id, 0)
    }

    fn find_wave_depth(&self, scene: usize, id: u16, depth: u32) -> Option<(usize, usize)> {
        let s = self.scenes.get(scene)?;
        if let Some(i) = s.cdf.iter().position(|&w| w as u16 == id) {
            return self.wave_ref(scene, i);
        }
        if let Some(i) = s.cex.iter().position(|&w| w as u16 == id) {
            if s.external {
                // `UpdateWave_Extern`: `scene << 16 | wave`, found in that scene's cdf.
                let w = s.cex[i];
                let (other, wid) = ((w >> 16) as usize, w as u16);
                let k = self.scenes.get(other)?.cdf.iter().position(|&c| c as u16 == wid)?;
                return self.wave_ref(other, k);
            }
            return self.wave_ref(scene, s.cdf.len() + i);
        }
        if depth < 8 {
            for &dep in &s.dependencies {
                if let Some(r) = self.find_wave_depth(dep as usize, id, depth + 1) {
                    return Some(r);
                }
            }
        }
        None
    }

    fn wave_ref(&self, archive: usize, wave: usize) -> Option<(usize, usize)> {
        (wave < self.archives.get(archive)?.waves.len()).then_some((archive, wave))
    }

    pub fn wave(&self, r: (usize, usize)) -> &Wave {
        &self.archives[r.0].waves[r.1]
    }
}

// ---------------------------------------------------------------- IBNK

/// An instrument bank: 240 program slots (`Bank_`, bx.h). 0x00-0x7F are instruments, 0x80-0xE3
/// voices (unused by Pikmin's sequences), 0xE4-0xEF percussion sets.
#[derive(Debug, Clone)]
pub struct Ibnk {
    /// `Ibnk_::_08`, the id sequences select banks by.
    pub virtual_id: u32,
    pub programs: Vec<Option<Program>>,
}

#[derive(Debug, Clone)]
pub enum Program {
    Inst(Box<Inst>),
    Perc(Box<Perc>),
}

/// `Inst_`.
#[derive(Debug, Clone)]
pub struct Inst {
    pub flag: u32,
    /// `Inst_ +0x08`; Play_1shot multiplies it into the volume.
    pub volume: f32,
    /// `Inst_ +0x0C`; multiplied into the pitch.
    pub pitch: f32,
    pub oscs: [Option<Osc>; 2],
    pub rands: [Option<Rand>; 2],
    pub sensors: [Option<Sense>; 2],
    pub keys: Vec<KeyRegion>,
}

/// `InstKeymap_`: used for keys up to `base_key`.
#[derive(Debug, Clone)]
pub struct KeyRegion {
    pub base_key: u8,
    pub vels: Vec<Vmap>,
}

/// `Vmap_`: used for velocities up to `base_vel`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vmap {
    pub base_vel: u8,
    /// Virtual WSYS id << 16 | wave id.
    pub wave_id: u32,
    pub volume: f32,
    pub pitch: f32,
}

/// `Osc_`: an envelope or LFO. Tables are (curve, time, value) triplets ending in a 0xD jump,
/// 0xE sustain or 0xF stop (`Bank_OscToOfs`, bankdrv.c).
#[derive(Debug, Clone, PartialEq)]
pub struct Osc {
    /// What it modulates: 0 volume, 1 pitch, 2 pan, 3 fx, 4 dolby (`Osc_Update_Param`).
    pub mode: u8,
    pub rate: f32,
    pub attack: Option<Vec<[i16; 3]>>,
    pub release: Option<Vec<[i16; 3]>>,
    pub width: f32,
    pub vertex: f32,
}

/// `Rand_`: a random offset applied to one parameter at note-on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rand {
    pub id: u8,
    pub value: f32,
    pub range: f32,
}

/// `Sense_`: a parameter scaled by key or velocity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sense {
    pub id: u8,
    pub kind: u8,
    pub threshold: u8,
    pub min: f32,
    pub max: f32,
}

/// `Perc_` ('PER2'): one entry per key.
#[derive(Debug, Clone)]
pub struct Perc {
    pub keys: Vec<Option<PercKey>>,
    pub pan: [i8; 128],
    pub release: [u16; 128],
}

/// `PercKeymap_`.
#[derive(Debug, Clone)]
pub struct PercKey {
    /// `+0x00`, used as the volume by `Play_1shot_Perc`.
    pub volume: f32,
    /// `+0x04`, used as the pitch.
    pub pitch: f32,
    pub vels: Vec<Vmap>,
}

impl Ibnk {
    fn parse(d: &[u8]) -> Result<Self, Error> {
        magic(d, 0, b"IBNK")?;
        let virtual_id = u32_at(d, 8)?;
        magic(d, 0x20, b"BANK")?;
        let vmap = |at: usize| -> Result<Vmap, Error> {
            Ok(Vmap {
                base_vel: u8_at(d, at)?,
                wave_id: u32_at(d, at + 4)?,
                volume: f32_at(d, at + 8)?,
                pitch: f32_at(d, at + 0xC)?,
            })
        };
        let vels = |count_at: usize, list_at: usize| -> Result<Vec<Vmap>, Error> {
            let n = u32_at(d, count_at)? as usize;
            if n > 128 {
                return Err(Error::Bad(format!("{n} velocity regions at {count_at:#x}")));
            }
            (0..n)
                .filter_map(|k| ptr_at(d, list_at + k * 4).transpose())
                .map(|p| vmap(p?))
                .collect()
        };
        let table = |at: Option<usize>| -> Result<Option<Vec<[i16; 3]>>, Error> {
            let Some(mut at) = at else {
                return Ok(None);
            };
            let mut t = Vec::new();
            loop {
                let e = [
                    u16_at(d, at)? as i16,
                    u16_at(d, at + 2)? as i16,
                    u16_at(d, at + 4)? as i16,
                ];
                t.push(e);
                at += 6;
                if (0xD..=0xF).contains(&e[0]) || t.len() > 256 {
                    break;
                }
            }
            Ok(Some(t))
        };
        let mut programs = Vec::with_capacity(0xF0);
        for i in 0..0xF0usize {
            let Some(p) = ptr_at(d, 0x24 + i * 4)? else {
                programs.push(None);
                continue;
            };
            if i < 0x80 {
                magic(d, p, b"INST")?;
                let osc = |k: usize| -> Result<Option<Osc>, Error> {
                    let Some(o) = ptr_at(d, p + 0x10 + k * 4)? else {
                        return Ok(None);
                    };
                    Ok(Some(Osc {
                        mode: u8_at(d, o)?,
                        rate: f32_at(d, o + 4)?,
                        attack: table(ptr_at(d, o + 8)?)?,
                        release: table(ptr_at(d, o + 0xC)?)?,
                        width: f32_at(d, o + 0x10)?,
                        vertex: f32_at(d, o + 0x14)?,
                    }))
                };
                let rand = |k: usize| -> Result<Option<Rand>, Error> {
                    let Some(r) = ptr_at(d, p + 0x18 + k * 4)? else {
                        return Ok(None);
                    };
                    Ok(Some(Rand {
                        id: u8_at(d, r)?,
                        value: f32_at(d, r + 4)?,
                        range: f32_at(d, r + 8)?,
                    }))
                };
                let sense = |k: usize| -> Result<Option<Sense>, Error> {
                    let Some(s) = ptr_at(d, p + 0x20 + k * 4)? else {
                        return Ok(None);
                    };
                    Ok(Some(Sense {
                        id: u8_at(d, s)?,
                        kind: u8_at(d, s + 1)?,
                        threshold: u8_at(d, s + 2)?,
                        min: f32_at(d, s + 4)?,
                        max: f32_at(d, s + 8)?,
                    }))
                };
                let nkeys = u32_at(d, p + 0x28)? as usize;
                if nkeys > 128 {
                    return Err(Error::Bad(format!("{nkeys} key regions at {p:#x}")));
                }
                let keys = (0..nkeys)
                    .filter_map(|k| ptr_at(d, p + 0x2C + k * 4).transpose())
                    .map(|k| {
                        let k = k?;
                        Ok(KeyRegion {
                            base_key: u8_at(d, k)?,
                            vels: vels(k + 4, k + 8)?,
                        })
                    })
                    .collect::<Result<_, Error>>()?;
                programs.push(Some(Program::Inst(Box::new(Inst {
                    flag: u32_at(d, p + 4)?,
                    volume: f32_at(d, p + 8)?,
                    pitch: f32_at(d, p + 0xC)?,
                    oscs: [osc(0)?, osc(1)?],
                    rands: [rand(0)?, rand(1)?],
                    sensors: [sense(0)?, sense(1)?],
                    keys,
                }))));
            } else if i >= 0xE4 {
                let keys = (0..128)
                    .map(|k| {
                        let Some(pk) = ptr_at(d, p + 0x88 + k * 4)? else {
                            return Ok(None);
                        };
                        Ok(Some(PercKey {
                            volume: f32_at(d, pk)?,
                            pitch: f32_at(d, pk + 4)?,
                            vels: vels(pk + 0x10, pk + 0x14)?,
                        }))
                    })
                    .collect::<Result<_, Error>>()?;
                let mut pan = [0i8; 128];
                let mut release = [0u16; 128];
                for k in 0..128 {
                    pan[k] = u8_at(d, p + 0x288 + k)? as i8;
                    release[k] = u16_at(d, p + 0x308 + k * 2)?;
                }
                programs.push(Some(Program::Perc(Box::new(Perc { keys, pan, release }))));
            } else {
                // Voices: never registered by Pikmin's banks in practice.
                programs.push(None);
            }
        }
        Ok(Ibnk { virtual_id, programs })
    }

    pub fn inst(&self, program: u8) -> Option<&Inst> {
        match self.programs.get(program as usize)? {
            Some(Program::Inst(i)) => Some(i),
            _ => None,
        }
    }

    pub fn perc(&self, program: u8) -> Option<&Perc> {
        match self.programs.get(program as usize)? {
            Some(Program::Perc(p)) => Some(p),
            _ => None,
        }
    }
}

impl Inst {
    /// `Bank_GetInstKeymap` (bankdrv.c): the first region whose top key covers `key`.
    pub fn key_region(&self, key: u8) -> Option<&KeyRegion> {
        self.keys.iter().find(|k| key <= k.base_key)
    }
}

/// `Bank_GetInstVmap` / `Bank_GetPercVmap`: the first region whose top velocity covers `vel`.
pub fn vmap_for(vels: &[Vmap], vel: u8) -> Option<&Vmap> {
    vels.iter().find(|v| vel <= v.base_vel)
}
