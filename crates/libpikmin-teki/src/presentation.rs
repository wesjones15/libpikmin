//! Presentation bits shared by every host; activation from the source state, not guessed motion names.
use crate::{Teki, p2};
pub const VALID: u8 = 128;
pub const FIRE: u8 = 1;
pub const GAS: u8 = 2;
pub const SLEEP: u8 = 4;
pub const HAIR: u8 = 8;
pub const PINCH: u8 = 16;
pub const SHOT: u8 = 32;
pub const DEAD: u8 = 64;
pub fn flags(t: &Teki) -> u8 {
    let mut f = VALID;
    if !t.alive() {
        f |= DEAD;
    }
    if t.kind == p2::FIERY_BULBLAX && t.alive() && !t.flags[3] {
        f |= FIRE;
    }
    if t.kind == p2::GAS_PIPE && t.state == crate::gashiba::ATTACK {
        f |= GAS;
    }
    if matches!(
        t.kind,
        p2::ORANGE_BULBORB | p2::HAIRY_BULBORB | p2::FIERY_BULBLAX
    ) && t.state == crate::bulborb2::SLEEP
    {
        f |= SLEEP;
    }
    if t.kind == p2::HAIRY_BULBORB && t.alive() && t.hp > t.f(p2::fp::LIFE) * 0.5 {
        f |= HAIR;
    }
    // BigFoot::updatePinchLife / Damagumo::updatePinchLife.
    if crate::longlegs::is_long_legs(t.kind) && t.alive() && t.hp < t.f(p2::fp::LIFE) * 0.35 {
        f |= PINCH;
    }
    if t.kind == p2::MAN_AT_LEGS && t.state == crate::houdai::SHOT {
        f |= SHOT;
    }
    f
}
pub fn sound_id(name: &str) -> Option<u32> {
    Some(match name {
        "HOUDAI_BEAM" => 0x3055,
        "HOUDAI_WATER_IMPACT" => 0x5805,
        "DAMAGUMO_SMOKE" => 0x5844,
        "BIGFOOT_RAISE1" => 0x5848,
        "BIGFOOT_RAISE2" => 0x5849,
        "BIGFOOT_RAISE3" => 0x584a,
        "BIGFOOT_RAISE_FAST" => 0x584b,
        "FIRE_CHAPPY_FLAME" => 0x50b4,
        "FIRE_CHAPPY_F_END" => 0x58b5,
        _ => return None,
    })
}
