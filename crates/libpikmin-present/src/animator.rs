//! Which animation each sim object plays, and at what frame, picked from sim state the way
//! the game's states do (`Piki::updateWalkAnimation`, `Navi::updateWalkAnimation`, the
//! `startMotion` calls in pikiState.cpp, naviState.cpp, aiAttack.cpp and aiTransport.cpp).
//! Where the sim keeps its own animation counter (swing, latch, lift, pluck, punch, death)
//! that frame is used, so the pose matches the gameplay timing exactly.

use std::collections::HashMap;

use glam::Vec3;
use libpikmin::captain::{ThrowState, WhistleState};
use libpikmin::onion::{OnionAnim, OnionId, PluckPhase};
use libpikmin::piki::AttackMode;
use libpikmin::plucker::PluckerJob;
use libpikmin::{Captain, CaptainId, Event, Onion, Params, Piki, PikiColor, PikiState, Sprout, SproutId, SproutState};
use libpikmin_assets::motion::Motion;
use libpikmin_assets::{Anim, AnimSet};

/// An animation and the frame to pose it at.
pub type Posed<'a> = Option<(&'a Anim, f32)>;

/// How an animation's frame is driven.
#[derive(Debug, Clone, Copy)]
pub enum Clock {
    /// Use the sim's own counter.
    Sim(f32),
    /// Normalized host time over the disc clip's key range.
    Progress(f32),
    /// Loop over the anim keys at this many frames per second.
    Loop(f32),
    /// Play once and hold the last frame.
    Once(f32),
    /// Start at this frame, then loop over the motion's loop keys (`Pick` while carrying:
    /// `ActTransport` starts it at 11).
    LoopFrom(f32, f32),
    /// Repeat only an airborne section of a cinematic clip.
    LoopRange(f32, f32, f32),
}

#[derive(Debug, Clone, Copy)]
pub struct Player {
    /// AnimMgr index being played.
    pub anim: usize,
    pub frame: f32,
}

/// Player keys, one space per kind of object.
pub fn piki_key(id: libpikmin::PikiId) -> u64 {
    id.0 as u64
}
pub fn captain_key(id: CaptainId) -> u64 {
    (1u64 << 40) | id.0 as u64
}
pub fn onion_key(index: usize) -> u64 {
    (3u64 << 40) | index as u64
}
pub fn sprout_key(id: SproutId) -> u64 {
    (4u64 << 40) | id.0 as u64
}

pub fn advance<'a>(
    players: &mut HashMap<u64, Player>,
    key: u64,
    set: &'a AnimSet,
    motion: Motion,
    clock: Clock,
    dt: f32,
) -> Option<(&'a Anim, f32)> {
    advance_index(players, key, set, motion.anim_index()?, clock, dt)
}

/// Like [`advance`] for sets without a motion table (items, tekis): `index` is the AnimMgr
/// entry itself.
pub fn advance_index<'a>(
    players: &mut HashMap<u64, Player>,
    key: u64,
    set: &'a AnimSet,
    index: usize,
    clock: Clock,
    dt: f32,
) -> Option<(&'a Anim, f32)> {
    let (info, anim) = set.get(index)?;
    let start = *info.keys.first().unwrap_or(&0) as f32;
    let end = (*info.keys.last().unwrap_or(&0) as f32).max(start);
    let first = match clock {
        Clock::LoopFrom(_, from) | Clock::LoopRange(_, from, _) => from,
        _ => start,
    };
    let p = players.entry(key).or_insert(Player {
        anim: index,
        frame: first,
    });
    if p.anim != index {
        *p = Player {
            anim: index,
            frame: first,
        };
    }
    match clock {
        Clock::Progress(t) => p.frame = start + (end - start) * t.clamp(0.0, 1.0),
        Clock::LoopRange(fps, lo, hi) => p.frame = lo + (p.frame - lo + fps * dt).rem_euclid(hi - lo),
        Clock::Sim(f) => p.frame = f,
        Clock::Loop(fps) => {
            p.frame += fps * dt;
            if end > start && p.frame >= end {
                p.frame = start + (p.frame - start) % (end - start);
            }
        }
        Clock::Once(fps) => p.frame = (p.frame + fps * dt).min(end),
        Clock::LoopFrom(fps, _) => {
            let key = |ty: u8| {
                let (n, _) = info.info_keys.iter().find(|(_, t)| *t == ty)?;
                info.keys.get(*n as usize).map(|&f| f as f32)
            };
            let (lo, hi) = key(0).zip(key(1)).unwrap_or((start, end));
            p.frame += fps * dt;
            if hi > lo && p.frame >= hi {
                p.frame = lo + (p.frame - lo) % (hi - lo);
            }
        }
    }
    let last = anim.frames.saturating_sub(1) as f32;
    Some((anim, p.frame.clamp(0.0, last)))
}

/// `updateWalkAnimation`: WAIT, ASIBUMI, WALK, RUN or NIGERU by speed, with the walk/run
/// rate scaled between its min and max.
pub fn locomotion(speed: f32, th: [f32; 4], rate: [f32; 5]) -> (Motion, Clock) {
    let ramp = |lo: f32, hi: f32, a: f32, b: f32| a + (b - a) * ((speed - lo) / (hi - lo)).clamp(0.0, 1.0);
    if speed < th[0] {
        (Motion::Wait, Clock::Loop(30.0))
    } else if speed < th[1] {
        (Motion::Asibumi, Clock::Loop(30.0))
    } else if speed < th[2] {
        (Motion::Walk, Clock::Loop(ramp(th[1], th[2], rate[0], rate[1])))
    } else if speed < th[3] {
        (Motion::Run, Clock::Loop(ramp(th[2], th[3], rate[2], rate[3])))
    } else {
        (Motion::Nigeru, Clock::Loop(rate[4]))
    }
}

/// `Piki::pikiColors` (piki.cpp).
pub fn piki_rgb(c: PikiColor) -> [u8; 4] {
    match c {
        PikiColor::Blue => [0, 50, 255, 255],
        PikiColor::Red => [255, 30, 0, 255],
        PikiColor::Yellow => [255, 210, 0, 255],
    }
}

/// How a Pikmin is drawn this frame.
#[derive(Debug, Clone, Copy)]
pub struct PikiLook {
    pub motion: Motion,
    pub clock: Clock,
    /// Squash (flattened) or shrink (fading, climbing into the Onion).
    pub scale: Vec3,
    /// 1 visible, 0 gone.
    pub fade: f32,
}

/// A Pikmin's motion, or `None` if it isn't drawn (dead, swallowed).
pub fn piki_look(params: &Params, p: &Piki) -> Option<PikiLook> {
    let pk = &params.piki;
    let th = [
        pk.step_speed_threshold,
        pk.walk_speed_threshold,
        pk.run_speed_threshold,
        pk.escape_speed_threshold,
    ];
    let rate = [
        pk.min_walk_anim_speed,
        pk.max_walk_anim_speed,
        pk.min_run_anim_speed,
        pk.max_run_anim_speed,
        pk.escape_anim_speed,
    ];
    let mut scale = Vec3::ONE;
    let mut fade = 1.0;
    let walk = locomotion(p.vel.with_y(0.0).length(), th, rate);
    if p.jump.is_some() {
        return Some(PikiLook { motion: Motion::RollJmp, clock: Clock::Loop(30.0), scale, fade });
    }
    let (motion, clock) = match p.state {
        PikiState::Dead | PikiState::Swallowed { .. } => return None,
        PikiState::Idle | PikiState::Squad { .. } | PikiState::GoHang { .. } => walk,
        PikiState::Held { .. } => (Motion::Hang, Clock::Loop(30.0)),
        PikiState::Flying { .. } => (Motion::RollJmp, Clock::Loop(30.0)),
        PikiState::Attack(a) => match a.mode {
            AttackMode::Approach => walk,
            AttackMode::JumpPrep => (Motion::StillJump, Clock::Once(30.0)),
            AttackMode::Jumping => (Motion::Jump, Clock::Once(30.0)),
            AttackMode::Swing { frame, .. } => (Motion::Attack, Clock::Sim(frame)),
            AttackMode::Latched { frame, .. } => (Motion::Kuttuku, Clock::Sim(frame)),
        },
        // `PikiFlickState`: JHIT flying, JKOKE landing (held while lying), GETUP.
        PikiState::Knocked { phase, .. } => match phase {
            libpikmin::KnockPhase::Flying { frame } => (Motion::JHit, Clock::Sim(frame)),
            libpikmin::KnockPhase::Landing { frame } => (Motion::JKoke, Clock::Sim(frame)),
            libpikmin::KnockPhase::Down { .. } => (Motion::JKoke, Clock::Sim(params.anim.jkoke.end)),
            libpikmin::KnockPhase::GettingUp { frame } => (Motion::GetUp, Clock::Sim(frame)),
        },
        PikiState::Carry(c) => match c.phase {
            libpikmin::carry::CarryPhase::Lift { frame, .. } => (Motion::PickLoop, Clock::Sim(frame)),
            // `ActTransport::doLift`: PICK from frame 11, looping while it moves.
            libpikmin::carry::CarryPhase::Move => (Motion::Pick, Clock::LoopFrom(30.0, 11.0)),
            _ => walk,
        },
        PikiState::Pressed { .. } => {
            scale = Vec3::new(1.8, 0.08, 1.8);
            (Motion::Wait, Clock::Loop(0.0))
        }
        PikiState::Dying { frame, end } => {
            // `PikiDyingState::init` picks DEAD, DEAD2 or DEAD3; their lengths tell which one
            // the sim chose.
            let m = if end < 109.0 {
                Motion::Dead
            } else if end < 121.0 {
                Motion::Dead2
            } else {
                Motion::Dead3
            };
            (m, Clock::Sim(frame))
        }
        PikiState::Absorb { frame, .. } => (Motion::Mizunomi, Clock::Sim(frame)),
        // `ActEnter` / `ActExit`: climbing a leg, shrinking into the pod near the top.
        PikiState::Enter { climbing: false, .. } => walk,
        PikiState::Enter { ratio, .. } | PikiState::Exit { ratio, .. } => {
            let k = libpikmin::onion_menu::SHRINK_FROM;
            if ratio > k {
                scale = Vec3::splat(((1.0 - ratio) / (1.0 - k)).clamp(0.0, 1.0));
            }
            (Motion::HNoboru, Clock::Loop(30.0))
        }
        // The Onion's plucker: down a leg as called-out Pikmin come, then Olimar's own pluck
        // (Pikmin and Navi share `pikis/animMgr.bin`, so the Pikmin model plays NUKU as is).
        PikiState::Plucker(pl) => match pl.job {
            PluckerJob::Descend { ratio, .. } => {
                let k = libpikmin::onion_menu::SHRINK_FROM;
                if ratio > k {
                    scale = Vec3::splat(((1.0 - ratio) / (1.0 - k)).clamp(0.0, 1.0));
                }
                (Motion::HNoboru, Clock::Loop(30.0))
            }
            PluckerJob::Walk { .. } => walk,
            PluckerJob::Pull { frame, .. } => (Motion::Nuku, Clock::Sim(frame)),
        },
        PikiState::GrowUp { frame, second, .. } => (
            if second { Motion::GrowUp2 } else { Motion::GrowUp1 },
            Clock::Sim(frame),
        ),
        PikiState::Fading { timer } => {
            fade = (timer / 1.5).clamp(0.0, 1.0);
            scale = Vec3::splat(fade.max(0.05));
            (Motion::Dead, Clock::Sim(1e6))
        }
    };
    Some(PikiLook { motion, clock, scale, fade })
}

/// A captain's motion (naviState.cpp). `throwing`: the throw motion is still playing.
pub fn captain_motion(params: &Params, c: &Captain, throwing: bool) -> (Motion, Clock) {
    let nv = &params.navi;
    let th = [
        nv.asibumi_start_speed,
        nv.walk_start_speed,
        nv.run_start_speed,
        nv.escape_start_speed,
    ];
    let rate = [
        nv.walk_anim_min_frames,
        nv.walk_anim_max_frames,
        nv.run_anim_min_frames,
        nv.run_anim_max_frames,
        nv.escape_anim_min_frames,
    ];
    let speed = c.vel.with_y(0.0).length();
    if c.down {
        (Motion::ODead, Clock::Once(30.0))
    } else if c.damage_frames > 0.0 {
        (Motion::Damage, Clock::Sim(params.anim.damage.end - c.damage_frames))
    } else if c.jump.is_some() {
        if CAPTAIN_PIKMIN_JUMP.load(std::sync::atomic::Ordering::Relaxed) {
            (Motion::RollJmp, Clock::Loop(30.0))
        } else {
            (Motion::OCarry, Clock::LoopFrom(30.0, 10.0))
        }
    } else if let Some(p) = c.punch {
        (Motion::Punch, Clock::Sim(p.frame))
    } else if let Some(s) = c.strike {
        // The dismissal's own motion (`NaviReleaseState`).
        (Motion::Fue, Clock::Sim(s.frame))
    } else if let Some(libpikmin::onion::Pluck {
        phase: PluckPhase::Pull { frame, fast, .. },
        ..
    }) = c.pluck
    {
        (if fast { Motion::NukuFast } else { Motion::Nuku }, Clock::Sim(frame))
    } else if throwing {
        (Motion::Throw, Clock::Once(30.0))
    } else if matches!(c.throw, ThrowState::Wait { held: Some(_), .. }) {
        (Motion::ThrowWait, Clock::Loop(30.0))
    } else if !matches!(c.whistle, WhistleState::Off) && speed < nv.walk_start_speed {
        (Motion::Fue, Clock::Loop(30.0))
    } else {
        locomotion(speed, th, rate)
    }
}

static CAPTAIN_PIKMIN_JUMP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Host extension: Olimar's jump plays the Pikmin's own (`RollJmp`, the tumble a thrown or jumping
/// Pikmin plays; Pikmin and Olimar share the motion table) instead of the geyser ride tipped
/// upright. Off by default.
pub fn set_captain_pikmin_jump(on: bool) {
    CAPTAIN_PIKMIN_JUMP.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// How far Olimar tips forward while jumping. His jump reuses the geyser ride (`OCarry`),
/// whose loop lies him on his back with his head behind him; tipped this far he rides it
/// feet first instead of lying in the air.
pub const JUMP_TILT_DEGREES: f32 = 75.0;

/// The captain's model root, in his own frame (multiply under his position and facing): an
/// identity, except while he jumps, when [`JUMP_TILT_DEGREES`] stands the geyser pose up.
/// It pivots at the height his body lies at, lifted so his feet stay above his origin.
pub fn captain_root(params: &Params, c: &Captain) -> glam::Mat4 {
    use glam::Mat4;
    if captain_motion(params, c, false).0 != Motion::OCarry {
        return Mat4::IDENTITY;
    }
    let pivot = Vec3::new(0.0, 3.0, 0.0);
    Mat4::from_translation(pivot + Vec3::Y * 4.0)
        * Mat4::from_rotation_x(JUMP_TILT_DEGREES.to_radians())
        * Mat4::from_translation(-pivot)
}

/// An Onion's motion (`GoalItem`, goalItem.cpp): its AnimMgr index and clock, or `None`
/// while it's away. `GoalItem::mOnionColour` order (blue, red, yellow) picks the flight
/// motions.
pub fn onion_motion(o: &Onion) -> Option<(usize, Clock)> {
    let colour = o.colors[0].index();
    if let Some((landing, progress)) = o.flight_motion {
        return Some(((if landing { libpikmin::anim::ANIM_GOAL_LAND } else { libpikmin::anim::ANIM_GOAL_TAKEOFF }) + colour,
            Clock::Progress(progress)));
    }
    Some(match o.anim {
        OnionAnim::Idle => (1, Clock::Loop(30.0)),
        OnionAnim::Hover => (libpikmin::anim::ANIM_GOAL_TAKEOFF + colour, Clock::LoopRange(30.0, 200.0, 260.0)),
        OnionAnim::Suction { frame } => (2, Clock::Sim(frame)),
        OnionAnim::Emitting { frame } => (0, Clock::Sim(frame)),
        OnionAnim::TakeOff { frame } => (libpikmin::anim::ANIM_GOAL_TAKEOFF + colour, Clock::Sim(frame)),
        OnionAnim::Landing { frame } => (libpikmin::anim::ANIM_GOAL_LAND + colour, Clock::Sim(frame)),
        OnionAnim::Away => return None,
    })
}

/// `PikiHeadMotion` (PikiHeadItem.h): indices into `objects/pikihead/anims.bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PikiHeadMotion {
    TaneFall = 0,
    TaneUp = 1,
    Hatuga = 2,
    UgWait = 3,
    UgGrow = 4,
    Sioreru = 5,
    TaneDown = 6,
}

/// What's remembered about a sprout to pick its motion.
struct SproutTrack {
    state: std::mem::Discriminant<SproutState>,
    happa: libpikmin::Happa,
    motion: PikiHeadMotion,
}

struct FrameTrack {
    motion: usize,
    tick: u64,
    prev: f32,
    cur: f32,
}

/// Animation players for every drawn object, and the little state the sim doesn't keep.
#[derive(Default)]
pub struct Animator {
    pub players: HashMap<u64, Player>,
    sprouts: HashMap<SproutId, SproutTrack>,
    throw_timer: HashMap<CaptainId, f32>,
    render: Option<(u64, f32)>,
    frames: HashMap<u64, FrameTrack>,
    /// Sprout animation sound keys hit this frame: (parent Onion, value, position). Drained by
    /// the caller.
    pub sprout_sounds: Vec<(Option<OnionId>, u8, Vec3)>,
}

impl Animator {
    /// The same point between sim ticks as the body's position and facing. Standalone
    /// animator callers retain the original clocks unless they opt in here.
    pub fn render_clock(&mut self, tick: u64, alpha: f32) {
        self.render = Some((tick, alpha.clamp(0.0, 1.0)));
    }

    fn clock(&mut self, key: u64, motion: usize, clock: Clock) -> Clock {
        let Some((tick, alpha)) = self.render else { return clock; };
        let value = match clock {
            Clock::Sim(f) => f,
            _ => { self.frames.remove(&key); return clock; }
        };
        let track = self.frames.entry(key).or_insert(FrameTrack {
            motion, tick, prev: value, cur: value,
        });
        // A new clip or a counter wrapping/restarting must not blend back across its end.
        if track.motion != motion || value < track.cur {
            *track = FrameTrack { motion, tick, prev: value, cur: value };
        } else if track.tick != tick {
            track.prev = track.cur;
            track.cur = value;
            track.tick = tick;
        }
        let frame = track.prev + (track.cur - track.prev) * alpha;
        Clock::Sim(frame)
    }

    pub fn on_event(&mut self, e: &Event) {
        if let Event::Thrown { captain, .. } = e {
            // The throw motion's length (`NaviThrowState`).
            self.throw_timer.insert(*captain, 9.0 / 30.0);
        }
    }

    /// Once per frame, before posing.
    pub fn begin_frame(&mut self, dt: f32) {
        for t in self.throw_timer.values_mut() {
            *t -= dt;
        }
    }

    pub fn throwing(&self, c: CaptainId) -> bool {
        self.throw_timer.get(&c).is_some_and(|t| *t > 0.0)
    }

    pub fn piki<'a>(&mut self, set: &'a AnimSet, p: &Piki, look: &PikiLook, dt: f32) -> Option<(&'a Anim, f32)> {
        let clock = self.clock(piki_key(p.id), look.motion.anim_index()?, look.clock);
        advance(&mut self.players, piki_key(p.id), set, look.motion, clock, dt)
    }

    pub fn captain<'a>(&mut self, set: &'a AnimSet, params: &Params, c: &Captain, dt: f32) -> Option<(&'a Anim, f32)> {
        let (motion, clock) = captain_motion(params, c, self.throwing(c.id));
        let clock = self.clock(captain_key(c.id), motion.anim_index()?, clock);
        advance(&mut self.players, captain_key(c.id), set, motion, clock, dt)
    }

    /// The Onion at `index`'s pose this frame, plus where its motion was last frame (for its
    /// effect keys). `None` while it's away.
    pub fn onion<'a>(
        &mut self,
        set: &'a AnimSet,
        index: usize,
        o: &Onion,
        dt: f32,
    ) -> Option<(usize, Posed<'a>, Option<f32>)> {
        let (motion_idx, clock) = onion_motion(o)?;
        let key = onion_key(index);
        let clock = self.clock(key, motion_idx, clock);
        let before = self.players.get(&key).filter(|p| p.anim == motion_idx).map(|p| p.frame);
        let anim = advance_index(&mut self.players, key, set, motion_idx, clock, dt);
        Some((motion_idx, anim, before))
    }

    /// A sprout or flying seed: the pikihead item (`PikiHeadItem::refresh`, pikiheadItem.cpp),
    /// its motion picked like `PikiHeadAI` does. Sound keys crossed go to `sprout_sounds`.
    pub fn sprout<'a>(&mut self, set: &'a AnimSet, sp: &Sprout, dt: f32) -> Option<(&'a Anim, f32)> {
        let track = self.sprouts.entry(sp.id).or_insert(SproutTrack {
            state: std::mem::discriminant(&sp.state),
            happa: sp.happa,
            motion: PikiHeadMotion::TaneFall,
        });
        let state = std::mem::discriminant(&sp.state);
        let entered = track.state != state;
        let grew = track.happa != sp.happa;
        track.motion = match sp.state {
            SproutState::Flying => PikiHeadMotion::TaneFall,
            SproutState::Buried { .. } => PikiHeadMotion::TaneDown,
            SproutState::Tane { .. } => PikiHeadMotion::TaneUp,
            SproutState::Ready { .. } if entered => PikiHeadMotion::Hatuga,
            SproutState::Ready { .. } if grew => PikiHeadMotion::UgGrow,
            SproutState::Ready { .. } => track.motion,
            SproutState::Wilted { .. } => PikiHeadMotion::Sioreru,
        };
        track.state = state;
        track.happa = sp.happa;
        let key = sprout_key(sp.id);
        // HATUGA and UGGROW play once, then UGWAIT loops.
        let mut motion = track.motion;
        if matches!(motion, PikiHeadMotion::Hatuga | PikiHeadMotion::UgGrow)
            && self.players.get(&key).is_some_and(|p| {
                p.anim == motion as usize
                    && set
                        .get(motion as usize)
                        .is_some_and(|(i, a)| p.frame >= (*i.keys.last().unwrap_or(&0) as f32).min(a.frames as f32 - 1.0))
            })
        {
            motion = PikiHeadMotion::UgWait;
            track.motion = motion;
        }
        let clock = match motion {
            PikiHeadMotion::TaneFall | PikiHeadMotion::UgWait => Clock::Loop(30.0),
            _ => Clock::Once(30.0),
        };
        let before = self.players.get(&key).copied();
        let anim = advance_index(&mut self.players, key, set, motion as usize, clock, dt);
        // Sound keys crossed this frame (`AICreature::animationKeyUpdated` →
        // `PikiHeadItem::playSound`).
        if let (Some(after), Some((info, _))) = (self.players.get(&key).copied(), set.get(motion as usize)) {
            let start = *info.keys.first().unwrap_or(&0) as f32;
            let end = *info.keys.last().unwrap_or(&0) as f32;
            let ranges: Vec<(f32, f32)> = match before {
                Some(b) if b.anim == after.anim && after.frame >= b.frame => vec![(b.frame, after.frame)],
                Some(b) if b.anim == after.anim => vec![(b.frame, end), (start, after.frame)],
                _ => vec![(start, after.frame + 1e-3)],
            };
            for e in info.event_keys.iter().filter(|e| e.event == 0) {
                let f = e.frame as f32;
                if ranges.iter().any(|&(a, b)| f >= a && f < b) {
                    self.sprout_sounds.push((sp.onion, e.value, sp.pos));
                }
            }
        }
        anim
    }

    /// Forgets sprouts that aren't in `live` any more.
    pub fn retain_sprouts(&mut self, live: &[SproutId]) {
        self.sprouts.retain(|id, _| live.contains(id));
    }

    /// Forgets the players of objects not drawn this frame (by key).
    pub fn retain_players(&mut self, live: &std::collections::HashSet<u64>) {
        self.players.retain(|k, _| live.contains(k));
        self.frames.retain(|k, _| live.contains(k));
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn frame(clock: Clock) -> f32 {
        match clock { Clock::Sim(f) | Clock::Progress(f) => f, _ => panic!("not a sampled clock") }
    }

    #[test]
    fn sim_poses_advance_between_ticks_and_reset_on_clip_changes() {
        let mut animator = Animator::default();
        animator.render_clock(1, 0.0);
        assert_eq!(frame(animator.clock(0, 1, Clock::Sim(4.0))), 4.0);
        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
            animator.render_clock(2, alpha);
            assert_eq!(frame(animator.clock(0, 1, Clock::Sim(6.0))), 4.0 + 2.0 * alpha);
        }
        animator.render_clock(3, 0.5);
        assert_eq!(frame(animator.clock(0, 2, Clock::Sim(1.0))), 1.0);
        animator.render_clock(4, 0.5);
        assert_eq!(frame(animator.clock(0, 2, Clock::Sim(0.0))), 0.0);
        // Host-driven landing/takeoff progress can change without a sim tick.
        assert_eq!(frame(animator.clock(1, 2, Clock::Progress(0.2))), 0.2);
        animator.render_clock(5, 0.5);
        assert_eq!(frame(animator.clock(1, 2, Clock::Progress(0.4))), 0.4);
    }
}
