//! Candypop Buds in the Pikmin 1 view: Pikmin 2's `Pom` model (`enemy/data/Pom/model/enemy.bmd`, from
//! `libsms::extract::p2_enemy`), with the petals in the bud's colour, posed on the disc's own clips for the bud's phase
//! (`PomState.cpp`: `type1` opens, `type2` closes, `type3` spits, `type4` swings, `dead` withers; the closed bud is
//! `wait`). The rules are `libpikmin::bulb`.
//!
//! Spec: `Pom::Obj::setPomColor` and `changeMaterial` (Pom.cpp): the petal material `hanabira1_v` has TEV colour 0 set to
//! the bud's `mRgbColor` (Blue 50,50,255, Red 255,20,20, Yellow 255,255,20, Purple 28,0,52, White 200,255,220).
//!
//! Host adaptation: the game recolours the material at run time; here one body per colour is baked from a copy of the
//! model whose colour table entry for that material is rewritten (the copies are written beside the model in the
//! ignored cache, once).

use std::path::Path;

use glam::Mat4;
use libpikmin::bulb::{Bulb, BulbState};
use libpikmin::PikiColor;
use libpikmin_assets::model::Batch;
use libsms_present::enemy::EnemyBody;

use crate::tev;

const MODEL: &str = "enemy/data/Pom/model/enemy.bmd";
const CLIPS: &str = "enemy/data/Pom/anim";
/// The petal material's colour table entry, as the model has it (red, alpha 0), as big-endian shorts.
const PETAL: [u8; 8] = [0, 255, 0, 20, 0, 20, 0, 0];

/// `Pom::Obj::setPomColor`'s `mRgbColor` for each colour (Blue, Red, Yellow, Purple, White).
pub const PETAL_COLOURS: [[u8; 3]; 5] = [[50, 50, 255], [255, 20, 20], [255, 255, 20], [28, 0, 52], [200, 255, 220]];

/// One colour's body in the view's tables.
pub struct BulbBody {
    pub body: EnemyBody,
    pub model: u32,
    pub tex_base: i32,
}

/// The bud in each of Pikmin's five colours, by [`PikiColor::index`].
pub struct BulbSet {
    pub bodies: Vec<BulbBody>,
}

/// The model's bytes with the petals' colour table entry rewritten to `rgb`.
pub fn recoloured(model: &[u8], rgb: [u8; 3]) -> Result<Vec<u8>, String> {
    let at: Vec<usize> = model.windows(PETAL.len()).enumerate().filter(|(_, w)| *w == PETAL).map(|(i, _)| i).collect();
    let [at] = at[..] else { return Err(format!("the petal colour appears {} times in the model", at.len())) };
    let mut out = model.to_vec();
    for (k, c) in rgb.iter().enumerate() {
        out[at + 2 * k] = 0;
        out[at + 2 * k + 1] = *c;
    }
    Ok(out)
}

impl BulbSet {
    /// Whether the Pikmin 2 cache `dir` has the files.
    pub fn available(dir: &Path) -> bool {
        dir.join(MODEL).is_file() && dir.join(CLIPS).join("type1.bca").is_file()
    }

    /// Bakes the five bodies; `first_model` and `first_tex` are where they will sit in the view's tables, in colour order.
    pub fn load(dir: &Path, first_model: u32, first_tex: i32) -> Result<Self, String> {
        let original = std::fs::read(dir.join(MODEL)).map_err(|e| format!("{MODEL}: {e}"))?;
        let mut bodies = Vec::new();
        let mut tex = first_tex;
        for (i, rgb) in PETAL_COLOURS.iter().enumerate() {
            let rel = format!("enemy/data/Pom/model/enemy_c{i}.bmd");
            let path = dir.join(&rel);
            if !path.is_file() {
                std::fs::write(&path, recoloured(&original, *rgb)?).map_err(|e| format!("{rel}: {e}"))?;
            }
            let body = EnemyBody::load_swapped(dir, &rel, CLIPS, &[])?;
            let next = tex + body.textures.list.len() as i32;
            bodies.push(BulbBody { body, model: first_model + i as u32, tex_base: tex });
            tex = next;
        }
        Ok(BulbSet { bodies })
    }

    pub fn body(&self, colour: PikiColor) -> Option<&BulbBody> {
        self.bodies.get(colour.index())
    }

    pub fn batches(&self, index: usize) -> Vec<Batch> {
        self.bodies.get(index).map_or_else(Vec::new, |b| crate::pianta::batches_of(&b.body.model, &b.body.bind_world(), 1.0))
    }

    pub fn textures(&self, index: usize) -> Vec<tev::Baked> {
        self.bodies.get(index).map_or_else(Vec::new, |b| crate::pianta::textures_of(&b.body.textures))
    }

    /// The clip and frame a bud in `state` shows, 30 frames a second.
    pub fn clip(state: BulbState) -> (&'static str, f32) {
        match state {
            BulbState::Open { t } => ("type1", t * 30.0),
            // The swing plays when a Pikmin has just gone in, then the bud stands open as the open clip ends.
            BulbState::Swing { t } => if t * 30.0 < 20.0 { ("type4", t * 30.0) } else { ("type1", f32::MAX) },
            BulbState::Close { t } => ("type2", t * 30.0),
            BulbState::Shot { t, .. } => ("type3", t * 30.0),
            BulbState::Dead { t } => ("dead", t * 30.0),
        }
    }

    /// The joints of `bulb` now, model space.
    pub fn joints(&self, bulb: &Bulb) -> Vec<Mat4> {
        let Some(b) = self.body(bulb.colour) else { return Vec::new() };
        let (name, frame) = Self::clip(bulb.state);
        let frames = b.body.clip_frames(name).unwrap_or(1.0);
        b.body.pose(name, frame.min((frames - 1.0).max(0.0))).unwrap_or_else(|| b.body.bind_world())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libpikmin_assets::Assets;

    fn dir() -> Option<std::path::PathBuf> {
        let dir = Assets::default_dir().join("p2");
        BulbSet::available(&dir).then_some(dir)
    }

    #[test]
    fn the_petal_colour_is_rewritten_in_one_place() {
        let mut model = vec![9u8; 40];
        model[10..18].copy_from_slice(&PETAL);
        let blue = recoloured(&model, [50, 50, 255]).unwrap();
        assert_eq!(&blue[10..18], &[0, 50, 0, 50, 0, 255, 0, 0]);
        assert_eq!(&blue[..10], &model[..10]);
        assert_eq!(&blue[18..], &model[18..]);
        assert!(recoloured(&[0; 16], [1, 2, 3]).is_err(), "no petal colour to rewrite");
    }

    #[test]
    fn the_timings_in_the_core_are_the_discs_clips() {
        let Some(dir) = dir() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let body = EnemyBody::load_swapped(&dir, MODEL, CLIPS, &[]).unwrap();
        let secs = |c: &str| body.clip_frames(c).unwrap() / 30.0;
        assert!((secs("type2") - libpikmin::bulb::CLOSE_TIME).abs() < 1e-3, "close");
        assert!((secs("type3") - libpikmin::bulb::SHOT_TIME).abs() < 1e-3, "shot");
        assert!((secs("dead") - libpikmin::bulb::WITHER_TIME).abs() < 1e-3, "wither");
        assert!(body.height() > 10.0, "a flower: {}", body.height());
    }

    #[test]
    fn each_colour_bakes_and_the_petals_differ() {
        let Some(dir) = dir() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let set = BulbSet::load(&dir, 100, 50).unwrap();
        assert_eq!(set.bodies.len(), 5);
        let sums: Vec<u64> = (0..5).map(|i| set.textures(i).iter().map(|t| t.rgba.iter().map(|&b| b as u64).sum::<u64>()).sum()).collect();
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(sums[i], sums[j], "colours {i} and {j} bake the same");
            }
            assert!(!set.batches(i).is_empty());
        }
        let bulb = Bulb {
            id: libpikmin::BulbId(1), pos: glam::Vec3::ZERO, colour: PikiColor::Purple, queen: false, used: 0, swallowed: 0, same: 0,
            change: 0.0, state: BulbState::Shot { t: 0.5, fired: false },
        };
        assert_eq!(set.joints(&bulb).len(), 11);
    }
}
