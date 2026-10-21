//! `EffectMgr` ids the enemies use (EffectMgr.h `EFF_*`); the particle files are libpikmin-assets'
//! `effect_table`.

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
