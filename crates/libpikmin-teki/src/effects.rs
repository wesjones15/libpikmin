//! `EffectMgr` ids the enemies use (EffectMgr.h `EFF_*`); the particle files are libpikmin-assets'
//! `effect_table`.

pub const COLLEC_ASHI: usize = 93;
pub const COLLEC_HIKI: usize = 94;
pub const COLLEC_ST_A: usize = 95;
pub const COLLEC_ST_B: usize = 96;
pub const COLLEC_HM_A: usize = 97;
pub const COLLEC_HM_B: usize = 98;

pub const BUBBLES: usize = 15;
pub const GROW_PELLET_A1: usize = 70;
pub const GROW_PELLET_A2: usize = 71;
pub const GROW_PELLET_B: usize = 72;
pub const GROW_PELLET_C: usize = 73;
pub const HIBA_FIRE: usize = 227;
pub const SHELL_BOOM: usize = 91;
pub const SHELL_TWINKLE: usize = 92;
pub const DORORO_EYE: usize = 157;
pub const DORORO_DEAD_FRAGMENTS: usize = 162;
pub const DORORO_HOWL_ZIGZAGS: usize = 163;
pub const DORORO_HOWL_RIPPLES: usize = 164;
pub const DORORO_HOWL_HALO: usize = 165;
pub const DORORO_DEAD_FUMES: usize = 166;
pub const TAMAGO_DEAD_FUMES: usize = 167;
pub const TAMAGO_EYE_GLOW: usize = 168;
pub const TAMAGO_DEAD_FRAGMENTS: usize = 169;
pub const TAMAGO_DEAD_SMOKE: usize = 170;
pub const DORORO_BODY_FRAGMENTS: usize = 171;
pub const DORORO_BODY_SPECKS: usize = 172;
pub const DORORO_BODY_TRAIL: usize = 173;
pub const DORORO_BODY_SMOKE: usize = 175;
pub const MAR_WIND_JET: usize = 176;
pub const MAR_DEAD_SMOKE: usize = 179;
pub const MAR_DEAD_JET: usize = 180;
pub const ONYON_BUBBLES: usize = 237;
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
        (SMOKY_PROGG, 0..=2) => vec![SMOKE_RING_M],
        (PELLET_POSY, 0) => vec![GROW_PELLET_A1],
        (PELLET_POSY, 1) => vec![GROW_PELLET_A2],
        (PELLET_POSY, 2) => vec![GROW_PELLET_B, GROW_PELLET_C],
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
