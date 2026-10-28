//! Pikmin 2's enemies as kinds of the same enemy list: what the disc says about each (`enemy/parm/enemyParms.szs`
//! text and the clips' lengths), read from the cache `libsms::extract::p2_enemy` fills (`<assets>/p2/enemy/`).
//!
//! Pikmin 2's enemies are `Game::<Name>::Obj : EnemyBase` classes (plugProjectNishimuraU/ and friends), not Pikmin
//! 1's `Tai*` strategies, but they ride the same machinery here: a [`Strategy`](crate::tai::Strategy) of states, the
//! enemy's [`Teki`](crate::object::Teki) registers, and the host's body. Differences the host sees:
//! - The kinds start at [`FIRST`]; the model is a J3D `.bmd` the host draws on the `.bca` clips (the clips' order is the
//!   `enemyAnimMgr.txt` order, which is the motion index the state reports), not a Pikmin 1 `.mod`.
//! - `floats[n]` is the general parm `fpNN` (`EnemyParmsBase::Parms`, `n` = NN) and `floats[100 + n]` the enemy's own
//!   `fpNN` block; `ints` likewise for `ipNN` (see [`fp`] and [`fp::own`]).
//! - The sounds an enemy asks for are named like the decomp's `PSSE_EN_*` ids without the prefix (see [`sound_id`]).

use std::path::Path;
use std::sync::Arc;

use glam::Vec3;
use libpikmin_assets::{AnimMgr, AnimSet, Model, Skeleton};

use crate::info::{MotionData, TekiInfo, TekiMotion};

/// The first Pikmin 2 kind (the Pikmin 1 kinds are `0..KINDS`).
pub const FIRST: u8 = 27;
/// Iridescent Flint Beetle (`Koganemushi::Obj`).
pub const FLINT_BEETLE: u8 = 27;
/// Iridescent Glint Beetle (`Wealthy::Obj`).
pub const GLINT_BEETLE: u8 = 28;
/// Doodlebug (`Fart::Obj`).
pub const DOODLEBUG: u8 = 29;

/// One Pikmin 2 kind: where its files are (`<cache>/enemy/...`) and what it is called.
pub struct Kind {
    /// The common name.
    pub name: &'static str,
    /// `enemy/parm/<parm>/enemyparm.txt` (lower case, as the archive spells it).
    pub parm: &'static str,
    /// The folder of `enemyAnimMgr.txt` and `enemyColl.txt`, which the Kogane family shares.
    pub shared: &'static str,
    /// `enemy/data/<data>/model/enemy.bmd` and `anim/*.bca`: the Kogane family shares one model.
    pub data: &'static str,
    /// The texture file the kind swaps in (`Mgr::getChangeTexture`), under `enemy/data/`.
    pub texture: &'static str,
}

pub const KIND_TABLE: [Kind; 3] = [
    Kind { name: "Iridescent Flint Beetle", parm: "kogane", shared: "kogane", data: "Kogane", texture: "Kogane/kogane_s3tc.bti" },
    Kind { name: "Iridescent Glint Beetle", parm: "wealthy", shared: "kogane", data: "Kogane", texture: "Wealthy/oogane_s3tc.bti" },
    Kind { name: "Doodlebug", parm: "fart", shared: "kogane", data: "Kogane", texture: "Fart/babakogane_s3tc.bti" },
];

pub fn kind(kind: u8) -> Option<&'static Kind> {
    kind.checked_sub(FIRST).and_then(|i| KIND_TABLE.get(i as usize))
}

/// Parameter indices (`EnemyParmsBase::Parms::mGeneral`, enemyParmsBase.h).
pub mod fp {
    /// `mHealth`.
    pub const LIFE: usize = 0;
    /// `mHeightOffsetFromFloor`.
    pub const HEIGHT_OFFSET: usize = 1;
    pub const MASS: usize = 5;
    /// `mMoveSpeed`.
    pub const MOVE_SPEED: usize = 6;
    /// `mTurnSpeed`: the share of the angle left that one tick turns.
    pub const TURN_SPEED: usize = 8;
    pub const TERRITORY: usize = 9;
    pub const SIGHT_RADIUS: usize = 12;
    pub const VIEW_ANGLE: usize = 13;
    /// `mMaxAttackRange`.
    pub const MAX_ATTACK_RANGE: usize = 20;
    /// `mAttackRadius`.
    pub const ATTACK_RADIUS: usize = 22;
    /// `mAttackDamage`.
    pub const ATTACK_DAMAGE: usize = 24;
    /// `mMaxTurnAngle`, degrees.
    pub const MAX_TURN_ANGLE: usize = 28;
    /// The enemy's own block (`ProperParms`).
    pub mod own {
        pub const BASE: usize = 100;
        /// Kogane: `mMinAppearTime`, `mMaxAppearTime` (`fp01`, `fp02`).
        pub const MIN_APPEAR: usize = BASE + 1;
        pub const MAX_APPEAR: usize = BASE + 2;
        /// `mMinTravelTime`, `mMaxTravelTime`.
        pub const MIN_TRAVEL: usize = BASE + 10;
        pub const MAX_TRAVEL: usize = BASE + 11;
        /// `mMinStopTime`, `mMaxStopTime`.
        pub const MIN_STOP: usize = BASE + 20;
        pub const MAX_STOP: usize = BASE + 21;
        /// `mTurnAngle`, degrees.
        pub const TURN_ANGLE: usize = BASE + 30;
        /// `mScale`.
        pub const SCALE: usize = BASE + 40;
    }
}

const SLOTS: usize = 160;

/// The text parameter files: blocks of `{id} type value` ended by `{_eof}`. The first block is
/// `Creature::Property`, the second `EnemyParmsBase`'s general parms, the third the enemy's own
/// (`Parameters::read` for each, enemyParmsBase.h and the enemy's `Parms::read`).
pub fn parse_parms(text: &str) -> Vec<Vec<(String, f32)>> {
    let mut blocks = vec![Vec::new()];
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some(rest) = line.strip_prefix('{') else { continue };
        let Some((id, value)) = rest.split_once('}') else { continue };
        if id == "_eof" {
            blocks.push(Vec::new());
            continue;
        }
        let mut w = value.split_whitespace();
        let (_ty, v) = (w.next(), w.next());
        if let Some(v) = v.and_then(|v| v.parse::<f32>().ok()) {
            blocks.last_mut().unwrap().push((id.to_owned(), v));
        }
    }
    blocks
}

/// One clip of `enemyAnimMgr.txt` (`AnimMgr::load`): its file and its keys `(frame, type)`.
#[derive(Debug, Clone)]
pub struct ClipInfo {
    pub file: String,
    pub keys: Vec<(f32, u32)>,
}

/// `enemyAnimMgr.txt`: a count, then per clip a block with the source path, the file name and `frame type` pairs
/// ended by `-1`. Key types are `SysShape::KeyEvent`'s (0 loop start, 1 loop end, 2.. the enemy's own).
pub fn parse_anim_mgr(text: &str) -> Vec<ClipInfo> {
    let mut clips = Vec::new();
    let mut cur: Option<(Vec<String>, Vec<(f32, u32)>)> = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match (line, &mut cur) {
            ("{", None) => cur = Some((Vec::new(), Vec::new())),
            ("}", Some(_)) => {
                let (names, keys) = cur.take().unwrap();
                if let Some(file) = names.get(1) {
                    clips.push(ClipInfo { file: file.clone(), keys });
                }
            }
            (l, Some((names, keys))) => {
                let mut w = l.split_whitespace();
                let (a, b) = (w.next(), w.next());
                match (a.and_then(|a| a.parse::<i32>().ok()), b.and_then(|b| b.parse::<u32>().ok())) {
                    // The two path lines never parse as `number number`.
                    (Some(f), Some(t)) if names.len() >= 2 => keys.push((f as f32, t)),
                    _ if l == "-1" => {}
                    _ => names.push(l.to_owned()),
                }
            }
            _ => {}
        }
    }
    clips
}

/// The length in frames of a J3D `.bca`/`.bck` (its `ANF1`/`ANK1` chunk: loop mode, angle scale, then the duration).
pub fn clip_frames(data: &[u8]) -> Option<f32> {
    if data.len() < 0x20 {
        return None;
    }
    let count = u32::from_be_bytes(data[0x0c..0x10].try_into().ok()?) as usize;
    let mut at = 0x20;
    for _ in 0..count {
        let tag = data.get(at..at + 4)?;
        let size = u32::from_be_bytes(data.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        if tag == b"ANK1" || tag == b"ANF1" {
            return Some(u16::from_be_bytes(data.get(at + 10..at + 12)?.try_into().ok()?) as f32);
        }
        at += size.max(8);
    }
    None
}

/// What a kind needs from the cache: the model, the texture, the parms and the clips.
pub fn is_available(root: &Path, kind: u8) -> bool {
    let Some(k) = self::kind(kind) else { return false };
    let e = root.join("enemy");
    e.join("parm").join(k.parm).join("enemyparm.txt").is_file()
        && e.join("parm").join(k.shared).join("enemyanimmgr.txt").is_file()
        && e.join("data").join(k.data).join("model/enemy.bmd").is_file()
}

fn read_text(p: &Path) -> Result<String, String> {
    // The files are Shift-JIS; the ids and numbers are ASCII and the comments are ignored.
    std::fs::read(p).map(|b| b.iter().map(|&c| if c < 0x80 { c as char } else { '?' }).collect()).map_err(|e| format!("{}: {e}", p.display()))
}

impl TekiInfo {
    /// A Pikmin 2 kind from `root` (the cache folder holding `enemy/`). The body is a J3D model the host draws, so
    /// the Pikmin 1 shape, skeleton and animations are empty; `lo`/`hi` come from the collision tree's radius.
    pub fn load_p2(root: &Path, kind: u8) -> Result<TekiInfo, String> {
        let k = self::kind(kind).ok_or("not a Pikmin 2 kind")?;
        let parm = root.join("enemy/parm").join(k.parm);
        let blocks = parse_parms(&read_text(&parm.join("enemyparm.txt"))?);
        let mut floats = vec![0.0f32; SLOTS];
        let mut ints = vec![0i32; SLOTS];
        for (i, base) in [(1usize, 0usize), (2, fp::own::BASE)] {
            for (id, v) in blocks.get(i).into_iter().flatten() {
                let Some(n) = id.get(2..).and_then(|n| n.parse::<usize>().ok()).filter(|&n| base + n < SLOTS) else { continue };
                match id.get(..2) {
                    Some("fp") => floats[base + n] = *v,
                    Some("ip") => ints[base + n] = v.round() as i32,
                    _ => {}
                }
            }
        }
        let scale = floats[fp::own::SCALE].max(0.01);
        // The first collision sphere is the whole body (`enemyColl.txt`: child count, then the radius).
        let shared = root.join("enemy/parm").join(k.shared);
        let radius = read_text(&shared.join("enemycoll.txt"))
            .ok()
            .and_then(|t| t.lines().filter_map(|l| l.split('#').next()).filter_map(|l| l.trim().parse::<f32>().ok()).nth(1))
            .unwrap_or(40.0);
        let r = radius * scale;

        let clips = parse_anim_mgr(&read_text(&shared.join("enemyanimmgr.txt"))?);
        let anim_dir = root.join("enemy/data").join(k.data).join("anim");
        let motion_data: Vec<MotionData> = clips
            .iter()
            .map(|c| {
                let frames = std::fs::read(anim_dir.join(&c.file)).ok().and_then(|b| clip_frames(&b)).unwrap_or(1.0);
                // The animator reads `keys` as the anim keys from the first to the last, and `info_keys` as
                // (index into `keys`, type): the clip's start and end bracket the listed keys.
                let mut keys = vec![0.0];
                let mut info_keys = Vec::new();
                for &(f, ty) in &c.keys {
                    if ty <= 5 {
                        info_keys.push((keys.len(), ty as u8));
                        keys.push(f);
                    }
                }
                keys.push(frames);
                MotionData { keys, info_keys, events: Vec::new(), speed: 30.0, dynamic_speed: false, frames }
            })
            .collect();
        let whole = |index: usize| {
            let m = motion_data.get(index);
            TekiMotion { index: index as u32, start: 0.0, end: m.map_or(1.0, |m| m.frames), loop_start: 0.0, loop_end: m.map_or(1.0, |m| m.frames), action0: 0.0, action1: 0.0 }
        };
        let model = Model::default();
        let skeleton = Skeleton::new(&model);
        Ok(TekiInfo {
            kind,
            model: 0,
            life: floats[fp::LIFE],
            scale,
            weight: floats[fp::MASS],
            run_speed: floats[fp::MOVE_SPEED],
            turn_speed: floats[fp::TURN_SPEED],
            visible_range: floats[fp::SIGHT_RADIUS],
            visible_angle: floats[fp::VIEW_ANGLE],
            attackable_range: floats[fp::MAX_ATTACK_RANGE],
            attackable_angle: 0.0,
            attack_range: floats[fp::ATTACK_RADIUS],
            attack_power: floats[fp::ATTACK_DAMAGE],
            collision_radius: r,
            corpse_size: 0.0,
            corpse_height: 0.0,
            corpse_min_carriers: 1,
            corpse_max_carriers: 1,
            corpse_seeds_matching: 0,
            corpse_seeds_other: 0,
            swallow_count: 1,
            carcass_size: 3,
            corpse_offset: Vec3::ZERO,
            lo: Vec3::new(-r, 0.0, -r),
            hi: Vec3::new(r, r * 2.0, r),
            motions: std::array::from_fn(|i| whole(i.min(motion_data.len().saturating_sub(1)))),
            motion_data,
            shape: Arc::new(model),
            skeleton: Arc::new(skeleton),
            anims: Arc::new(AnimSet::new(AnimMgr { base_path: String::new(), infos: Vec::new() }, Vec::new())),
            floats,
            ints,
            clips: clips.iter().map(|c| c.file.trim_end_matches(".bca").trim_end_matches(".bck").to_owned()).collect(),
        })
    }
}

/// The Pikmin 2 sound id (`SoundID.h`) of an `Out::Sound` name (a `PSSE_EN_*` without the prefix). Ids without the 0x800
/// bit (`FART_BUZZ`, `OTAKARA_ATK_GAS`) are held sounds the host re-requests each tick; the others play once.
pub fn sound_id(name: &str) -> Option<u32> {
    Some(match name {
        "ENEMY_LOOSE_ITEM" => 0x5810,
        "KOGANE_WALK" => 0x5872,
        "KOGANE_HIT" => 0x5873,
        "OOGANE_HIT" => 0x5880,
        "TAMAGOMUSHI_DIVE" => 0x58A9,
        "TAMAGOMUSHI_APPEAR" => 0x5915,
        "FART_GAS" => 0x5960,
        "FART_HIT" => 0x5961,
        "FART_BUZZ" => 0x5162,
        "OTAKARA_ATK_GAS" => 0x5164,
        _ => return None,
    })
}
