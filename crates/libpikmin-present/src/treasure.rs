//! Pikmin 2's treasures as things in the Pikmin 1 view: each treasure pellet (`PelletDesc::ship_part` from
//! [`libpikmin::TREASURE_PART_BASE`]) is drawn with its own J3D model from the Pikmin 2 cache
//! (`libsms::extract::p2_treasure`), resting on the ground, wobbling on its `carry.bck` while Pikmin carry it, and
//! sparkling; the sounds it makes are Pikmin 2's. The bakes and the configs are `libsms_present::treasure`.
//!
//! Spec: `Pellet::onCreateShape` / `setupAnim` (pelletMgr.cpp: the model and the looped `carry.bck`),
//! `Pellet::bounceCallback` (the landing sounds, `0x3808 + size` on soil and `0x380B + size` in water, size by
//! `p_radius` 10 / 30), `onyonMgr.cpp` (`PSSE_EV_HOME_PELLET_BACUUM` while the pellet is sucked in and
//! `_FINISH` when it is gone), `itemTreasure.cpp` (`PSSE_EV_TREASURE_JUMP_OUT` when one appears, with `efx::TOtakaraAp`).
//!
//! Host adaptation: a model is drawn at its own scale (Pikmin 2's units are Pikmin 1's); the sparkle and the bursts
//! are stand-ins for JPA2 emitters, drawn like Louie's navi light from the resources' own texture and colours
//! (`libsms_assets::jpa2`). The framebuffer-refracting gems (`Indirect::Yes`) draw their plain texture.

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat4, Vec3};
use libpikmin_assets::model::Batch;
use libsms_assets::jpa2::Jpc;
use libsms_assets::treasure::Table;
use libsms_present::treasure::Treasure;

use crate::effects::Flat;
use crate::navilight::{Glow, glow};
use crate::tev;

/// One treasure's body in the view's tables.
pub struct TreasureBody {
    pub treasure: Treasure,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
}

/// All 188 treasures, by their index in `otakara_config.txt`.
pub struct TreasureSet {
    pub bodies: Vec<TreasureBody>,
}

impl TreasureSet {
    /// Bakes every treasure of the Pikmin 2 cache `dir` (its `treasure/` folder); `first_model` and `first_tex` are
    /// where they will sit in the view's tables, in order.
    pub fn load(dir: &Path, first_model: u32, first_tex: i32) -> Result<Self, String> {
        let root = dir.join("treasure");
        let table = Table::load(&root)?;
        let mut bodies = Vec::with_capacity(table.treasures.len());
        let mut tex = first_tex;
        for (i, c) in table.treasures.iter().enumerate() {
            let treasure = Treasure::load(&root, c)?;
            let next = tex + treasure.textures.list.len() as i32;
            bodies.push(TreasureBody { treasure, model: first_model + i as u32, tex_base: tex });
            tex = next;
        }
        Ok(TreasureSet { bodies })
    }

    pub fn body(&self, index: usize) -> Option<&TreasureBody> {
        self.bodies.get(index)
    }

    /// The model table entry's batches (bind pose, own units).
    pub fn batches(&self, index: usize) -> Vec<Batch> {
        self.body(index).map_or_else(Vec::new, |b| {
            crate::pianta::batches_of(&b.treasure.model, &b.treasure.rig.bind_world(), 1.0)
        })
    }

    pub fn textures(&self, index: usize) -> Vec<tev::Baked> {
        self.body(index).map_or_else(Vec::new, |b| crate::pianta::textures_of(&b.treasure.textures))
    }

    /// The joints: the carry wobble `seconds` into a carry while `carrying`, else the bind pose.
    pub fn joints(&self, index: usize, carrying: bool, seconds: f32) -> Vec<Mat4> {
        let Some(b) = self.body(index) else { return Vec::new() };
        if carrying { b.treasure.pose(b.treasure.carry_frame(seconds.max(0.0))) } else { b.treasure.rig.bind_world() }
    }
}

/// What a treasure's stand-in effects are.
pub struct TreasureFx {
    /// `PID_PelKira_3`, a yellow twinkle.
    sparkle: Glow,
    /// `PID_OtakaraApL_1`, the dust of a treasure appearing.
    dust: Glow,
    /// `PID_UfoPodGeku_2`, the big pale flash of the ship taking it in.
    flash: Glow,
}

pub const SPARKLE: u16 = 0x144;
pub const DUST: u16 = 0x127;
pub const FLASH: u16 = 0x1C5;

/// How long a burst lasts, seconds.
pub const BURST_SECONDS: f32 = 0.8;

/// The kind of burst.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Burst {
    /// A treasure appears (`efx::TOtakaraAp`).
    Appear,
    /// A ship takes a treasure in (`efx::TUfoPodGepu`).
    Delivered,
}

impl TreasureFx {
    /// Reads the three resources from the Pikmin 2 cache `dir`: the shared `effects/game.jpc` when Louie's extraction
    /// put it there, else the treasure extraction's copy.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = [dir.join("effects/game.jpc"), dir.join("treasure/effects/ebisawa_game.jpc")]
            .into_iter().find(|p| p.is_file()).ok_or("no effects/game.jpc")?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let jpc = Jpc::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let need = |id: u16| glow(&jpc, id, 1.0).ok_or_else(|| format!("no resource {id:#x}"));
        Ok(TreasureFx { sparkle: need(SPARKLE)?, dust: need(DUST)?, flash: need(FLASH)? })
    }

    /// Three twinkles circling a treasure of footprint `radius` standing at `feet`, `time` seconds into the run;
    /// `key` (the pellet's id) keeps neighbours out of step.
    pub fn sparkle(&mut self, flat: &mut Flat, feet: Vec3, radius: f32, key: u32, time: f32, right: Vec3, up: Vec3) {
        let phase = key as f32 * 1.7;
        for k in 0..3 {
            let a = time * 1.1 + phase + k as f32 * std::f32::consts::TAU / 3.0;
            let twinkle = 0.5 + 0.5 * (time * 4.3 + phase * 2.0 + k as f32 * 2.1).sin();
            let centre = feet + Vec3::new(a.cos() * radius * 0.8, radius * (0.5 + 0.35 * (a * 1.3).sin()), a.sin() * radius * 0.8);
            self.sparkle.push(flat, centre, radius * (0.1 + 0.14 * twinkle), 0.25 + 0.75 * twinkle, right, up);
        }
    }

    /// A burst `age` seconds old at `centre` for a treasure of footprint `radius`.
    pub fn burst(&mut self, flat: &mut Flat, kind: Burst, centre: Vec3, radius: f32, age: f32, right: Vec3, up: Vec3) {
        let p = (age / BURST_SECONDS).clamp(0.0, 1.0);
        match kind {
            Burst::Appear => self.dust.push(flat, centre + Vec3::Y * radius * 0.3, radius * (0.7 + 0.9 * p), 0.8 * (1.0 - p), right, up),
            Burst::Delivered => self.flash.push(flat, centre, radius * (0.9 + 1.8 * p), 0.9 * (1.0 - p), right, up),
        }
    }
}

/// What the view remembers of each treasure pellet between frames, to find the moments sounds and bursts belong to.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    /// 0 lying, 1 carried, 2 sucked in.
    pub state: u8,
    /// View time the carry began (the wobble's clock), and when the carry sound last played.
    pub carry_began: f32,
    pub last_move_cue: f32,
}

/// A sound for the host's Pikmin 2 output, at a place in sim units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub id: u32,
    pub at: Vec3,
}

/// `PSSE_PK_VC_LIFT_SUCCESS`, `_MOVE` (SoundID.h), the Onion's suction, and its finish; the treasure hopping out.
pub const LIFT: u32 = 0x282D;
pub const MOVE: u32 = 0x282E;
pub const SUCK: u32 = 0x3804;
pub const FINISH: u32 = 0x3805;
pub const APPEAR: u32 = 0x3833;
/// How often the carrying chatter repeats, seconds.
pub const MOVE_EVERY: f32 = 2.5;

/// The landing sound for a treasure of footprint `radius` on soil (`0x3808` + size) or in water (`0x380B` + size);
/// size 0, 1, 2 by `Pellet::bounceCallback`'s p_radius thresholds of 10 and 30.
pub fn landing(radius: f32, water: bool) -> u32 {
    let size = if radius > 30.0 { 2 } else if radius > 10.0 { 1 } else { 0 };
    (if water { 0x380B } else { 0x3808 }) + size
}

/// The moments of one treasure pellet in a frame: `prev` is what the view remembered (None the first time it is
/// seen), `state` the sim's now (0 lying, 1 carried, 2 sucked in, 3 gone). Returns the cues, the burst to start and
/// what to remember (None once it is gone). Pure, so it is tested without a window.
pub fn moments(prev: Option<Track>, state: u8, at: Vec3, radius: f32, now: f32) -> (Vec<Cue>, Option<Burst>, Option<Track>) {
    let mut cues = Vec::new();
    let mut burst = None;
    let Some(before) = prev else {
        // First sight: it has just appeared (a pellet the view meets mid-carry, as a client does, is not announced).
        if state == 0 {
            cues.push(Cue { id: APPEAR, at });
            burst = Some(Burst::Appear);
        }
        let track = Track { state: state.min(2), carry_began: now, last_move_cue: now };
        return if state == 3 { (cues, burst, None) } else { (cues, burst, Some(track)) };
    };
    let mut track = before;
    match (before.state, state) {
        (0, 1) => {
            cues.push(Cue { id: LIFT, at });
            cues.push(Cue { id: MOVE, at });
            track.carry_began = now;
            track.last_move_cue = now;
        }
        (1, 0) => cues.push(Cue { id: landing(radius, false), at }),
        (_, 2) if before.state != 2 => cues.push(Cue { id: SUCK, at }),
        (_, 3) => {
            cues.push(Cue { id: FINISH, at });
            burst = Some(Burst::Delivered);
        }
        _ => {}
    }
    if state == 1 && before.state == 1 && now - before.last_move_cue >= MOVE_EVERY {
        cues.push(Cue { id: MOVE, at });
        track.last_move_cue = now;
    }
    track.state = state.min(2);
    (cues, burst, if state == 3 { None } else { Some(track) })
}

/// The view's memory of the treasure pellets, by pellet id.
pub type Tracks = HashMap<u32, Track>;

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Option<std::path::PathBuf> {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets/p2");
        d.join("treasure/config/otakara_config.txt").is_file().then_some(d)
    }

    #[test]
    fn a_treasures_moments_in_order() {
        let at = Vec3::new(1.0, 2.0, 3.0);
        // It appears.
        let (cues, burst, t) = moments(None, 0, at, 35.0, 0.0);
        assert_eq!(cues, vec![Cue { id: APPEAR, at }]);
        assert_eq!(burst, Some(Burst::Appear));
        // Lying still says nothing.
        let (cues, burst, t) = moments(t, 0, at, 35.0, 1.0);
        assert!(cues.is_empty() && burst.is_none());
        // Pikmin lift it: the lift and the first carrying chatter.
        let (cues, _, t) = moments(t, 1, at, 35.0, 2.0);
        assert_eq!(cues.iter().map(|c| c.id).collect::<Vec<_>>(), vec![LIFT, MOVE]);
        // The chatter repeats, not every frame.
        let (cues, _, t) = moments(t, 1, at, 35.0, 3.0);
        assert!(cues.is_empty());
        let (cues, _, t) = moments(t, 1, at, 35.0, 2.0 + MOVE_EVERY + 0.01);
        assert_eq!(cues.iter().map(|c| c.id).collect::<Vec<_>>(), vec![MOVE]);
        // Dropped: it lands (big, so 0x380A).
        let (cues, _, t) = moments(t, 0, at, 35.0, 6.0);
        assert_eq!(cues.iter().map(|c| c.id).collect::<Vec<_>>(), vec![0x380A]);
        // Lifted again, then sucked in, then gone: the suction once, the finish with a flash, and no memory left.
        let (_, _, t) = moments(t, 1, at, 35.0, 7.0);
        let (cues, _, t) = moments(t, 2, at, 35.0, 8.0);
        assert_eq!(cues.iter().map(|c| c.id).collect::<Vec<_>>(), vec![SUCK]);
        let (cues, _, t) = moments(t, 2, at, 35.0, 8.1);
        assert!(cues.is_empty());
        let (cues, burst, t) = moments(t, 3, at, 35.0, 9.0);
        assert_eq!(cues.iter().map(|c| c.id).collect::<Vec<_>>(), vec![FINISH]);
        assert_eq!(burst, Some(Burst::Delivered));
        assert!(t.is_none());
        // A client meets a pellet mid-carry: no appearance.
        let (cues, burst, _) = moments(None, 1, at, 35.0, 0.0);
        assert!(cues.is_empty() && burst.is_none());
    }

    #[test]
    fn landing_sounds_follow_size() {
        assert_eq!((landing(8.0, false), landing(20.0, false), landing(50.0, false)), (0x3808, 0x3809, 0x380A));
        assert_eq!((landing(8.0, true), landing(20.0, true), landing(50.0, true)), (0x380B, 0x380C, 0x380D));
    }

    #[test]
    fn every_treasure_loads_into_the_view_tables() {
        let Some(dir) = dir() else { eprintln!("SKIP: no Pikmin 2 treasure cache"); return };
        let set = TreasureSet::load(&dir, 100, 50).expect("load");
        assert_eq!(set.bodies.len(), 188);
        let mut tex = 50;
        for (i, b) in set.bodies.iter().enumerate() {
            assert_eq!((b.model, b.tex_base), (100 + i as u32, tex), "treasure {i}");
            tex += b.treasure.textures.list.len() as i32;
            assert!(!set.batches(i).is_empty(), "treasure {i}");
            assert_eq!(set.textures(i).len(), b.treasure.textures.list.len());
            for (carrying, t) in [(false, 0.0), (true, 0.0), (true, 1.7), (true, 90.0)] {
                let j = set.joints(i, carrying, t);
                assert_eq!(j.len(), b.treasure.rig.len());
                assert!(j.iter().all(|m| m.is_finite()), "treasure {i}");
            }
        }
        assert!(set.body(188).is_none() && set.joints(188, true, 0.0).is_empty());
    }

    #[test]
    fn the_stand_in_effects_draw_additive_layers() {
        let Some(dir) = dir() else { eprintln!("SKIP: no Pikmin 2 treasure cache"); return };
        let mut fx = TreasureFx::load(&dir).expect("effects");
        let mut flat = Flat::default();
        fx.sparkle(&mut flat, Vec3::ZERO, 30.0, 7, 0.3, Vec3::X, Vec3::Y);
        assert_eq!(flat.quads.len(), 6, "three twinkles of two layers");
        fx.burst(&mut flat, Burst::Appear, Vec3::ZERO, 30.0, 0.2, Vec3::X, Vec3::Y);
        fx.burst(&mut flat, Burst::Delivered, Vec3::ZERO, 30.0, 0.2, Vec3::X, Vec3::Y);
        assert_eq!(flat.quads.len(), 10);
        assert!(flat.quads.iter().all(|q| q.additive && q.color.iter().all(|c| c.is_finite()) && q.corners.iter().all(|c| c.is_finite())));
        // The burst has faded by its end.
        let before = flat.quads.len();
        fx.burst(&mut flat, Burst::Delivered, Vec3::ZERO, 30.0, BURST_SECONDS, Vec3::X, Vec3::Y);
        assert!(flat.quads[before..].iter().all(|q| q.color[..3].iter().all(|c| *c == 0.0)));
    }
}
