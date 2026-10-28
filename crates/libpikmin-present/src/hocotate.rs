//! Louie's ship: the Hocotate Ship from Pikmin 2 (`user/Kando/ufo/arc.szs`: `ufo.bmd` with `pmotion1-3.bck` for
//! the pot, radar and speaker, `suction.bck` for taking in an item), drawn in place of the Dolphin for ships whose
//! owner wears Louie. Only the body changes: where the ship lands, hovers and takes off, where it receives parts and
//! all its sounds and effects are the Dolphin's, so it stands whole (the Pikmin 1 ship's wreck and 30 parts are not
//! shown on it).
//!
//! It is sized to the Dolphin and put on the ground where the Dolphin's contact point is, turned so its `goal` joint
//! (where Pikmin 2's ship takes things in) faces the host's goal spot.

use std::path::Path;

use glam::{Mat4, Vec3};
use libpikmin_assets::model::Batch;
use libsms_present::body::Louie;

use crate::tev;

/// The model and clips, relative to the Pikmin 2 cache.
pub const MODEL: &str = "ufo/ufo.bmd";
pub const MOTIONS: &str = "ufo";

pub struct HocotateSkin {
    pub body: Louie,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
    /// Yaw of the `goal` joint from the `ctr` joint in the bind pose: how far the model must turn to face its goal spot
    /// along +Z (the Dolphin's convention).
    pub goal_yaw: f32,
}

impl HocotateSkin {
    pub fn load(dir: &Path, model: u32, tex_base: i32) -> Result<Self, String> {
        let body = Louie::load_with(dir, MODEL, MOTIONS)?;
        let bind = body.bind_world();
        let at = |name: &str| body.rig.find(name).and_then(|j| bind.get(j)).map(|m| m.w_axis.truncate());
        let goal_yaw = match (at("goal"), at("ctr")) {
            (Some(g), Some(c)) => {
                let d = g - c;
                if d.x.hypot(d.z) > 1.0 { d.x.atan2(d.z) } else { 0.0 }
            }
            _ => 0.0,
        };
        Ok(HocotateSkin { body, model, tex_base, goal_yaw })
    }

    /// Height of the ship in its own units.
    pub fn height(&self) -> f32 {
        self.body.height()
    }

    /// The joints at `time` seconds: the pot takes an item in (`suction`) while `sucking`, else the bind pose.
    pub fn joints(&self, sucking: bool, time: f32) -> Vec<Mat4> {
        if sucking
            && let Some(frames) = self.body.clip_frames("suction")
        {
            return self.body.pose("suction", (time * 30.0) % frames.max(1.0)).unwrap_or_else(|| self.body.bind_world());
        }
        self.body.bind_world()
    }

    /// The model-table entry's batches for the host (at the bind pose, unscaled).
    pub fn batches(&self) -> Vec<Batch> {
        crate::pianta::batches_of(&self.body.model, &self.body.bind_world(), 1.0)
    }

    pub fn textures(&self) -> Vec<tev::Baked> {
        crate::pianta::textures_of(&self.body.textures)
    }

    /// Bind-pose bounds, for the model table.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.body.model.lo, self.body.model.hi)
    }

    /// Lowest point of the bind pose: the hull's contact with the ground.
    pub fn bottom(&self) -> f32 {
        self.body.model.lo.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ship_loads_and_poses() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets/p2");
        if !dir.join(MODEL).is_file() {
            eprintln!("SKIP: no Pikmin 2 ship");
            return;
        }
        let ship = HocotateSkin::load(&dir, 0, 0).expect("load");
        assert!(ship.height() > 100.0 && ship.height() < 200.0, "{}", ship.height());
        assert!(ship.goal_yaw.is_finite());
        for (sucking, t) in [(false, 0.0), (true, 0.0), (true, 0.5)] {
            let j = ship.joints(sucking, t);
            assert_eq!(j.len(), ship.body.rig.len());
            assert!(j.iter().all(|m| m.is_finite()));
        }
        assert!(!ship.batches().is_empty());
    }
}
