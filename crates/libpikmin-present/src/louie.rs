//! Olimar's Louie alt-skin: Pikmin 2's Louie (`orima_model/orima3.bmd`, from the user's own Pikmin 2 disc
//! through `libsms::extract::p2`) drawn in place of Olimar, posed by Louie's own motions (`motion/*.bca`).
//! Everything else about the captain stays Olimar's: his sim, his jump, his whistle and his throws.
//!
//! Which Pikmin 2 motion plays follows what Olimar's motion would be (`animator::captain_motion`), by the
//! same names the two games share (`wait`, `walk`, `run2`, `throw`, `nuku`, `fue`, `punch`, `damage`, `dead`,
//! `jump`). A motion's progress is Olimar's: where his clip is in its key range, Louie's is in his. Host
//! adaptation; the sim, its timing and the effects are Pikmin 1's. The lowest skinned vertex is put on the
//! ground each frame.

use std::path::Path;

use glam::{Mat4, Vec3};
use libpikmin::Captain;
use libpikmin_assets::Model;
use libpikmin_assets::model::Batch;
use libpikmin_assets::motion::Motion;
use libsms_present::body::Louie;

use crate::tev;

/// Skin id in the host's order (`pik_view_set_captain_skin`): between Olimar (0) and the Pianta (2).
pub const SKIN_LOUIE: u8 = 1;

pub struct LouieSkin {
    pub body: Louie,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
    /// Sim units per Pikmin 2 unit: Louie stands as tall as Olimar (times `ratio`).
    pub scale: f32,
    olimar_height: f32,
    ratio: f32,
}

/// The Pikmin 2 motion for one of Olimar's, and whether it holds its last frame instead of looping with
/// Olimar's clock (`once`).
fn motion_name(motion: Motion) -> &'static str {
    match motion {
        Motion::Wait => "wait",
        Motion::Asibumi => "asibumi",
        Motion::Walk => "walk",
        Motion::Run | Motion::Nigeru => "run2",
        Motion::Throw => "throw",
        Motion::ThrowWait => "trwwait",
        Motion::Nuku | Motion::NukuFast => "nuku",
        Motion::Fue => "fue",
        Motion::Punch => "punch",
        Motion::Damage => "damage",
        Motion::ODead => "dead",
        Motion::Fall => "fall",
        Motion::RollJmp => "rolljmp",
        Motion::OCarry | Motion::Jump | Motion::StillJump => "jump",
        _ => "wait",
    }
}

/// What Louie says: Pikmin 2 sound effect ids (`SoundID.h` in the decomp, `PSSE_PL_*`; the `_LUI` / `_LUGI` ones are his
/// own lines). `None` for the cries he does not have (his jump is silent, like Olimar's in Pikmin 2).
pub const fn voice_id(voice: crate::pianta::Voice) -> Option<u32> {
    use crate::pianta::Voice;
    match voice {
        Voice::Jump => None,
        // `PSSE_PL_THROW`: the player's throw sound.
        Voice::Throw => Some(0x806),
        // `PSSE_PL_DAMAGE_LUI`.
        Voice::Hurt => Some(0x891),
        // `PSSE_PL_DOOM_LUGI`.
        Voice::Down => Some(0x871),
        // `PSSE_PL_SYUGO_LUI` (the assembly call) and `PSSE_PL_KAISAN_LUI` (dismissal). The name table has the call at
        // 0x9F, a held sound the game re-requests every frame while the whistle blows (it dies after one frame
        // otherwise); with the 0x800 bit, as here, the same entry plays through once.
        Voice::Whistle => Some(0x89F),
        Voice::Dismiss => Some(0x8A1),
        // `PSSE_PL_PUNCH_LUI`.
        Voice::Punch => Some(0x88E),
    }
}

impl LouieSkin {
    pub fn load(dir: &Path, model: u32, tex_base: i32, olimar_height: f32) -> Result<Self, String> {
        let body = Louie::load(dir)?;
        let mut skin = LouieSkin { body, model, tex_base, scale: 1.0, olimar_height, ratio: 1.0 };
        skin.refit();
        Ok(skin)
    }

    fn refit(&mut self) {
        self.scale = self.olimar_height * self.ratio / self.body.height().max(1e-3);
    }

    /// Stand `ratio` times as tall as Olimar.
    pub fn set_height_ratio(&mut self, ratio: f32) {
        if ratio.is_finite() && ratio > 0.1 {
            self.ratio = ratio;
            self.refit();
        }
    }

    /// His joints this frame, in his own units. `phase` is how far through its key range the captain's
    /// current Olimar clip is (0..1); a jump runs on the jump's own seconds.
    pub fn joints(&self, c: &Captain, motion: Motion, phase: f32) -> Vec<Mat4> {
        let name = if c.jump.is_some() && !matches!(motion, Motion::RollJmp) { "jump" } else { motion_name(motion) };
        let name = if self.body.clip_frames(name).is_some() { name } else { "wait" };
        let frames = self.body.clip_frames(name).unwrap_or(1.0);
        let frame = match c.jump {
            // 30 frames a second, held on the last frame while he is still up.
            Some(j) if name == "jump" => (j.elapsed * 30.0).min(frames),
            _ => phase.clamp(0.0, 1.0) * frames,
        };
        self.body.pose(name, frame).unwrap_or_else(|| self.body.bind_world())
    }

    /// The model-table entry's batches for the host.
    pub fn batches(&self) -> Vec<Batch> {
        crate::pianta::batches_of(&self.body.model, &self.body.bind_world(), self.scale)
    }

    pub fn textures(&self) -> Vec<tev::Baked> {
        crate::pianta::textures_of(&self.body.textures)
    }

    /// Bind-pose bounds in sim units, for the model table.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.body.model.lo * self.scale, self.body.model.hi * self.scale)
    }
}

/// Olimar's bind height from his model, for fitting Louie to it.
pub fn olimar_height(model: &Model) -> f32 {
    let skel = libpikmin_assets::Skeleton::new(model);
    let pose = skel.pose(Mat4::IDENTITY, None);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    if let Ok(batches) = model.batches() {
        for b in &batches {
            for v in &b.vertices {
                let y = pose.skin(v).0.y;
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
    }
    if hi > lo { hi - lo } else { 25.1 }
}
