//! The stepping gait of Pikmin 2's Long Legs (`IKSystemMgr` and `IKSystemBase`, plugProjectNishimuraU/IKSystemMgr.cpp,
//! IKSystemBase.cpp): four three-joint legs that take turns stepping, a body that stands on its four feet and follows
//! their average, and the pose solver ([`crate::ik`]) the host runs each frame to fold the legs onto the feet.
//!
//! The gait: while a walk goes on ([`Legs::start_motion`]) the legs step in turn, right front (`rhand`, leg 0), left
//! front (1), right back (`rfoot`, 2), left back (3): leg 0 lifts, and when it lands the next one does, and so on, then
//! the round starts again. The goal of each round ([`Legs::set_goal`]) is a point up to `move_speed` away toward the
//! target (or, when the target is more than 30 degrees off the facing, a turn on the spot of up to `max_turn_angle`):
//! each foot goes to that point plus its own offset, turned to the new facing. A foot's step is a quadratic through
//! where it lifted, a peak over three quarters of the way (`height_offset` up) and the ground; its progress speeds up on
//! the way down (`downward_accel`) and slows on the way up (`raise_slowdown`). The body's place is the feet's average and
//! its height their weighted heights; the body is drawn at a point that follows it like a spring (`trace`).
//!
//! The decomp measures the feet and segment lengths from the model's joints when the landing finishes
//! (`startProgramedIK`); a core has no model, so a [`LegRig`] built from the model's landing pose carries them.
//!
//! Host adaptations: the ground under every foot is the one height the host reports for the enemy (`Teki::ground_y`),
//! where the decomp asks the map for each foot (`mapMgr->getMinY`, and `mLegCount` samples around BigFoot's feet).

use std::f32::consts::{PI, TAU};
use std::path::Path;
use std::sync::Arc;

use glam::{Mat4, Vec3};

use crate::ik;
use crate::object::Teki;

pub const LEGS: usize = 4;
/// Leg joint names in the order the decomp sets the legs up (`setupIKSystem`): right front, left front, right back, left back.
pub const JOINT_NAMES: [[&str; 3]; LEGS] = [
    ["rhand1jnt", "rhand2jnt", "rhand3jnt"],
    ["lhand1jnt", "lhand2jnt", "lhand3jnt"],
    ["rfoot1jnt", "rfoot2jnt", "rfoot3jnt"],
    ["lfoot1jnt", "lfoot2jnt", "lfoot3jnt"],
];

/// A leg's stepping state (`mLegStates`): 0 down and waiting, 1 lifted and swinging, 2 just landed, 3 done this round.
pub const WAITING: i32 = 0;
pub const SWINGING: i32 = 1;
pub const LANDED: i32 = 2;
pub const DONE: i32 = 3;

/// What the model gives the gait: where the legs are and how long their segments are when the legs are first set up.
#[derive(Debug, Clone)]
pub struct LegRig {
    /// The joints of each leg (hip, knee, foot) in the model's skeleton.
    pub joints: [[usize; 3]; LEGS],
    /// Model-space places of the hips and feet, and the segment lengths, in the landing pose.
    pub hips: [Vec3; LEGS],
    pub feet: [Vec3; LEGS],
    pub top_to_middle: [f32; LEGS],
    pub middle_to_bottom: [f32; LEGS],
    /// The Man-at-Legs' gun: the `gun` joint's place and barrel direction (its X axis) in the landing pose, and the joints
    /// that carry it (`tamajnt`, `gun`).
    pub gun: Option<GunRig>,
    /// The radius of the foot spheres (`lfsp` ..) in `enemyColl.txt`; 0 for a body whose feet do not hurt.
    pub foot_radius: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct GunRig {
    pub joints: [usize; 2],
    pub pos: Vec3,
    pub dir: Vec3,
}

impl LegRig {
    /// The rig of a model whose joints are `names`, in the pose `pose` (model space).
    pub fn from_pose(names: &[String], pose: &[Mat4]) -> Option<LegRig> {
        let find = |n: &str| names.iter().position(|x| x == n);
        let mut joints = [[0usize; 3]; LEGS];
        for (leg, set) in JOINT_NAMES.iter().enumerate() {
            for (i, n) in set.iter().enumerate() {
                joints[leg][i] = find(n)?;
            }
        }
        let at = |j: usize| pose.get(j).map(|m| m.w_axis.truncate());
        let (mut hips, mut feet) = ([Vec3::ZERO; LEGS], [Vec3::ZERO; LEGS]);
        let (mut a, mut b) = ([0.0; LEGS], [0.0; LEGS]);
        for leg in 0..LEGS {
            let [h, m, f] = joints[leg];
            let (ph, pm, pf) = (at(h)?, at(m)?, at(f)?);
            hips[leg] = ph;
            feet[leg] = pf;
            a[leg] = ph.distance(pm);
            b[leg] = pm.distance(pf);
        }
        let gun = find("gun").zip(find("tamajnt")).and_then(|(g, t)| {
            let m = pose.get(g)?;
            Some(GunRig { joints: [t, g], pos: m.w_axis.truncate(), dir: m.x_axis.truncate().normalize_or_zero() })
        });
        Some(LegRig { joints, hips, feet, top_to_middle: a, middle_to_bottom: b, gun, foot_radius: 0.0 })
    }

    /// The rig of Pikmin 2 kind `kind` under the cache `root`: the model's joints in the last pose of its landing clip (where
    /// `startProgramedIK` reads them) and the radius of its foot spheres.
    pub fn load(root: &Path, kind: &crate::p2::Kind) -> Option<LegRig> {
        let base = root.join("enemy/data").join(kind.data);
        let bmd = std::fs::read(base.join("model/enemy.bmd")).ok()?;
        let skeleton = libsms::Skeleton::from_bmd(&bmd)?;
        let clip = libsms::Clip::parse_bca(&std::fs::read(base.join("anim/landing.bca")).ok()?)?;
        let pose = skeleton.pose(&clip, (clip.frames - 1.0).max(0.0));
        let mut rig = LegRig::from_pose(&skeleton.names, &pose)?;
        let coll = std::fs::read(root.join("enemy/parm").join(kind.shared).join("enemycoll.txt")).ok();
        rig.foot_radius = coll.map_or(0.0, |b| foot_sphere_radius(&String::from_utf8_lossy(&b)));
        Some(rig)
    }
}

/// `IKSystemParms`.
#[derive(Debug, Clone, Copy)]
pub struct GaitParms {
    pub bend_factor: f32,
    /// Degrees.
    pub max_turn_angle: f32,
    pub move_speed: f32,
    pub min_move_speed: f32,
    /// `mEnragedAngle`, which `getViewAngle` returns: the angle off the facing, in degrees, past which a step turns.
    pub view_angle: f32,
    pub bottom_move_speed: f32,
    pub raise_slowdown: f32,
    pub downward_accel: f32,
    pub max_decel: f32,
    pub min_decel: f32,
    pub height_offset: f32,
    pub interpolation: f32,
    pub trace_move_rate: f32,
    pub trace_damping: f32,
    pub trace_height: f32,
}

impl Default for GaitParms {
    fn default() -> Self {
        GaitParms {
            bend_factor: 0.0,
            max_turn_angle: 60.0,
            move_speed: 75.0,
            min_move_speed: 0.0,
            view_angle: 30.0,
            bottom_move_speed: 3.0,
            raise_slowdown: -0.15,
            downward_accel: 0.5,
            max_decel: 10.0,
            min_decel: -2.0,
            height_offset: 120.0,
            interpolation: 0.75,
            trace_move_rate: 0.1,
            trace_damping: 0.7,
            trace_height: -1.5,
        }
    }
}

/// One leg (`IKSystemBase`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Leg {
    pub enabled: bool,
    pub blend: bool,
    pub on_ground: bool,
    pub bend: f32,
    pub move_ratio: f32,
    move_timer: f32,
    /// Where the foot is (`mTargetPosition`).
    pub foot: Vec3,
    path: [Vec3; 3],
}

/// What the gait did this tick that a host plays a sound or a shake for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LegEvent {
    Lifted(usize),
    Landed(usize),
}

#[derive(Debug, Clone)]
pub struct Legs {
    pub rig: Arc<LegRig>,
    pub parms: GaitParms,
    /// `mIsIKActive`: the legs are standing on their own feet (after the landing).
    pub active: bool,
    /// `mInMotion`: the round of steps goes on.
    pub in_motion: bool,
    /// `mOnGround`: the walk is over for good (a death), no further legs start.
    pub grounded: bool,
    /// `mFaceDir` and the body's place (`mCentrePosition`), and where it is drawn (`mTraceCentrePosition`).
    pub face: f32,
    pub centre: Vec3,
    pub trace: Vec3,
    trace_vel: Vec3,
    distance_offset: f32,
    leg_angle: [f32; LEGS],
    pub states: [i32; LEGS],
    /// `mTargetPosition`: where the walk is headed.
    pub goal: Vec3,
    leg_goal: [Vec3; LEGS],
    pub legs: [Leg; LEGS],
    /// Whether leg `i` has hurt anything yet this stomp.
    pub stomped: [bool; LEGS],
    /// The Man-at-Legs' gun and shells.
    pub gun: crate::houdai::Gun,
}

/// The legs as a host draws them: the body's base, which legs the solver folds onto which feet.
#[derive(Debug, Clone, Copy)]
pub struct LegsView {
    /// The legs are on their feet (before the landing finishes the body plays its clip alone).
    pub active: bool,
    /// The body plays a blended clip (the flick): the clip's twist stays.
    pub blend: bool,
    /// Where the body is drawn and the way it faces (`mTraceCentrePosition`, `mFaceDir`).
    pub trace: Vec3,
    pub face: f32,
    pub feet: [Vec3; LEGS],
    pub bend: [f32; LEGS],
    /// Where the Man-at-Legs' gun points (the world direction of its barrel) while it is out of its rest.
    pub gun: Option<Vec3>,
}

/// A Y rotation by `yaw` (the way `Teki::forward` faces at `yaw`), applied to a model-space vector.
pub fn to_world(v: Vec3, yaw: f32) -> Vec3 {
    let (s, c) = yaw.sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

/// The inverse of [`to_world`].
pub fn to_local(v: Vec3, yaw: f32) -> Vec3 {
    to_world(v, -yaw)
}

/// `clampAngle`: into -PI..PI.
fn wrap(a: f32) -> f32 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a < -PI {
        a += TAU;
    }
    a
}

/// `getAngDist`: how far to turn from `from` to face `to`, -PI..PI.
fn ang_dist(to: f32, from: f32) -> f32 {
    wrap(to - from)
}

impl Legs {
    /// `IKSystemMgr::init` at the enemy's place.
    pub fn new(rig: Arc<LegRig>, pos: Vec3, yaw: f32) -> Legs {
        Legs {
            rig,
            parms: GaitParms::default(),
            active: false,
            in_motion: false,
            grounded: false,
            face: yaw,
            centre: pos,
            trace: pos,
            trace_vel: Vec3::ZERO,
            distance_offset: 100.0,
            leg_angle: [0.0; LEGS],
            states: [0; LEGS],
            goal: Vec3::ZERO,
            leg_goal: [Vec3::ZERO; LEGS],
            legs: [Leg { on_ground: true, ..Default::default() }; LEGS],
            stomped: [false; LEGS],
            gun: Default::default(),
        }
    }

    /// `startProgramedIK`: the legs stand on the feet the landing left them on. `pos`/`yaw` is where the body is.
    pub fn start_programmed(&mut self, pos: Vec3, yaw: f32) {
        self.active = true;
        self.in_motion = false;
        self.grounded = false;
        self.states = [0; LEGS];
        for i in 0..LEGS {
            let foot = pos + to_world(self.rig.feet[i], yaw);
            self.legs[i] = Leg { enabled: true, on_ground: self.legs[i].on_ground, move_ratio: 2.0, foot, ..Default::default() };
        }
        self.distance_offset = pos.distance(self.legs[0].foot);
        for i in 0..LEGS {
            let f = self.legs[i].foot;
            self.leg_angle[i] = (f.x - pos.x).atan2(f.z - pos.z) - yaw;
        }
    }

    /// `startIKMotion`.
    pub fn start_motion(&mut self) {
        self.in_motion = true;
        self.grounded = false;
        self.states = [0; LEGS];
    }

    /// `finishIKMotion`: the round that is going on ends and the walk stops.
    pub fn finish_motion(&mut self) {
        self.in_motion = false;
    }

    /// `forceFinishIKMotion`.
    pub fn force_finish(&mut self) {
        self.in_motion = false;
        self.grounded = true;
    }

    /// `isFinishIKMotion`: the walk is over and every leg is down.
    pub fn finished(&self) -> bool {
        !self.in_motion && self.states.iter().all(|&s| s == DONE)
    }

    /// `startBlendMotion` / `finishBlendMotion`.
    pub fn set_blend(&mut self, on: bool) {
        for l in &mut self.legs {
            l.blend = on;
        }
    }

    pub fn set_goal(&mut self, goal: Vec3) {
        self.goal = goal;
    }

    /// `IKSystemMgr::isCollisionCheck`: leg `i`'s foot is coming down on something (`lfsp` .. `rhsp`, the foot spheres).
    pub fn stomping(&self, i: usize) -> bool {
        (self.states[i] == SWINGING || self.states[i] == LANDED) && self.legs[i].move_ratio > 1.0
    }

    /// Where the foot sphere of leg `i` is (`getCollisionCentre` with the sphere's offset under the foot).
    pub fn foot_sphere(&self, i: usize) -> Vec3 {
        self.legs[i].foot + Vec3::new(0.0, -7.5, 0.0)
    }

    /// The foot spheres' radius (`enemyColl.txt`), 0 for a body whose feet do not hurt.
    pub fn foot_radius(&self) -> f32 {
        self.rig.foot_radius
    }

    pub fn view(&self) -> LegsView {
        LegsView {
            active: self.active,
            blend: self.legs[0].blend,
            trace: self.trace,
            face: self.face,
            feet: std::array::from_fn(|i| self.legs[i].foot),
            bend: std::array::from_fn(|i| self.legs[i].bend),
            gun: self.gun.rotating.then(|| self.gun.dir()),
        }
    }

    /// `IKSystemBase::startMovePosition`.
    fn start_step(&mut self, i: usize, mut to: Vec3, ground: f32) {
        let p = self.parms;
        let leg = &mut self.legs[i];
        leg.on_ground = false;
        leg.bend = 0.0;
        leg.move_ratio = 0.0;
        leg.move_timer = 0.0;
        leg.path[0] = leg.foot;
        to.y = ground;
        leg.path[2] = to;
        let mut mid = to * p.interpolation + leg.path[0] * (1.0 - p.interpolation);
        mid.y += p.height_offset;
        leg.path[1] = mid;
        self.stomped[i] = false;
    }

    /// `IKSystemBase::update`.
    fn update_leg(&mut self, i: usize, ground: f32, dt: f32) {
        let p = self.parms;
        let leg = &mut self.legs[i];
        if !leg.enabled {
            return;
        }
        if !leg.on_ground {
            // `moveBottomJointPosition`
            leg.move_timer += if leg.move_ratio < 1.0 { p.raise_slowdown } else { p.downward_accel };
            leg.move_timer = leg.move_timer.clamp(p.min_decel, p.max_decel);
            leg.move_ratio += (p.bottom_move_speed + leg.move_timer) * dt;
            leg.foot = ik::calc_lagrange(&leg.path, leg.move_ratio);
            // `onGroundPosition`: a foot that went into the ground stands on it.
            if leg.move_ratio > 1.0 && ground > leg.foot.y {
                leg.foot.y = ground;
                leg.on_ground = true;
            }
        }
        leg.bend = ik::bend_ratio(leg.on_ground, leg.move_ratio, p.bend_factor);
    }

    /// `IKSystemMgr::setNextCentrePosition`.
    fn next_centre(&mut self, owner: Vec3, owner_yaw: f32) {
        let p = self.parms;
        let to_goal = ang_dist((self.goal.x - owner.x).atan2(self.goal.z - owner.z), owner_yaw);
        let mut turn = to_goal;
        let next;
        if to_goal.abs() <= p.view_angle.to_radians() {
            let (o, g) = (Vec3::new(owner.x, 0.0, owner.z), Vec3::new(self.goal.x, 0.0, self.goal.z));
            let dist = o.distance(g);
            next = if dist > p.move_speed {
                o + (g - o).normalize_or_zero() * p.move_speed
            } else if dist < p.min_move_speed {
                o + (g - o).normalize_or_zero() * p.min_move_speed
            } else {
                g
            };
        } else {
            next = Vec3::new(owner.x, 0.0, owner.z);
            let max = p.max_turn_angle.to_radians();
            if turn.abs() > max {
                turn = max.copysign(turn);
            }
        }
        let face = turn + owner_yaw;
        for i in 0..LEGS {
            let a = face + self.leg_angle[i];
            self.leg_goal[i] = next + Vec3::new(self.distance_offset * a.sin(), 0.0, self.distance_offset * a.cos());
        }
    }

    /// `IKSystemMgr::updateController`; returns the events of legs that lifted or landed.
    fn controller(&mut self, owner: Vec3, owner_yaw: f32, ground: f32, events: &mut Vec<LegEvent>) {
        if self.in_motion {
            if self.states.iter().all(|&s| s == DONE) {
                self.states = [0; LEGS];
            }
            if self.states.iter().all(|&s| s == WAITING) {
                self.next_centre(owner, owner_yaw);
                self.start_step(0, self.leg_goal[0], ground);
                self.states[0] = SWINGING;
                events.push(LegEvent::Lifted(0));
            }
        }
        for i in 0..LEGS {
            if self.states[i] == SWINGING {
                if self.legs[i].on_ground {
                    self.states[i] = LANDED;
                }
            } else if self.states[i] == LANDED {
                self.states[i] = DONE;
                self.trace_vel.y += self.parms.trace_height;
                events.push(LegEvent::Landed(i));
                // The next leg goes when this one lands (leg 3's next would be leg 0, which the round does not restart).
                let next = i + 1;
                if next > 0 && next < LEGS && !self.grounded {
                    self.start_step(next, self.leg_goal[next], ground);
                    self.states[next] = SWINGING;
                    events.push(LegEvent::Lifted(next));
                }
            }
        }
    }

    /// `IKSystemMgr::doUpdate`: one tick. `owner` and `owner_yaw` are where the enemy is now, `ground` the height under it.
    /// Returns the legs' events; afterwards `centre` and `face` are where the enemy is (`Obj::updateIKSystem`).
    pub fn update(&mut self, owner: Vec3, owner_yaw: f32, ground: f32, dt: f32) -> Vec<LegEvent> {
        let mut events = Vec::new();
        for i in 0..LEGS {
            self.update_leg(i, ground, dt);
        }
        self.controller(owner, owner_yaw, ground, &mut events);
        // `calcFaceDir`, `calcCentrePosition`, `calcTraceCentrePosition`.
        if self.active {
            let (a, b) = (self.legs[0].foot, self.legs[1].foot);
            self.face = wrap(((a.x + b.x) / 2.0 - self.centre.x).atan2((a.z + b.z) / 2.0 - self.centre.z));
            let feet: Vec<Vec3> = self.legs.iter().map(|l| l.foot).collect();
            let mut c = Vec3::ZERO;
            for f in &feet {
                c.x += f.x;
                c.z += f.z;
            }
            c.x *= 0.25;
            c.z *= 0.25;
            let mut h: Vec<f32> = feet.iter().map(|f| f.y).collect();
            h.sort_by(|x, y| x.total_cmp(y));
            for (w, y) in [0.4, 0.3, 0.2, 0.1].iter().zip(&h) {
                c.y += w * y;
            }
            self.centre = c;
            self.trace_vel += (self.centre - self.trace) * self.parms.trace_move_rate;
            self.trace += self.trace_vel;
            self.trace_vel *= self.parms.trace_damping;
        } else {
            self.face = owner_yaw;
            self.centre = owner;
            self.trace = owner;
        }
        events
    }
}

/// `Obj::updateIKSystem` for an enemy with legs: the gait runs and the enemy goes where it says. Returns its events.
pub fn update_common(t: &mut Teki, dt: f32) -> Vec<LegEvent> {
    let ground = t.ground_y.unwrap_or(t.pos.y);
    let (pos, yaw) = (t.pos, t.yaw);
    let Some(legs) = t.legs.as_mut() else { return Vec::new() };
    let events = legs.update(pos, yaw, ground, dt);
    t.pos = legs.centre;
    t.yaw = legs.face;
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rig of four legs standing on a square, as a synthetic model would give.
    fn rig() -> Arc<LegRig> {
        let corners = [Vec3::new(-75.0, 6.0, 75.0), Vec3::new(75.0, 6.0, 75.0), Vec3::new(-75.0, 6.0, -75.0), Vec3::new(75.0, 6.0, -75.0)];
        Arc::new(LegRig {
            joints: [[0; 3]; LEGS],
            hips: corners.map(|c| Vec3::new(0.0, 184.0, 0.0) + c * 0.0),
            feet: corners,
            top_to_middle: [160.0; LEGS],
            middle_to_bottom: [120.0; LEGS],
            gun: None,
            foot_radius: 15.0,
        })
    }

    fn standing() -> Legs {
        let mut l = Legs::new(rig(), Vec3::ZERO, 0.0);
        l.start_programmed(Vec3::ZERO, 0.0);
        l
    }

    fn run(l: &mut Legs, ticks: usize, goal: Vec3) {
        for _ in 0..ticks {
            l.set_goal(goal);
            let (pos, yaw) = (l.centre, l.face);
            l.update(pos, yaw, 0.0, 1.0 / 30.0);
        }
    }

    #[test]
    fn the_feet_start_where_the_landing_left_them() {
        let l = standing();
        for i in 0..LEGS {
            assert!((l.legs[i].foot - to_world(l.rig.feet[i], 0.0)).length() < 1e-4);
        }
        assert!((l.distance_offset - Vec3::new(75.0, 6.0, 75.0).length()).abs() < 1e-3);
    }

    #[test]
    fn the_legs_step_in_turn_and_the_body_follows_its_feet_toward_the_goal() {
        let mut l = standing();
        l.start_motion();
        let goal = Vec3::new(0.0, 0.0, 600.0);
        let mut lifted = Vec::new();
        for _ in 0..600 {
            l.set_goal(goal);
            let (pos, yaw) = (l.centre, l.face);
            for e in l.update(pos, yaw, 0.0, 1.0 / 30.0) {
                if let LegEvent::Lifted(i) = e {
                    lifted.push(i);
                }
            }
        }
        // 0 1 2 3 0 1 2 3 ..: each leg lifts when the one before it lands.
        assert!(lifted.len() >= 8, "{lifted:?}");
        for (n, i) in lifted.iter().enumerate() {
            assert_eq!(*i, n % 4, "{lifted:?}");
        }
        assert!(l.centre.z > 150.0, "the body went ahead: {}", l.centre);
        assert!(l.centre.x.abs() < 40.0, "and kept straight: {}", l.centre);
        // Facing the way it goes.
        assert!(l.face.abs() < 0.2, "{}", l.face);
        // Every foot is on the ground or in its step.
        for leg in &l.legs {
            assert!(leg.foot.y >= -1e-3 && leg.foot.y < 130.0, "{}", leg.foot);
        }
    }

    #[test]
    fn a_goal_off_to_the_side_makes_it_turn_in_place_first() {
        let mut l = standing();
        l.start_motion();
        // Due east, 90 degrees off its facing: past the 30 degree view angle, so the first rounds turn.
        let goal = Vec3::new(800.0, 0.0, 0.0);
        run(&mut l, 90, goal);
        assert!(l.centre.length() < 80.0, "turning on the spot: {}", l.centre);
        assert!(l.face > 0.5, "and facing round: {}", l.face);
        run(&mut l, 600, goal);
        assert!(l.centre.x > 100.0, "then walking: {}", l.centre);
    }

    #[test]
    fn finishing_the_walk_lets_every_leg_land_and_then_it_is_over() {
        let mut l = standing();
        l.start_motion();
        run(&mut l, 20, Vec3::new(0.0, 0.0, 600.0));
        l.finish_motion();
        let mut ticks = 0;
        while !l.finished() && ticks < 600 {
            run(&mut l, 1, Vec3::new(0.0, 0.0, 600.0));
            ticks += 1;
        }
        assert!(l.finished(), "{:?}", l.states);
        assert!(l.legs.iter().all(|l| l.on_ground));
    }

    #[test]
    fn a_foot_comes_down_on_the_ground_height_it_is_given() {
        let mut l = standing();
        l.start_motion();
        for _ in 0..300 {
            l.set_goal(Vec3::new(0.0, 0.0, 600.0));
            let (pos, yaw) = (l.centre, l.face);
            l.update(pos, yaw, 25.0, 1.0 / 30.0);
        }
        for leg in l.legs.iter().filter(|l| l.on_ground) {
            assert!((leg.foot.y - 25.0).abs() < 1e-3, "{}", leg.foot);
        }
    }

    #[test]
    fn the_feet_stomp_only_while_coming_down() {
        let mut l = standing();
        l.start_motion();
        let mut stomps = 0;
        let mut at_rest_stomping = 0;
        for _ in 0..400 {
            l.set_goal(Vec3::new(0.0, 0.0, 600.0));
            let (pos, yaw) = (l.centre, l.face);
            l.update(pos, yaw, 0.0, 1.0 / 30.0);
            for i in 0..LEGS {
                if l.stomping(i) {
                    stomps += 1;
                    assert!(l.legs[i].move_ratio > 1.0 && !l.legs[i].on_ground || l.states[i] == LANDED);
                }
                if l.states[i] == WAITING && l.stomping(i) {
                    at_rest_stomping += 1;
                }
            }
        }
        assert!(stomps > 0 && at_rest_stomping == 0);
    }
}

/// The radius of the collision sphere `lfsp` in an `enemyColl.txt` (each sphere is a radius line, then its `{id}`), or 0.
pub fn foot_sphere_radius(text: &str) -> f32 {
    let mut radius = 0.0;
    for line in text.lines() {
        let (value, comment) = line.split_once('#').unwrap_or((line, ""));
        let value = value.trim();
        if comment.trim().starts_with("radius") {
            radius = value.parse().unwrap_or(radius);
        } else if value == "{lfsp}" {
            return radius;
        }
    }
    0.0
}
