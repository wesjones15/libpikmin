//! Pikmin 2's enemies for hosts that fight them (`libpikmin_teki::p2`): each kind's J3D body from the cache
//! `libsms::extract::p2_enemy` writes (`<assets>/p2/enemy/`), posed on its `.bca` clips by the motion index and frame
//! the host names in [`crate::teki::TekiInstance`], like the Pikmin 1 enemies.
//!
//! The model is drawn at the enemy's own scale (`mScale`, `TekiInfo::scale`) times the instance's: the Kogane family
//! grows from nothing as it pops up, which the enemy reports as its size share.

use std::path::Path;
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use libpikmin_teki::info::TekiInfo;
use libpikmin_teki::legs::LegsView;
use libpikmin_teki::p2;
use libsms_present::enemy::EnemyBody;

use libpikmin_assets::model::Batch;

use crate::tev;

/// One loaded Pikmin 2 kind.
pub struct P2Teki {
    pub info: TekiInfo,
    pub body: EnemyBody,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
    /// A Long Legs boss's leg rig (`libpikmin_teki::legs`), from the model's landing pose: the legs are folded onto the feet the core says.
    pub legs: Option<libpikmin_teki::legs::LegRig>,
}

impl P2Teki {
    /// The clip a motion index names (`enemyAnimMgr.txt` order).
    pub fn clip(&self, motion: u32) -> Option<&str> {
        self.info.clips.get(motion as usize).map(String::as_str)
    }

    /// The model-table entry's batches (bind pose, at the enemy's scale).
    pub fn batches(&self) -> Vec<Batch> {
        crate::pianta::batches_of(&self.body.model, &self.body.bind_world(), self.info.scale)
    }

    pub fn textures(&self) -> Vec<tev::Baked> {
        crate::pianta::textures_of(&self.body.textures)
    }

    /// `IKSystemBase::makeMatrix` for each leg (and the Man-at-Legs' gun): the leg joints of `joints` (the clip's pose, model space)
    /// are folded onto the feet `lv` names, which are world places; the body is drawn at `lv.trace` facing `lv.face`.
    pub fn fold_legs(&self, joints: &mut [Mat4], lv: &LegsView) {
        use libpikmin_teki::ik::{Blend, LegInput, apply_leg, solve_leg};
        use libpikmin_teki::legs::{LEGS, to_local};
        let Some(rig) = &self.legs else { return };
        if !lv.active {
            return;
        }
        let s = self.info.scale.max(1e-3);
        for i in 0..LEGS {
            let [h, m, _] = rig.joints[i];
            let foot = to_local(lv.feet[i] - lv.trace, lv.face) / s;
            let blend = lv.blend.then(|| Blend { top_x: joints[h].x_axis.truncate(), top_y: joints[h].y_axis.truncate(), middle_y: joints[m].y_axis.truncate() });
            let pose = solve_leg(&LegInput {
                top: joints[h].w_axis.truncate(),
                foot,
                top_to_middle: rig.top_to_middle[i],
                middle_to_bottom: rig.middle_to_bottom[i],
                bend: lv.bend[i],
                blend,
            });
            apply_leg(joints, rig.joints[i], &pose, lv.blend);
        }
        // The gun: head and barrel swing about the barrel's root onto the direction the core aims it.
        if let (Some(dir), Some(g)) = (lv.gun, rig.gun) {
            let pivot = joints[g.joints[1]].w_axis.truncate();
            let rest = joints[g.joints[1]].x_axis.truncate().normalize_or_zero();
            let want = to_local(dir, lv.face).normalize_or_zero();
            if rest != Vec3::ZERO && want != Vec3::ZERO {
                let swing = Mat4::from_translation(pivot) * Mat4::from_quat(Quat::from_rotation_arc(rest, want)) * Mat4::from_translation(-pivot);
                for j in g.joints {
                    joints[j] = swing * joints[j];
                }
            }
        }
    }
}

/// The body of Pikmin 2 kind `kind` from the cache `dir`: its model and clips, with the variant's texture.
pub fn load_body(dir: &Path, kind: u8) -> Result<EnemyBody, String> {
    let k = p2::kind(kind).ok_or("not a Pikmin 2 kind")?;
    let base = format!("enemy/data/{}", k.data);
    let textures: Vec<String> = k.textures.iter().map(|t| format!("enemy/data/{t}")).collect();
    let textures: Vec<&str> = textures.iter().map(String::as_str).collect();
    EnemyBody::load_swapped(dir, &format!("{base}/model/enemy.bmd"), &format!("{base}/anim"), &textures)
}

/// What the disc says about `kind` with the body's real bounds (the parm files give only a collision sphere), at the
/// enemy's own scale.
pub fn load_info(dir: &Path, kind: u8) -> Result<TekiInfo, String> {
    let mut info = TekiInfo::load_p2(dir, kind)?;
    fit_bounds(&mut info, &load_body(dir, kind)?);
    Ok(info)
}

fn fit_bounds(info: &mut TekiInfo, body: &EnemyBody) {
    info.lo = body.model.lo * info.scale;
    info.hi = body.model.hi * info.scale;
}

/// Loads every Pikmin 2 kind the cache `dir` (the folder holding `enemy/`) has files for; `first_model` and
/// `first_tex` are where they will sit in the view's tables, in kind order.
pub fn load(dir: &Path, first_model: u32, first_tex: i32) -> Vec<Arc<P2Teki>> {
    let mut out: Vec<Arc<P2Teki>> = Vec::new();
    let mut tex = first_tex;
    for kind in p2::FIRST..p2::FIRST + p2::KIND_TABLE.len() as u8 {
        let Some(k) = p2::kind(kind) else { continue };
        let loaded = (|| {
            let mut info = TekiInfo::load_p2(dir, kind)?;
            info.model = first_model + out.len() as u32;
            let body = load_body(dir, kind)?;
            fit_bounds(&mut info, &body);
            let legs = libpikmin_teki::longlegs::is_long_legs(kind).then(|| landed_rig(&body)).flatten();
            Ok::<_, String>(P2Teki { info, body, model: first_model + out.len() as u32, tex_base: tex, legs })
        })();
        match loaded {
            Ok(t) => {
                tex += t.body.textures.list.len() as i32;
                out.push(Arc::new(t));
            }
            Err(e) => eprintln!("Pikmin 2 enemy {}: {e}", k.name),
        }
    }
    out
}

/// A Long Legs body's rig from its model and the last frame of its landing clip, where the core's gait starts from.
fn landed_rig(body: &EnemyBody) -> Option<libpikmin_teki::legs::LegRig> {
    let frames = body.clip_frames("landing")?;
    let pose = body.pose("landing", (frames - 1.0).max(0.0))?;
    libpikmin_teki::legs::LegRig::from_pose(&body.rig.names, &pose)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libpikmin_assets::Assets;
    use libpikmin_teki::legs::{LEGS, LegsView, to_world};

    fn body(kind: u8) -> Option<P2Teki> {
        let dir = Assets::default_dir().join("p2");
        if !p2::is_available(&dir, kind) {
            return None;
        }
        let body = load_body(&dir, kind).ok()?;
        let info = load_info(&dir, kind).ok()?;
        let legs = landed_rig(&body);
        Some(P2Teki { info, body, model: 0, tex_base: 0, legs })
    }

    /// The legs fold onto the feet they are given: each foot joint lands on its target (a world place, here for a body at
    /// `trace` facing `face`), and the leg's two segments keep the lengths the model has.
    #[test]
    fn the_legs_fold_onto_their_feet_whatever_way_the_body_faces() {
        for kind in [p2::BEADY_LONG_LEGS, p2::RAGING_LONG_LEGS, p2::MAN_AT_LEGS] {
            let Some(b) = body(kind) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
            let rig = b.legs.clone().expect("a rig");
            let frames = b.body.clip_frames("wait").unwrap();
            let joints = b.body.pose("wait", frames * 0.3).unwrap();
            let (trace, face) = (Vec3::new(120.0, 4.0, -80.0), 0.9);
            // Rest feet, moved: one lifted, one forward, two where they stand.
            let mut feet: [Vec3; LEGS] = std::array::from_fn(|i| trace + to_world(rig.feet[i], face));
            feet[0] += to_world(Vec3::new(10.0, 60.0, 30.0), face);
            feet[2] += to_world(Vec3::new(-20.0, 0.0, 25.0), face);
            let lv = LegsView { active: true, blend: false, trace, face, feet, bend: [0.3, 0.0, 0.0, 0.0], gun: None };
            let mut folded = joints.clone();
            b.fold_legs(&mut folded, &lv);
            let to_model = |p: Vec3| libpikmin_teki::legs::to_local(p - trace, face) / b.info.scale;
            for i in 0..LEGS {
                let [h, m, f] = rig.joints[i];
                let (top, knee, foot) = (folded[h].w_axis.truncate(), folded[m].w_axis.truncate(), folded[f].w_axis.truncate());
                assert!((foot - to_model(feet[i])).length() < 1e-2, "{kind} leg {i}: {foot} vs {}", to_model(feet[i]));
                assert!(((knee - top).length() - rig.top_to_middle[i]).abs() < 0.05, "{kind} leg {i} thigh");
                assert!(((foot - knee).length() - rig.middle_to_bottom[i]).abs() < 0.05, "{kind} leg {i} shin");
                // The hip stays where the body's clip put it.
                assert!((top - joints[h].w_axis.truncate()).length() < 1e-3);
            }
            // The body's own joints are untouched.
            assert_eq!(folded[0], joints[0]);
        }
    }

    #[test]
    fn the_mans_gun_swings_onto_the_direction_it_is_aimed() {
        let Some(b) = body(p2::MAN_AT_LEGS) else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let rig = b.legs.clone().unwrap();
        let g = rig.gun.expect("a gun");
        let joints = b.body.pose("wait", 0.0).unwrap();
        let feet: [Vec3; LEGS] = std::array::from_fn(|i| to_world(rig.feet[i], 0.0));
        let aim = Vec3::new(0.3, -0.5, 0.8).normalize();
        let lv = LegsView { active: true, blend: false, trace: Vec3::ZERO, face: 0.0, feet, bend: [0.0; LEGS], gun: Some(aim) };
        let mut folded = joints.clone();
        b.fold_legs(&mut folded, &lv);
        let barrel = folded[g.joints[1]].x_axis.truncate().normalize();
        assert!((barrel - aim).length() < 1e-3, "{barrel} vs {aim}");
        // It turns about its root.
        assert!((folded[g.joints[1]].w_axis - joints[g.joints[1]].w_axis).truncate().length() < 1e-3);
    }
}
