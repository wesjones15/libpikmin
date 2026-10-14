//! Captains (decomp `Navi`). Several can exist at once; each owns a squad formation.

use glam::{Vec2, Vec3};

use crate::event::Event;
use crate::formation::Plate;
use crate::math::{ang_dist, dir_of, dist_xz, round_ang, yaw_of};
use crate::piki::{PikiColor, PikiState};
use crate::sim::{Sim, integrate};
use crate::{CaptainId, Host, PikiId};

/// How far above Olimar the cursor looks down for the ground. Pikmin's maps keep the cursor
/// on a heightfield; a 3D host needs the probe to start above any slope within reach.
const CURSOR_PROBE_LIFT: f32 = 100.0;
/// How high above his feet the locked-on cursor looks back at him (host extension).
const LOCK_EYE_HEIGHT: f32 = 24.0;
/// The locked-on cursor stays at least this far from the captain's eye.
const LOCK_MIN_DISTANCE: f32 = 20.0;
/// Clear space between a locked-on target's surface and its cursor.
const LOCK_MARGIN: f32 = 4.0;

/// Per-frame controls for one captain. Buttons are levels (held = true); the sim detects edges.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CaptainInput {
    /// Movement stick in world XZ (x = world X, y = world Z), already camera-relative.
    /// Each axis is clamped to [-1, 1] like `Navi::makeVelocity`.
    pub stick: Vec2,
    pub whistle: bool,
    pub throw: bool,
    pub dismiss: bool,
    /// C-stick: swarm direction in world XZ, camera-relative like `stick`
    /// (`Navi::makeCStick`). Zero = neutral.
    pub swarm: Vec2,
    /// When set, the host drives the captain's body (a player avatar, libsm64...) and the sim
    /// only runs the cursor, whistle, throw and squad.
    pub pose: Option<CaptainPose>,
    /// A world point to put the cursor on (a mouse or camera aim), kept within the cursor's
    /// reach (`NaviParms` cursor max radius). Not in the game, where only the stick moves the
    /// cursor; hosts that aim with a camera use it instead.
    pub cursor: Option<Vec3>,
    /// Picks the next colour to throw (Pikmin 2's type switch; not in Pikmin 1, which always
    /// throws the nearest).
    pub cycle: bool,
    /// Host extension: jump on the rising edge. The modifier blocks jumping even if pressed.
    pub jump: bool,
    pub jump_blocked: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CaptainPose {
    pub pos: Vec3,
    pub vel: Vec3,
    pub face: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WhistleState {
    Off,
    /// Button held, circle growing (`mWhistleCircleMode == 1`). No Pikmin are called yet.
    Expanding {
        timer: f32,
    },
    /// Calling every tick at `radius` (`mWhistleCircleMode == 2`).
    Calling {
        timer: f32,
        radius: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThrowState {
    Idle,
    /// `NaviThrowWaitState`: holding the throw button.
    Wait {
        held: Option<PikiId>,
        pending: Option<PikiId>,
        pending_timeout: f32,
        hold_time: f32,
    },
}

/// Host extension: where to draw a locked-on cursor. Pikmin 1's cursor lies on the ground; the
/// part a lock aims at can be anywhere on the enemy, so the view stands the cursor up in front
/// of its whole body, facing the captain, at the usual size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LockView {
    /// In front of the middle of the target's body as the captain sees it.
    pub pos: Vec3,
    /// The captain's eye, which the cursor faces.
    pub eye: Vec3,
}

#[derive(Debug, Clone)]
pub struct Captain {
    pub id: CaptainId,
    pub pos: Vec3,
    pub vel: Vec3,
    pub target_vel: Vec3,
    pub face: f32,
    pub on_ground: bool,
    pub jump: Option<crate::jump::Jump>,
    /// Cursor offset from the captain, XZ (`mCursorPosition`).
    pub cursor: Vec3,
    /// Cursor on the ground in world space (`mCursorWorldPos`). Throws land here and the whistle is centred here.
    pub cursor_world: Vec3,
    /// Seconds since any input (`mNeutralTime`).
    pub neutral_time: f32,
    pub whistle: WhistleState,
    pub throw: ThrowState,
    pub plate: Plate,
    /// Plucking a sprout. Movement, whistle and throw are locked meanwhile.
    pub pluck: Option<crate::onion::Pluck>,
    /// Chained plucking (`mFastPluckKeyTaps > 0`): faster pulls, wider search.
    pub fast_pluck: bool,
    /// Punching (`NaviAttackState`).
    pub punch: Option<Punch>,
    /// The C-stick is pushing the squad (`!mIsCStickNeutral`).
    pub swarming: bool,
    /// Last squad direction (`mPlateYaw`).
    pub plate_yaw: f32,
    /// `mHealth`, from `NaviParams::health`.
    pub health: f32,
    /// Frames left of the damage animation; invulnerable while > 0.
    pub damage_frames: f32,
    /// Health ran out (`NaviDeadState`) or the host put him down.
    pub down: bool,
    /// The colour he throws next, once the player has picked one with `cycle`; None throws
    /// the nearest Pikmin, as in Pikmin 1.
    pub throw_color: Option<PikiColor>,
    /// Optional host lock-on. Vanilla throws remain unchanged when absent.
    pub lock_target: Option<crate::host::CreatureId>,
    /// Multiplies his run speed: the host's own speed stat over its base (items, buffs).
    /// 1 in Pikmin.
    pub speed_scale: f32,
    /// Multiplies how far his cursor reaches (and so how far he throws) and how fast it moves
    /// out there. 1 in Pikmin.
    pub reach_scale: f32,
    /// Host extension: how fast his punches and plucks, and his Pikmin's attacks, play (the
    /// host's attack speed over its base). 1 in Pikmin.
    pub attack_rate: f32,
    /// Host extension: multiplies his Pikmin's walk and run speeds (the host's inherited move
    /// speed). 1 in Pikmin.
    pub squad_speed: f32,
    /// Host extension: jumps he may make in mid-air (Hopoo Feather), and how many he's made
    /// since leaving the ground.
    pub air_jumps: u32,
    pub air_jumps_used: u32,
    /// Host extension: jump with no mid-air jumps left starts a stomp (H3AD-5T v2).
    pub can_stomp: bool,
    /// Stomping down since this height.
    pub stomp: Option<f32>,
    /// Set while the lock-on target is alive and has a part to aim at.
    pub lock_view: Option<LockView>,
    /// Host extension: a dismiss strike in progress, and seconds until the next.
    pub strike: Option<crate::strike::Strike>,
    pub strike_cooldown: f32,
    /// Highest point of the current time in the air (host extension: fall damage).
    pub(crate) air_peak: f32,
    /// The tick his current jump started, so later mid-air jumps reach his followers' cues.
    pub(crate) jump_tick: Option<u64>,
    rearrange_pending: bool,
    pub(crate) input: CaptainInput,
    pub(crate) pressed: Pressed,
    movement_held: bool,
    actions_held: bool,
    pub(crate) launched: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Punch {
    /// "PUNCH" animation counter.
    pub frame: f32,
    /// Landed on something this punch.
    pub hit: bool,
    /// Whistle pressed mid-punch: whistle when it ends (`mGatherRequested`).
    pub whistle_queued: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Pressed {
    pub jump: bool,
    pub whistle: bool,
    pub throw: bool,
    pub dismiss: bool,
    pub cycle: bool,
}

impl Captain {
    pub fn swarm_input(&self) -> Vec2 { if self.swarming { self.input.swarm } else { Vec2::ZERO } }
    pub(crate) fn new(id: CaptainId, pos: Vec3, health: f32) -> Self {
        Self {
            id,
            pos,
            vel: Vec3::ZERO,
            target_vel: Vec3::ZERO,
            face: 0.0,
            on_ground: false,
            jump: None,
            cursor: Vec3::ZERO,
            cursor_world: pos,
            neutral_time: 0.0,
            whistle: WhistleState::Off,
            throw: ThrowState::Idle,
            plate: Plate::default(),
            pluck: None,
            fast_pluck: false,
            punch: None,
            swarming: false,
            plate_yaw: std::f32::consts::PI,
            health,
            damage_frames: 0.0,
            down: false,
            throw_color: None,
            lock_target: None,
            speed_scale: 1.0,
            reach_scale: 1.0,
            attack_rate: 1.0,
            squad_speed: 1.0,
            air_jumps: 0,
            air_jumps_used: 0,
            can_stomp: false,
            stomp: None,
            lock_view: None,
            strike: None,
            strike_cooldown: 0.0,
            air_peak: pos.y,
            jump_tick: None,
            rearrange_pending: false,
            input: CaptainInput::default(),
            pressed: Pressed::default(),
            movement_held: false,
            actions_held: false,
            launched: false,
        }
    }

    /// Current whistle circle radius, or 0 when not whistling.
    pub fn whistle_radius(&self, sim_params: &crate::Params) -> f32 {
        let n = &sim_params.navi;
        match self.whistle {
            WhistleState::Off => 0.0,
            WhistleState::Expanding { timer } => {
                n.whistle_min_radius
                    + (timer / n.whistle_expand_time).min(1.0)
                        * (n.whistle_max_radius - n.whistle_min_radius)
            }
            WhistleState::Calling { radius, .. } => radius,
        }
    }

    /// The Pikmin in hand, if any.
    pub fn held_piki(&self) -> Option<PikiId> {
        match self.throw {
            ThrowState::Wait { held, .. } => held,
            ThrowState::Idle => None,
        }
    }

    /// Where a held Pikmin dangles. Approximates the `'rhnd'` collision sphere on Olimar's right hand.
    pub fn hand_pos(&self) -> Vec3 {
        self.pos + dir_of(self.face) * 6.0 + Vec3::new(0.0, 8.0, 0.0)
    }
}

impl Sim {
    /// Host crowd control: roots hold voluntary movement, stuns also pause actions.
    /// External launches and gravity continue while control is held.
    pub fn set_captain_hold(&mut self, captain: CaptainId, movement: bool, actions: bool) {
        if let Some(c) = self.captains.get_mut(captain.0 as usize) {
            c.movement_held = movement || actions;
            c.actions_held = actions;
        }
    }

    /// Host launch (a jump pad, geyser or knockback), in sim units per second.
    /// Positive `gravity` preserves a host pad's ballistic trajectory. `squad` queues
    /// that takeoff for following Pikmin; ordinary knockback leaves it false.
    pub fn launch_captain(&mut self, captain: CaptainId, velocity: Vec3, gravity: f32, squad: bool) {
        if !velocity.is_finite() || !gravity.is_finite() || gravity < 0.0 { return; }
        let gravity = (gravity > 0.0).then_some(gravity);
        let mut cue = None;
        if let Some(c) = self.captains.get_mut(captain.0 as usize) {
            c.vel = velocity;
            if let Some(g) = gravity {
                // integrate applies gravity before position: centre the velocity on the tick.
                c.vel.y += 0.5 * g * crate::TICK_DT;
            }
            c.on_ground = false;
            c.launched = true;
            c.pluck = None;
            c.punch = None;
            c.jump = Some(crate::jump::Jump { horizontal: velocity.with_y(0.0), gravity, ..Default::default() });
            c.stomp = None;
            if squad {
                c.jump_tick = Some(self.tick);
                cue = Some(crate::jump::JumpCue { origin: c.pos, horizontal: velocity.with_y(0.0),
                    launch: Some((velocity.y, gravity.unwrap_or(self.params.ai.gravity))), tick: self.tick,
                    air: [0.0; crate::jump::MAX_AIR_JUMPS], air_count: 0 });
            }
        }
        if let Some(cue) = cue {
            self.queue_squad_jump(captain.0 as usize, cue);
        }
    }

    /// Host extension: a throw with no lock-on is steered in flight toward the cursor point it
    /// was thrown at, the way a locked-on throw follows its creature, so it arrives there after
    /// `throw_flight_time` whatever the captain's speed or the cursor's height. This replaces
    /// the original parabola's hold-time height; off by default.
    pub fn set_cursor_homing(&mut self, on: bool) {
        self.cursor_homing = on;
    }

    pub fn set_lock_target(&mut self, captain: CaptainId, target: Option<crate::host::CreatureId>) {
        if let Some(c) = self.captains.get_mut(captain.0 as usize) { c.lock_target = target; }
    }
    pub(crate) fn update_captain(&mut self, ci: usize, host: &mut dyn Host, dt: f32) {
        let input = self.captains[ci].input;
        let pressed = std::mem::take(&mut self.captains[ci].pressed);

        self.update_captain_damage(ci, dt);
        let cooldown = self.captains[ci].strike_cooldown;
        self.captains[ci].strike_cooldown = (cooldown - dt).max(0.0);
        if self.captains[ci].down {
            let c = &mut self.captains[ci];
            c.strike = None;
            c.vel = Vec3::ZERO;
            c.target_vel = Vec3::ZERO;
            c.whistle = WhistleState::Off;
            c.pluck = None;
            c.jump = None;
            return;
        }

        if self.captains[ci].actions_held {
            let held = CaptainInput { pose: input.pose, cursor: input.cursor, ..Default::default() };
            self.captain_move(ci, host, dt, held, false);
            let c = &mut self.captains[ci];
            c.whistle = WhistleState::Off;
            c.swarming = false;
            if c.on_ground { c.jump = None; }
            self.captain_formation(ci);
            return;
        }

        if self.update_pluck(ci, dt, input.throw, pressed.throw) {
            self.captain_move(ci, host, dt, input, true);
            self.captain_formation(ci);
            return;
        }

        // The action button plucks when a sprout is in reach, else throws
        // (`Navi::procActionButton` runs before entering `NaviThrowWaitState`).
        if pressed.throw
            && self.captains[ci].jump.is_none()
            && matches!(self.captains[ci].throw, ThrowState::Idle)
            && let Some(sprout) = self.find_pluckable(ci)
        {
            self.start_pluck(ci, sprout);
            self.captain_move(ci, host, dt, input, true);
            self.captain_formation(ci);
            return;
        }

        if pressed.jump && !input.jump_blocked && !self.captains[ci].movement_held && input.pose.is_none()
            && self.captains[ci].damage_frames <= 0.0
            && self.captains[ci].punch.is_none()
        {
            let c = &self.captains[ci];
            if c.on_ground {
                self.start_captain_jump(ci);
            } else if c.stomp.is_none() && c.air_jumps_used < c.air_jumps {
                self.start_air_jump(ci);
            } else if c.stomp.is_none() && c.can_stomp {
                self.start_stomp(ci);
            }
        }
        let was_grounded = self.captains[ci].on_ground;
        let takeoff_origin = self.captains[ci].pos;
        self.captain_move(ci, host, dt, input, false);
        // Host-driven captains (VR) leave the ground through their host's controller.
        if input.pose.is_some() && was_grounded && !self.captains[ci].on_ground
            && self.captains[ci].vel.y > 0.0 && !input.jump_blocked
        {
            self.record_captain_jump(ci, takeoff_origin);
        }
        self.update_stomp(ci);
        let c = &mut self.captains[ci];
        if c.on_ground { c.jump = None; c.air_jumps_used = 0; }
        else if let Some(jump) = &mut c.jump { jump.elapsed += dt; }
        self.captain_formation(ci);
        if self.captain_punch(ci, host, dt, pressed) {
            return;
        }
        if self.captain_strike(ci, host, dt) {
            return;
        }
        self.captain_whistle(ci, dt, input, pressed);
        if pressed.cycle {
            self.cycle_throw_color(ci, input.throw);
        }
        self.captain_throw(ci, dt, input, pressed);
        if pressed.dismiss {
            if self.dismiss_strike.is_some() {
                self.start_strike(ci);
            } else {
                self.dismiss(ci);
            }
        }
    }

    /// `Navi::makeVelocity`: stick → target velocity, cursor movement, light-tilt aiming.
    /// `locked`: the body is being moved by something else (plucking); only the cursor runs.
    fn captain_move(
        &mut self,
        ci: usize,
        host: &mut dyn Host,
        dt: f32,
        input: CaptainInput,
        locked: bool,
    ) {
        let navi = &self.params.navi;
        let accel = self.params.navi_creature.acceleration;
        let face_adjust = self.params.navi_creature.face_dir_adjust;
        let gravity = self.captains[ci].jump.and_then(|j| j.gravity).unwrap_or(self.params.ai.gravity);
        let c = &mut self.captains[ci];

        c.neutral_time += dt;
        if input.whistle || input.throw || input.dismiss {
            c.neutral_time = 0.0;
        }

        let stick = if c.movement_held { Vec3::ZERO } else { Vec3::new(
            input.stick.x.clamp(-1.0, 1.0),
            0.0,
            input.stick.y.clamp(-1.0, 1.0),
        ) };
        let stick_mag = stick.length();

        // canNaviRunFast() always returns true in the shipped game, so it's always mRunSpeed.
        c.target_vel = stick * (navi.run_speed * c.speed_scale);

        // The cursor leads the captain in the stick direction, sliding along its max radius.
        let reach = navi.cursor_max_radius * c.reach_scale;
        let mut cursor_vel = stick.normalize_or_zero() * (navi.cursor_move_speed * c.reach_scale);
        let mut next = c.cursor + cursor_vel * dt;
        if next.length() >= reach {
            let radial = next.normalize_or_zero();
            cursor_vel -= radial * radial.dot(cursor_vel);
            next = c.cursor + cursor_vel * dt;
        }
        c.cursor = next;
        if let Some(aim) = input.cursor {
            let off = Vec3::new(aim.x - c.pos.x, 0.0, aim.z - c.pos.z);
            c.cursor = off.clamp_length_max(reach);
        }

        if stick_mag > navi.neutral_stick_threshold {
            c.neutral_time = 0.0;
        }
        // A light tilt (or standing still long enough) aims at the cursor instead of walking.
        let idle_long = c.neutral_time >= navi.cursor_move_delay_time;
        let aiming = !locked
            && (idle_long || stick_mag > navi.neutral_stick_threshold)
            && stick_mag <= navi.cursor_move_stick_threshold;
        if aiming {
            c.target_vel = Vec3::ZERO;
            if c.cursor.length_squared() > 1e-6 {
                c.face = round_ang(c.face + 0.2 * ang_dist(yaw_of(c.cursor), c.face));
            }
        }

        if locked && !c.launched {
            c.target_vel = Vec3::ZERO;
            if let Some(g) = host.ground_height(c.pos + Vec3::new(0.0, 20.0, 0.0)) {
                c.pos.y = g;
                c.on_ground = true;
            }
        } else if let Some(pose) = input.pose {
            c.pos = pose.pos;
            c.vel = pose.vel;
            c.face = pose.face;
            c.launched = false;
            c.on_ground = pose.vel.y <= 0.0 && host.ground_height(pose.pos + Vec3::Y)
                .is_some_and(|g| (pose.pos.y - g).abs() < 2.0);
            // The host places him (a teleport, or its own controller): no fall to report.
            c.air_peak = c.pos.y;
        } else {
            let mut on_ground = c.on_ground;
            let was_grounded = on_ground;
            if c.movement_held && !c.launched {
                c.vel.x = 0.0;
                c.vel.z = 0.0;
            }
            // Keep a pad's horizontal throw through flight rather than accelerating back
            // to the movement stick on the next tick. Ground contact restores normal walking.
            let target = if c.launched { c.vel.with_y(0.0) } else { c.target_vel };
            integrate(
                host,
                &mut c.pos,
                &mut c.vel,
                target,
                &mut on_ground,
                navi.body_collision_radius,
                accel,
                gravity,
                dt,
            );
            c.on_ground = on_ground;
            if on_ground { c.launched = false; }
            // Host extension: report how far he fell on touching down, for fall damage.
            if !on_ground {
                c.air_peak = if was_grounded { c.pos.y } else { c.air_peak.max(c.pos.y) };
            } else if !was_grounded {
                let drop = (c.air_peak - c.pos.y).max(0.0);
                let (captain, stomp) = (c.id, c.stomp.is_some());
                c.air_peak = c.pos.y;
                self.events.push(crate::Event::CaptainLanded { captain, drop, stomp });
            }
            let speed = c.vel.x.hypot(c.vel.z);
            if !aiming && speed > 1.0 {
                c.face = round_ang(c.face + face_adjust * ang_dist(yaw_of(c.vel), c.face));
            }
        }

        // Probe from well above Olimar, so ground rising toward the cursor is found too.
        let ground = host
            .ground_height(c.pos + c.cursor + Vec3::Y * CURSOR_PROBE_LIFT)
            .unwrap_or(c.pos.y);
        c.cursor_world = Vec3::new(c.pos.x + c.cursor.x, ground + 1.0, c.pos.z + c.cursor.z);
        c.lock_view = None;
        if let Some(target) = c.lock_target.and_then(|id| self.creatures.get(id)).filter(|t| t.alive)
            && let Some(part) = self.creatures.parts_of(target).iter().find(|p| p.stickable)
        {
            c.cursor_world = part.centre;
            c.cursor = Vec3::new(part.centre.x - c.pos.x, 0., part.centre.z - c.pos.z);
            let eye = c.pos + Vec3::Y * LOCK_EYE_HEIGHT;
            let pos = lock_marker(self.creatures.parts_of(target), eye);
            c.lock_view = Some(LockView { pos, eye });
        }
    }

    /// Formation upkeep (`Navi::makeCStick`). With the C-stick pushed the plate swings toward
    /// it (40% of the way if within 120° of where it points, else straight there) and slims
    /// down as the stick goes further; neutral, it settles behind the captain.
    fn captain_formation(&mut self, ci: usize) {
        let navi = &self.params.navi;
        let plate_parms = &self.params.plate;
        let c = &self.captains[ci];
        let swarm = Vec3::new(c.input.swarm.x, 0.0, c.input.swarm.y);
        let dead_zone = 0.05;

        if swarm.length() > dead_zone {
            let input_yaw = yaw_of(swarm);
            let squad_yaw = c.plate.direction;
            let target = if dir_of(input_yaw).dot(dir_of(squad_yaw)) > (2.094_395_2f32).cos() {
                round_ang(squad_yaw + 0.4 * ang_dist(input_yaw, squad_yaw))
            } else {
                round_ang(input_yaw)
            };
            let s = ((swarm.length().min(1.0) - dead_zone) / (1.0 - dead_zone)).max(0.0);
            let strength = if s >= 0.9 { 1.0 } else { 0.6 * (s / 0.9) };
            let (pos, vel) = (c.pos, c.vel);
            let c = &mut self.captains[ci];
            c.neutral_time = 0.0;
            c.swarming = true;
            c.plate_yaw = target;
            c.plate.refresh(plate_parms, strength);
            c.plate.set_pos(plate_parms, pos, target, vel);
            return;
        }

        // Distance bands from the nearest squad member decide when the plate re-sorts.
        let mut nearest = f32::MAX;
        for &id in &c.plate.members {
            let p = &self.pikis[id.0 as usize];
            if !matches!(p.state, PikiState::Held { .. } | PikiState::GoHang { .. }) {
                nearest = nearest.min((p.pos - c.pos).length());
            }
        }
        let band = if nearest < navi.piki_wait_range {
            0
        } else if nearest < navi.piki_formation_change_range {
            1
        } else {
            2
        };

        let behind = c.face + std::f32::consts::PI;
        let moving = c.target_vel.length() >= 50.0 || matches!(c.throw, ThrowState::Wait { .. });
        let (pos, vel) = (c.pos, c.vel);
        let pikis = &self.pikis;
        let c = &mut self.captains[ci];
        c.swarming = false;
        c.plate.refresh(plate_parms, 0.0);
        if band == 2 {
            if c.rearrange_pending {
                c.plate.rearrange(pos, |id| pikis[id.0 as usize].pos);
                c.rearrange_pending = false;
            }
            c.plate.set_pos(plate_parms, pos, behind, vel);
            c.plate_yaw = behind;
        } else {
            c.rearrange_pending = true;
            let dir = if moving { behind } else { c.plate_yaw };
            if moving {
                c.plate_yaw = behind;
            }
            c.plate.set_pos(plate_parms, pos, dir, vel);
        }
    }

    /// `NaviGatherState::exec`. The circle grows while held and only calls Pikmin once it's
    /// full size (then keeps calling for `whistle_hold_time`) or when released early.
    fn captain_whistle(
        &mut self,
        ci: usize,
        dt: f32,
        input: CaptainInput,
        pressed: crate::captain::Pressed,
    ) {
        let n = self.params.navi.clone();
        if pressed.whistle {
            self.cancel_throw(ci);
            self.captains[ci].whistle = WhistleState::Expanding { timer: 0.0 };
        }

        let mut call = None;
        let c = &mut self.captains[ci];
        match c.whistle {
            WhistleState::Off => {}
            WhistleState::Expanding { timer } => {
                if input.whistle {
                    let t = timer + dt;
                    if t > n.whistle_expand_time {
                        c.whistle = WhistleState::Calling {
                            timer: 0.0,
                            radius: n.whistle_max_radius,
                        };
                        call = Some(n.whistle_max_radius);
                    } else {
                        c.whistle = WhistleState::Expanding { timer: t };
                    }
                } else {
                    let r = n.whistle_min_radius
                        + (timer / n.whistle_expand_time)
                            * (n.whistle_max_radius - n.whistle_min_radius);
                    c.whistle = WhistleState::Calling {
                        timer: 0.0,
                        radius: r,
                    };
                    call = Some(r);
                }
            }
            WhistleState::Calling { timer, radius } => {
                call = Some(radius);
                let t = timer + dt;
                if !input.whistle || t > n.whistle_hold_time {
                    c.whistle = WhistleState::Off;
                } else {
                    c.whistle = WhistleState::Calling { timer: t, radius };
                }
            }
        }

        if let Some(radius) = call {
            let centre = self.captains[ci].cursor_world;
            self.events.push(Event::Whistle {
                captain: self.captains[ci].id,
                centre,
                radius,
            });
            self.call_pikis(ci, centre, radius);
        }
    }

    /// `Navi::callPikis`, with our multi-captain rule: a whistle recruits every eligible Pikmin
    /// in range, including members of other captains' squads.
    fn call_pikis(&mut self, ci: usize, centre: Vec3, radius: f32) {
        let me = self.captains[ci].id;
        for pi in 0..self.pikis.len() {
            let p = &self.pikis[pi];
            if !p.is_alive() || dist_xz(p.pos, centre) >= radius {
                continue;
            }
            // `mIsWhistlePending`: a knocked-down Pikmin gets up first, then joins
            // (`PikiFlickState`).
            if matches!(p.state, PikiState::Knocked { .. }) {
                let alive = p.health > 0.0;
                if let PikiState::Knocked { whistled, .. } = &mut self.pikis[pi].state
                    && alive
                {
                    *whistled = Some(me);
                }
                continue;
            }
            let eligible = match p.state {
                PikiState::Idle | PikiState::Attack(_) | PikiState::Carry(_) => true,
                // Booked below instead.
                PikiState::Knocked { .. } => false,
                PikiState::Pressed { .. }
                | PikiState::Swallowed { .. }
                | PikiState::Dying { .. }
                | PikiState::Fading { .. } => false,
                PikiState::Squad { .. } => p.captain != Some(me),
                // In flight or in another captain's hand: can't be grabbed.
                PikiState::Flying { .. } | PikiState::GoHang { .. } | PikiState::Held { .. } => {
                    false
                }
                // `Navi::callPikis` (navi.cpp) skips drinking and blooming Pikmin.
                PikiState::Absorb { .. } | PikiState::GrowUp { .. } => false,
                // `Navi::callPikis` skips `EnterMode` / `ExitMode`.
                PikiState::Enter { .. } | PikiState::Exit { .. } => false,
                // The Onion's plucker answers to its Onion.
                PikiState::Plucker(_) => false,
                PikiState::Dead => false,
            };
            if !eligible {
                continue;
            }
            let stolen_from = p.captain.filter(|&c| c != me && p.state.in_squad());
            let id = p.id;
            self.leave_current(id);
            self.join_squad(id, me);
            self.events.push(Event::Recruited {
                piki: id,
                captain: me,
                stolen_from,
            });
        }
    }

    /// `NaviThrowWaitState` / `NaviThrowState`.
    fn captain_throw(
        &mut self,
        ci: usize,
        dt: f32,
        input: CaptainInput,
        pressed: crate::captain::Pressed,
    ) {
        let n = self.params.navi.clone();

        if pressed.throw
            && matches!(self.captains[ci].throw, ThrowState::Idle)
            && matches!(self.captains[ci].whistle, WhistleState::Off)
            && !self.start_throw_wait(ci)
        {
            // Nobody to throw: `NaviThrowWaitState::exec` goes to `NAVISTATE_Attack`.
            self.captains[ci].punch = Some(Punch {
                frame: 0.0,
                hit: false,
                whistle_queued: false,
            });
        }

        let ThrowState::Wait {
            mut held,
            mut pending,
            mut pending_timeout,
            mut hold_time,
        } = self.captains[ci].throw
        else {
            return;
        };

        if let Some(pid) = pending {
            let p = &self.pikis[pid.0 as usize];
            pending_timeout -= dt;
            if pending_timeout < 0.0 || !matches!(p.state, PikiState::GoHang { .. }) {
                self.cancel_throw(ci);
                return;
            }
            if (p.pos - self.captains[ci].pos).length() <= n.pluck_grab_range {
                self.pikis[pid.0 as usize].state = PikiState::Held {
                    captain: self.captains[ci].id,
                };
                held = Some(pid);
                pending = None;
            }
        }

        if let Some(hid) = held
            && !matches!(self.pikis[hid.0 as usize].state, PikiState::Held { .. })
        {
            self.cancel_throw(ci);
            return;
        }

        hold_time = (hold_time + dt).min(n.throw_hold_max_time);
        self.captains[ci].throw = ThrowState::Wait {
            held,
            pending,
            pending_timeout,
            hold_time,
        };

        if !input.throw {
            match held {
                Some(hid) => {
                    self.captains[ci].throw = ThrowState::Idle;
                    self.throw_piki(ci, hid, hold_time);
                }
                // Released before the Pikmin arrived: keep waiting for it (the original keeps
                // the wait state until the timeout or the Pikmin reaches the hand).
                None if pending.is_some() => {}
                None => self.captains[ci].throw = ThrowState::Idle,
            }
        }
    }

    /// The order the colour switch goes in.
    const CYCLE: [PikiColor; 3] = [PikiColor::Red, PikiColor::Yellow, PikiColor::Blue];

    /// The colours among `ci`'s squad (formed up, walking to his hand or held), as bits by
    /// [`PikiColor::index`].
    fn squad_colors(&self, ci: usize) -> u32 {
        let c = &self.captains[ci];
        let mut bits = 0;
        for &id in &c.plate.members {
            let p = &self.pikis[id.0 as usize];
            if matches!(
                p.state,
                PikiState::Squad { .. } | PikiState::GoHang { .. } | PikiState::Held { .. }
            ) {
                bits |= 1 << p.color.index();
            }
        }
        bits
    }

    /// Steps the colour pick on: Any (the nearest Pikmin, the default), then Red, Yellow and
    /// Blue in turn, skipping colours he has none of, then Any again. A Pikmin already in his
    /// hand goes back and, with the button still `held`, one of the new pick comes instead.
    fn cycle_throw_color(&mut self, ci: usize, held: bool) {
        let bits = self.squad_colors(ci);
        let now = self.captains[ci].throw_color;
        // Places 0 (Any), 1.. (the colours in `CYCLE`).
        let at = now.and_then(|c| Self::CYCLE.iter().position(|&x| x == c)).map_or(0, |k| k + 1);
        let next = (1..=Self::CYCLE.len() + 1)
            .map(|k| (at + k) % (Self::CYCLE.len() + 1))
            .find(|&k| k == 0 || bits & (1 << Self::CYCLE[k - 1].index()) != 0)
            .map_or(None, |k| (k > 0).then(|| Self::CYCLE[k - 1]));
        self.captains[ci].throw_color = next;
        if now != next && matches!(self.captains[ci].throw, ThrowState::Wait { .. }) {
            self.cancel_throw(ci);
            if held {
                self.start_throw_wait(ci);
            }
        }
    }

    /// `NaviThrowWaitState::init`: pick the nearest squad member within 80, preferring ones behind.
    /// With a colour picked (`throw_color`, and not locked on) only that colour; when he has none of it left,
    /// the nearest of any colour, which becomes the pick.
    fn start_throw_wait(&mut self, ci: usize) -> bool {
        // Host extension: locked onto an enemy, the nearest Pikmin goes whatever colour is picked,
        // and the pick is left as it was.
        let want = if self.captains[ci].lock_target.is_some() { None } else { self.captains[ci].throw_color };
        let grab = self.params.navi.pluck_grab_range;
        let c = &self.captains[ci];
        let fwd = dir_of(c.face);
        // The nearest of the picked colour, and the nearest of any.
        let mut best: Option<(PikiId, f32)> = None;
        let mut any: Option<(PikiId, f32)> = None;
        let mut have_want = false;
        for &id in &c.plate.members {
            let p = &self.pikis[id.0 as usize];
            if !matches!(p.state, PikiState::Squad { .. }) {
                continue;
            }
            have_want |= want.is_some_and(|w| p.color == w);
            let diff = p.pos - c.pos;
            let mut len = diff.length();
            if diff.dot(fwd) > -0.1 {
                len += 10.0;
            }
            if len >= 80.0 {
                continue;
            }
            if any.is_none_or(|(_, d)| len < d) {
                any = Some((id, len));
            }
            if want.is_none_or(|w| p.color == w) && best.is_none_or(|(_, d)| len < d) {
                best = Some((id, len));
            }
        }
        // Out of the picked colour altogether: the nearest of the rest, which becomes the pick.
        let Some((id, dist)) = (if have_want { best } else { best.or(any) }) else {
            return false;
        };
        if want.is_some() {
            self.captains[ci].throw_color = Some(self.pikis[id.0 as usize].color);
        }
        let c = &self.captains[ci];
        let captain = c.id;
        let (held, pending) = if dist <= grab {
            self.pikis[id.0 as usize].state = PikiState::Held { captain };
            (Some(id), None)
        } else {
            self.pikis[id.0 as usize].state = PikiState::GoHang { captain };
            (None, Some(id))
        };
        self.captains[ci].throw = ThrowState::Wait {
            held,
            pending,
            pending_timeout: 3.0,
            hold_time: 0.0,
        };
        true
    }

    /// `NaviAttackState`: walk on while the "PUNCH" animation plays. From its `KEY_Action0`
    /// to the end it hits every non-flying creature within reach (`getCentreSize` of both +
    /// `mPunchRadius`) and within 135° of facing, until something takes the hit. Returns
    /// true while punching.
    fn captain_punch(&mut self, ci: usize, host: &mut dyn Host, dt: f32, pressed: Pressed) -> bool {
        let Some(mut punch) = self.captains[ci].punch else {
            return false;
        };
        let anim = self.params.anim.punch;
        let reach_extra = self.params.navi.body_collision_radius + self.params.navi.punch_radius;
        let damage = self.params.navi.punch_damage;
        punch.whistle_queued |= pressed.whistle;
        punch.frame += self.params.anim.fps * dt * self.captains[ci].attack_rate;

        let (cpos, face, cid) = (
            self.captains[ci].pos,
            self.captains[ci].face,
            self.captains[ci].id,
        );
        if !punch.hit && punch.frame >= anim.action0 {
            let targets: Vec<_> = self
                .creatures
                .creatures
                .iter()
                .filter(|c| c.alive && !c.flying && c.parts_len > 0)
                .filter(|c| {
                    let diff = c.pos - cpos;
                    let size = self.creatures.bounds(c).1;
                    diff.length() <= size + reach_extra
                        && ang_dist(face, yaw_of(diff)).abs() < 2.356_194_5
                })
                .map(|c| c.id)
                .collect();
            for creature in targets {
                if host.damage(
                    creature,
                    damage,
                    None,
                    crate::host::DamageSource::Captain(cid),
                ) {
                    punch.hit = true;
                    self.events.push(Event::Punched {
                        captain: cid,
                        creature,
                        damage,
                    });
                }
            }
        }

        if punch.frame >= anim.end {
            self.captains[ci].punch = None;
            if punch.whistle_queued {
                self.captains[ci].whistle = WhistleState::Expanding { timer: 0.0 };
            }
        } else {
            self.captains[ci].punch = Some(punch);
        }
        true
    }

    fn cancel_throw(&mut self, ci: usize) {
        if let ThrowState::Wait { held, pending, .. } = self.captains[ci].throw {
            for id in [held, pending].into_iter().flatten() {
                let p = &mut self.pikis[id.0 as usize];
                if matches!(p.state, PikiState::Held { .. } | PikiState::GoHang { .. }) {
                    p.state = PikiState::squad();
                }
            }
        }
        self.captains[ci].throw = ThrowState::Idle;
    }

    /// `Navi::throwPiki`: a parabola that reaches the cursor after `throw_flight_time`.
    fn throw_piki(&mut self, ci: usize, id: PikiId, hold_time: f32) {
        let n = &self.params.navi;
        let gravity = self.params.ai.gravity;
        let c = &mut self.captains[ci];
        c.plate.remove(id);

        let p = &mut self.pikis[id.0 as usize];
        p.pos = c.pos + Vec3::new(0.0, 10.0, 0.0);
        let to = c.cursor_world - p.pos;
        let dist = to.x.hypot(to.z);
        let angle = to.x.atan2(to.z);
        p.face = round_ang(angle);

        let half_time = 0.5 * n.throw_flight_time;
        let height = if p.color == PikiColor::Yellow {
            n.yellow_throw_height
        } else {
            n.throw_min_height
                + (hold_time / n.throw_hold_max_time) * (n.throw_max_height - n.throw_min_height)
        };
        let v_speed = gravity * 0.5 * half_time + height / half_time;
        let h_speed = dist / (2.0 * half_time);
        p.vel = Vec3::new(h_speed * angle.sin(), v_speed, h_speed * angle.cos()) + c.vel;
        p.target_vel = p.vel;
        p.on_ground = false;
        p.state = PikiState::Flying { ground_frames: 0 };
        // A lock-on is followed; without one, a host that asks for it has the throw home on
        // the cursor point (which also makes up for the captain's own speed).
        let (creature, point) = (c.lock_target, self.cursor_homing.then_some(c.cursor_world));
        p.guided_throw = (creature.is_some() || point.is_some()).then(|| crate::piki::GuidedThrow {
            creature,
            point: point.or(creature.map(|_| c.cursor_world)),
            remaining: n.throw_flight_time.max(0.2),
        });
        p.captain = Some(c.id);
        self.events.push(Event::Thrown {
            piki: id,
            captain: c.id,
        });
    }

    /// Dismiss: everyone in the squad goes idle where they stand.
    pub(crate) fn dismiss(&mut self, ci: usize) {
        self.cancel_throw(ci);
        let members = std::mem::take(&mut self.captains[ci].plate.members);
        for id in members {
            let p = &mut self.pikis[id.0 as usize];
            p.state = PikiState::Idle;
            p.dismissed = true;
        }
        self.events.push(Event::Dismissed {
            captain: self.captains[ci].id,
        });
    }
}

/// Where a locked-on cursor floats: on the line from `eye` to the middle of the creature's
/// whole body (the box around all its parts, not the one part the lock aims at), just in
/// front of the first part that line enters. If it misses every part it hangs at the middle.
fn lock_marker(parts: &[crate::CreaturePart], eye: Vec3) -> Vec3 {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in parts {
        lo = lo.min(p.centre - Vec3::splat(p.radius));
        hi = hi.max(p.centre + Vec3::splat(p.radius));
    }
    let middle = (lo + hi) * 0.5;
    let to = middle - eye;
    let dist = to.length();
    let dir = to.normalize_or(Vec3::Z);
    let mut enter = dist;
    let mut hit = false;
    for p in parts {
        let oc = eye - p.centre;
        let b = oc.dot(dir);
        let disc = b * b - (oc.length_squared() - p.radius * p.radius);
        if disc < 0.0 {
            continue;
        }
        let (near, far) = (-b - disc.sqrt(), -b + disc.sqrt());
        if far < 0.0 {
            continue;
        }
        let t = near.max(0.0);
        if !hit || t < enter {
            enter = t;
            hit = true;
        }
    }
    let t = if hit { enter - LOCK_MARGIN } else { dist };
    eye + dir * t.max(LOCK_MIN_DISTANCE.min(dist))
}
