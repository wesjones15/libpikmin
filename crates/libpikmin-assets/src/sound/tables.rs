//! Sound tables the game keeps in `main.dol`, copied out at extraction:
//!
//! - pikiinter.c: `EVENT_OFFSET`, `ACTION_STATUS`, `EVENT_DIST_SCALE` (how a positional event's
//!   actions map to port commands, their priorities and groups, and distance falloff);
//! - piki_bgm.c: `bgm_mute_set` / `bgm_volume_set` (which tracks of each song play in its
//!   normal/battle mixes) and the game volume level tables;
//! - soundMgr.cpp: `soundTable` (each `SE_*` sound's JAudio id and event type).
//!
//! In every release these sit back to back from `EVENT_OFFSET` at the same relative offsets,
//! so each release needs only that address and `soundTable`'s (from the projectPiki symbol
//! maps, `config/<release>/symbols.txt`).

use crate::{Assets, Error};

pub const TABLES: &str = "SndData/jaudio_tables.bin";
pub const SOUND_TABLE: &str = "SndData/sound_table.txt";
pub const ONION_BOOT: &str = "SndData/onion_boot.bin";

/// pikidemo.c's demo4/demo5 timed commands, copied from the user's executable.
/// Their relative addresses are shared by the supported retail releases (symbol maps).
pub fn extract_onion_boot(dol: &[u8], game_id: &str, version: u8) -> Result<Vec<u8>, Error> {
    let &(_, _, base, _) = RELEASES.iter().find(|r| r.0 == game_id && r.1 == version)
        .ok_or_else(|| Error::Bad("unsupported Onion sound-table release".into()))?;
    Ok(Dol(dol).bytes(base + 0xA68, 0x54)?.to_vec())
}

/// Timed commands and terminal frame for FindRedOnyon (4), or Yellow/Blue (5/6).
pub fn onion_boot_commands(assets: &Assets, red: bool) -> Result<(Vec<(f32, u16)>, f32), Error> {
    let bytes = assets.read(ONION_BOOT)?;
    let data = bytes.get(if red { 0..0x2C } else { 0x2C..0x54 }).ok_or(Error::Truncated(0))?;
    let mut commands = Vec::new();
    for pair in data.chunks_exact(4) {
        let frame = i16::from_be_bytes([pair[0], pair[1]]);
        let cmd = i16::from_be_bytes([pair[2], pair[3]]);
        if frame < 0 { return Err(Error::Bad("negative Onion sound frame".into())); }
        if cmd == -1 { return Ok((commands, frame as f32)); }
        if cmd >= 0 { commands.push((frame as f32, cmd as u16)); }
    }
    Err(Error::Bad("unterminated Onion sound timeline".into()))
}

/// (game id, disc version) -> (`EVENT_OFFSET`, `soundTable`) virtual addresses.
const RELEASES: [(&str, u8, u32, u32); 5] = [
    ("GPIE01", 0, 0x8022_59C8, 0x802B_3334),
    ("GPIE01", 1, 0x8022_5A68, 0x802B_33D4),
    ("GPIJ01", 1, 0x8022_AB48, 0x802B_8E64),
    ("GPIJ01", 2, 0x8022_B028, 0x802B_933C),
    ("GPIP01", 0, 0x8022_78C8, 0x802B_5C94),
];

// Offsets from `EVENT_OFFSET` and sizes.
const ACTION_STATUS: (u32, usize) = (0x20, 0x684);
const DIST_SCALE: (u32, usize) = (0x6A4, 0x20);
const BGM_MUTE: u32 = 0x6C8;
const BGM_VOLUME: u32 = 0x718;
const GAME_BGM: (u32, usize) = (0x768, 0x2C);
const GAME_DEMO: (u32, usize) = (0x794, 0x16);
const GAME_SE: (u32, usize) = (0x7AC, 0x16);
const GAME_STREAM: (u32, usize) = (0x7C4, 0x16);
const SONGS: usize = 20;
const SOUNDS: usize = 0x19DC / 0x14;

/// Maps virtual addresses to `main.dol` file offsets through its section table.
struct Dol<'a>(&'a [u8]);

impl Dol<'_> {
    fn be(&self, o: usize) -> u32 {
        u32::from_be_bytes(self.0[o..o + 4].try_into().unwrap())
    }

    fn offset(&self, va: u32) -> Option<usize> {
        (0..18).find_map(|i| {
            let (off, addr, size) = (self.be(i * 4), self.be(0x48 + i * 4), self.be(0x90 + i * 4));
            (size != 0 && va >= addr && va < addr + size).then(|| (off + (va - addr)) as usize)
        })
    }

    fn bytes(&self, va: u32, n: usize) -> Result<&[u8], Error> {
        let o = self
            .offset(va)
            .ok_or(Error::Bad(format!("address {va:#x} not in main.dol")))?;
        self.0.get(o..o + n).ok_or(Error::Truncated(o))
    }

    fn u32(&self, va: u32) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.bytes(va, 4)?.try_into().unwrap()))
    }

    fn cstr(&self, va: u32) -> Result<String, Error> {
        let o = self
            .offset(va)
            .ok_or(Error::Bad(format!("string {va:#x} not in main.dol")))?;
        let s = &self.0[o..];
        let end = s.iter().take(64).position(|&b| b == 0).unwrap_or(0);
        Ok(String::from_utf8_lossy(&s[..end]).into_owned())
    }
}

/// Copies the tables out of `main.dol`: the binary blob for [`TABLES`] and the text for
/// [`SOUND_TABLE`].
pub fn extract(dol: &[u8], game_id: &str, version: u8) -> Result<(Vec<u8>, String), Error> {
    let &(_, _, base, sound) = RELEASES
        .iter()
        .find(|r| r.0 == game_id && r.1 == version)
        .ok_or_else(|| {
            Error::Bad(format!(
                "no sound table addresses for {game_id} revision {version}"
            ))
        })?;
    let d = Dol(dol);
    let mut out = b"LPJT".to_vec();
    out.extend_from_slice(d.bytes(base, 0x20)?);
    out.extend_from_slice(d.bytes(base + ACTION_STATUS.0, ACTION_STATUS.1)?);
    out.extend_from_slice(d.bytes(base + DIST_SCALE.0, DIST_SCALE.1)?);
    // Both song tables are arrays of pointers.
    for i in 0..SONGS as u32 {
        let p = d.u32(base + BGM_MUTE + i * 4)?;
        out.extend_from_slice(d.bytes(p, 8)?);
    }
    for i in 0..SONGS as u32 {
        let p = d.u32(base + BGM_VOLUME + i * 4)?;
        out.extend_from_slice(d.bytes(p, 8)?);
    }
    for (o, n) in [GAME_BGM, GAME_DEMO, GAME_SE, GAME_STREAM] {
        out.extend_from_slice(d.bytes(base + o, n)?);
    }
    let mut names = String::from("# se_id\tjac_id\tloop\tevent\tname (soundTable, soundMgr.cpp)\n");
    for i in 0..SOUNDS as u32 {
        let e = sound + i * 0x14;
        let (se, jac, name, looping, event) = (
            d.u32(e)?,
            d.u32(e + 4)?,
            d.u32(e + 8)?,
            d.u32(e + 0xC)?,
            d.u32(e + 0x10)?,
        );
        let name = if name == 0 {
            String::new()
        } else {
            d.cstr(name)?
        };
        names.push_str(&format!(
            "{}\t{}\t{}\t{}\t{name}\n",
            se as i32, jac as i32, looping, event as i32
        ));
    }
    let t = JaudioTables::parse(&out, &names)?;
    if t.event_offset[0] != 0
        || t.event_offset[1] != 1
        || t.dist_scale[0] != 1.0
        || t.sounds.first().map(|s| s.se_id) != Some(0)
    {
        return Err(Error::Bad(
            "sound tables in main.dol don't look right for this release".into(),
        ));
    }
    Ok((out, names))
}

/// `ActionStatus` (pikiinter.c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionStatus {
    /// 0x10: limit by group (low nibble = max concurrent), 0x20: don't steal.
    pub flags: u8,
    pub prio: u8,
    pub group: u8,
    /// Port command sent to the event's track.
    pub cmd: u16,
}

/// A `soundTable` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEntry {
    pub se_id: i32,
    pub jac_id: i32,
    pub looping: bool,
    /// `JacEventType`: -2 system, -1 player (Olimar), 1+ positional events.
    pub event: i32,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct JaudioTables {
    pub event_offset: [i32; 8],
    pub action_status: Vec<ActionStatus>,
    pub dist_scale: [f32; 8],
    /// Per song: track mute masks for mix modes 0-3.
    pub bgm_mute: [[u16; 4]; SONGS],
    /// Per song: volume for normal and quiet (menu) intensity.
    pub bgm_volume: [[f32; 2]; SONGS],
    pub game_bgm_vol: [f32; 11],
    pub game_demo_vol: [u16; 11],
    pub game_se_vol: [u16; 11],
    pub game_stream_vol: [u16; 11],
    pub sounds: Vec<SoundEntry>,
}

impl JaudioTables {
    pub fn load(assets: &Assets) -> Result<Self, Error> {
        let names = String::from_utf8_lossy(&assets.read(SOUND_TABLE)?).into_owned();
        Self::parse(&assets.read(TABLES)?, &names)
    }

    pub fn parse(b: &[u8], names: &str) -> Result<Self, Error> {
        let need = 4 + 0x20 + 0x684 + 0x20 + SONGS * 16 + 0x2C + 0x16 * 3;
        if b.len() < need || &b[..4] != b"LPJT" {
            return Err(Error::Bad("jaudio_tables.bin is not a table file".into()));
        }
        let rd = |at: &mut usize, n: usize| -> &[u8] {
            *at += n;
            &b[*at - n..*at]
        };
        let be32 = |s: &[u8], i: usize| u32::from_be_bytes(s[i * 4..i * 4 + 4].try_into().unwrap());
        let be16 = |s: &[u8], i: usize| u16::from_be_bytes(s[i * 2..i * 2 + 2].try_into().unwrap());
        let mut at = 4;
        let eo = rd(&mut at, 0x20);
        let event_offset = std::array::from_fn(|i| be32(eo, i) as i32);
        let action_status = rd(&mut at, 0x684)
            .as_chunks::<6>()
            .0
            .iter()
            .map(|c| ActionStatus {
                flags: c[0],
                prio: c[1],
                group: c[2],
                cmd: u16::from_be_bytes([c[4], c[5]]),
            })
            .collect();
        let ds = rd(&mut at, 0x20);
        let dist_scale = std::array::from_fn(|i| f32::from_bits(be32(ds, i)));
        let mut bgm_mute = [[0u16; 4]; SONGS];
        for m in bgm_mute.iter_mut() {
            let s = rd(&mut at, 8);
            *m = std::array::from_fn(|i| be16(s, i));
        }
        let mut bgm_volume = [[0f32; 2]; SONGS];
        for v in bgm_volume.iter_mut() {
            let s = rd(&mut at, 8);
            *v = std::array::from_fn(|i| f32::from_bits(be32(s, i)));
        }
        let gb = rd(&mut at, 0x2C);
        let game_bgm_vol = std::array::from_fn(|i| f32::from_bits(be32(gb, i)));
        let gd = rd(&mut at, 0x16);
        let game_demo_vol = std::array::from_fn(|i| be16(gd, i));
        let gs = rd(&mut at, 0x16);
        let game_se_vol = std::array::from_fn(|i| be16(gs, i));
        let gt = rd(&mut at, 0x16);
        let game_stream_vol = std::array::from_fn(|i| be16(gt, i));
        let sounds = names
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .filter_map(|l| {
                let mut f = l.splitn(5, '\t');
                Some(SoundEntry {
                    se_id: f.next()?.parse().ok()?,
                    jac_id: f.next()?.parse().ok()?,
                    looping: f.next()? != "0",
                    event: f.next()?.parse().ok()?,
                    name: f.next().unwrap_or("").to_string(),
                })
            })
            .collect();
        Ok(JaudioTables {
            event_offset,
            action_status,
            dist_scale,
            bgm_mute,
            bgm_volume,
            game_bgm_vol,
            game_demo_vol,
            game_se_vol,
            game_stream_vol,
            sounds,
        })
    }

    /// A sound by its `SE_*` id (`PikiSoundID`).
    pub fn sound_by_id(&self, se_id: i32) -> Option<&SoundEntry> {
        self.sounds.iter().find(|s| s.se_id == se_id)
    }

    /// A sound by its `SE_*` name.
    pub fn sound(&self, name: &str) -> Option<&SoundEntry> {
        self.sounds.iter().find(|s| s.name == name)
    }
}
