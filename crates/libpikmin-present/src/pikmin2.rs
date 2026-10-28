//! Purple and White Pikmin, from Pikmin 2: `piki_p2_black.bmd` and `piki_p2_white.bmd` (`PikiMgr::loadBmd`), from the
//! user's own Pikmin 2 disc through `libsms::extract::p2`, drawn on their own motions (`motion/*.bca`; the
//! Pikmin 2 Pikmin share one 11-joint rig and the names Pikmin 1 uses: `wait`, `walk`, `run2`, `nuku`, ...).
//!
//! Which motion plays follows the Pikmin 1 motion the animator picked for the same state
//! (`animator::piki_look`), by name; how far through it is that clip's progress, so attack and carry timing keep
//! the sim's. Host adaptation: the sim is Pikmin 1's. The leaf, bud or flower sits on `happajnt3` (joint 8).
//! They stand as tall as a blue Pikmin times `mNewColorScale` (`Params::p2`). Without the Pikmin 2 files the
//! view draws them as tinted Pikmin 1 bodies.

use std::path::Path;

use glam::{Mat4, Vec3};
use libpikmin::PikiColor;
use libpikmin_assets::model::Batch;
use libpikmin_assets::motion::Motion;
use libsms_present::body::{Louie, PIKMIN2_FILES};

use crate::tev;

/// Which joint carries the leaf, bud or flower: `happajnt3`.
pub const HAPPA_JOINT: usize = 8;

/// One Purple or White body.
pub struct Body {
    pub body: Louie,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
}

pub struct Pikmin2Skin {
    pub purple: Body,
    pub white: Body,
    /// A blue Pikmin 1 Pikmin's height in sim units, which these are fitted to.
    p1_height: f32,
}

/// The Pikmin 2 motion for one of Pikmin 1's, by the names the two games share.
pub fn motion_name(motion: Motion) -> &'static str {
    match motion {
        Motion::Run => "run2",
        Motion::Nigeru => "nigeru",
        Motion::Walk => "walk",
        Motion::Wait | Motion::Tanemaki => "wait",
        Motion::Pick => "pick_put",
        Motion::PickLoop => "pickloop",
        Motion::Nuku | Motion::NukuFast => "nuku",
        Motion::Nukareru => "nukareru",
        Motion::NukareFast => "nukare3",
        Motion::Dead | Motion::ODead => "dead",
        Motion::Dead2 => "dead2",
        Motion::Dead3 => "dead3",
        Motion::Damage => "damage",
        Motion::Asibumi => "asibumi",
        Motion::GrowUp1 => "grow_up1",
        Motion::GrowUp2 => "grow_up2",
        Motion::Korobu => "korobu",
        Motion::Jump | Motion::StillJump | Motion::OCarry | Motion::WaveJmp => "jump",
        Motion::Attack => "attack1",
        Motion::Punch => "punch",
        Motion::Throw => "throw",
        Motion::ThrowWait => "trwwait",
        Motion::Hang => "hang",
        Motion::Fall => "fall",
        Motion::JKoke => "jkoke",
        Motion::JHit => "jhit",
        Motion::GetUp => "getup",
        Motion::RollJmp | Motion::RotJump => "rolljmp",
        Motion::Akubi => "akubi",
        Motion::Mizunomi => "mizunomi",
        Motion::Kuttuku => "kuttuku",
        Motion::Fue => "fue",
        Motion::HNoboru | Motion::Noboru => "hnoboru",
        Motion::Press1 | Motion::SPress => "wait",
    }
}

impl Pikmin2Skin {
    /// Loads both bodies from the Pikmin 2 cache `dir`; `first_model` and `first_tex` are where they will sit in
    /// the view's tables (purple, then white), `p1_height` a blue Pikmin's height.
    pub fn load(dir: &Path, first_model: u32, first_tex: i32, p1_height: f32) -> Result<Self, String> {
        let purple = Louie::load_from(dir, PIKMIN2_FILES[0])?;
        let white = Louie::load_from(dir, PIKMIN2_FILES[1])?;
        let purple_tex = first_tex;
        let white_tex = first_tex + purple.textures.list.len() as i32;
        Ok(Pikmin2Skin {
            purple: Body { body: purple, model: first_model, tex_base: purple_tex },
            white: Body { body: white, model: first_model + 1, tex_base: white_tex },
            p1_height,
        })
    }

    /// The body that draws `color`, if it is one of Pikmin 2's.
    pub fn body(&self, color: PikiColor) -> Option<&Body> {
        match color {
            PikiColor::Purple => Some(&self.purple),
            PikiColor::White => Some(&self.white),
            _ => None,
        }
    }

    /// Sim units per Pikmin 2 unit when `new_colour_scale` is `mNewColorScale`.
    pub fn scale(&self, color: PikiColor, new_colour_scale: f32) -> f32 {
        self.body(color).map_or(1.0, |b| self.p1_height * new_colour_scale / b.body.height().max(1e-3))
    }

    /// The joints (model space, its own units) for `color` playing `motion`, `phase` (0..1) through it.
    pub fn joints(&self, color: PikiColor, motion: Motion, phase: f32) -> Vec<Mat4> {
        let Some(b) = self.body(color) else { return Vec::new() };
        let name = motion_name(motion);
        let name = if b.body.clip_frames(name).is_some() { name } else { "wait" };
        let frames = b.body.clip_frames(name).unwrap_or(1.0);
        b.body.pose(name, phase.clamp(0.0, 1.0) * frames).unwrap_or_else(|| b.body.bind_world())
    }

    /// The model-table entry's batches for the host.
    pub fn batches(&self, color: PikiColor, new_colour_scale: f32) -> Vec<Batch> {
        let Some(b) = self.body(color) else { return Vec::new() };
        crate::pianta::batches_of(&b.body.model, &b.body.bind_world(), self.scale(color, new_colour_scale))
    }

    pub fn textures(&self, color: PikiColor) -> Vec<tev::Baked> {
        self.body(color).map_or_else(Vec::new, |b| crate::pianta::textures_of(&b.body.textures))
    }

    /// Bind-pose bounds in sim units, for the model table.
    pub fn bounds(&self, color: PikiColor, new_colour_scale: f32) -> (Vec3, Vec3) {
        let s = self.scale(color, new_colour_scale);
        self.body(color).map_or((Vec3::ZERO, Vec3::ZERO), |b| (b.body.model.lo * s, b.body.model.hi * s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_motion_has_a_pikmin2_clip() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/p2");
        if !PIKMIN2_FILES.iter().all(|f| dir.join(f).is_file()) {
            eprintln!("SKIP: no Pikmin 2 cache");
            return;
        }
        let b = Louie::load_from(&dir, PIKMIN2_FILES[0]).unwrap();
        for m in [
            Motion::Run, Motion::Nigeru, Motion::Walk, Motion::Wait, Motion::Pick, Motion::Nuku, Motion::Nukareru,
            Motion::Dead, Motion::Dead2, Motion::Dead3, Motion::Damage, Motion::Asibumi, Motion::OCarry,
            Motion::Tanemaki, Motion::GrowUp1, Motion::GrowUp2, Motion::Korobu, Motion::Jump, Motion::StillJump,
            Motion::Attack, Motion::Punch, Motion::Throw, Motion::Hang, Motion::Fall, Motion::JKoke, Motion::JHit,
            Motion::GetUp, Motion::RollJmp, Motion::WaveJmp, Motion::ThrowWait, Motion::Akubi, Motion::PickLoop,
            Motion::Mizunomi, Motion::Kuttuku, Motion::Fue, Motion::Press1, Motion::SPress, Motion::HNoboru,
            Motion::Noboru, Motion::RotJump, Motion::NukuFast, Motion::NukareFast, Motion::ODead,
        ] {
            assert!(b.clip_frames(motion_name(m)).is_some(), "no clip {} for {m:?}", motion_name(m));
        }
    }
}
