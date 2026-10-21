//! `EffectMgr` ids the enemies use (EffectMgr.h `EFF_*`); the particle files are libpikmin-assets'
//! `effect_table`.

pub const COLLEC_ASHI: usize = 93;
pub const COLLEC_HIKI: usize = 94;
pub const COLLEC_ST_A: usize = 95;
pub const COLLEC_ST_B: usize = 96;
pub const COLLEC_HM_A: usize = 97;
pub const COLLEC_HM_B: usize = 98;

pub const BUBBLES: usize = 15;
pub const KINOKO_DEATH_SPORES: usize = 148;
pub const KINOKO_DEATH_SMOKE: usize = 149;
pub const KINOKO_CHARGE_SPORES: usize = 150;
pub const KINOKO_SPORES_UP: usize = 151;
pub const KINOKO_ATTACK_CLOUD: usize = 152;
pub const KINOKO_POST_ATTACK_CLOUD: usize = 153;
pub const KINOKO_ATTACK_SPORES: usize = 154;
pub const KINOKO_POST_ATTACK_SPORES: usize = 155;
pub const FROG_WATER_2: usize = 84;
pub const FROG_BUBBLE_RING_S: usize = 85;
pub const DIRT_CLOUD: usize = 50;
pub const DIRT_SPRAY: usize = 51;
pub const KABEKUI_EAT_BRIDGE_A: usize = 79;
pub const KABEKUI_EAT_BRIDGE_B: usize = 80;
pub const BEATLE_FLICK1: usize = 136;
pub const BEATLE_SHOOT_ROCK_HALO: usize = 137;
pub const BEATLE_FLICK3: usize = 140;
pub const BEATLE_ROCK_BLAST: usize = 146;
pub const BIG_DUST_RING: usize = 224;
pub const SMOKE_RING_M: usize = 67;
pub const SMOKE_RING_S: usize = 68;

/// `TekiStrategy::createEffect(teki, index)`: what a `KEY_PlayEffect` event of a kind's animation makes.
pub fn keyed(kind: u8, index: u8) -> Vec<usize> {
    use crate::info::*;
    match (kind, index) {
        (PUFFSTOOL, 0) => vec![KINOKO_SPORES_UP],
        (PUFFSTOOL, 1) => vec![KINOKO_ATTACK_SPORES, KINOKO_ATTACK_CLOUD],
        (PUFFSTOOL, 2) => vec![KINOKO_POST_ATTACK_SPORES, KINOKO_POST_ATTACK_CLOUD],
        (PUFFSTOOL, 3) => vec![KINOKO_DEATH_SPORES, KINOKO_DEATH_SMOKE],
        _ => Vec::new(),
    }
}

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
