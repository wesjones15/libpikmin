//! The Watery Blowhog of Pikmin 2 (`Wtank::Obj : Tank::Obj : EnemyBase`, plugProjectNishimuraU/Tank.cpp, TankState.cpp,
//! Wtank.cpp): a hippo-like beast that patrols its territory, turns on the nearest Pikmin or player it sees, and blows a
//! long stream of water bubbles at whatever stands in front of it; it shakes off Pikmin that hang on it and, killed, leaves a
//! corpse. (The Fiery Blowhog, `Ftank`, differs by its fire; Pikmin 1's is already kind 7.)
//!
//! States (`TankStateID`): [`DEAD`], [`WAIT`], [`MOVE`], [`MOVE_TURN`], [`CHASE_TURN`], [`ATTACK`], [`FLICK`]. Registers:
//! `reg[0]` how far the stream has grown (`mAttackTimer`, 0 to 1), `reg[1]` the caution timer, `reg[2]` the walk timer,
//! `reg[3]` the patrol direction (`mTurnGoalDir`); `targets[0]` the target and `target_pos` the patrol goal.
//!
//! Host adaptations: the stream is the same cylinder the Fiery Blowhog's flame is (`Teki::fire`, which hurts the Pikmin
//! and players in it) instead of bubbles that trap Pikmin (`InteractBubble`), with a damage of its own because the disc's
//! `mAttackDamage` for the bubbles is zero; the stream is not cut short by walls (`emitCollideRatio`).

use std::f32::consts::{FRAC_PI_3, TAU};
use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const WAIT: i32 = 1;
pub const MOVE: i32 = 2;
pub const MOVE_TURN: i32 = 3;
pub const CHASE_TURN: i32 = 4;
pub const ATTACK: i32 = 5;
pub const FLICK: i32 = 6;

/// `TankAnimID`.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_MOVE: usize = 1;
pub const ANIM_FLICK: usize = 2;
pub const ANIM_ATTACK: usize = 3;
pub const ANIM_TURN: usize = 4;
pub const ANIM_WAIT: usize = 5;

const GROWTH: usize = 0;
const CAUTION: usize = 1;
const WALK: usize = 2;
const TURN_DIR: usize = 3;
/// The stream's damage as the host's cylinder hurts.
const STREAM_DAMAGE: f32 = 40.0;
const SHAKE_BLOWS: usize = 1;
const SHAKE_KNOCKBACK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
const ALERT: usize = 29;

fn view_angle(t: &Teki) -> f32 {
    if t.reg[CAUTION] < t.f(ALERT) { 180.0 } else { t.f(fp::VIEW_ANGLE) }
}

/// `EnemyFunc::getNearestPikminOrNavi(view, sight)`.
fn nearest(t: &Teki, w: &World) -> Option<Cand> {
    let (sight, half) = (t.f(fp::SIGHT_RADIUS), (view_angle(t) * 0.5).to_radians());
    let me = t.pos;
    t.closest(w, |c| dist_xz(me, c.pos) <= sight).filter(|c| t.calc_target_angle(c.pos) <= half)
}

/// `Obj::isAttackable(false)`: something stands in the stream's reach in front of it.
fn attackable(t: &Teki, w: &World) -> Option<Cand> {
    let dir = t.forward();
    let (range, radius) = (t.f(fp::MAX_ATTACK_RANGE), t.f(fp::ATTACK_RADIUS));
    t.candidates(w).into_iter().find(|c| {
        let d = c.pos - t.pos;
        let along = dir.dot(d);
        let side = (d - dir * along).with_y(0.0).length();
        d.y.abs() < radius && side < radius && along > 0.0 && along < range
    })
}

fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

fn dead(t: &mut Teki) -> bool {
    hurt(t);
    if t.hp <= 0.0 {
        t.next_override = Some(DEAD);
        return true;
    }
    false
}

fn flicking(t: &Teki) -> bool {
    t.damage_count > t.i(SHAKE_BLOWS) as f32
}

/// What the state that finishes a motion does next: dead, flick, attack, chase the nearest, or `otherwise`.
fn decide(t: &mut Teki, w: &mut World, otherwise: i32) {
    t.next_override = Some(if t.hp <= 0.0 {
        DEAD
    } else if flicking(t) {
        FLICK
    } else if attackable(t, w).is_some() {
        ATTACK
    } else if let Some(c) = nearest(t, w) {
        t.targets[0] = Some(c.target);
        t.reg[CAUTION] = 0.0;
        CHASE_TURN
    } else {
        otherwise
    });
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(7, WAIT);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.reg[CAUTION] = t.f(ALERT);
        t.set_option(opt::DAMAGE_COUNTABLE);
        t.target_pos = t.pos;
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.fire = None;
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

    // StateWait: stands, then looks about for something to go for, or sets out on its patrol.
    s.set(
        WAIT,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.targets[0] = None;
                t.start_motion(ANIM_WAIT);
            })
            .on_act(|t, w| {
                t.reg[CAUTION] += w.dt;
                if dead(t) {
                    return true;
                }
                t.manual_speed = (flicking(t) || attackable(t, w).is_some()).then_some(60.0);
                if t.animation_finished() {
                    t.manual_speed = None;
                    decide(t, w, MOVE_TURN);
                    if t.next_override == Some(MOVE_TURN) && w.rng.f32() < 0.2 {
                        t.next_override = Some(WAIT);
                    }
                    return true;
                }
                false
            })
            .on_finish(|t, _| t.manual_speed = None)
            .build()],
    );

    // StateMove: walks to the patrol goal for up to three seconds.
    s.set(
        MOVE,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[WALK] = 0.0;
                t.targets[0] = None;
                t.start_motion(ANIM_MOVE);
                t.manual_speed = Some(60.0);
            })
            .on_act(|t, w| {
                t.reg[CAUTION] += w.dt;
                let goal = t.target_pos;
                if dist_xz(t.pos, goal) > 50.0 && t.reg[WALK] < 3.0 {
                    turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    set_target_speed(t, t.f(fp::MOVE_SPEED));
                } else {
                    t.target_vel = Vec3::ZERO;
                    t.finish_motion();
                }
                if dead(t) {
                    return true;
                }
                if flicking(t) || attackable(t, w).is_some() {
                    t.target_vel = Vec3::ZERO;
                    t.finish_motion();
                } else if let Some(c) = nearest(t, w) {
                    t.targets[0] = Some(c.target);
                    t.reg[CAUTION] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.finish_motion();
                }
                t.reg[WALK] += w.dt;
                if t.animation_finished() {
                    decide(t, w, WAIT);
                    return true;
                }
                false
            })
            .on_finish(|t, _| t.manual_speed = None)
            .build()],
    );

    // StateMoveTurn: turns toward the next patrol point, a sixth of a circle round its territory.
    s.set(
        MOVE_TURN,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.reg[TURN_DIR] = (t.reg[TURN_DIR] + FRAC_PI_3) % TAU;
                let r = t.f(fp::TERRITORY);
                t.target_pos = t.nest + Vec3::new(r * t.reg[TURN_DIR].sin(), 0.0, r * t.reg[TURN_DIR].cos());
                t.targets[0] = None;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_TURN);
            })
            .on_act(|t, w| {
                t.reg[CAUTION] += w.dt;
                let goal = t.target_pos;
                let left = turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                if dead(t) {
                    return true;
                }
                if flicking(t) || attackable(t, w).is_some() || left.abs() <= 10.0f32.to_radians() {
                    t.finish_motion();
                    t.manual_speed = Some(60.0);
                } else if nearest(t, w).is_some() {
                    t.finish_motion();
                }
                if t.animation_finished() {
                    t.manual_speed = None;
                    decide(t, w, MOVE);
                    return true;
                }
                false
            })
            .on_finish(|t, _| t.manual_speed = None)
            .build()],
    );

    // StateChaseTurn: turns to face its target.
    s.set(
        CHASE_TURN,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_TURN);
            })
            .on_act(|t, w| {
                t.reg[CAUTION] += w.dt;
                if dead(t) {
                    return true;
                }
                if flicking(t) || attackable(t, w).is_some() {
                    t.finish_motion();
                    t.manual_speed = Some(60.0);
                }
                if let Some(c) = t.target(0, w) {
                    t.reg[CAUTION] = 0.0;
                    let left = turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    let out = dist_xz(t.pos, c.pos) > t.f(fp::SIGHT_RADIUS) || left.abs() > (view_angle(t) * 0.5).to_radians() + 1.0;
                    if out {
                        t.targets[0] = None;
                        t.finish_motion();
                    }
                } else {
                    let goal = t.target_pos;
                    let left = turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    if left.abs() <= 10.0f32.to_radians() {
                        t.finish_motion();
                    }
                }
                if t.animation_finished() {
                    t.manual_speed = None;
                    decide(t, w, WAIT);
                    return true;
                }
                false
            })
            .on_finish(|t, _| t.manual_speed = None)
            .build()],
    );

    // StateAttack: breathes in, then blows the stream, which grows out to its full reach.
    s.set(
        ATTACK,
        vec![Action::new(WAIT)
            .on_start(|t, w| {
                t.reg[GROWTH] = 0.0;
                t.reg[CAUTION] = 0.0;
                t.flags[0] = false;
                t.targets[0] = None;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_ATTACK);
                w.out.push(Out::Sound { name: "WATERTANK_BREATH", pos: t.pos });
            })
            .on_act(|t, w| {
                if dead(t) {
                    return true;
                }
                if t.flags[0] {
                    t.reg[GROWTH] = (t.reg[GROWTH] + 2.0 * w.dt).min(1.0);
                    let mouth = t.pos + Vec3::Y * 12.0 + t.forward() * (t.coll_radius * 0.5);
                    t.fire = Some(Fire { from: mouth, dir: t.forward(), length: t.f(fp::MAX_ATTACK_RANGE) * t.reg[GROWTH], radius: t.f(fp::ATTACK_RADIUS), damage: STREAM_DAMAGE });
                    if t.timer_elapsed(5) {
                        t.timers[5] = 1.0;
                        w.out.push(Out::Sound { name: "WATERTANK_SHOT", pos: t.pos });
                    }
                }
                if t.key(key::ACTION0) {
                    t.flags[0] = true;
                }
                if t.animation_finished() {
                    t.fire = None;
                    t.flags[0] = false;
                    t.next_override = Some(if t.hp <= 0.0 {
                        DEAD
                    } else if flicking(t) {
                        FLICK
                    } else if let Some(c) = nearest(t, w) {
                        t.targets[0] = Some(c.target);
                        CHASE_TURN
                    } else {
                        WAIT
                    });
                    return true;
                }
                false
            })
            .on_finish(|t, _| {
                t.fire = None;
                t.flags[0] = false;
            })
            .build()],
    );

    // StateFlick: shakes the Pikmin off and knocks back what is near, then attacks.
    s.set(
        FLICK,
        vec![Action::new(ATTACK)
            .on_start(|t, _| {
                t.targets[0] = None;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
            })
            .on_act(|t, w| {
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                if t.key(key::ACTION0) {
                    let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCKBACK), t.f(SHAKE_DAMAGE));
                    w.sim.shake_off(t.creature);
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                    for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                    }
                    t.damage_count = 0.0;
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { ATTACK });
                    return true;
                }
                false
            })
            .build()],
    );
    s
}
