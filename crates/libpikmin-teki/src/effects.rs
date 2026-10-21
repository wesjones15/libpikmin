//! `EffectMgr` ids the enemies use (EffectMgr.h `EFF_*`); the particle files are libpikmin-assets'
//! `effect_table`.

pub const COLLEC_ASHI: usize = 93;
pub const COLLEC_HIKI: usize = 94;
pub const COLLEC_ST_A: usize = 95;
pub const COLLEC_ST_B: usize = 96;
pub const COLLEC_HM_A: usize = 97;
pub const COLLEC_HM_B: usize = 98;

pub const BEATLE_FLICK1: usize = 136;
pub const BEATLE_SHOOT_ROCK_HALO: usize = 137;
pub const BEATLE_FLICK3: usize = 140;
pub const BEATLE_ROCK_BLAST: usize = 146;
pub const BIG_DUST_RING: usize = 224;
pub const SMOKE_RING_M: usize = 67;
pub const SMOKE_RING_S: usize = 68;

/// `BTeki::createDeadEffect`: wave, glow and smoke by `TPI_CarcassSize` (0 large, 1 medium, 2 small).
pub fn death(carcass: i32) -> Vec<usize> {
    match carcass {
        2 => vec![61, 62, 63],
        1 => vec![58, 59, 60],
        0 => vec![55, 56, 57],
        _ => Vec::new(),
    }
}

/// `BTeki::createSoulEffect`.
pub fn soul(carcass: i32) -> Option<usize> {
    match carcass {
        2 => Some(66),
        1 => Some(65),
        0 => Some(64),
        _ => None,
    }
}
