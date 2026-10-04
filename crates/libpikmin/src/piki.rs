//! Pikmin (decomp `Piki`) and their behaviour.

use std::f32::consts::PI;

use glam::Vec3;

use crate::event::Event;
use crate::host::{CreatureFrame, CreatureId, CreatureView};
use crate::math::{ang_dist, dist_xz, rotate_y, round_ang, yaw_of};
use crate::sim::{Sim, integrate};
use crate::{CaptainId, Host, PikiId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PikiColor {
    Blue,
    Red,
    Yellow,
}

impl PikiColor {
    pub const ALL: [PikiColor; 3] = [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow];

    /// The decomp's colour index (Blue 0, Red 1, Yellow 2).
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL[i % 3]
    }
}

/// Leaf / bud / flower stage (`mHappa`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Happa {
    #[default]
    Leaf,
    Bud,
    Flower,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttackMode {
    /// `ActJumpAttack` state 0: run at the nearest part.
    Approach,
    /// State 2: line up for a jump.
    JumpPrep,
    /// State 1: in the air after jumping at the target.
    Jumping,
    /// State 4: standing swing at a part within reach. `frame` is the "ATTACK" animation
    /// counter; `hit` is set once this swing has landed.
    Swing { frame: f32, hit: bool },
    /// State 5: stuck on a part, hitting once per "KUTTUKU" animation loop. `offset` is in the
    /// creature's yaw frame, relative to the part centre.
    Latched {
        part: u32,
        offset: Vec3,
        frame: f32,
        hit: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attack {
    pub target: CreatureId,
    pub mode: AttackMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PikiState {
    /// Free and standing around (`PikiMode::FreeMode`).
    Idle,
    /// Following a captain's formation (`ActCrowd`).
    Squad {
        lost_timer: f32,
        near_counter: u8,
        reslotted: bool,
    },
    /// Walking to the captain's hand to be thrown (`PIKISTATE_GoHang`).
    GoHang {
        captain: CaptainId,
    },
    /// In the captain's hand (`PIKISTATE_Hanged`).
    Held {
        captain: CaptainId,
    },
    /// Thrown (`PikiFlyingState`).
    Flying {
        ground_frames: u8,
    },
    Attack(Attack),
    /// Shaken off and lying on the ground (`PikiFlickState`).
    Knocked {
        timer: f32,
    },
    /// Carrying a pellet or corpse (`ActTransport`).
    Carry(crate::carry::Carry),
    /// Flattened (`PikiPressedState`): invincible for `stun`, gets up when `death_timer` runs out.
    Pressed {
        stun: f32,
        death_timer: f32,
    },
    /// In an enemy's mouth (`PikiSwallowedState`), riding its part `part`.
    Swallowed {
        creature: CreatureId,
        part: u32,
        offset: Vec3,
    },
    /// Playing a death animation (`PikiDyingState`).
    Dying {
        frame: f32,
        end: f32,
    },
    /// Shrinking out (`PikiDeadState`).
    Fading {
        timer: f32,
    },
    /// Gone.
    Dead,
}

impl PikiState {
    pub fn squad() -> Self {
        PikiState::Squad {
            lost_timer: 0.0,
            near_counter: 0,
            reslotted: false,
        }
    }

    /// Counts as a member of a captain's formation.
    pub fn in_squad(&self) -> bool {
        matches!(
            self,
            PikiState::Squad { .. } | PikiState::GoHang { .. } | PikiState::Held { .. }
        )
    }

    pub fn latched_to(&self) -> Option<CreatureId> {
        match self {
            PikiState::Attack(Attack {
                target,
                mode: AttackMode::Latched { .. },
            }) => Some(*target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Piki {
    pub id: PikiId,
    pub color: PikiColor,
    pub happa: Happa,
    pub pos: Vec3,
    pub vel: Vec3,
    pub target_vel: Vec3,
    pub face: f32,
    /// Body radius, random per Pikmin between `min_piki_size` and `max_piki_size`.
    pub size: f32,
    pub on_ground: bool,
    /// Captain this Pikmin follows or last followed (`mNavi`).
    pub captain: Option<CaptainId>,
    pub state: PikiState,
    /// `mHealth`, starts at `mPikiMaxHealth`.
    pub health: f32,
}

impl Piki {
    /// Not dying or dead.
    pub fn is_alive(&self) -> bool {
        !matches!(
            self.state,
            PikiState::Dying { .. } | PikiState::Fading { .. } | PikiState::Dead
        )
    }

    /// Still counts against the field limit (anything but fully gone).
    pub fn on_field(&self) -> bool {
        !matches!(self.state, PikiState::Dead)
    }
}

/// Nearest stickable part to `from`, else nearest part. Stands in for
/// `getNearestCollPart(pos, '*t**')`.
fn nearest_part(frame: &CreatureFrame, c: &CreatureView, from: Vec3) -> Option<(u32, Vec3, f32)> {
    let parts = frame.parts_of(c);
    let pick = |want_stickable: bool| {
        parts
            .iter()
            .enumerate()
            .filter(|(_, p)| !want_stickable || p.stickable)
            .min_by(|a, b| {
                let da = (a.1.centre - from).length() - a.1.radius;
                let db = (b.1.centre - from).length() - b.1.radius;
                da.total_cmp(&db)
            })
            .map(|(i, p)| (i as u32, p.centre, p.radius))
    };
    pick(true).or_else(|| pick(false))
}

impl Sim {
    pub(crate) fn update_piki(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        match self.pikis[pi].state {
            PikiState::Dead => {}
            PikiState::Carry(c) => self.piki_carry(pi, c, host, dt),
            PikiState::Pressed { .. }
            | PikiState::Swallowed { .. }
            | PikiState::Dying { .. }
            | PikiState::Fading { .. } => self.update_dying(pi, host, dt),
            PikiState::Idle => self.piki_idle(pi, host, dt),
            PikiState::Squad { .. } => self.piki_squad(pi, host, dt),
            PikiState::GoHang { captain } => self.piki_go_hang(pi, captain, host, dt),
            PikiState::Held { captain } => {
                let hand = self.captains[captain.0 as usize].hand_pos();
                let face = self.captains[captain.0 as usize].face;
                let p = &mut self.pikis[pi];
                p.pos = hand;
                p.vel = Vec3::ZERO;
                p.face = face;
            }
            PikiState::Flying { .. } => self.piki_flying(pi, host, dt),
            PikiState::Attack(a) => self.piki_attack(pi, a, host, dt),
            PikiState::Knocked { timer } => {
                let t = timer - dt;
                self.physics(pi, host, dt);
                let p = &mut self.pikis[pi];
                p.target_vel = Vec3::ZERO;
                if t <= 0.0 && p.on_ground {
                    if p.health <= 0.0 {
                        self.start_dying(pi, crate::damage::DeathCause::Attacked);
                    } else {
                        p.state = PikiState::Idle;
                    }
                } else {
                    p.state = PikiState::Knocked { timer: t };
                }
            }
        }
    }

    pub(crate) fn physics(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        let accel = self.params.piki_creature.acceleration;
        let gravity = self.params.ai.gravity;
        let face_adjust = self.params.piki_creature.face_dir_adjust;
        let p = &mut self.pikis[pi];
        let mut on_ground = p.on_ground;
        integrate(
            host,
            &mut p.pos,
            &mut p.vel,
            p.target_vel,
            &mut on_ground,
            accel,
            gravity,
            dt,
        );
        p.on_ground = on_ground;
        if p.target_vel.x.hypot(p.target_vel.z) > 1.0 {
            p.face = round_ang(p.face + face_adjust * ang_dist(yaw_of(p.target_vel), p.face));
        }
    }

    /// `Piki::getSpeed`: lerp from walk to this Pikmin's max run speed.
    pub(crate) fn piki_speed(&self, pi: usize, ratio: f32) -> f32 {
        let pp = &self.params.piki;
        let scale = self.params.ai.do_scale_happa_move_speed != 0;
        let max = match self.pikis[pi].happa {
            Happa::Flower if scale => pp.max_flower_move_speed,
            Happa::Bud if scale => pp.max_bud_move_speed,
            _ => pp.max_leaf_move_speed,
        };
        (max - pp.min_move_speed) * ratio + pp.min_move_speed
    }

    /// `Piki::getAttackPower`
    pub(crate) fn attack_power(&self, pi: usize) -> f32 {
        let pp = &self.params.piki;
        match self.pikis[pi].color {
            PikiColor::Blue => pp.blue_attack_power,
            PikiColor::Red => pp.red_attack_power,
            PikiColor::Yellow => pp.yellow_attack_power,
        }
    }

    fn piki_idle(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        self.pikis[pi].target_vel = Vec3::ZERO;
        // "ピキ：オートアタック". Off on the retail disc; on with code defaults.
        if self.params.ai.auto_attack != 0 && (self.tick + pi as u64).is_multiple_of(10) {
            self.act_on_situation(pi);
        }
        self.physics(pi, host, dt);
    }

    /// `ActCrowd::exec`, minus the idle-boredom and huddle-when-still branches.
    fn piki_squad(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        let Some(cid) = self.pikis[pi].captain else {
            self.pikis[pi].state = PikiState::Idle;
            return;
        };
        let id = self.pikis[pi].id;
        let c = &self.captains[cid.0 as usize];
        let Some(slot) = c.plate.slot_of(id) else {
            self.pikis[pi].state = PikiState::Idle;
            return;
        };
        let slot_pos = c.plate.slot_world(slot);
        let (cpos, plate_offset) = (c.pos, c.plate.offset);
        let accel = self.params.piki_creature.acceleration;
        let full = self.piki_speed(pi, 1.0);
        let half = self.piki_speed(pi, 0.5);
        let pp = self.params.piki.clone();

        let p = &mut self.pikis[pi];
        let PikiState::Squad {
            mut lost_timer,
            mut near_counter,
            mut reslotted,
        } = p.state
        else {
            unreachable!()
        };
        let to = slot_pos - p.pos;
        let dist = to.x.hypot(to.z);
        let dir = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();

        if dist <= 7.0 {
            near_counter = 0;
        } else if dist < 15.0 {
            near_counter = (near_counter + 1).min(6);
        } else {
            near_counter = 0;
        }

        let face_captain = |p: &mut Piki| {
            let d = cpos - p.pos;
            p.face = round_ang(p.face + 0.3 * ang_dist(yaw_of(d), p.face));
        };

        let mut sidestep = false;
        if dist <= 7.0 || (near_counter < 6 && dist <= 15.0) {
            p.target_vel = Vec3::ZERO;
            face_captain(p);
        } else if dist < 15.0 {
            // Arrival: brake so we don't overshoot the slot.
            let factor = 10.0 / accel;
            let stop_full = 0.5 * (full / factor) * full;
            let speed_now = p.vel.length();
            let stop_now = 0.5 * (speed_now / factor) * speed_now;
            if dist < stop_now {
                p.target_vel = Vec3::ZERO;
                face_captain(p);
            } else if dist < stop_full {
                let s = speed_now + 0.5 * (speed_now * speed_now + 8.0 * factor * dist).sqrt();
                p.target_vel = dir * s;
            } else {
                p.target_vel = dir * full;
            }
            sidestep = true;
        } else {
            p.target_vel = dir * full;
            sidestep = true;
        }

        // Pikmin in front of the captain arc around him instead of walking through.
        if sidestep {
            let from_captain = p.pos - cpos;
            let plate_to_captain = (cpos - plate_offset).normalize_or_zero();
            if plate_to_captain.dot(from_captain) > 0.0 {
                let mut side = Vec3::new(-from_captain.z, 0.0, from_captain.x).normalize_or_zero();
                if slot % 2 == 0 {
                    side = -side;
                }
                let speed = p.target_vel.length();
                p.target_vel = (p.target_vel + side * half).normalize_or_zero() * speed;
            }
        }

        // Slip / break range (`mFormationSlipRange`, `mFormationBreakRange`, `mMaxLostChildTime`).
        let mut leave = false;
        if dist < pp.formation_slip_range {
            lost_timer = 0.0;
            reslotted = false;
        } else if dist < pp.formation_break_range {
            lost_timer += dt;
            if !reslotted {
                reslotted = true;
                let plate = &mut self.captains[cid.0 as usize].plate;
                plate.remove(id);
                plate.add(id);
            }
            leave = lost_timer > pp.max_lost_child_time;
        } else {
            leave = true;
        }

        let p = &mut self.pikis[pi];
        p.state = PikiState::Squad {
            lost_timer,
            near_counter,
            reslotted,
        };
        if leave {
            self.leave_current(id);
            self.pikis[pi].state = PikiState::Idle;
        }
        self.physics(pi, host, dt);
    }

    fn piki_go_hang(&mut self, pi: usize, captain: CaptainId, host: &mut dyn Host, dt: f32) {
        let cpos = self.captains[captain.0 as usize].pos;
        let full = self.piki_speed(pi, 1.0);
        let p = &mut self.pikis[pi];
        let to = cpos - p.pos;
        p.target_vel = Vec3::new(to.x, 0.0, to.z).normalize_or_zero() * full;
        self.physics(pi, host, dt);
    }

    /// `PikiFlyingState`: ballistic until we touch a creature part or the ground.
    fn piki_flying(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        let p = &mut self.pikis[pi];
        p.target_vel = p.vel;
        self.physics(pi, host, dt);

        if let Some((target, part, surface)) = self.touching_part(pi, None) {
            if self.creatures.parts[part as usize].stickable {
                self.latch(pi, target, part, surface);
            } else {
                // Bounced off something we can't hold: drop straight down.
                let p = &mut self.pikis[pi];
                p.vel.x *= -0.3;
                p.vel.z *= -0.3;
            }
            return;
        }

        // `procCollideMsg` with a pellet that has a free slot: start carrying it.
        let (pos, size) = (self.pikis[pi].pos, self.pikis[pi].size);
        if let Some(pel) = self
            .pellets
            .iter()
            .find(|pl| pl.can_take_carrier() && pl.touches(pos, size))
            .map(|pl| pl.id)
            && self.start_carry(pi, pel)
        {
            return;
        }

        let p = &mut self.pikis[pi];
        if p.on_ground {
            // `procBounceMsg` → `Piki::actOnSituaton`.
            p.state = PikiState::Idle;
            p.target_vel = Vec3::ZERO;
            self.events.push(Event::Landed { piki: p.id });
            self.act_on_situation(pi);
        }
    }

    /// First creature part this Pikmin overlaps. Returns (creature, global part index, surface point).
    fn touching_part(
        &self,
        pi: usize,
        only: Option<CreatureId>,
    ) -> Option<(CreatureId, u32, Vec3)> {
        let p = &self.pikis[pi];
        for c in &self.creatures.creatures {
            if !c.alive || only.is_some_and(|id| id != c.id) {
                continue;
            }
            for gi in c.parts() {
                let part = &self.creatures.parts[gi as usize];
                let d = p.pos - part.centre;
                if d.length() < part.radius + p.size {
                    let n = d.normalize_or(Vec3::Y);
                    return Some((c.id, gi, part.centre + n * part.radius));
                }
            }
        }
        None
    }

    /// `Piki::startStickObject` + entering `ActJumpAttack` state 5.
    fn latch(&mut self, pi: usize, target: CreatureId, global_part: u32, surface: Vec3) {
        let Some(c) = self.creatures.get(target) else {
            return;
        };
        let local_part = global_part - c.parts_start;
        let part = self.creatures.parts[global_part as usize];
        let offset = rotate_y(surface - part.centre, -c.yaw);
        let p = &mut self.pikis[pi];
        p.pos = surface;
        p.vel = Vec3::ZERO;
        p.target_vel = Vec3::ZERO;
        p.on_ground = false;
        p.state = PikiState::Attack(Attack {
            target,
            mode: AttackMode::Latched {
                part: local_part,
                offset,
                frame: 0.0,
                hit: false,
            },
        });
        self.events.push(Event::Latched {
            piki: p.id,
            creature: target,
        });
    }

    /// `Piki::actOnSituaton` / `graspSituation`: the nearest of a creature within
    /// `idle_attack_search_range` of its edge, or a pellet with a free slot within
    /// `idle_work_search_range`.
    pub(crate) fn act_on_situation(&mut self, pi: usize) {
        let range = self.params.piki.idle_attack_search_range;
        let pos = self.pikis[pi].pos;
        let mut best: Option<(CreatureId, f32)> = None;
        for c in &self.creatures.creatures {
            if !c.alive || c.flying || c.parts_len == 0 {
                continue;
            }
            let (centre, size) = self.creatures.bounds(c);
            let d = dist_xz(pos, centre);
            if d <= range + size && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((c.id, d));
            }
        }
        let pellet = self.nearest_free_pellet(pos, self.pikis[pi].size);
        match (best, pellet) {
            (Some((_, cd)), Some((pel, pd))) if pd < cd => {
                self.start_carry(pi, pel);
            }
            (Some((target, _)), _) => {
                self.pikis[pi].state = PikiState::Attack(Attack {
                    target,
                    mode: AttackMode::Approach,
                });
            }
            (None, Some((pel, _))) => {
                self.start_carry(pi, pel);
            }
            (None, None) => {}
        }
    }

    /// `ActAttack` + `ActJumpAttack::exec`.
    fn piki_attack(&mut self, pi: usize, a: Attack, host: &mut dyn Host, dt: f32) {
        let Some(c) = self.creatures.get(a.target).copied() else {
            return self.finish_attack(pi);
        };
        if !c.alive || (c.flying && !matches!(a.mode, AttackMode::Latched { .. })) {
            return self.finish_attack(pi);
        }

        if let AttackMode::Latched {
            part,
            offset,
            frame,
            hit,
        } = a.mode
        {
            let Some(part_ref) = self.creatures.parts_of(&c).get(part as usize).copied() else {
                return self.knock_off(pi, a.target);
            };
            let id = self.pikis[pi].id;
            {
                let p = &mut self.pikis[pi];
                p.pos = part_ref.centre + rotate_y(offset, c.yaw);
                p.vel = Vec3::ZERO;
                p.face = yaw_of(part_ref.centre - p.pos);
            }
            // The cling animation loops, so it never finishes and aiconst p10 ("attack once")
            // never lets go: latched Pikmin hit once per loop until shaken off or the target dies.
            let anim = self.params.anim.kuttuku;
            let mut frame = frame + self.params.anim.fps * dt;
            let mut hit = hit;
            match anim.loop_range {
                Some((start, end)) if frame >= end => {
                    frame -= end - start;
                    hit = false;
                }
                None if frame >= anim.end => {
                    if self.params.ai.attack_once != 0 {
                        return self.finish_attack(pi);
                    }
                    frame = 0.0;
                    hit = false;
                }
                _ => {}
            }
            if !hit && frame >= anim.action0 && frame < anim.action1 {
                let dmg = self.attack_power(pi);
                if host.damage(a.target, dmg, Some(part), id) {
                    hit = true;
                    self.events.push(Event::Hit {
                        piki: id,
                        creature: a.target,
                        damage: dmg,
                        latched: true,
                    });
                }
            }
            self.pikis[pi].state = PikiState::Attack(Attack {
                mode: AttackMode::Latched {
                    part,
                    offset,
                    frame,
                    hit,
                },
                ..a
            });
            return;
        }

        let Some((_, part_centre, part_radius)) =
            nearest_part(&self.creatures, &c, self.pikis[pi].pos)
        else {
            return self.finish_attack(pi);
        };
        let size = part_radius + self.pikis[pi].size;
        let half = self.piki_speed(pi, 0.5);
        let full = self.piki_speed(pi, 1.0);
        let dmg = self.attack_power(pi);
        let anim = self.params.anim.attack;
        let fps = self.params.anim.fps;
        let attack_once = self.params.ai.attack_once != 0;
        let r = self.rng.f32();

        let p = &mut self.pikis[pi];
        let to = part_centre - p.pos;
        let dist2d = to.x.hypot(to.z);
        let dist3d = to.length();
        let flat = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
        let angle = ang_dist(yaw_of(to), p.face);

        let mut mode = a.mode;
        match a.mode {
            AttackMode::Approach => {
                if angle.abs() < PI / 10.0 && dist3d < size + 10.0 {
                    p.target_vel = Vec3::ZERO;
                    mode = AttackMode::Swing {
                        frame: 0.0,
                        hit: false,
                    };
                } else {
                    if dist2d < size && r > 0.7 {
                        mode = AttackMode::JumpPrep;
                    }
                    p.face = round_ang(p.face + 0.2 * angle);
                    p.target_vel = flat * full;
                }
            }
            AttackMode::JumpPrep => {
                if dist2d < size {
                    p.target_vel = -flat * half;
                } else if dist2d > size + 6.0 {
                    p.target_vel = flat * half;
                } else {
                    p.target_vel = Vec3::ZERO;
                    if angle.abs() < PI / 10.0 && p.on_ground {
                        let d = to.normalize_or_zero();
                        p.vel = Vec3::new(100.0 * d.x, 200.0, 100.0 * d.z);
                        p.on_ground = false;
                        mode = AttackMode::Jumping;
                    } else {
                        p.face = round_ang(p.face + 0.2 * angle);
                    }
                }
            }
            AttackMode::Jumping => {
                p.target_vel = Vec3::new(p.vel.x, 0.0, p.vel.z);
            }
            AttackMode::Swing { frame, hit } => {
                p.target_vel = Vec3::ZERO;
                let frame = frame + fps * dt;
                let mut hit = hit;
                // Between KEY_Action0 and KEY_Action1 the swing connects if the part is within
                // 10 and roughly in front; otherwise it retries next tick (mIsCriticalHit).
                if !hit && frame >= anim.action0 && frame < anim.action1 {
                    let reach = dist3d - size;
                    let id = p.id;
                    if reach < 10.0
                        && angle.abs() < PI / 4.0
                        && host.damage(a.target, dmg, None, id)
                    {
                        hit = true;
                        self.events.push(Event::Hit {
                            piki: id,
                            creature: a.target,
                            damage: dmg,
                            latched: false,
                        });
                    }
                }
                if frame >= anim.end {
                    // KEY_Finished: one attack done (aiconst p10), else back to state 0,
                    // which swings again if still in reach.
                    if attack_once {
                        return self.finish_attack(pi);
                    }
                    mode = AttackMode::Approach;
                } else {
                    mode = AttackMode::Swing { frame, hit };
                }
            }
            AttackMode::Latched { .. } => unreachable!(),
        }
        self.pikis[pi].state = PikiState::Attack(Attack { mode, ..a });
        self.physics(pi, host, dt);

        if mode == AttackMode::Jumping {
            if let Some((target, part, surface)) = self.touching_part(pi, Some(a.target))
                && self.creatures.parts[part as usize].stickable
            {
                self.latch(pi, target, part, surface);
                return;
            }
            if self.pikis[pi].on_ground {
                self.pikis[pi].state = PikiState::Attack(Attack {
                    mode: AttackMode::Approach,
                    ..a
                });
            }
        }
    }

    /// Attack is over (target dead or gone, or attack-once). Fall off, then go idle or rejoin.
    fn finish_attack(&mut self, pi: usize) {
        let p = &mut self.pikis[pi];
        if p.state.latched_to().is_some() {
            p.vel = Vec3::new(0.0, 30.0, 0.0);
            p.on_ground = false;
        }
        p.state = PikiState::Idle;
        let (id, captain) = (p.id, p.captain);
        if self.params.ai.return_after_attack != 0
            && let Some(cid) = captain
        {
            let range = self.params.piki.post_work_join_party_range;
            if (self.captains[cid.0 as usize].pos - self.pikis[pi].pos).length() < range {
                self.join_squad(id, cid);
            }
        }
    }

    /// Shaken off a creature (`PikiFlickState`): fly outward and lie on the ground for a while.
    pub(crate) fn knock_off(&mut self, pi: usize, from: CreatureId) {
        let centre = self.creatures.get(from).map(|c| self.creatures.bounds(c).0);
        let from_pos = centre.unwrap_or(self.pikis[pi].pos - Vec3::X);
        self.knock_from(pi, from_pos, Some(from));
    }

    /// Thrown outward from `from` (120 out, 150 up) and down for a random time between
    /// `mMinFlickKnockDownTime` and `mMaxFlickKnockDownTime`.
    pub(crate) fn knock_from(&mut self, pi: usize, from: Vec3, creature: Option<CreatureId>) {
        let pp = &self.params.piki;
        let down = self
            .rng
            .range(pp.min_flick_knockdown_time, pp.max_flick_knockdown_time);
        let p = &mut self.pikis[pi];
        let out = p.pos - from;
        let out = Vec3::new(out.x, 0.0, out.z).normalize_or(Vec3::X);
        p.vel = out * 120.0 + Vec3::new(0.0, 150.0, 0.0);
        p.target_vel = Vec3::ZERO;
        p.on_ground = false;
        p.state = PikiState::Knocked { timer: down };
        self.events.push(Event::KnockedOff {
            piki: p.id,
            creature,
        });
    }

    /// Remove a Pikmin from whatever it's part of (formation, a captain's hand, a pellet).
    pub(crate) fn leave_current(&mut self, id: PikiId) {
        if matches!(self.pikis[id.0 as usize].state, PikiState::Carry(_)) {
            self.stop_carry(id.0 as usize);
        }
        let p = &self.pikis[id.0 as usize];
        if p.state.in_squad()
            && let Some(cid) = p.captain
        {
            let c = &mut self.captains[cid.0 as usize];
            c.plate.remove(id);
            if c.held_piki() == Some(id)
                || matches!(c.throw, crate::captain::ThrowState::Wait { pending: Some(pp), .. } if pp == id)
            {
                c.throw = crate::captain::ThrowState::Idle;
            }
        }
        let p = &mut self.pikis[id.0 as usize];
        if p.state.latched_to().is_some() {
            p.vel = Vec3::ZERO;
            p.on_ground = false;
        }
        p.state = PikiState::Idle;
    }

    pub(crate) fn join_squad(&mut self, id: PikiId, captain: CaptainId) {
        let p = &mut self.pikis[id.0 as usize];
        p.captain = Some(captain);
        p.state = PikiState::squad();
        self.captains[captain.0 as usize].plate.add(id);
    }
}
