//! Olimar's Pianta alt-skin: a Sunshine Pianta (from the user's Sunshine disc, loaded by
//! `libsms_present::body`) drawn in place of Olimar's model, so the same captain, camera, flash
//! and beam-in all apply.
//!
//! Which clip poses him (host adaptation; nothing here is Pikmin or Sunshine gameplay):
//! - standing still, walking, running and fleeing: the Pianta's own clips (`mom_wait`, `mom_walk`, `mom_run`: his palm
//!   and leaves sway in them), whose sound keys (`bas/mom_walk.bas`, `mom_run.bas`) give his footsteps;
//! - acrobatics, in Mario's clips retargeted onto him: the jump (`ma_jump`, `ma_broad_jump` for a running jump, the
//!   double and triple jump pairs), the ground pound (`hipsr`, `hipat`), the dismiss strike's dive (`sldct`) and the
//!   landings (`jmped`, `2jmed`, `hiped`);
//! - everything else (throw, pluck, whistle, dismiss, damage, knockdown): Olimar's own clip,
//!   retargeted joint by joint through [`OLIMAR_ALIASES`].
//!
//! The palm and leaves have no Olimar or Mario counterpart, so they always play one of the Pianta's
//! own clips ([`libsms_present::body::PLANT_JOINTS`]). The lowest skinned vertex is put on the
//! ground each frame, since the Pianta has no foot contact of his own.
//! Sunshine's `koshi_null` time-lag callback deforms the skirt before skinning;
//! a live enemy lock blends in the authored talk palm and leaf tracks.

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat4, Quat, Vec2, Vec3};
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

/// What a skin's voice says, in place of Olimar's own cries. The Pianta has the first four; Louie
/// (`louie::voice_id`) has his calls too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Jump,
    Throw,
    Hurt,
    Down,
    /// The whistle's call (`Event::Whistle`).
    Whistle,
    /// The dismiss call (`Event::Dismissed`).
    Dismiss,
    /// The punch (`Event::Punched`).
    Punch,
}

impl Voice {
    /// The Sunshine cue (`MSD_SE_NPC_VM_MONTM_*`, the male Pianta's speech lines; SoundEffects.hpp). Chosen by
    /// their names (proud, invitation, surprise, regret), not heard.
    pub const fn cue(self) -> Option<&'static str> {
        match self {
            Voice::Jump => Some("MSD_SE_NPC_VM_MONTM_PROUD"),
            Voice::Throw => Some("MSD_SE_NPC_VM_MONTM_INVITN"),
            Voice::Hurt => Some("MSD_SE_NPC_VM_MONTM_SURPRS"),
            Voice::Down => Some("MSD_SE_NPC_VM_MONTM_REGRET"),
            // Olimar's whistle, dismiss and punch have no cry to replace.
            Voice::Whistle | Voice::Dismiss | Voice::Punch => None,
        }
    }
}

/// Sunshine's units per Pikmin unit (Mario is 160 tall, Olimar 25), so the voice's distances sound right.
pub const SOUND_SCALE: f32 = 160.0 / 25.1;

/// How tall the Pianta stands in the host's sim units (to the top of his head, before the palm),
/// until the host says. Olimar is 25.1.
pub const DEFAULT_HEIGHT: f32 = 30.0;

/// Where a captain is in a chain of mid-air jumps, and the acrobatics around it.
#[derive(Default, Clone, Copy)]
struct JumpTrack {
    last: f32,
    air: u32,
    /// The first jump of this flight was a running one (`broad_jump`).
    long: bool,
    /// The view's seconds when the ground pound / the dive began (0: not under way).
    stomp_start: f32,
    dive_start: f32,
    /// Airborne last call, since when, and how it ended (the landing clip).
    air_since: f32,
    was_air: bool,
    landing: Option<&'static str>,
    landing_start: f32,
    /// The frame of the own locomotion clip last time, for its sound keys.
    last_frame: Option<f32>,
    locked: bool,
    plant_start: f32,
    plant_weight: f32,
    last_time: Option<f32>,
}

/// Mario's and the Pianta's clips run at 60 frames a second.
const FPS: f32 = 60.0;

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
    /// The sounds in the Pianta's clips (`bas/mom_<clip>.bas`): (frame, sound id) by clip.
    keys: HashMap<&'static str, Vec<(f32, u32)>>,
    /// Sounds his clips triggered since the host last took them: (captain, `JAISoundID`).
    pub cues: Vec<(u32, u32)>,
    /// Sunshine's waist callback has independent world-space history per captain.
    waist_lag: HashMap<u32, libsms_present::mtx_effect::TimeLag>,
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
            keys: HashMap::new(), cues: Vec::new(), waist_lag: HashMap::new(),
        };
        // His footsteps: the walk and run clips' sound keys (`TBaseNPC` animation sounds).
        let prefix = if female { "mow_" } else { "mom_" };
        let folder = if female { "npc/montewcommon/bas" } else { "npc/montemcommon/bas" };
        for clip in ["walk", "run"] {
            if let Ok(d) = std::fs::read(dir.join(folder).join(format!("{prefix}{clip}.bas")))
                && let Ok(t) = libsms_assets::sound::AnimSoundTable::parse(&d)
            {
                skin.keys.insert(clip, t.sounds.iter().map(|k| (k.start_frame, k.id)).collect());
            }
        }
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
    /// captain's current Olimar clip is (0..1), `time` the view's seconds, `olimar` his posed rig, `run_speed` the
    /// captain's top speed (sim units a second), which tells a running jump from a standing one.
    #[allow(clippy::too_many_arguments)]
    pub fn joints(&mut self, id: u32, c: &Captain, motion: Motion, phase: f32, time: f32, run_speed: f32, olimar: &Pose) -> Vec<Mat4> {
        let mut track = self.jumps.get(&id).copied().unwrap_or_default();
        let speed = c.vel.x.hypot(c.vel.z);
        let airborne = c.jump.is_some() || !c.on_ground;
        let pounding = c.stomp.is_some();
        let diving = c.strike.is_some();
        // The acrobatics' bookkeeping: when each began, and what a touchdown is.
        if pounding && track.stomp_start == 0.0 { track.stomp_start = time.max(1e-3); }
        if diving && track.dive_start == 0.0 { track.dive_start = time.max(1e-3); }
        if airborne && !track.was_air {
            track.air_since = time;
            track.landing = None;
        }
        if !airborne && track.was_air && time - track.air_since > 0.25 && speed < 0.6 * run_speed {
            track.landing = Some(if track.stomp_start > 0.0 { "hiped" } else if track.air > 0 { "2jmed" } else { "jmped" });
            track.landing_start = time;
        }
        track.was_air = airborne;
        let wait_frame = {
            let frames = self.body.own_clip_frames("wait").unwrap_or(60.0).max(1.0);
            (time * FPS).rem_euclid(frames)
        };
        let mario = |body: &Pianta, name: &str, t: f32| -> Option<Vec<Mat4>> {
            let frames = body.mario_clip_frames(name)?;
            body.pose_mario(name, t.clamp(0.0, frames), Some(("wait", wait_frame)))
        };
        let posed: Option<Vec<Mat4>>;
        if pounding {
            // `hipsr` winds up, `hipat` falls: `TMario`'s ground pound.
            let t = (time - track.stomp_start) * FPS;
            let start = self.body.mario_clip_frames("hipsr").unwrap_or(0.0);
            posed = if t < start { mario(&self.body, "hipsr", t) } else { mario(&self.body, "hipat", t - start) };
        } else if diving {
            posed = mario(&self.body, "sldct", (time - track.dive_start) * FPS);
        } else if let Some(j) = c.jump {
            // A new jump while airborne restarts the clock: a double, then a triple jump.
            if j.elapsed + 0.05 < track.last { track.air += 1; }
            if track.last == 0.0 && track.air == 0 { track.long = speed > 0.75 * run_speed; }
            track.last = j.elapsed;
            let t = j.elapsed * FPS;
            let (first, second) = match track.air {
                0 if track.long && self.body.mario_clip_frames("broad_jump").is_some() => ("broad_jump", ""),
                0 => ("jump", ""),
                1 => ("2jmp1", "2jmp2"),
                _ => ("tjmp1", "tjmp2"),
            };
            posed = match self.body.mario_clip_frames(first) {
                Some(f) if !second.is_empty() && t >= f => mario(&self.body, second, t - f),
                Some(_) => mario(&self.body, first, t),
                None => None,
            };
        } else if !c.on_ground {
            // Off a ledge or knocked into the air: the jump's falling end.
            let end = self.body.mario_clip_frames("jump").unwrap_or(0.0);
            posed = mario(&self.body, "jump", end);
        } else {
            track.last = 0.0;
            track.air = 0;
            track.long = false;
            let landing = track.landing.filter(|&name| {
                self.body.mario_clip_frames(name).is_some_and(|f| (time - track.landing_start) * FPS < f)
            });
            if landing.is_none() { track.landing = None; }
            posed = if let Some(name) = landing {
                mario(&self.body, name, (time - track.landing_start) * FPS)
            } else {
                match motion {
                    Motion::Wait | Motion::Asibumi => {
                        track.last_frame = None;
                        self.body.pose_own("wait", wait_frame)
                    }
                    Motion::Walk | Motion::Run | Motion::Nigeru => {
                        let name: &'static str = if motion == Motion::Walk { "walk" } else { "run" };
                        let frames = self.body.own_clip_frames(name);
                        let frame = frames.map(|f| phase * f);
                        let own = frame.and_then(|fr| self.body.pose_own(name, fr));
                        if let (Some(frame), Some(frames)) = (frame, frames) {
                            // The clip's sound keys (his footsteps) as the frame passes them.
                            if let (Some(prev), Some(keys)) = (track.last_frame, self.keys.get(name)) {
                                for &(at, sound) in keys {
                                    let at = at.rem_euclid(frames);
                                    let crossed = if frame >= prev { at > prev && at <= frame } else { at > prev || at <= frame };
                                    if crossed { self.cues.push((id, sound)); }
                                }
                            }
                            track.last_frame = Some(frame);
                        }
                        own
                    }
                    _ => {
                        track.last_frame = None;
                        None
                    }
                }
            };
        }
        if !pounding { track.stomp_start = 0.0; }
        if !diving { track.dive_start = 0.0; }
        let mut joints = posed.unwrap_or_else(|| {
            self.body.pose_from(&self.retarget, &olimar.joints, Some(("wait", self.plant_frame("wait", time))))
        });
        // Sunshine's talk palm, grafted over locomotion / acrobatics / Olimar's
        // actions only while an enemy lock has a live aim point. Pellets do not trigger it.
        let locked = c.lock_target.is_some() && c.lock_view.is_some();
        if locked && !track.locked { track.plant_start = time; }
        let dt = track.last_time.map_or(0.0, |last| (time - last).clamp(0.0, 0.25));
        let change = dt / self.body.talk_blend_seconds;
        track.plant_weight = if locked { (track.plant_weight + change).min(1.0) } else { (track.plant_weight - change).max(0.0) };
        if track.plant_weight > 0.0 {
            self.body.talk_plant(&mut joints, (time - track.plant_start) * FPS, track.plant_weight);
        }
        track.locked = locked;
        track.last_time = Some(time);
        self.jumps.insert(id, track);
        // MtxUtil.cpp::TMtxTimeLag runs in world coordinates before skinning.
        // Convert the captain's position to the Pianta model's own Sunshine units.
        let root = Mat4::from_rotation_translation(Quat::from_rotation_y(c.face), c.pos / self.scale);
        self.body.apply_waist_lag(&mut joints, root, time, self.waist_lag.entry(id).or_default());
        joints
    }

    pub fn reset_pose(&mut self, id: u32) {
        self.waist_lag.remove(&id);
        self.jumps.remove(&id);
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
        batches_of(&self.body.model, &self.body.bind_world(), self.scale)
    }

    /// His baked textures as the view's, in order.
    pub fn textures(&self) -> Vec<tev::Baked> {
        textures_of(&self.body.textures)
    }

    /// Bind-pose bounds in sim units, for the model table.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        (self.body.model.lo * self.scale, self.body.model.hi * self.scale)
    }

    pub fn olimar_joints(&self) -> usize {
        self.olimar.len()
    }
}

/// A libsms-present model as libpikmin-assets batches (bind-skinned vertices, in the model's own batch order),
/// for the host's model table.
pub(crate) fn batches_of(model: &libsms_present::view::Model, bind: &[Mat4], scale: f32) -> Vec<Batch> {
    let pose = PiantaPose::new(&model.bmd, Mat4::from_scale(Vec3::splat(scale)), bind);
    model.batches.iter().enumerate().map(|(bi, b)| Batch {
        material: b.material,
        cull: match model.batch_cull[bi] { 0 => Cull::Back, 1 => Cull::Front, _ => Cull::None },
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

/// A body's baked textures as the view's.
pub(crate) fn textures_of(textures: &libsms_present::view::Textures) -> Vec<tev::Baked> {
    textures.list.iter().map(|t| tev::Baked {
        width: t.width,
        height: t.height,
        wrap: t.wrap,
        rgba: t.rgba.clone(),
        env: t.env,
    }).collect()
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

    #[test]
    fn live_enemy_lock_uses_talk_plant_and_keeps_captains_independent() {
        let Some((a, sms)) = assets() else { eprintln!("SKIP: assets absent"); return };
        let m = a.model(paths::OLIMAR).unwrap();
        let mut skin = PiantaSkin::load(&sms, &m, 19, 0, false, 1).unwrap();
        let rest = OlimarSkeleton::new(&m).pose(Mat4::IDENTITY, None);
        let mut sim = libpikmin::Sim::default();
        let id = sim.add_captain(Vec3::ZERO);
        let ordinary = sim.captain(id).clone();
        let mut locked = ordinary.clone();
        locked.lock_target = Some(7);
        locked.lock_view = Some(libpikmin::captain::LockView { pos: Vec3::Z * 100.0, eye: Vec3::Y * 30.0 });
        let mut last = Vec::new();
        let mut normal = Vec::new();
        for frame in 0..=60 {
            let time = frame as f32 / 60.0;
            last = skin.joints(0, &locked, Motion::Wait, 0.0, time, 160.0, &rest);
            normal = skin.joints(1, &ordinary, Motion::Wait, 0.0, time, 160.0, &rest);
        }
        let leaf = skin.body.rig.find("leafL_jnt").unwrap();
        assert!(!last[leaf].abs_diff_eq(normal[leaf], 0.01), "lock changes the palm");
        for name in ["body_jnt", "koshi_null", "mino_null", "armRT_jnt", "legRT_jnt", "nose_jnt"] {
            let j = skin.body.rig.find(name).unwrap();
            assert!(last[j].abs_diff_eq(normal[j], 1e-4), "lock leaves {name} on its current action");
        }
        // A pellet aim point and a dead/stale enemy lock never perk a fresh captain's plant.
        let mut pellet = ordinary.clone();
        pellet.lock_view = locked.lock_view;
        let mut stale = locked.clone();
        stale.lock_view = None;
        for (captain, c) in [(2, &pellet), (3, &stale)] {
            let pose = skin.joints(captain, c, Motion::Wait, 0.0, 1.0, 160.0, &rest);
            assert!(pose[leaf].abs_diff_eq(normal[leaf], 1e-4));
        }
        for frame in 61..=120 {
            last = skin.joints(0, &ordinary, Motion::Wait, 0.0, frame as f32 / 60.0, 160.0, &rest);
        }
        let normal = skin.joints(1, &ordinary, Motion::Wait, 0.0, 2.0, 160.0, &rest);
        assert!(last[leaf].abs_diff_eq(normal[leaf], 1e-4), "unlock returns to the current idle plant");
        skin.reset_pose(0);
        assert!(!skin.waist_lag.contains_key(&0) && !skin.jumps.contains_key(&0));
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
