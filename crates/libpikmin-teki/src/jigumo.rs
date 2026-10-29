//! The Hermit Crawmad of Pikmin 2 (`Jigumo::Obj : EnemyBase`, plugProjectMorimuraU/jigumo.cpp, jigumoState.cpp): a spider that lives in a
//! hole in the ground and cannot be hurt while it is in or near it. It waits and dips into its hole ([`WAIT`], [`HIDE`], [`APPEAR`]) until
//! someone comes within its territory, turns to face them ([`SEARCH`]), then lunges ([`ATTACK`], or the short [`SATTACK`] at its feet)
//! and bites. What it catches it carries backwards to its hole ([`CARRY`]) and eats there ([`EAT`]); a miss ([`MISS`]) leaves it walking home
//! slowly ([`RETURN`]). Only while it carries or walks home is it open to damage: the rest of the time every hit is a `pressCallBack` that
//! does nothing. It shakes off Pikmin clinging to it ([`FLICK`]) and dies where it stands ([`DEAD`]).
//!
//! States (`Jigumo::StateID`): [`WAIT`], [`APPEAR`], [`HIDE`], [`DEAD`], [`ATTACK`], [`MISS`], [`RETURN`], [`CARRY`], [`FLICK`], [`EAT`],
//! [`SEARCH`], [`SATTACK`], [`SMISS`]. Registers: `reg[0]` the hiding or attack timer, `reg[1..3]` where the dive began, `reg[3]`/`reg[4]` the
//! walk-pause timer and its length, `reg[5]` its scale; `flags[0]` it is reversing (carries), `[1]` the lunge or bite is live, `[2]` the
//! bite landed, `[3]` it still turns to its target; `count[0]` the clip Search switches to, `[1]` it is pausing on the way home, `[2]` the
//! swallow key has passed; `action_state` is `mNextState`, `target_pos` `mGoalPosition`.
//!
//! Host adaptations: the hole (`Nest::Obj`, the house model) is not drawn; the head's invulnerability (a hit nearer the head than the body
//! is ignored) is left out, there is one hurt sphere; the zig-zag of the walk home (`mIsGradualTurnActive`), the climbing of walls and the
//! unstick nudge need a map and are left out; the lunge catches whatever is at its mouth whatever its height.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const WAIT: i32 = 0;
pub const APPEAR: i32 = 1;
pub const HIDE: i32 = 2;
pub const DEAD: i32 = 3;
pub const ATTACK: i32 = 4;
pub const MISS: i32 = 5;
pub const RETURN: i32 = 6;
pub const CARRY: i32 = 7;
pub const FLICK: i32 = 8;
pub const EAT: i32 = 9;
pub const SEARCH: i32 = 10;
pub const SATTACK: i32 = 11;
pub const SMISS: i32 = 12;

/// `Jigumo::AnimID`, which is the order of `enemyanimmgr.txt`.
pub const ANIM_APPEAR: usize = 0;
pub const ANIM_ATTACK: usize = 1;
pub const ANIM_BACK_RUN: usize = 2;
pub const ANIM_BACK_WAIT: usize = 3;
pub const ANIM_DEAD: usize = 4;
pub const ANIM_EAT: usize = 5;
pub const ANIM_FLICK: usize = 6;
pub const ANIM_HIDE: usize = 7;
pub const ANIM_RDIVE: usize = 8;
pub const ANIM_RFLICK: usize = 9;
pub const ANIM_RUN_AWAY: usize = 10;
pub const ANIM_SATTACK: usize = 11;
pub const ANIM_SMISS: usize = 12;
pub const ANIM_TURN: usize = 13;
pub const ANIM_WAIT: usize = 15;
pub const ANIM_MISS: usize = 16;

// Registers.
const TIMER: usize = 0;
const DIVE_X: usize = 1;
const DIVE_Z: usize = 2;
const PAUSE_T: usize = 3;
const PAUSE_LEN: usize = 4;
const SCALE: usize = 5;

// The general parms the strategies read beyond `fp::`.
const SEARCH_DISTANCE: usize = 14;
const SEARCH_ANGLE: usize = 15;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
// Its own block (`ProperParms`): carry and return speeds, the scale range, the hiding time.
const CARRY_SPEED: usize = fp::own::BASE + 1;
const RETURN_SPEED: usize = fp::own::BASE + 2;
const MIN_SCALE: usize = fp::own::BASE + 3;
const MAX_SCALE: usize = fp::own::BASE + 4;
const HIDING_TIME: usize = fp::own::BASE + 1;

/// `Parms::mSAttackActiveFrame`.
const SATTACK_ACTIVE_FRAME: f32 = 13.0;
/// `Parms::mMaxPauseTime` (frames) and `mPauseSpeedModifier`.
const MAX_PAUSE: f32 = 35.0;
const PAUSE_SPEED: f32 = 0.15;
/// How far in front of it the mouth is, before scaling.
const MOUTH_REACH: f32 = 20.0;
const TICK: f32 = 1.0 / 30.0;

fn scale(t: &Teki) -> f32 {
    if t.reg[SCALE] > 0.0 { t.reg[SCALE] } else { 1.0 }
}

/// `getNearestPikiOrNavi`: the nearest player or Pikmin within `radius` and a cone `angle` degrees wide.
fn nearest(t: &Teki, w: &World, angle: f32, radius: f32) -> Option<Cand> {
    let me = t.pos;
    let half = (angle * 0.5).to_radians();
    t.closest(w, |c| dist_xz(me, c.pos) <= radius && (angle >= 360.0 || t.calc_target_angle(c.pos) <= half))
}

fn stuck(t: &Teki, w: &World) -> i32 {
    w.sim.pikis().iter().filter(|p| p.state.latched_to() == Some(t.creature)).count() as i32
}

/// `EnemyFunc::isStartFlick`: enough blows for the number clinging (the disc's `ip01`..`ip07`).
fn start_flick(t: &Teki, w: &World) -> bool {
    let n = stuck(t, w);
    let blow = if n < t.i(2) {
        1
    } else if n < t.i(4) {
        3
    } else if n < t.i(6) {
        5
    } else {
        7
    };
    t.damage_count > t.i(blow) as f32
}

/// The hurt-and-dead check every state shares: a blow takes health, none left is `Dead`.
fn dead_check() -> Act {
    Action::new(DEAD)
        .on_act(|t, _| {
            if t.stored_damage > 0.0 {
                t.make_damaged();
            }
            t.hp <= 0.0
        })
        .build()
}

/// Something at the mouth is swallowed (`EnemyFunc::eatPikmin`): one mouth slot.
fn bite(t: &mut Teki, w: &mut World) -> bool {
    let s = scale(t);
    let mouth = t.pos + t.forward() * (MOUTH_REACH * s);
    w.sim.swallow_pikis(t.creature, 0, mouth, (18.0 * s).max(25.0) + 12.0, 1) > 0
}

/// `EnemyFunc::attackNavi` for a player in front of it.
fn hit_navis(t: &Teki, w: &mut World, radius: f32) -> bool {
    let (half, damage) = (t.f(fp::ATTACK_HIT_ANGLE).to_radians(), t.f(fp::ATTACK_DAMAGE));
    let mut any = false;
    for n in w.navis.iter().filter(|n| n.alive) {
        if dist_xz(t.pos, n.pos) - n.size <= radius && t.calc_target_angle(n.pos) <= half {
            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 0.0 });
            any = true;
        }
    }
    any
}

/// `Obj::revisionAnimPos`: slides from where the dive began to the hole as the clip plays.
fn slide_home(t: &mut Teki, rate: f32) {
    let k = (rate * t.anim.counter).min(1.0);
    let (from_x, from_z) = (t.reg[DIVE_X], t.reg[DIVE_Z]);
    t.pos.x = from_x + (t.nest.x - from_x) * k;
    t.pos.z = from_z + (t.nest.z - from_z) * k;
}

fn turn_around(yaw: f32) -> f32 {
    (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
}

/// `Obj::walkFunc` for the walk home: heads for the hole at `speed`; `backwards` runs it tail first (the carry).
fn walk_home(t: &mut Teki, speed: f32, backwards: bool) {
    let goal = t.target_pos;
    if backwards {
        t.yaw = turn_around(t.yaw);
    }
    turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
    set_target_speed(t, speed);
    if backwards {
        t.yaw = turn_around(t.yaw);
    }
}

fn goal_distance(t: &Teki) -> f32 {
    dist_xz(t.pos, t.target_pos)
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(13, APPEAR);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.count = [0; 4];
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE | opt::INVINCIBLE);
        t.clear_option(opt::ATARI);
    }));

    // StateDead: `deathProcedure`, the clip, then it is gone.
    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                w.sim.spit_out(t.creature);
                t.clear_option(opt::ATARI);
                t.target_vel = Vec3::ZERO;
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(ANIM_DEAD);
            })
            .on_act(|t, w| {
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );

    // StateAppear: underground, the Appear clip held on its first frame, until someone is within its territory.
    s.set(
        APPEAR,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, w| {
                    if t.reg[SCALE] == 0.0 {
                        // `setParameters`: one of five sizes between the smallest and the largest.
                        let (lo, hi) = (t.f(MIN_SCALE), t.f(MAX_SCALE));
                        let step = ((w.rng.f32() * 5.0) as i32).min(4) as f32;
                        t.reg[SCALE] = lo + (hi - lo) / 5.0 * step;
                        t.fade = t.reg[SCALE];
                    }
                    t.target_vel = Vec3::ZERO;
                    t.pos.x = t.nest.x;
                    t.pos.z = t.nest.z;
                    t.set_option(opt::INVINCIBLE);
                    t.clear_option(opt::ATARI | opt::VISIBLE);
                    t.flags[0] = false;
                    t.flags[1] = false;
                    t.reg[TIMER] = 0.0;
                    t.start_motion(ANIM_APPEAR);
                    t.manual_speed = Some(0.0);
                })
                .on_act(|t, w| {
                    t.reg[TIMER] += w.dt;
                    if t.reg[TIMER] <= t.i(HIDING_TIME) as f32 * TICK {
                        return false;
                    }
                    if t.manual_speed.is_some() {
                        let (me, terr) = (t.pos, t.f(fp::TERRITORY));
                        if let Some(c) = t.closest(w, |c| dist_xz(me, c.pos) <= terr) {
                            t.manual_speed = None;
                            t.set_option(opt::VISIBLE);
                            t.targets[0] = Some(c.target);
                            // `turnToTarget(target, 1, 360)`: all the way round at once.
                            turn_to_target(t, c.pos, 1.0, 360.0);
                        }
                        return false;
                    }
                    t.animation_finished()
                })
                .on_finish(|t, _| {
                    t.manual_speed = None;
                    t.set_option(opt::VISIBLE);
                })
                .build(),
        ],
    );

    // StateWait: in the hole's mouth looking about; anyone in sight is searched for, otherwise it dips and comes up again.
    s.set(
        WAIT,
        vec![
            dead_check(),
            Action::new(SEARCH)
                .on_start(|t, _| {
                    t.start_motion(ANIM_WAIT);
                    t.pos.x = t.nest.x;
                    t.pos.z = t.nest.z;
                    t.targets[0] = None;
                    t.action_state = -1;
                    t.set_option(opt::INVINCIBLE);
                    t.clear_option(opt::ATARI);
                })
                .on_act(|t, w| {
                    if t.action_state < 0 {
                        match nearest(t, w, t.f(fp::VIEW_ANGLE), t.f(fp::SIGHT_RADIUS)) {
                            Some(c) => {
                                t.targets[0] = Some(c.target);
                                t.target_pos = c.pos;
                                t.action_state = SEARCH;
                            }
                            None => {
                                t.flags[0] = true;
                                t.action_state = HIDE;
                            }
                        }
                        t.finish_motion();
                    }
                    if t.animation_finished() {
                        t.next_override = Some(t.action_state);
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateHide: down into the hole (the Hide clip, or RDive after a walk home, sliding to the hole as it goes).
    s.set(
        HIDE,
        vec![
            dead_check(),
            Action::new(APPEAR)
                .on_start(|t, w| {
                    t.start_motion(if t.flags[0] { ANIM_HIDE } else { ANIM_RDIVE });
                    t.reg[DIVE_X] = t.pos.x;
                    t.reg[DIVE_Z] = t.pos.z;
                    t.target_vel = Vec3::ZERO;
                    t.set_option(opt::INVINCIBLE);
                    t.clear_option(opt::ATARI);
                    // `flickStickPikmin` with the disc's chance of 1: whatever clings is thrown off.
                    w.sim.shake_off(t.creature);
                })
                .on_act(|t, _| {
                    if !t.flags[0] {
                        slide_home(t, 0.05);
                    }
                    if t.animation_finished() {
                        t.pos.x = t.nest.x;
                        t.pos.z = t.nest.z;
                        t.yaw = turn_around(t.yaw);
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateSearch: turns to what it saw; the bite starts once it faces it.
    s.set(
        SEARCH,
        vec![
            dead_check(),
            Action::new(ATTACK)
                .on_start(|t, _| {
                    t.start_motion(ANIM_TURN);
                    t.clear_option(opt::ATARI);
                    t.targets[0] = None;
                    t.count[0] = -1;
                })
                .on_act(|t, w| {
                    let now = t.current_motion();
                    let (turn, max_turn) = (t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    if let Some(c) = nearest(t, w, t.f(SEARCH_ANGLE), t.f(SEARCH_DISTANCE)) {
                        t.targets[0] = Some(c.target);
                        if t.calc_target_angle(c.pos) < 0.01 {
                            t.finish_motion();
                            t.count[0] = -1;
                            t.target_pos = c.pos;
                        } else if now == ANIM_WAIT as i32 {
                            t.finish_motion();
                            t.count[0] = ANIM_TURN as i32;
                        } else {
                            turn_to_target(t, c.pos, turn, max_turn);
                        }
                    } else if let Some(c) = nearest(t, w, t.f(fp::VIEW_ANGLE), t.f(fp::SIGHT_RADIUS)) {
                        t.targets[0] = Some(c.target);
                        if t.calc_target_angle(c.pos) < 0.1 {
                            if now == ANIM_TURN as i32 {
                                t.count[0] = ANIM_WAIT as i32;
                                t.finish_motion();
                            }
                        } else if now == ANIM_WAIT as i32 {
                            t.count[0] = ANIM_TURN as i32;
                            t.finish_motion();
                        } else {
                            turn_to_target(t, c.pos, turn, max_turn);
                        }
                    } else {
                        t.next_override = Some(WAIT);
                        return true;
                    }
                    if t.animation_finished() {
                        if t.count[0] < 0 {
                            let reach = t.f(fp::ATTACK_RADIUS) * scale(t);
                            t.next_override = Some(if dist_xz(t.pos, t.target_pos) < reach { SATTACK } else { ATTACK });
                            return true;
                        }
                        let m = t.count[0] as usize;
                        t.start_motion(m);
                    }
                    false
                })
                .build(),
        ],
    );

    // StateAttack: the lunge. It keeps turning until the clip's key, then runs at where the target was and bites what it meets.
    s.set(
        ATTACK,
        vec![
            dead_check(),
            Action::new(MISS)
                .on_start(|t, _| {
                    t.start_motion(ANIM_ATTACK);
                    t.action_state = MISS;
                    t.flags = [false, false, false, true];
                })
                .on_act(|t, w| {
                    if t.flags[3]
                        && let Some(c) = nearest(t, w, t.f(SEARCH_ANGLE), t.f(SEARCH_DISTANCE))
                    {
                        turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                        t.target_pos = c.pos;
                    }
                    if t.key(key::ACTION0) {
                        t.flags[1] = true;
                        t.flags[3] = false;
                        t.set_option(opt::ATARI);
                        w.out.push(Out::Sound { name: if t.in_water { "JIGUMO_ATTACK_WATER" } else { "JIGUMO_ATTACK_SOIL" }, pos: t.pos });
                    } else if t.animation_finished() {
                        t.flags[1] = false;
                        t.target_vel = Vec3::ZERO;
                        t.target_pos = t.nest;
                        t.next_override = Some(t.action_state);
                        return true;
                    }
                    if t.flags[1] {
                        if !t.flags[2] && hit_navis(t, w, t.f(fp::ATTACK_RADIUS)) {
                            t.flags[2] = true;
                        }
                        set_target_speed(t, t.f(fp::MOVE_SPEED));
                        if bite(t, w) {
                            t.action_state = CARRY;
                            t.flags[0] = true;
                            t.flags[1] = false;
                            t.target_vel = Vec3::ZERO;
                        }
                        if dist_xz(t.pos, t.target_pos) < 10.0 {
                            t.flags[1] = false;
                            t.target_vel = Vec3::ZERO;
                        }
                    }
                    false
                })
                .on_finish(|t, w| {
                    t.target_vel = Vec3::ZERO;
                    w.out.push(Out::Sound { name: "JIGUMO_ATTACK_V", pos: t.pos });
                })
                .build(),
        ],
    );

    // StateMiss: the turn-about clip, then the walk home.
    s.set(
        MISS,
        vec![
            dead_check(),
            Action::new(RETURN)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_MISS);
                })
                .on_act(|t, _| t.animation_finished())
                .on_finish(|t, _| t.yaw = t.calc_target_direction(t.nest))
                .build(),
        ],
    );

    // StateReturn: home at the return speed, open to blows; it dives when it gets there.
    s.set(
        RETURN,
        vec![
            dead_check(),
            Action::new(FLICK)
                .on_start(|t, _| {
                    t.flags[0] = false;
                    t.target_pos = t.nest;
                    t.clear_option(opt::INVINCIBLE);
                    t.start_motion(ANIM_RUN_AWAY);
                })
                .on_act(|t, w| {
                    walk_home(t, t.f(RETURN_SPEED), false);
                    if start_flick(t, w) {
                        t.action_state = RETURN;
                        return true;
                    }
                    if goal_distance(t) < 20.0 * scale(t) {
                        t.finish_motion();
                    }
                    if t.animation_finished() {
                        t.target_vel = Vec3::ZERO;
                        if stuck(t, w) > 0 {
                            t.action_state = HIDE;
                            return true;
                        }
                        t.next_override = Some(HIDE);
                        return true;
                    }
                    false
                })
                .on_finish(|t, _| t.set_option(opt::INVINCIBLE))
                .build(),
        ],
    );

    // StateCarry: tail first to the hole with the prey in its mouth, now and then pausing, slower the more cling to it.
    s.set(
        CARRY,
        vec![
            dead_check(),
            Action::new(FLICK)
                .on_start(|t, w| {
                    t.target_pos = t.nest;
                    t.clear_option(opt::INVINCIBLE);
                    t.start_motion(ANIM_BACK_RUN);
                    t.count[1] = 0;
                    t.reg[PAUSE_T] = 0.0;
                    t.reg[PAUSE_LEN] = w.rng.f32() * MAX_PAUSE * TICK;
                })
                .on_act(|t, w| {
                    let mut speed = t.f(CARRY_SPEED);
                    t.reg[PAUSE_T] += w.dt;
                    if t.key(key::LOOP_END) && t.reg[PAUSE_T] > t.reg[PAUSE_LEN] {
                        t.count[1] ^= 1;
                        t.start_motion(if t.count[1] == 1 { ANIM_BACK_WAIT } else { ANIM_BACK_RUN });
                        t.reg[PAUSE_LEN] = w.rng.f32() * MAX_PAUSE * TICK;
                        t.reg[PAUSE_T] = 0.0;
                    }
                    if t.count[1] == 1 && t.reg[PAUSE_T] < t.reg[PAUSE_LEN] {
                        speed *= PAUSE_SPEED * (1.0 - t.reg[PAUSE_T] / t.reg[PAUSE_LEN]);
                    }
                    // Pikmin hanging on drag it: a fifth of its speed each, at most four fifths.
                    speed *= 1.0 - (0.2 * stuck(t, w) as f32).min(0.8);
                    walk_home(t, speed, true);
                    if start_flick(t, w) {
                        t.action_state = CARRY;
                        return true;
                    }
                    let limit = 10.0 * scale(t) + 20.0;
                    let d = goal_distance(t);
                    if d < limit * 1.05 {
                        t.finish_motion();
                    }
                    if d < limit {
                        t.target_vel = Vec3::ZERO;
                        if stuck(t, w) > 1 {
                            t.action_state = EAT;
                            return true;
                        }
                        t.next_override = Some(EAT);
                        return true;
                    }
                    false
                })
                .on_finish(|t, _| t.set_option(opt::INVINCIBLE))
                .build(),
        ],
    );

    // StateFlick: shakes whatever clings to it off, then goes on with what it was doing.
    s.set(
        FLICK,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.set_option(opt::INVINCIBLE);
                    t.start_motion(if t.flags[0] { ANIM_FLICK } else { ANIM_RFLICK });
                })
                .on_act(|t, w| {
                    if t.key(key::ACTION0) {
                        let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                        w.sim.shake_off(t.creature);
                        w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                        for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                        }
                        t.damage_count = 0.0;
                    }
                    if t.animation_finished() {
                        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { WAIT });
                        return true;
                    }
                    false
                })
                .on_finish(|t, _| {
                    // Back to what it was doing: still open to blows if that was the carry or the walk home.
                    if matches!(t.action_state, CARRY | RETURN) {
                        t.clear_option(opt::INVINCIBLE);
                    }
                })
                .build(),
        ],
    );

    // StateEat: the dive with the prey; at the clip's swallow key it is eaten.
    s.set(
        EAT,
        vec![
            dead_check(),
            Action::new(HIDE)
                .on_start(|t, _| {
                    t.reg[DIVE_X] = t.pos.x;
                    t.reg[DIVE_Z] = t.pos.z;
                    t.target_vel = Vec3::ZERO;
                    t.count[2] = 0;
                    t.set_option(opt::INVINCIBLE);
                    t.start_motion(ANIM_EAT);
                })
                .on_act(|t, w| {
                    slide_home(t, 0.1);
                    if t.count[2] == 0 && t.anim.counter >= t.key_value(8) {
                        t.count[2] = 1;
                        if w.sim.chew(t.creature) > 0 {
                            w.out.push(Out::Sound { name: "JIGUMO_EAT", pos: t.pos });
                        }
                    }
                    t.animation_finished()
                })
                .on_finish(|t, w| {
                    w.sim.chew(t.creature);
                    t.pos.x = t.nest.x;
                    t.pos.z = t.nest.z;
                })
                .build(),
        ],
    );

    // StateSAttack: the quick bite at its feet. Live from its 13th frame to the clip's second key; the swallow comes at its tenth key.
    s.set(
        SATTACK,
        vec![
            dead_check(),
            Action::new(SEARCH)
                .on_start(|t, _| {
                    t.start_motion(ANIM_SATTACK);
                    t.flags = [false, false, false, false];
                    t.count[2] = 0;
                    t.target_vel = Vec3::ZERO;
                })
                .on_act(|t, w| {
                    if !t.flags[1] && t.count[2] == 0 && t.anim.counter >= SATTACK_ACTIVE_FRAME && t.anim.counter < t.key_value(3) {
                        t.flags[1] = true;
                        t.set_option(opt::ATARI);
                    }
                    if t.flags[1] {
                        if bite(t, w) {
                            t.flags[2] = true;
                        }
                        let reach = 50.0 * scale(t);
                        hit_navis(t, w, reach);
                    }
                    if t.key(key::ACTION0) {
                        w.out.push(Out::Sound { name: if t.in_water { "JIGUMO_ATTACK_WATER" } else { "JIGUMO_ATTACK_SOIL" }, pos: t.pos });
                    }
                    if t.key(key::ACTION1) {
                        t.flags[1] = false;
                        t.clear_option(opt::ATARI);
                        if !t.flags[2] {
                            t.next_override = Some(SMISS);
                            return true;
                        }
                    }
                    if t.count[2] == 0 && t.flags[2] && t.anim.counter >= t.key_value(10) {
                        t.count[2] = 1;
                        w.sim.chew(t.creature);
                    }
                    t.animation_finished()
                })
                .on_finish(|t, w| {
                    t.flags[1] = false;
                    t.clear_option(opt::ATARI);
                    w.sim.chew(t.creature);
                })
                .build(),
        ],
    );

    s.set(
        SMISS,
        vec![
            dead_check(),
            Action::new(SEARCH)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_SMISS);
                })
                .on_act(|t, _| t.animation_finished())
                .build(),
        ],
    );
    s
}
