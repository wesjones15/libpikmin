//! Enemy ("teki") assets: `tekis/<name>/<name>.mod` + `.anm`, animation keys in
//! `tekikeys/<name>.key` (an AnimMgr file) and parameters in `tekipara/<name>.bin`
//! (`TekiMgr::TekiMgr` / `startStage`, plugPikiNakata/tekimgr.cpp).
//!
//! Teki motions index the `.key` AnimMgr directly (`PaniTekiAnimator::createMotionTable`):
//! see [`TekiMotion`].

use crate::Error;
use crate::reader::Reader;

/// `TekiMotion` (PaniAnimator.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TekiMotion {
    Dead = 0,
    Damage = 1,
    Wait1 = 2,
    Wait2 = 3,
    WaitAct1 = 4,
    WaitAct2 = 5,
    Move1 = 6,
    Move2 = 7,
    Attack = 8,
    Flick = 9,
    Type1 = 10,
    Type2 = 11,
    Type3 = 12,
    Type4 = 13,
    Type5 = 14,
}

/// Float parameter indices (`TPF_*`, TekiParameters.h).
pub mod tpf {
    pub const LIFE: usize = 0;
    pub const SCALE: usize = 1;
    pub const WEIGHT: usize = 2;
    pub const WALK_VELOCITY: usize = 3;
    pub const RUN_VELOCITY: usize = 4;
    pub const TURN_VELOCITY: usize = 5;
    pub const VISIBLE_RANGE: usize = 6;
    pub const VISIBLE_ANGLE: usize = 7;
    pub const ATTACKABLE_RANGE: usize = 8;
    pub const ATTACKABLE_ANGLE: usize = 9;
    pub const ATTACK_RANGE: usize = 10;
    pub const ATTACK_HIT_RANGE: usize = 11;
    pub const ATTACK_POWER: usize = 12;
    pub const ATTACK_WAIT_PERIOD: usize = 13;
    pub const UNGUARDED_PERIOD: usize = 14;
    pub const DANGER_TERRITORY_RANGE: usize = 15;
    pub const SAFETY_TERRITORY_RANGE: usize = 16;
    pub const LIFE_GAUGE_OFFSET: usize = 17;
    pub const SHADOW_SIZE: usize = 18;
    pub const RIPPLE_SCALE: usize = 19;
    pub const CORPSE_SIZE: usize = 20;
    pub const CORPSE_HEIGHT: usize = 21;
    pub const SPAWN_DISTANCE: usize = 22;
    pub const SPAWN_HEIGHT: usize = 23;
    pub const SPAWN_VELOCITY: usize = 24;
    pub const LIFE_PERIOD: usize = 25;
    pub const MESSAGE_RANGE: usize = 26;
    pub const WARN_PERIOD: usize = 27;
    pub const VISIBLE_HEIGHT: usize = 28;
    pub const FLIGHT_HEIGHT: usize = 29;
    pub const DAMAGE_MOTION_PERIOD: usize = 30;
    pub const DAMAGE_MOTION_AMPLITUDE: usize = 31;
    pub const FLICK_PROBABILITY: usize = 32;
    pub const UPPER_FLICK_POWER: usize = 33;
    pub const LOWER_FLICK_POWER: usize = 34;
    pub const UPPER_ATTACK_POWER: usize = 35;
    pub const LOWER_ATTACK_POWER: usize = 36;
    pub const LIFE_RECOVER_RATE: usize = 37;
    pub const TRACE_ANGLE: usize = 38;
    pub const SPAWN_PELLET_VELOCITY_HORIZ: usize = 39;
    pub const SPAWN_PELLET_VELOCITY_VERT: usize = 40;
    pub const CORPSE_VELOCITY_HORIZ: usize = 41;
    pub const CORPSE_VELOCITY_VERT: usize = 42;
    pub const BOMB_DAMAGE_RATE: usize = 43;
    pub const COLLISION_RADIUS: usize = 44;
    pub const LOWER_RANGE: usize = 45;
    /// First teki-specific float (`SWALLOWPF_*`, `CHAPPYPF_*`, ...).
    pub const SPECIFIC: usize = 50;
}

/// Int parameter indices (`TPI_*`).
pub mod tpi {
    pub const CORPSE_TYPE: usize = 0;
    pub const SHADOW_TYPE: usize = 1;
    pub const CULLING_TYPE: usize = 2;
    pub const SPAWN_TYPE: usize = 3;
    pub const SPAWN_COUNT: usize = 4;
    pub const ANIMATION_TYPE: usize = 5;
    pub const CARCASS_SIZE: usize = 6;
    pub const MODEL_TYPE: usize = 7;
    pub const LIGHT_TYPE: usize = 8;
    pub const SWALLOW_COUNT: usize = 9;
    pub const FLICK_PIKI_COUNT1: usize = 10;
    pub const FLICK_PIKI_COUNT2: usize = 11;
    pub const FLICK_PIKI_COUNT3: usize = 12;
    pub const FLICK_DAMAGE_COUNT1: usize = 13;
    pub const FLICK_DAMAGE_COUNT2: usize = 14;
    pub const FLICK_DAMAGE_COUNT3: usize = 15;
    pub const FLICK_DAMAGE_COUNT4: usize = 16;
    pub const SPAWN_PELLET_SCALE_OFF: usize = 17;
    pub const SPAWN_PELLET_FORWARD: usize = 18;
    /// First teki-specific int.
    pub const SPECIFIC: usize = 20;
}

#[derive(Debug, Clone)]
pub struct TekiParams {
    pub version: i32,
    pub ints: Vec<i32>,
    pub floats: Vec<f32>,
}

impl TekiParams {
    /// `TekiParameters::read` (tekiparameters.cpp) for files of version 9 and later, which
    /// store every int then every float. `ints`/`floats` are the type's counts (20 + extra,
    /// 50 + extra), e.g. 20/56 for the Spotty Bulborb and 21/53 for the Dwarf Bulborb.
    pub fn parse(data: &[u8], ints: usize, floats: usize) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let version = r.i32()?;
        if version < 9 {
            return Err(Error::Bad(format!("tekipara version {version} is too old")));
        }
        r.skip(8 * 4)?; // parameter-set IDs
        let ints = (0..ints).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?;
        let mut floats = (0..floats).map(|_| r.f32()).collect::<Result<Vec<_>, _>>()?;
        if version == 9 {
            floats[tpf::COLLISION_RADIUS] = 16.0;
        } else if version == 10 {
            floats[tpf::LOWER_RANGE] = floats[tpf::ATTACK_RANGE];
        }
        Ok(TekiParams {
            version,
            ints,
            floats,
        })
    }

    /// Reads with counts inferred from the file size.
    pub fn parse_any(data: &[u8], ints: usize) -> Result<Self, Error> {
        let floats = (data.len().saturating_sub(4 + 32 + ints * 4)) / 4;
        Self::parse(data, ints, floats)
    }

    pub fn f(&self, i: usize) -> f32 {
        self.floats.get(i).copied().unwrap_or(0.0)
    }

    pub fn i(&self, i: usize) -> i32 {
        self.ints.get(i).copied().unwrap_or(0)
    }
}

/// File paths for a teki type name (`TekiMgr::typeNames`).
pub fn teki_paths(name: &str) -> [String; 4] {
    [
        format!("tekis/{name}/{name}.mod"),
        format!("tekis/{name}/{name}.anm"),
        format!("tekikeys/{name}.key"),
        format!("tekipara/{name}.bin"),
    ]
}
