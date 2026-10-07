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

#[derive(Debug, Clone)]
pub struct Captain {
    pub id: CaptainId,
    pub pos: Vec3,
    pub vel: Vec3,
    pub target_vel: Vec3,
    pub face: f32,
    pub on_ground: bool,
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
    rearrange_pending: bool,
    pub(crate) input: CaptainInput,
    pub(crate) pressed: Pressed,
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
    pub whistle: bool,
    pub throw: bool,
    pub dismiss: bool,
}

impl Captain {
    pub(crate) fn new(id: CaptainId, pos: Vec3, health: f32) -> Self {
        Self {
            id,
            pos,
            vel: Vec3::ZERO,
            target_vel: Vec3::ZERO,
            face: 0.0,
            on_ground: false,
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
            rearrange_pending: false,
            input: CaptainInput::default(),
            pressed: Pressed::default(),
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
    pub(crate) fn update_captain(&mut self, ci: usize, host: &mut dyn Host, dt: f32) {
        let input = self.captains[ci].input;
        let pressed = std::mem::take(&mut self.captains[ci].pressed);

        self.update_captain_damage(ci, dt);
        if self.captains[ci].down {
            let c = &mut self.captains[ci];
            c.vel = Vec3::ZERO;
            c.target_vel = Vec3::ZERO;
            c.whistle = WhistleState::Off;
            c.pluck = None;
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
            && matches!(self.captains[ci].throw, ThrowState::Idle)
            && let Some(sprout) = self.find_pluckable(ci)
        {
            self.start_pluck(ci, sprout);
            self.captain_move(ci, host, dt, input, true);
            self.captain_formation(ci);
            return;
        }

        self.captain_move(ci, host, dt, input, false);
        self.captain_formation(ci);
        if self.captain_punch(ci, host, dt, pressed) {
            return;
        }
        self.captain_whistle(ci, dt, input, pressed);
        self.captain_throw(ci, dt, input, pressed);
        if pressed.dismiss {
            self.dismiss(ci);
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
        let gravity = self.params.ai.gravity;
        let c = &mut self.captains[ci];

        c.neutral_time += dt;
        if input.whistle || input.throw || input.dismiss {
            c.neutral_time = 0.0;
        }

        let stick = Vec3::new(
            input.stick.x.clamp(-1.0, 1.0),
            0.0,
            input.stick.y.clamp(-1.0, 1.0),
        );
        let stick_mag = stick.length();

        // canNaviRunFast() always returns true in the shipped game, so it's always mRunSpeed.
        c.target_vel = stick * navi.run_speed;

        // The cursor leads the captain in the stick direction, sliding along its max radius.
        let mut cursor_vel = stick.normalize_or_zero() * navi.cursor_move_speed;
        let mut next = c.cursor + cursor_vel * dt;
        if next.length() >= navi.cursor_max_radius {
            let radial = next.normalize_or_zero();
            cursor_vel -= radial * radial.dot(cursor_vel);
            next = c.cursor + cursor_vel * dt;
        }
        c.cursor = next;
        if let Some(aim) = input.cursor {
            let off = Vec3::new(aim.x - c.pos.x, 0.0, aim.z - c.pos.z);
            c.cursor = off.clamp_length_max(navi.cursor_max_radius);
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

        if locked {
            c.target_vel = Vec3::ZERO;
            if let Some(g) = host.ground_height(c.pos + Vec3::new(0.0, 20.0, 0.0)) {
                c.pos.y = g;
                c.on_ground = true;
            }
        } else if let Some(pose) = input.pose {
            c.pos = pose.pos;
            c.vel = pose.vel;
            c.face = pose.face;
            c.on_ground = true;
        } else {
            let mut on_ground = c.on_ground;
            integrate(
                host,
                &mut c.pos,
                &mut c.vel,
                c.target_vel,
                &mut on_ground,
                navi.body_collision_radius,
                accel,
                gravity,
                dt,
            );
            c.on_ground = on_ground;
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

    /// `NaviThrowWaitState::init`: pick the nearest squad member within 80, preferring ones behind.
    fn start_throw_wait(&mut self, ci: usize) -> bool {
        let grab = self.params.navi.pluck_grab_range;
        let c = &self.captains[ci];
        let fwd = dir_of(c.face);
        let mut best: Option<(PikiId, f32)> = None;
        let mut max_dist = 80.0;
        for &id in &c.plate.members {
            let p = &self.pikis[id.0 as usize];
            if !matches!(p.state, PikiState::Squad { .. }) {
                continue;
            }
            let diff = p.pos - c.pos;
            let mut len = diff.length();
            if diff.dot(fwd) > -0.1 {
                len += 10.0;
            }
            if len < max_dist {
                max_dist = len;
                best = Some((id, len));
            }
        }
        let Some((id, dist)) = best else {
            return false;
        };
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
        punch.frame += self.params.anim.fps * dt;

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
        }
        self.events.push(Event::Dismissed {
            captain: self.captains[ci].id,
        });
    }
}
