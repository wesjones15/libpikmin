//! Pikmin (decomp `Piki`) and their behaviour.

use std::f32::consts::PI;

use glam::Vec3;

use crate::event::Event;
use crate::host::{CreatureFrame, CreatureId, CreatureView};
use crate::math::{ang_dist, dist_xz, rotate_y, round_ang, yaw_of};
use crate::sim::{Sim, integrate};
use crate::{CaptainId, Host, PikiId};

/// Host extension: how much of a locked target's velocity the throw bakes into its initial arc.
/// 0: aim where it is now (vanilla cursor). 1: aim at the full intercept. The in-flight
/// guidance below only has to correct a target that changes speed or direction.
pub(crate) const GUIDANCE_LEAD_SHARE: f32 = 1.0;
/// Host extension: the most a throw leads, in sim units (~4 cm each). A teleporting or very fast
/// target must not send the arc off the map.
pub(crate) const GUIDANCE_MAX_LEAD: f32 = 300.0;
/// Host extension: a guard against a guided throw accelerating without bound near impact.
pub(crate) const GUIDANCE_MAX_SPEED: f32 = 1200.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PikiColor {
    Blue,
    Red,
    Yellow,
    /// Pikmin 2's Purple (`Game::Purple`, "black"). Not in Pikmin 1.
    Purple,
    /// Pikmin 2's White (`Game::White`). Not in Pikmin 1.
    White,
}

impl PikiColor {
    /// Pikmin 1's three colours: what an Onion takes by default and what the plain squad cycles through.
    pub const ALL: [PikiColor; 3] = [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow];

    /// Every colour, Pikmin 2's two included.
    pub const EVERY: [PikiColor; COLOUR_COUNT] =
        [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow, PikiColor::Purple, PikiColor::White];

    /// The colour index (Blue 0, Red 1, Yellow 2 as in the decomp; Purple 3, White 4).
    pub fn index(self) -> usize {
        self as usize
    }

    /// One of Pikmin 1's three, cycling (`i % 3`). Use [`PikiColor::from_id`] for Purple and White.
    pub fn from_index(i: usize) -> Self {
        Self::ALL[i % 3]
    }

    /// The colour with index `i`, if there is one.
    pub fn from_id(i: usize) -> Option<Self> {
        Self::EVERY.get(i).copied()
    }

    /// Whether Pikmin 2 added it.
    pub fn is_pikmin2(self) -> bool {
        matches!(self, PikiColor::Purple | PikiColor::White)
    }

    /// `PikiMgr::getColorTransportScale`: how many carriers it counts as toward lifting.
    pub fn transport_scale(self) -> u32 {
        if self == PikiColor::Purple { 10 } else { 1 }
    }
}

/// How many [`PikiColor`]s there are.
pub const COLOUR_COUNT: usize = 5;

/// Host extension: how far above a Pikmin its squad slot must be before it counts as stranded on
/// a lower level (sim units, about 6 m), and how long it must fail to move before being put there.
const STRANDED_GAP: f32 = 150.0;
const STRANDED_TIME: f32 = 2.0;

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
    /// Host extension: a dismiss strike sent this Pikmin at a chosen enemy.
    StrikeRush,
    /// Host extension: steer the strike's jump toward the enemy's moving stickable part.
    StrikeJump { remaining: f32 },
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

/// `PikiFlickState`'s phases, each timed by its motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnockPhase {
    /// `FLS_FlyingBack`: JHIT.
    Flying { frame: f32 },
    /// `FLS_Landing`: JKOKE.
    Landing { frame: f32 },
    /// `FLS_Downed`: lying there.
    Down { timer: f32 },
    /// `FLS_GettingUp`: GETUP.
    GettingUp { frame: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PikiState {
    /// Free and standing around (`PikiMode::FreeMode`).
    Idle,
    /// Following a captain's formation (`ActCrowd`). `formed` once it has reached its slot
    /// (`STATE_Formed`), which is when bumping into things gives it a job.
    Squad {
        lost_timer: f32,
        near_counter: u8,
        reslotted: bool,
        formed: bool,
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
    /// Shaken off (`PikiFlickState`): flying back, landing, lying down, getting up.
    Knocked {
        phase: KnockPhase,
        /// Whistled while down (`mIsWhistlePending`): gets up at once and joins this captain.
        whistled: Option<CaptainId>,
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
    /// Drinking nectar (`PikiAbsorbState`, MIZUNOMI). `drinking` once the loop starts;
    /// `drank` if it drank anything; `finishing` when the puddle was gone at a loop end.
    Absorb {
        puddle: crate::nectar::NectarId,
        frame: f32,
        drinking: bool,
        drank: bool,
        finishing: bool,
    },
    /// Blooming after nectar (`PikiGrowupState`): GROWUP1, or GROWUP2 if `second`.
    GrowUp {
        frame: f32,
        second: bool,
        bloomed: bool,
    },
    /// Going into an Onion (`ActEnter`): walking to leg `leg`, then climbing it. `ratio` is
    /// how far up the leg.
    Enter {
        onion: crate::onion::OnionId,
        leg: u8,
        climbing: bool,
        ratio: f32,
    },
    /// Sliding down an Onion leg after being called out (`ActExit`); joins its captain at the
    /// bottom.
    Exit {
        onion: crate::onion::OnionId,
        leg: u8,
        ratio: f32,
    },
    /// Host extension: an Onion's plucker, out pulling forgotten sprouts (`plucker.rs`).
    Plucker(crate::plucker::Plucker),
    /// `PikiNukareState`: rooted while the authored pluck clip plays.
    Plucked { sprout: crate::onion::SproutId, frame: f32, fast: bool, burst: bool },
    /// Gone.
    Dead,
}

impl PikiState {
    pub fn squad() -> Self {
        PikiState::Squad {
            lost_timer: 0.0,
            near_counter: 0,
            reslotted: false,
            formed: false,
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

/// Host extension: a throw re-aimed every tick so it arrives after `remaining` seconds. It
/// follows `creature` (a lock-on) while that has a holdable part, else heads for `point`
/// (the cursor where it was thrown; see [`Sim::set_cursor_homing`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuidedThrow {
    pub creature: Option<CreatureId>,
    pub point: Option<Vec3>,
    pub remaining: f32,
}

#[derive(Debug, Clone)]
pub struct Piki {
    pub id: PikiId,
    /// Incarnation of a reusable slot; presentation must not blend different occupants.
    pub generation: u32,
    pub color: PikiColor,
    pub happa: Happa,
    pub pos: Vec3,
    pub vel: Vec3,
    pub target_vel: Vec3,
    pub face: f32,
    /// Body radius, random per Pikmin between `min_piki_size` and `max_piki_size`.
    pub size: f32,
    pub on_ground: bool,
    /// Locomotion jump, independent of its job (never the thrown/attack state).
    pub jump: Option<crate::jump::Jump>,
    pub(crate) jump_queue: std::collections::VecDeque<crate::jump::JumpCue>,
    /// Captain this Pikmin follows or last followed (`mNavi`).
    pub captain: Option<CaptainId>,
    pub state: PikiState,
    /// `mHealth`, starts at `mPikiMaxHealth`.
    pub health: f32,
    /// Times it has bloomed from nectar (`mFloweringTimer`).
    pub blooms: u8,
    /// Dismissed by its captain and not called back since: it stays put even with
    /// [`Sim::set_auto_rejoin`].
    pub dismissed: bool,
    /// Seconds it has stood idle.
    pub idle_time: f32,
    /// The tick it last latched onto a creature (see [`Sim::set_shake_protection`]).
    pub latched_at: u64,
    /// A throw steered in flight (see [`GuidedThrow`]).
    pub guided_throw: Option<GuidedThrow>,
    /// Creatures it has attacked, newest overwriting oldest (see [`Sim::credit_kill`]).
    pub(crate) hit_log: [Option<CreatureId>; crate::growth::HIT_LOG],
    pub(crate) hit_next: u8,
    /// Kills credited toward its next stage (see [`Sim::set_kill_growth`]).
    pub kill_credit: u8,
    /// Host extension: seconds left frozen (see [`Sim::freeze_pikis_in_sphere`]). It stands
    /// still and does nothing until it thaws.
    pub frozen: f32,
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
        // Host extension: frozen (`Sim::freeze_pikis_in_sphere`), it only falls and stands.
        if self.pikis[pi].frozen > 0.0 {
            let p = &mut self.pikis[pi];
            p.frozen = (p.frozen - dt).max(0.0);
            if p.is_alive() && !matches!(p.state, PikiState::Held { .. }) {
                p.target_vel = Vec3::ZERO;
                p.vel.x = 0.0;
                p.vel.z = 0.0;
                self.physics(pi, host, dt);
                return;
            }
        }
        if self.update_piki_jump(pi, host, dt) { return; }
        if !matches!(self.pikis[pi].state, PikiState::Flying { .. }) { self.pikis[pi].guided_throw = None; }
        if !matches!(self.pikis[pi].state, PikiState::Idle) {
            self.pikis[pi].idle_time = 0.0;
        }
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
            PikiState::Absorb { .. } | PikiState::GrowUp { .. } => self.piki_nectar(pi, host, dt),
            PikiState::Enter { .. } | PikiState::Exit { .. } => self.piki_onion_travel(pi, host, dt),
            PikiState::Plucker(pl) => self.piki_plucker(pi, pl, host, dt),
            PikiState::Plucked { sprout, frame, fast, burst } => {
                let anim = if fast { self.params.anim.nukare_fast } else { self.params.anim.nukareru };
                let next = frame + self.params.anim.fps * dt * self.attack_rate_of(pi);
                // Both clips start together in NaviNukuAdjustState::exec. Captains
                // step before Pikmin; sample their counter to avoid a one-tick lead.
                let f = self.pikis[pi].captain.and_then(|c| self.captains[c.0 as usize].pluck)
                    .filter(|p| p.sprout == sprout)
                    .and_then(|p| match p.phase { crate::onion::PluckPhase::Pull { frame, .. } => Some(frame), _ => None })
                    .unwrap_or(next);
                if !burst && f >= anim.action0 {
                    self.events.push(Event::PluckBurst { piki: self.pikis[pi].id });
                }
                let p = &mut self.pikis[pi];
                p.vel = Vec3::ZERO;
                p.target_vel = Vec3::ZERO;
                p.state = PikiState::Plucked { sprout, frame: f, fast, burst: burst || f >= anim.action0 };
                if f >= anim.end {
                    // PikiNukareState::procAnimMsg KEY_Finished returns to formation.
                    if let Some(c) = p.captain.filter(|c| !self.captains[c.0 as usize].down) {
                        self.join_squad(self.pikis[pi].id, c);
                    } else {
                        self.pikis[pi].state = PikiState::Idle;
                    }
                }
            }
            PikiState::Knocked { phase, whistled } => self.piki_knocked(pi, phase, whistled, host, dt),
        }
    }

    pub(crate) fn physics(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        self.try_navigation_jump(pi, host);
        let accel = self.params.piki_creature.acceleration;
        let gravity = self.pikis[pi].jump.and_then(|j| j.gravity).unwrap_or(self.params.ai.gravity);
        let face_adjust = self.params.piki_creature.face_dir_adjust;
        let p = &mut self.pikis[pi];
        let mut on_ground = p.on_ground;
        integrate(
            host,
            &mut p.pos,
            &mut p.vel,
            p.target_vel,
            &mut on_ground,
            p.size,
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
        // Host extension: a pace relative to the captain's own top speed.
        if let (Some(pace), Some(cid)) = (self.pace, self.pikis[pi].captain) {
            let c = &self.captains[cid.0 as usize];
            let top = self.params.navi.run_speed * c.speed_scale * pace[self.pikis[pi].happa as usize];
            let min = pp.min_move_speed.min(top);
            return ((top - min) * ratio + min) * self.colour_speed(pi);
        }
        let scale = self.params.ai.do_scale_happa_move_speed != 0;
        let max = match self.pikis[pi].happa {
            Happa::Flower if scale => pp.max_flower_move_speed,
            Happa::Bud if scale => pp.max_bud_move_speed,
            _ => pp.max_leaf_move_speed,
        };
        ((max - pp.min_move_speed) * ratio + pp.min_move_speed) * self.squad_speed_of(pi) * self.colour_speed(pi)
    }

    /// Its captain's [`crate::Captain::squad_speed`].
    fn squad_speed_of(&self, pi: usize) -> f32 {
        self.pikis[pi].captain.map_or(1.0, |c| self.captains[c.0 as usize].squad_speed)
    }

    /// Its captain's [`crate::Captain::attack_rate`]: how fast its attack animations play.
    pub(crate) fn attack_rate_of(&self, pi: usize) -> f32 {
        self.pikis[pi].captain.map_or(1.0, |c| self.captains[c.0 as usize].attack_rate)
    }

    /// `Piki::getAttackPower`
    pub(crate) fn attack_power(&self, pi: usize) -> f32 {
        let pp = &self.params.piki;
        match self.pikis[pi].color {
            PikiColor::Blue => pp.blue_attack_power,
            PikiColor::Red => pp.red_attack_power,
            PikiColor::Yellow => pp.yellow_attack_power,
            PikiColor::Purple => self.params.p2.purple_attack_power,
            PikiColor::White => self.params.p2.white_attack_power,
        }
    }

    /// `Piki::getSpeed`'s colour factor: White runs faster, Purple slower (`mWhiteRunSpeedMultiplier`,
    /// `mPurpleRunSpeedMultiplier`).
    pub(crate) fn colour_speed(&self, pi: usize) -> f32 {
        match self.pikis[pi].color {
            PikiColor::White => self.params.p2.white_speed_multiplier,
            PikiColor::Purple => self.params.p2.purple_speed_multiplier,
            _ => 1.0,
        }
    }

    fn piki_idle(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        self.pikis[pi].target_vel = Vec3::ZERO;
        self.pikis[pi].idle_time += dt;
        if self.try_auto_carry(pi) || self.try_rejoin(pi) {
            self.physics(pi, host, dt);
            return;
        }
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
        let cue = self.pikis[pi].jump_queue.front().copied();
        let slot_pos = cue.map_or_else(|| c.plate.slot_world(slot), |j| j.origin);
        let (cpos, plate_offset, swarming) = (c.pos, c.plate.offset, c.swarming);
        let accel = self.params.piki_creature.acceleration;
        let full = self.piki_speed(pi, 1.0);
        let half = self.piki_speed(pi, 0.5);
        let pp = self.params.piki.clone();

        let PikiState::Squad {
            mut lost_timer,
            mut near_counter,
            mut reslotted,
            mut formed,
        } = self.pikis[pi].state
        else {
            unreachable!()
        };
        if cue.is_none() && formed && self.squad_contact(pi, swarming) {
            return;
        }
        if cue.is_none() && !swarming && self.try_auto_carry(pi) {
            return;
        }

        let p = &mut self.pikis[pi];
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
            formed = true;
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

        // Pikmin in front of the captain arc around him instead of walking through, except
        // while the C-stick sends them forward on purpose (`newVer && !check`).
        if sidestep && !swarming && cue.is_none() {
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
        // Host extension: no pathfinding. A squad Pikmin on the ground whose slot is a level or
        // more above it, and that cannot move because it is pressed against the cliff, can never
        // get there. After a couple of seconds, put it at its slot.
        let stalled = {
            let p = &self.pikis[pi];
            p.on_ground && dist > 7.0 && slot_pos.y - p.pos.y > STRANDED_GAP && p.vel.length() < 15.0
        };
        if stalled {
            lost_timer += dt;
            if lost_timer > STRANDED_TIME {
                let p = &mut self.pikis[pi];
                p.pos = slot_pos;
                p.vel = Vec3::ZERO;
                p.on_ground = true;
                lost_timer = 0.0;
            }
        } else if dist < pp.formation_slip_range {
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
        // With auto-rejoin a straggler keeps running after the captain instead.
        if self.auto_rejoin || cue.is_some() {
            leave = false;
        }

        let p = &mut self.pikis[pi];
        p.state = PikiState::Squad {
            lost_timer,
            near_counter,
            reslotted,
            formed,
        };
        if leave {
            self.leave_current(id);
            self.pikis[pi].state = PikiState::Idle;
        }
        self.physics(pi, host, dt);
    }

    /// `Piki::collisionCallback` for a formed squad member: bumping an enemy starts an attack
    /// (aiconst p05, always); bumping a pellet with a free slot starts carrying, but only while
    /// the C-stick is pushing (`distCheck`). Returns true if it left the squad for a job.
    fn squad_contact(&mut self, pi: usize, swarming: bool) -> bool {
        let (pos, size, id) = (self.pikis[pi].pos, self.pikis[pi].size, self.pikis[pi].id);
        // Separation keeps bodies exactly touching, so allow a unit of slack.
        let touch = 1.0;
        if self.params.ai.do_cstick_attack != 0 {
            let hit = self.creatures.creatures.iter().find(|c| {
                c.alive
                    && self.creatures.parts_of(c).iter().any(|part| {
                        let d = pos - part.centre;
                        d.length() < part.radius + size + touch
                            || (Vec3::new(d.x, 0.0, d.z).length() < part.radius + size + touch
                                && d.y.abs() < part.radius + size)
                    })
            });
            if let Some(c) = hit {
                let target = c.id;
                self.leave_current(id);
                self.pikis[pi].state = PikiState::Attack(Attack {
                    target,
                    mode: AttackMode::Approach,
                });
                return true;
            }
        }
        if swarming
            && let Some(pel) = self
                .pellets
                .iter()
                .find(|pl| pl.can_take_carrier() && pl.touches(pos, size + touch))
                .map(|pl| pl.id)
        {
            self.leave_current(id);
            return self.start_carry(pi, pel);
        }
        false
    }

    fn piki_go_hang(&mut self, pi: usize, captain: CaptainId, host: &mut dyn Host, dt: f32) {
        let c = &self.captains[captain.0 as usize];
        let cpos = c.pos;
        // Host extension: called to his hand, it sprints to catch him even at full tilt (a leaf
        // is slower than him), so he can throw on the run.
        let top = self.params.navi.run_speed * c.speed_scale;
        let mut full = self.piki_speed(pi, 1.0).max(crate::captain::GO_HANG_CATCH_UP * top);
        // Host lock-on: feed the hand in one sim tick, like keeping the squad pushed
        // up with Navi::makeCStick while tapping throw. This changes pickup cadence,
        // not Navi::throwPiki's flight time or the press/release control.
        let rapid = c.lock_view.is_some();
        let p = &mut self.pikis[pi];
        let to = cpos - p.pos;
        let flat = Vec3::new(to.x, 0.0, to.z);
        if rapid { full = flat.length() / dt.max(0.0001); }
        p.target_vel = flat.normalize_or_zero() * full;
        if rapid {
            // Immediate acceleration and a bounded step prevent fast pickups from
            // overshooting the grab radius and oscillating across Olimar's hand.
            p.vel.x = p.target_vel.x;
            p.vel.z = p.target_vel.z;
        }
        self.physics(pi, host, dt);
    }

    /// `PikiFlyingState`: ballistic until we touch a creature part or the ground.
    fn piki_flying(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        // Re-solve the remaining arc against the current hurtbox, so moving/flying
        // targets are met through normal collision and latch, never a teleport.
        // A locked-on creature is followed to its first holdable part; with none (or once it
        // has died) the throw keeps heading for the cursor point it was thrown at.
        if let Some(g) = self.pikis[pi].guided_throw {
            let mut aim = g.point;
            if let Some(id) = g.creature {
                if let Some(c) = self.creatures.get(id).filter(|c| c.alive) {
                    if let Some(part) = self.creatures.parts_of(c).iter().find(|p| p.stickable) {
                        // Aim at where the part will be on arrival: its current estimated
                        // velocity over the flight still to go. The lead shrinks with the
                        // remaining time and is zero on the final guidance tick, so contact
                        // goes through the actual hurtbox and a target that turned is tracked
                        // to where it really is.
                        let lead = (self.creature_velocity(id) * (g.remaining - dt).max(0.0))
                            .clamp_length_max(GUIDANCE_MAX_LEAD);
                        aim = Some(part.centre + lead);
                    }
                }
            }
            if let Some(aim) = aim.filter(|_| g.remaining > 0.) {
                let t = g.remaining.max(dt);
                let p = &mut self.pikis[pi];
                // Gravity-aware 3D arrival: the velocity that reaches `aim` in the time left,
                // with the gravity this tick's integration will add already accounted for.
                p.vel = ((aim - p.pos) / t + Vec3::Y * (self.params.ai.gravity * (t + dt) * 0.5))
                    .clamp_length_max(GUIDANCE_MAX_SPEED);
                p.guided_throw = Some(GuidedThrow { remaining: g.remaining - dt, ..g });
            } else { self.pikis[pi].guided_throw = None; }
        }
        let p = &mut self.pikis[pi];
        p.target_vel = p.vel;
        self.physics(pi, host, dt);

        // A Candypop Bud swallows a thrown Pikmin that comes down on its mouth (`Pom::Obj::pressCallBack`).
        if self.bulb_swallow(pi) {
            return;
        }

        if let Some((target, part, surface)) = self.touching_part(pi, None) {
            if self.pikis[pi].color == PikiColor::Purple {
                self.pound(pi, host, Some((target, part)));
            }
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

        if self.pikis[pi].on_ground && self.pikis[pi].color == PikiColor::Purple {
            self.pound(pi, host, None);
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

    /// A thrown Purple comes down: `PikiHipDropState::collisionCallback` hits the creature it landed on with
    /// `mPoundDamage`, and `earthquake` (`mPoundAOERange`) reaches the rest, flying ones included, which the host
    /// brings to the ground ([`Event::Grounded`]). Unlike Pikmin 2, the throw is not shortened or stopped at its peak:
    /// the host relies on far throws and lock-ons, so it lands where it was thrown and pounds there.
    fn pound(&mut self, pi: usize, host: &mut dyn Host, hit: Option<(CreatureId, u32)>) {
        let (id, pos) = (self.pikis[pi].id, self.pikis[pi].pos);
        self.events.push(Event::Pounded { piki: id, pos });
        if let Some((target, global)) = hit
            && let Some(c) = self.creatures.get(target).copied()
        {
            let damage = self.params.p2.pound_damage;
            if host.damage(target, damage, Some(global - c.parts_start), crate::host::DamageSource::Piki(id)) {
                self.note_attack(pi, target);
                self.events.push(Event::Hit { piki: id, creature: target, damage, latched: false });
            }
        }
        let radius = self.params.p2.pound_radius;
        let fliers: Vec<CreatureId> = self
            .creatures
            .creatures
            .iter()
            .filter(|c| c.alive && c.flying && c.parts_len > 0)
            .filter(|c| {
                let (centre, size) = self.creatures.bounds(c);
                dist_xz(pos, centre) <= radius + size
            })
            .map(|c| c.id)
            .collect();
        for creature in fliers {
            self.events.push(Event::Grounded { creature, piki: id });
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
        p.latched_at = self.tick;
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
        if !c.alive || (c.flying && !matches!(a.mode, AttackMode::Latched { .. } | AttackMode::StrikeRush | AttackMode::StrikeJump { .. })) {
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
            let mut frame = frame + self.params.anim.fps * dt * self.attack_rate_of(pi);
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
                if host.damage(
                    a.target,
                    dmg,
                    Some(part),
                    crate::host::DamageSource::Piki(id),
                ) {
                    hit = true;
                    self.note_attack(pi, a.target);
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
        let fps = self.params.anim.fps * self.attack_rate_of(pi);
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
            AttackMode::StrikeRush => {
                if !self.creatures.parts_of(&c).iter().any(|part| part.stickable) {
                    return self.finish_attack(pi);
                }
                p.face = round_ang(p.face + 0.2 * angle);
                if dist2d <= size + 35.0 && p.on_ground {
                    p.target_vel = Vec3::ZERO;
                    let t = 0.35;
                    p.vel = to / t + Vec3::Y * (self.params.ai.gravity * (t + dt) * 0.5);
                    p.target_vel = p.vel;
                    p.on_ground = false;
                    mode = AttackMode::StrikeJump { remaining: t - dt };
                } else {
                    p.target_vel = flat * full;
                }
            }
            AttackMode::StrikeJump { remaining } => {
                if remaining > 0.0 {
                    let t = remaining.max(dt);
                    p.vel = to / t + Vec3::Y * (self.params.ai.gravity * (t + dt) * 0.5);
                }
                p.target_vel = p.vel;
                mode = AttackMode::StrikeJump { remaining: (remaining - dt).max(0.0) };
            }
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
                        && host.damage(a.target, dmg, None, crate::host::DamageSource::Piki(id))
                    {
                        hit = true;
                        self.note_attack(pi, a.target);
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

        if mode == AttackMode::Jumping || matches!(mode, AttackMode::StrikeJump { .. }) {
            if let Some((target, part, surface)) = self.touching_part(pi, Some(a.target))
                && self.creatures.parts[part as usize].stickable
            {
                self.latch(pi, target, part, surface);
                return;
            }
            if self.pikis[pi].on_ground {
                self.pikis[pi].state = PikiState::Attack(Attack {
                    mode: if matches!(mode, AttackMode::StrikeJump { .. }) { AttackMode::StrikeRush } else { AttackMode::Approach },
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

    /// `PikiFlickState`: JHIT while flying back, JKOKE on landing, then down for a random time
    /// between `mMinFlickKnockDownTime` and `mMaxFlickKnockDownTime` (dead at that point if
    /// health is gone, straight to `PikiDeadState`), then GETUP. Each phase ends with its
    /// motion (`procAnimMsg` on `KEY_Finished`). A whistle cuts the lying short.
    fn piki_knocked(
        &mut self,
        pi: usize,
        phase: KnockPhase,
        whistled: Option<CaptainId>,
        host: &mut dyn Host,
        dt: f32,
    ) {
        let anim = self.params.anim.clone();
        let step = anim.fps * dt;
        self.physics(pi, host, dt);
        let p = &mut self.pikis[pi];
        p.target_vel = Vec3::ZERO;
        if !matches!(phase, KnockPhase::Flying { .. }) {
            p.vel.x *= 0.9;
            p.vel.z *= 0.9;
        }
        let next = match phase {
            KnockPhase::Flying { frame } if frame + step >= anim.jhit.end => KnockPhase::Landing { frame: 0.0 },
            KnockPhase::Flying { frame } => KnockPhase::Flying { frame: frame + step },
            KnockPhase::Landing { frame } if frame + step >= anim.jkoke.end => {
                if p.health <= 0.0 {
                    let id = p.id;
                    p.state = PikiState::Fading { timer: 1.5 };
                    self.piki_deaths += 1;
                    self.events.push(Event::PikiDied {
                        piki: id,
                        cause: crate::damage::DeathCause::Attacked,
                    });
                    return;
                }
                let pp = &self.params.piki;
                KnockPhase::Down {
                    timer: self.rng.range(pp.min_flick_knockdown_time, pp.max_flick_knockdown_time),
                }
            }
            KnockPhase::Landing { frame } => KnockPhase::Landing { frame: frame + step },
            KnockPhase::Down { timer } if timer - dt < 0.0 || whistled.is_some() => {
                KnockPhase::GettingUp { frame: 0.0 }
            }
            KnockPhase::Down { timer } => KnockPhase::Down { timer: timer - dt },
            KnockPhase::GettingUp { frame } if frame + step >= anim.getup.end => {
                // Up again; a bud or flower may lose a stage (25%).
                let id = p.id;
                if p.happa != Happa::Leaf && self.rng.f32() < 0.25 {
                    let p = &mut self.pikis[pi];
                    p.happa = if p.happa == Happa::Flower { Happa::Bud } else { Happa::Leaf };
                    self.events.push(Event::Deflowered { piki: id });
                }
                self.pikis[pi].state = PikiState::Idle;
                if let Some(cid) = whistled {
                    // `PikiFlickState::cleanup`: the booked whistle takes effect.
                    self.join_squad(id, cid);
                    self.events.push(Event::Recruited {
                        piki: id,
                        captain: cid,
                        stolen_from: None,
                    });
                }
                return;
            }
            KnockPhase::GettingUp { frame } => KnockPhase::GettingUp { frame: frame + step },
        };
        self.pikis[pi].state = PikiState::Knocked { phase: next, whistled };
    }

    /// Thrown outward from `from` (120 out, 150 up) into [`Sim::piki_knocked`].
    pub(crate) fn knock_from(&mut self, pi: usize, from: Vec3, creature: Option<CreatureId>) {
        let p = &mut self.pikis[pi];
        let out = p.pos - from;
        let out = Vec3::new(out.x, 0.0, out.z).normalize_or(Vec3::X);
        p.vel = out * 120.0 + Vec3::new(0.0, 150.0, 0.0);
        p.target_vel = Vec3::ZERO;
        p.on_ground = false;
        p.state = PikiState::Knocked {
            phase: KnockPhase::Flying { frame: 0.0 },
            whistled: None,
        };
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
        p.jump_queue.clear();
        if p.state.latched_to().is_some() {
            p.vel = Vec3::ZERO;
            p.on_ground = false;
        }
        p.state = PikiState::Idle;
    }

    /// Host option, not Pikmin 1: a separated Pikmin (done carrying or fighting, landed from
    /// a throw with nothing to do, fallen out of formation) goes back to its captain's squad
    /// after a moment, wherever he is. Dismissed Pikmin stay where they were left.
    fn try_rejoin(&mut self, pi: usize) -> bool {
        let p = &self.pikis[pi];
        if !self.auto_rejoin || p.dismissed || p.idle_time < self.rejoin_delay || !p.on_ground {
            return false;
        }
        let Some(cid) = p.captain else {
            return false;
        };
        if self.captains[cid.0 as usize].down {
            return false;
        }
        let id = p.id;
        self.join_squad(id, cid);
        true
    }

    pub(crate) fn join_squad(&mut self, id: PikiId, captain: CaptainId) {
        let p = &mut self.pikis[id.0 as usize];
        p.dismissed = false;
        p.captain = Some(captain);
        p.state = PikiState::squad();
        self.captains[captain.0 as usize].plate.add(id);
    }
}
