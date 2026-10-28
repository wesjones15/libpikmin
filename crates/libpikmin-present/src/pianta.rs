//! Olimar's Pianta alt-skin: a Sunshine Pianta (from the user's Sunshine disc, loaded by
//! `libsms_present::body`) drawn in place of Olimar's model, so the same captain, camera, flash
//! and beam-in all apply.
//!
//! Which clip poses him (host adaptation; nothing here is Pikmin or Sunshine gameplay):
//! - standing still: the Pianta's own idle (`mom_wait`, his palm and leaves sway in it);
//! - walking, running, fleeing and jumping: Mario's clips (`ma_walk`, `ma_run2`, `ma_jump`, the
//!   double and triple jump pairs), retargeted onto the Pianta;
//! - everything else (throw, pluck, whistle, dismiss, damage, knockdown): Olimar's own clip,
//!   retargeted joint by joint through [`OLIMAR_ALIASES`].
//!
//! The palm and leaves have no Olimar or Mario counterpart, so they always play one of the Pianta's
//! own clips ([`libsms_present::body::PLANT_JOINTS`]). The lowest skinned vertex is put on the
//! ground each frame, since the Pianta has no foot contact of his own.

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat4, Vec2, Vec3};
use libpikmin::Captain;
use libpikmin_assets::model::{Batch, BatchVertex, Cull, Skin};
use libpikmin_assets::motion::Motion;
use libpikmin_assets::{Model, Pose, Skeleton as OlimarSkeleton};
use libsms::{MtxCalc, Skeleton, Srt};
use libsms_assets::Pose as PiantaPose;
use libsms_assets::retarget::Retarget;
use libsms_present::body::Pianta;

use crate::tev;

/// Skin ids in the host's order (`pik_view_set_captain_skin`). 1 is kept for Louie.
pub const SKIN_OLIMAR: u8 = 0;
pub const SKIN_PIANTA: u8 = 2;

/// (Pianta joint, Olimar joint): Olimar's 12 joints are unnamed in the disc's model, so they are
/// `j<index>`. 0 pelvis root, 1 torso, 2 head, 3-6 antenna, 7 and 8 arms (x -1.5 and +1.5),
/// 9 hips, 10 and 11 legs. He has no elbows or knees, so the Pianta's forearms, hands, shins and
/// feet follow their upper limb rigidly.
pub const OLIMAR_ALIASES: &[(&str, &str)] = &[
    ("body_jnt", "j0"), ("koshi_null", "j9"), ("kubi", "j2"),
    ("armRT_jnt", "j7"), ("armLT_jnt", "j8"),
    ("legRT_jnt", "j10"), ("legLT_jnt", "j11"),
];

/// How tall the Pianta stands in the host's sim units (to the top of his head, before the palm),
/// until the host says. Olimar is 25.1.
pub const DEFAULT_HEIGHT: f32 = 30.0;

/// Where a captain is in a chain of mid-air jumps.
#[derive(Default, Clone, Copy)]
struct JumpTrack {
    last: f32,
    air: u32,
}

pub struct PiantaSkin {
    pub body: Pianta,
    olimar: Skeleton,
    retarget: Retarget,
    /// The model's index in [`crate::view::View::models`].
    pub model: u32,
    /// Where this body's textures start in [`crate::view::View::textures`].
    pub tex_base: i32,
    /// Sim units per Pianta unit.
    pub scale: f32,
    /// Head-top height of the bind pose in Pianta units.
    head_top: f32,
    jumps: HashMap<u32, JumpTrack>,
}

/// Olimar's rig as a `libsms` skeleton, to retarget from.
fn olimar_rig(model: &Model) -> Skeleton {
    let n = model.joints.len();
    Skeleton {
        names: (0..n).map(|i| format!("j{i}")).collect(),
        parents: model.joints.iter().map(|j| (j.parent >= 0).then_some(j.parent as usize)).collect(),
        bind: model.joints.iter().map(|j| Srt { scale: j.scale, rot: j.rotation, trans: j.translation }).collect(),
        calc: MtxCalc::Basic,
        compensate: vec![false; n],
    }
}

impl PiantaSkin {
    pub fn load(dir: &Path, olimar: &Model, model: u32, tex_base: i32, female: bool, colour: usize) -> Result<Self, String> {
        let body = Pianta::load(dir, female, colour)?;
        let olimar = olimar_rig(olimar);
        let retarget = body.retarget_from(&olimar, OLIMAR_ALIASES);
        let top = body.rig.find("yashi_jnt").map_or(body.height(), |j| body.bind_world()[j].w_axis.y);
        let mut skin = PiantaSkin {
            body, olimar, retarget, model, tex_base, scale: 1.0, head_top: top.max(1.0), jumps: HashMap::new(),
        };
        skin.set_height(DEFAULT_HEIGHT);
        Ok(skin)
    }

    /// Stand `units` tall (to the top of the head).
    pub fn set_height(&mut self, units: f32) {
        if units.is_finite() && units > 0.1 {
            self.scale = units / self.head_top;
        }
    }

    /// The Pianta's joints this frame, in his own units. `phase` is how far through its loop the
    /// captain's current Olimar clip is (0..1), `time` the view's seconds, `olimar` his posed rig.
    pub fn joints(&mut self, id: u32, c: &Captain, motion: Motion, phase: f32, time: f32, olimar: &Pose) -> Vec<Mat4> {
        let mario = |name: &str, t: f32, plant: &str| -> Option<Vec<Mat4>> {
            let frames = self.body.mario_clip_frames(name)?;
            self.body.pose_mario(name, t.clamp(0.0, frames), Some((plant, self.plant_frame(plant, time))))
        };
        let mut track = self.jumps.get(&id).copied().unwrap_or_default();
        let posed: Option<Vec<Mat4>>;
        if let Some(j) = c.jump {
            // A new jump while airborne restarts the clock: a double, then a triple jump.
            if j.elapsed + 0.05 < track.last { track.air += 1; }
            track.last = j.elapsed;
            let t = j.elapsed * 60.0;
            let (first, second) = match track.air { 0 => ("jump", ""), 1 => ("2jmp1", "2jmp2"), _ => ("tjmp1", "tjmp2") };
            posed = match self.body.mario_clip_frames(first) {
                Some(f) if !second.is_empty() && t >= f => mario(second, t - f, "wait"),
                Some(_) => mario(first, t, "wait"),
                None => None,
            };
        } else {
            track = JumpTrack::default();
            let sway = |name: &str| -> Option<Vec<Mat4>> {
                let frames = self.body.mario_clip_frames(name)?;
                mario(name, phase * frames, if name == "walk" { "walk" } else { "run" })
            };
            posed = match motion {
                Motion::Wait | Motion::Asibumi => self.body.pose_own("wait", self.plant_frame("wait", time)),
                Motion::Walk => sway("walk"),
                Motion::Run | Motion::Nigeru => sway("run2"),
                _ => None,
            };
        }
        self.jumps.insert(id, track);
        posed.unwrap_or_else(|| {
            self.body.pose_from(&self.retarget, &olimar.joints, Some(("wait", self.plant_frame("wait", time))))
        })
    }

    /// The frame of the Pianta's own clip `name` at `time` seconds, looping (60 frames a second).
    fn plant_frame(&self, name: &str, time: f32) -> f32 {
        let frames = self.body.own_clip_frames(name).unwrap_or(60.0).max(1.0);
        (time * 60.0).rem_euclid(frames)
    }

    pub fn skin_pose(&self, joints: &[Mat4]) -> PiantaPose {
        PiantaPose::new(&self.body.model.bmd, Mat4::from_scale(Vec3::splat(self.scale)), joints)
    }

    /// The model-table entry and batches for the host (vertices in the order of the Pianta's own
    /// batches, so the posed ones line up).
    pub fn batches(&self) -> Vec<Batch> {
        let bind = self.body.bind_world();
        let pose = PiantaPose::new(&self.body.model.bmd, Mat4::from_scale(Vec3::splat(self.scale)), &bind);
        self.body.model.batches.iter().enumerate().map(|(bi, b)| Batch {
            material: b.material,
            cull: match self.body.model.batch_cull[bi] { 0 => Cull::Back, 1 => Cull::Front, _ => Cull::None },
            vertices: b.vertices.iter().map(|v| {
                let (p, n) = pose.skin(v);
                BatchVertex {
                    position: p,
                    normal: n.normalize_or_zero(),
                    uv: Vec2::new(v.uv[0].x, v.uv[0].y),
                    color: [255; 4],
                    skin: match v.skin {
                        libsms_assets::bmd::Skin::Joint(j) => Skin::Joint(j),
                        libsms_assets::bmd::Skin::Envelope(e) => Skin::Envelope(e),
                    },
                }
            }).collect(),
            indices: b.indices.clone(),
        }).collect()
    }

    /// His baked textures as the view's, in order.
    pub fn textures(&self) -> Vec<tev::Baked> {
        self.body.textures.list.iter().map(|t| tev::Baked {
            width: t.width,
            height: t.height,
            wrap: t.wrap,
            rgba: t.rgba.clone(),
            env: t.env,
        }).collect()
    }

    /// Bind-pose bounds in sim units, for the model table.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.body.model.lo * self.scale, self.body.model.hi * self.scale)
    }

    pub fn olimar_joints(&self) -> usize {
        self.olimar.len()
    }
}

/// Convenience for tests: Olimar's disc skeleton in pikmin-assets form.
pub fn olimar_of(model: &Model) -> OlimarSkeleton {
    OlimarSkeleton::new(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libpikmin_assets::{Assets, paths};

    fn assets() -> Option<(Assets, std::path::PathBuf)> {
        let sms = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets");
        let sms = std::env::var_os("SMS_ASSETS").map(Into::into).unwrap_or(sms);
        if !libsms_present::body::pianta_files(false).iter().all(|f| sms.join(f).is_file()) { return None }
        Some((Assets::discover(None)?, sms))
    }

    #[test]
    fn olimar_aliases_name_real_joints() {
        let Some((a, sms)) = assets() else { eprintln!("SKIP: assets absent"); return };
        let m = a.model(paths::OLIMAR).unwrap();
        let skin = PiantaSkin::load(&sms, &m, 19, 0, false, 7).unwrap();
        for (pianta, olimar) in OLIMAR_ALIASES {
            assert!(skin.body.rig.find(pianta).is_some(), "{pianta}");
            assert!(skin.olimar.find(olimar).is_some(), "{olimar}");
        }
        assert_eq!(skin.olimar_joints(), m.joints.len());
    }
}

#[cfg(test)]
mod arms {
    use super::*;
    use libpikmin_assets::{AnimSet, Assets, paths};

    /// Mean direction of the vertices skinned to joint `j`, from the joint, in a pose.
    fn limb_dir(m: &Model, pose: &Pose, j: u16) -> Vec3 {
        let mut sum = Vec3::ZERO;
        let mut n = 0.0f32;
        for b in m.batches().unwrap() {
            for v in &b.vertices {
                if matches!(v.skin, Skin::Joint(k) if k == j) { sum += pose.skin(v).0; n += 1.0; }
            }
        }
        (sum / n.max(1.0) - pose.joints[j as usize].w_axis.truncate()).normalize_or_zero()
    }

    #[test]
    fn pianta_limbs_follow_olimars() {
        let sms = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets");
        if !libsms_present::body::pianta_files(false).iter().all(|f| sms.join(f).is_file()) { eprintln!("SKIP: no Sunshine assets"); return }
        let Some(a) = Assets::discover(None) else { eprintln!("SKIP: no Pikmin assets"); return };
        let m = a.model(paths::OLIMAR).unwrap();
        let skin = PiantaSkin::load(&sms, &m, 19, 0, false, 7).unwrap();
        let sk = OlimarSkeleton::new(&m);
        let set = AnimSet::load(&a, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE).unwrap();
        let rig = &skin.body.rig;
        let (ar, hr) = (rig.find("armRT_jnt").unwrap(), rig.find("handR_jnt").unwrap());
        let (lr, fr) = (rig.find("legRT_jnt").unwrap(), rig.find("footR_jnt").unwrap());
        let bind = sk.pose(Mat4::IDENTITY, None);
        let rest = skin.body.pose_from(&skin.retarget, &bind.joints, None);
        // At rest both stand with their right arm out to -x and their legs down.
        assert!(limb_dir(&m, &bind, 7).dot((rest[hr].w_axis - rest[ar].w_axis).truncate().normalize()) > 0.9);
        assert!(limb_dir(&m, &bind, 10).dot((rest[fr].w_axis - rest[lr].w_axis).truncate().normalize()) > 0.8);
        for motion in [Motion::Wait, Motion::Walk, Motion::Throw, Motion::Nuku, Motion::Fue, Motion::ODead, Motion::Punch, Motion::Damage] {
            let Some((info, anim)) = motion.anim_index().and_then(|i| set.get(i)) else { continue };
            let end = *info.keys.last().unwrap_or(&1) as f32;
            for t in [0.0, 0.5] {
                let pose = sk.pose(Mat4::IDENTITY, Some((anim, end * t)));
                let j = skin.body.pose_from(&skin.retarget, &pose.joints, None);
                assert!(j.iter().all(|x| x.is_finite()));
                // The Pianta's arm points where Olimar's does (they share +Z forward, x lateral).
                let (o, p) = (limb_dir(&m, &pose, 7), (j[hr].w_axis - j[ar].w_axis).truncate().normalize());
                assert!(o.dot(p) > 0.8, "{motion:?} {t}: olimar arm {o:?}, pianta arm {p:?}");
            }
        }
    }
}
