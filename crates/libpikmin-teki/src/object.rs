//! The enemy object (`BTeki`, tekibteki.cpp) and the world it acts in. Strategies (`tai.rs` and the
//! per-enemy modules) read and change a [`Teki`] through the same operations the decomp's
//! actions use: targeting conditions, turning, moving, flicking Pikmin off, timers, motions.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use glam::Vec3;
use libpikmin::host::{CreatureId, Host};
use libpikmin::{PelletId, PelletState, PikiId, PikiState, Sim};

use crate::anim::{self, Animator, key};
use crate::info::TekiInfo;
use libpikmin_assets::teki::{tpf, tpi};

pub type TekiId = u32;
/// `TAI_NO_TRANSIT`: the action changes no state.
pub const NO_TRANSIT: i32 = -1;
/// `TAI_RETURN_TRANSIT`: back to the state it came from.
pub const RETURN_TRANSIT: i32 = -2;

/// Something an enemy can go for or has hold of (`Creature*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Piki(PikiId),
    /// A host-provided player (`Navi`).
    Navi(u32),
    /// A pellet (what the Breadbug goes for).
    Pellet(PelletId),
}

/// A player the host asks enemies to treat as a Navi.
#[derive(Debug, Clone, Copy)]
pub struct Navi {
    pub id: u32,
    pub pos: Vec3,
    /// Body radius.
    pub size: f32,
    pub alive: bool,
}

/// `TekiOption`s.
pub mod opt {
    pub const VISIBLE: u32 = 1 << 0;
    pub const SHADOW_VISIBLE: u32 = 1 << 1;
    pub const ATARI: u32 = 1 << 3;
    pub const ALIVE: u32 = 1 << 4;
    pub const INVINCIBLE: u32 = 1 << 5;
    pub const DAMAGE_COUNTABLE: u32 = 1 << 6;
    pub const PRESSED: u32 = 1 << 7;
    pub const STOPPING_MOVE: u32 = 1 << 8;
    pub const GRAVITATABLE: u32 = 1 << 9;
    pub const STICKABLE: u32 = 1 << 10;
    pub const ALL_DEFAULT: u32 = VISIBLE | SHADOW_VISIBLE | ATARI | ALIVE | DAMAGE_COUNTABLE | GRAVITATABLE | STICKABLE;
}

/// What an enemy did that the host must carry out.
#[derive(Debug, Clone)]
pub enum Out {
    /// An `EffectMgr` effect at a place (`EFF_*`).
    Effect { id: usize, pos: Vec3 },
    /// A sound from the kind's sound table (`BTeki::playTableSound`).
    TableSound { teki: TekiId, index: u8 },
    /// A named sound effect (`SE_*`) at a place.
    Sound { name: &'static str, pos: Vec3 },
    /// A new enemy (`BTeki::spawnTeki`).
    Spawn { kind: u8, pos: Vec3, yaw: f32, parent: TekiId, velocity: Vec3 },
    /// Damage to a player (`InteractAttack` on a Navi): `knock` away from the point, if it knocks.
    HitNavi { teki: TekiId, navi: u32, damage: f32, from: Vec3, knock: f32 },
    /// A pellet dropped for the host to place (`BTeki::spawnPellets`): kind 0..2 numbered, color.
    SpawnPellets { pos: Vec3, number: u32, color: u32, count: u32 },
    /// Nectar (`spawnWaters`).
    SpawnNectar { pos: Vec3, count: u32 },
    /// The enemy is finished (`BTeki::die`); the host removes it.
    Died { teki: TekiId },
    /// The enemy began dying (`TaiStartDyingAction`): the host makes its corpse.
    Dying { teki: TekiId },
    /// `BTeki::sendMessage`: sleepers within `range` of `pos` wake up (`TekiEventType::WakeUpCall`).
    Message { from: TekiId, pos: Vec3, range: f32 },
    /// A shake of the camera/rumble (`cameraMgr->startVibrationEvent`).
    Shake { pos: Vec3, strength: f32 },
}

/// Events that reach a state (`TekiEvent`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Ground,
    Wall,
    /// Touched something: a Pikmin (`flying` when thrown) or player.
    Entity { other: Target, flying: bool },
    /// Squashed (`TekiEventType::Pressed`).
    Pressed,
    /// Called awake by another enemy (`TekiEventType::WakeUpCall`).
    WakeUp,
}

/// A small, fast random source (`NSystem::random`).
#[derive(Debug, Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    /// 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    /// `NMathF::occurred`: true with chance `p`.
    pub fn occurred(&mut self, p: f32) -> bool {
        self.f32() < p
    }
}

/// Everything outside the enemy that a strategy touches in one tick.
pub struct World<'a> {
    pub sim: &'a mut Sim,
    pub host: &'a dyn Host,
    pub navis: &'a [Navi],
    /// Other enemies' kind, state and place (`tekiMgr->findClosest` conditions).
    pub others: &'a [Other],
    pub out: &'a mut Vec<Out>,
    pub rng: &'a mut Rng,
    /// Seconds per tick (`NSystem::getFrameTime`).
    pub dt: f32,
}

/// What other enemies look like to one (`TekiStateCondition`, `TekiTypeCondition`).
#[derive(Debug, Clone, Copy)]
pub struct Other {
    pub id: TekiId,
    pub kind: u8,
    pub state: i32,
    pub pos: Vec3,
    pub alive: bool,
    /// The pellet it carries.
    pub holding: Option<PelletId>,
}

/// One candidate for targeting: a Pikmin or player as the conditions see it.
#[derive(Debug, Clone, Copy)]
pub struct Cand {
    pub target: Target,
    pub pos: Vec3,
    pub size: f32,
    /// Stuck on this enemy (`getStickObject() == this`).
    pub sticks_to_me: bool,
    /// Stuck on something (`getStickObject()` not null).
    pub sticking: bool,
    pub piki: Option<PikiId>,
}

/// An enemy.
#[derive(Debug, Clone)]
pub struct Teki {
    pub id: TekiId,
    pub kind: u8,
    pub info: Arc<TekiInfo>,
    /// The host's id for it, which the sim's creatures (parts, latched Pikmin) are keyed by.
    pub creature: CreatureId,
    pub pos: Vec3,
    pub yaw: f32,
    /// `mTargetVelocity`: where it is going, units a second (horizontal).
    pub target_vel: Vec3,
    /// Vertical speed while it is in the air (`mActionVelocity.y` / gravity).
    pub vel_y: f32,
    pub flying: bool,
    /// Its nest (`getNestPosition`): where it started.
    pub nest: Vec3,
    pub hp: f32,
    pub options: u32,
    /// The strategy's state, the one it came from, and whether the new one still has to start.
    pub state: i32,
    pub return_state: i32,
    pub state_ready: bool,
    pub timers: [f32; 8],
    /// `mCreaturePointers`.
    pub targets: [Option<Target>; 2],
    pub target_angle: f32,
    pub target_pos: Vec3,
    pub action_state: i32,
    pub queue: usize,
    pub damage_count: f32,
    pub stored_damage: f32,
    pub loop_count: i32,
    pub anim: Animator,
    pub anim_speed: f32,
    pub pre_stop_speed: f32,
    /// Scratch values the strategies keep between ticks (their own members).
    pub reg: [f32; 8],
    pub flags: [bool; 4],
    pub count: [i32; 4],
    /// Seconds of hit flash.
    pub flash: f32,
    /// A tick counter for the host's hit-reaction shake (`startDamageMotion`).
    pub shake: f32,
    pub age: f32,
    /// How far this enemy roams from its nest, set by whoever placed it (`FLT_TerritoryRange`).
    pub territory: f32,
    /// 0..1, how solid it is drawn (`FLT_Size` for the Honeywisp's fade).
    pub fade: f32,
    /// `PeveHorizontalSinWaveEvent`: a bobbing flight along a line.
    pub wave: Wave,
    /// `PeveAccelerationEvent`: velocity and acceleration for a flight.
    pub vel3: Vec3,
    pub accel3: Vec3,
    pub accel_on: bool,
    /// The position the events move (`mPositionIO`).
    pub wave_pos: Vec3,
    /// The pellet it has hold of (`getCreaturePointer(2)` for the Breadbug).
    pub held: Option<PelletId>,
    /// `getCreaturePointer(3)`: what it gave up on for a while (the Breadbug).
    pub targets_lost: Option<Target>,
    /// Where it is ambling to (`mRouteWayPoints` stand-in).
    pub wander: Vec3,
    /// The way it faced at its nest.
    pub nest_yaw: f32,
}

/// A bobbing flight (`PeveHorizontalSinWaveEvent`): the position advances by `dir` a second while its
/// height follows `amp * sin(theta) + offset`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wave {
    pub on: bool,
    pub dir: Vec3,
    pub offset: f32,
    pub amp: f32,
    pub theta: f32,
    pub omega: f32,
}

fn round_angle(a: f32) -> f32 {
    let mut a = a % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a
}

/// `NMathF::angleDifference`: 0..PI between two directions.
pub fn angle_difference(a: f32, b: f32) -> f32 {
    let d = (round_angle(a) - round_angle(b)).abs();
    if d > PI { TAU - d } else { d }
}

/// `NMathF::calcNearerDirection`: `target` shifted by a turn so it is within PI of `from`.
pub fn nearer_direction(from: f32, target: f32) -> f32 {
    let (f, mut t) = (round_angle(from), round_angle(target));
    if t - f > PI {
        t -= TAU;
    } else if f - t > PI {
        t += TAU;
    }
    t
}

pub fn dir_vec(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

pub fn dist_xz(a: Vec3, b: Vec3) -> f32 {
    (a - b).with_y(0.0).length()
}

impl Teki {
    pub fn new(id: TekiId, info: Arc<TekiInfo>, creature: CreatureId, pos: Vec3, yaw: f32) -> Self {
        let hp = info.life;
        Teki {
            id,
            kind: info.kind,
            info,
            creature,
            pos,
            yaw,
            target_vel: Vec3::ZERO,
            vel_y: 0.0,
            flying: false,
            nest: pos,
            hp,
            options: opt::ALL_DEFAULT,
            state: 0,
            return_state: 0,
            state_ready: true,
            timers: [0.0; 8],
            targets: [None; 2],
            target_angle: yaw,
            target_pos: pos,
            action_state: 0,
            queue: 0,
            damage_count: 0.0,
            stored_damage: 0.0,
            loop_count: 0,
            anim: Animator::new(),
            anim_speed: 30.0,
            pre_stop_speed: 30.0,
            reg: [0.0; 8],
            flags: [false; 4],
            count: [0; 4],
            flash: 0.0,
            shake: 0.0,
            age: 0.0,
            territory: 0.0,
            fade: 1.0,
            wave: Wave::default(),
            vel3: Vec3::ZERO,
            accel3: Vec3::ZERO,
            accel_on: false,
            wave_pos: pos,
            held: None,
            targets_lost: None,
            wander: pos,
            nest_yaw: yaw,
        }
    }

    // ---- parameters

    pub fn f(&self, i: usize) -> f32 {
        self.info.floats.get(i).copied().unwrap_or(0.0)
    }
    pub fn i(&self, i: usize) -> i32 {
        self.info.ints.get(i).copied().unwrap_or(0)
    }
    pub fn param_f(&self, i: usize) -> f32 {
        self.f(i)
    }

    // ---- options

    pub fn set_option(&mut self, o: u32) {
        self.options |= o;
    }
    pub fn clear_option(&mut self, o: u32) {
        self.options &= !o;
    }
    pub fn option(&self, o: u32) -> bool {
        self.options & o != 0
    }
    pub fn alive(&self) -> bool {
        self.option(opt::ALIVE)
    }

    // ---- motion

    /// `BTeki::startMotion`.
    pub fn start_motion(&mut self, motion: usize) {
        self.anim.start(motion, &self.info.motion_data);
    }
    /// `animationFinished`.
    pub fn animation_finished(&self) -> bool {
        self.anim.is_finishing()
    }
    pub fn key(&self, bit: u8) -> bool {
        self.anim.has(bit)
    }
    pub fn finish_motion(&mut self) {
        self.anim.finish(&self.info.motion_data);
    }
    pub fn current_motion(&self) -> i32 {
        self.anim.motion
    }
    pub fn key_value(&self, ty: u8) -> f32 {
        self.anim.key_value(ty, &self.info.motion_data)
    }

    // ---- movement

    pub fn stop_move(&mut self) {
        self.target_vel = Vec3::ZERO;
    }
    pub fn start_stopping_move(&mut self) {
        self.stop_move();
        self.set_option(opt::STOPPING_MOVE);
        self.pre_stop_speed = self.anim_speed;
    }
    pub fn finish_stopping_move(&mut self) {
        self.clear_option(opt::STOPPING_MOVE);
    }
    pub fn start_flying(&mut self) {
        self.flying = true;
    }
    pub fn finish_flying(&mut self) {
        self.flying = false;
    }

    /// `BTeki::moveToward`: head for `target` at `speed`; true once there (within two frames' travel).
    pub fn move_toward(&mut self, target: Vec3, speed: f32, dt: f32) -> bool {
        let d = (target - self.pos).with_y(0.0);
        let dist = d.length();
        self.target_vel = if dist > 1e-4 { d / dist * speed } else { Vec3::ZERO };
        dist <= 2.0 * speed * dt
    }

    pub fn direction(&self) -> f32 {
        round_angle(self.yaw)
    }
    pub fn forward(&self) -> Vec3 {
        dir_vec(self.yaw)
    }
    pub fn calc_target_direction(&self, p: Vec3) -> f32 {
        let d = p - self.pos;
        d.x.atan2(d.z)
    }
    /// `BTeki::calcTargetAngle`: how far off its facing a point is, 0..PI.
    pub fn calc_target_angle(&self, p: Vec3) -> f32 {
        angle_difference(self.direction(), self.calc_target_direction(p))
    }
    pub fn turn_to(&mut self, p: Vec3) {
        self.yaw = self.calc_target_direction(p);
    }
    /// `BTeki::turnToward`: turn toward an angle at `speed` radians a second; true on arrival.
    pub fn turn_toward(&mut self, angle: f32, speed: f32, dt: f32) -> bool {
        let face = self.direction();
        let near = nearer_direction(face, angle);
        let step = speed * dt;
        if near > face {
            if near - face < step {
                self.yaw = face + (near - face);
                return true;
            }
            self.yaw = face + step;
        } else {
            if face - near < step {
                self.yaw = face - (face - near);
                return true;
            }
            self.yaw = face - step;
        }
        false
    }
    pub fn rotate(&mut self, speed: f32, dt: f32) {
        self.yaw = round_angle(self.yaw) + speed * dt;
    }

    /// The way it faced when it was put at its nest.
    pub fn forward_at_nest(&self) -> Vec3 {
        dir_vec(self.nest_yaw)
    }

    pub fn territory_distance(&self) -> f32 {
        dist_xz(self.pos, self.nest)
    }

    // ---- timers

    /// `BTeki::timerElapsed`.
    pub fn timer_elapsed(&self, i: usize) -> bool {
        self.timers[i] <= 0.0
    }
    pub fn update_timers(&mut self, dt: f32) {
        for t in &mut self.timers {
            if *t > 0.0 {
                *t = (*t - dt).max(0.0);
            }
        }
    }

    // ---- sensing

    /// The sphere distance to a candidate along the ground (`calcCollisionDistance`).
    pub fn collision_distance(&self, c: &Cand) -> f32 {
        dist_xz(self.pos, c.pos) - c.size - self.info.collision_radius
    }

    /// `BTeki::visibleCreature`.
    pub fn visible(&self, c: &Cand) -> bool {
        let range = self.f(tpf::VISIBLE_RANGE);
        if c.pos.y > self.pos.y + self.f(tpf::VISIBLE_HEIGHT) {
            return false;
        }
        if dist_xz(self.pos, c.pos) > range {
            return false;
        }
        // While warning (timer 4), anything within range is seen.
        if !self.timer_elapsed(4) {
            return true;
        }
        self.calc_target_angle(c.pos) <= (self.f(tpf::VISIBLE_ANGLE) / 2.0).to_radians()
    }

    /// `BTeki::contactCreature`.
    pub fn contact(&self, c: &Cand) -> bool {
        if c.sticks_to_me {
            return true;
        }
        self.collision_distance(c) <= self.f(tpf::ATTACKABLE_RANGE) - 1.0
    }

    /// `BTeki::separateCreature`.
    pub fn separate(&self, c: &Cand) -> bool {
        if c.sticks_to_me {
            return false;
        }
        self.collision_distance(c) > self.f(tpf::ATTACKABLE_RANGE) + 1.0
    }

    /// `BTeki::attackableCreature`.
    pub fn attackable(&self, c: &Cand) -> bool {
        if c.sticks_to_me || !self.contact(c) {
            return false;
        }
        self.calc_target_angle(c.pos) <= self.f(tpf::ATTACKABLE_ANGLE).to_radians() / 2.0
    }

    /// Whether a Pikmin or player is the kind of thing the enemy sees (`TekiRecognitionCondition`).
    pub fn candidates(&self, w: &World) -> Vec<Cand> {
        let mut v = Vec::new();
        for n in w.navis.iter().filter(|n| n.alive) {
            v.push(Cand { target: Target::Navi(n.id), pos: n.pos, size: n.size, sticks_to_me: false, sticking: false, piki: None });
        }
        for p in w.sim.pikis() {
            if !p.is_alive() || matches!(p.state, PikiState::Swallowed { .. } | PikiState::Held { .. } | PikiState::Pressed { .. }) {
                continue;
            }
            let latched = p.state.latched_to();
            v.push(Cand {
                target: Target::Piki(p.id),
                pos: p.pos,
                size: p.size,
                sticks_to_me: latched == Some(self.creature),
                sticking: latched.is_some(),
                piki: Some(p.id),
            });
        }
        v
    }

    /// The candidate a target pointer names, if it is still fair game.
    pub fn cand_of(&self, t: Target, w: &World) -> Option<Cand> {
        match t {
            Target::Navi(id) => w.navis.iter().find(|n| n.id == id && n.alive).map(|n| Cand {
                target: t,
                pos: n.pos,
                size: n.size,
                sticks_to_me: false,
                sticking: false,
                piki: None,
            }),
            Target::Pellet(id) => {
                let p = w.sim.pellets().get(id.0 as usize)?;
                if p.state == PelletState::Gone {
                    return None;
                }
                Some(Cand { target: t, pos: p.pos, size: p.desc.radius, sticks_to_me: self.held == Some(id), sticking: false, piki: None })
            }
            Target::Piki(id) => {
                let p = w.sim.pikis().get(id.0 as usize)?;
                if !p.is_alive() || matches!(p.state, PikiState::Swallowed { .. } | PikiState::Held { .. } | PikiState::Pressed { .. }) {
                    return None;
                }
                let latched = p.state.latched_to();
                Some(Cand { target: t, pos: p.pos, size: p.size, sticks_to_me: latched == Some(self.creature), sticking: latched.is_some(), piki: Some(id) })
            }
        }
    }

    /// `getCreaturePointer(i)` resolved.
    pub fn target(&self, i: usize, w: &World) -> Option<Cand> {
        self.targets[i].and_then(|t| self.cand_of(t, w))
    }

    /// `BTeki::getClosestNaviPiki`: the nearest candidate satisfying `cond`.
    pub fn closest(&self, w: &World, cond: impl Fn(&Cand) -> bool) -> Option<Cand> {
        let mut best: Option<(f32, Cand)> = None;
        for c in self.candidates(w) {
            if !cond(&c) {
                continue;
            }
            let d = self.collision_distance(&c);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, c));
            }
        }
        best.map(|(_, c)| c)
    }

    /// `BTeki::nearestAngleTarget`: whether `new` should replace the current target (in contact and
    /// nearer its facing).
    pub fn nearest_angle_target(&self, new: Option<Cand>, w: &World) -> bool {
        let Some(new) = new else { return false };
        let Some(old) = self.target(0, w) else { return true };
        if new.target == old.target {
            return false;
        }
        let (n, o) = (self.contact(&new), self.contact(&old));
        if n {
            if o {
                return self.calc_target_angle(new.pos) < self.calc_target_angle(old.pos);
            }
            return true;
        }
        false
    }

    /// Pikmin close enough to be shaken off (`TekiLowerRangeCondition`), for the flick damage count.
    pub fn count_pikis_in_range(&self, w: &World, range: f32) -> i32 {
        w.sim
            .pikis()
            .iter()
            .filter(|p| p.is_alive() && dist_xz(self.pos, p.pos) - p.size - self.info.collision_radius <= range)
            .count() as i32
    }

    /// `getFlickDamageCount`.
    pub fn flick_damage_count(&self, pikis: i32) -> i32 {
        if pikis <= self.i(tpi::FLICK_PIKI_COUNT1) {
            self.i(tpi::FLICK_DAMAGE_COUNT1)
        } else if pikis <= self.i(tpi::FLICK_PIKI_COUNT2) {
            self.i(tpi::FLICK_DAMAGE_COUNT2)
        } else if pikis <= self.i(tpi::FLICK_PIKI_COUNT3) {
            self.i(tpi::FLICK_DAMAGE_COUNT3)
        } else {
            self.i(tpi::FLICK_DAMAGE_COUNT4)
        }
    }

    // ---- acting on the world

    /// `BTeki::flickUpper`: shake off the Pikmin stuck on it.
    pub fn flick_upper(&mut self, w: &mut World) {
        if w.rng.occurred(self.f(tpf::FLICK_PROBABILITY).max(0.0)) {
            w.sim.shake_off(self.creature);
        }
    }

    /// `BTeki::flickLower`: knock away what is close underneath it.
    pub fn flick_lower(&mut self, w: &mut World) {
        let range = self.f(tpf::LOWER_RANGE) + self.info.collision_radius;
        w.sim.flick_pikis_in_sphere(self.pos, range, self.f(tpf::LOWER_ATTACK_POWER), self.pos);
        for n in w.navis.iter().filter(|n| n.alive && dist_xz(self.pos, n.pos) - n.size <= range) {
            w.out.push(Out::HitNavi { teki: self.id, navi: n.id, damage: self.f(tpf::LOWER_ATTACK_POWER), from: self.pos, knock: self.f(tpf::LOWER_FLICK_POWER) });
        }
    }

    pub fn flick(&mut self, w: &mut World) {
        self.flick_upper(w);
        self.flick_lower(w);
    }

    /// `BTeki::outputHitCenter`: where its attack lands, ahead of it.
    pub fn hit_center(&self) -> Vec3 {
        self.pos + self.forward() * (self.info.collision_radius + self.f(tpf::ATTACK_RANGE))
    }

    /// `BTeki::createDeadEffect`.
    pub fn create_dead_effect(&self, w: &mut World) {
        let at = self.pos + Vec3::Y * ((self.info.hi.y - self.info.lo.y) * 0.5);
        for id in crate::effects::death(self.i(tpi::CARCASS_SIZE)) {
            w.out.push(Out::Effect { id, pos: at });
        }
    }

    /// `BTeki::spawnTeki`.
    pub fn spawn_teki(&self, kind: u8, w: &mut World) {
        let pos = self.pos + self.forward() * (self.info.collision_radius + 10.0);
        w.out.push(Out::Spawn { kind, pos, yaw: self.yaw, parent: self.id, velocity: Vec3::ZERO });
    }

    /// `BTeki::die`.
    pub fn die(&mut self, w: &mut World) {
        self.clear_option(opt::ALIVE);
        w.out.push(Out::Died { teki: self.id });
    }

    /// A hit (`TekiInteractType::Attack`): the damage waits in `stored_damage` until a reaction
    /// action applies it (`makeDamaged`), and counts toward shaking Pikmin off.
    pub fn take_damage(&mut self, amount: f32) {
        if self.option(opt::INVINCIBLE) || !self.alive() {
            return;
        }
        if self.option(opt::DAMAGE_COUNTABLE) {
            self.damage_count += 1.0;
        }
        self.stored_damage += amount;
        self.flash = 0.08;
    }

    /// `BTeki::makeDamaged`.
    pub fn make_damaged(&mut self) {
        self.hp = (self.hp - self.stored_damage).max(0.0);
        self.stored_damage = 0.0;
    }

    /// `BTeki::startDamageMotion`: the jiggle after a hit.
    pub fn start_damage_motion(&mut self, period: f32) {
        if self.timer_elapsed(3) {
            self.timers[3] = period;
            self.shake = period;
        }
    }

    pub fn anim_events(&mut self, w: &mut World, ev: Vec<anim::Event>) {
        for e in ev {
            if let anim::Event::Sound(i) = e {
                w.out.push(Out::TableSound { teki: self.id, index: i });
            }
        }
    }
}
