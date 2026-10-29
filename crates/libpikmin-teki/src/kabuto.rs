//! The Armored Cannon Beetle Larva of Pikmin 2 (`Kabuto::Obj : EnemyBase`, plugProjectNishimuraU/Kabuto.cpp,
//! KabutoState.cpp): an armoured grub that wanders about its home (`Wait`, `Turn`, `Move`), turns on the nearest Pikmin or
//! player it sees and spits a rock at whatever stands in a line ahead of it (`Attack`), shakes off Pikmin that cling to it
//! (`Flick`) and, killed, leaves a corpse. The rock is the rolling boulder of Pikmin 1 (kind 20), shot from its mouth. (The
//! fixed variants, `FixStay` .. `FixFlick` for the Decorated and fixed Cannon Beetles, are placed turrets and not ported.)
//!
//! States (`KabutoStateID`): [`DEAD`], [`WAIT`], [`TURN`], [`MOVE`], [`FLICK`], [`ATTACK`]. Registers: `reg[0]` the state
//! timer, `reg[1]` the alert timer (`mAlertTimer`: a wide view while it is below `mAlertDuration`), `action_state` the state
//! to go to when the clip ends, `flags[0]` walking (the walk clip's loop start), `target_pos` the wander goal.
//!
//! Host adaptations: the stone is spawned with a fixed speed of its own (the `Rock` object's flight is the host's boulder);
//! the larva does not hold itself to the ground (`EB_Constrained`).

use std::f32::consts::TAU;
use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::info::IWAGON;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const WAIT: i32 = 1;
pub const TURN: i32 = 2;
pub const MOVE: i32 = 3;
pub const FLICK: i32 = 4;
pub const ATTACK: i32 = 5;

/// `KabutoAnimID`.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_MOVE: usize = 1;
pub const ANIM_FLICK: usize = 2;
pub const ANIM_ATTACK: usize = 3;
pub const ANIM_PIVOT: usize = 4;
pub const ANIM_WAIT: usize = 5;

const TIMER: usize = 0;
const ALERT_T: usize = 1;
const ALERT_DURATION: usize = 29;
const SHAKE_KNOCKBACK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
const SHAKE_BLOWS: usize = 1;
/// How fast the stone leaves the mouth (the boulder's roll speed).
const STONE_SPEED: f32 = 250.0;

fn view_angle(t: &Teki) -> f32 {
    if t.reg[ALERT_T] < t.f(ALERT_DURATION) { 180.0 } else { t.f(fp::VIEW_ANGLE) }
}

/// `Obj::getSearchedTarget`.
fn searched(t: &mut Teki, w: &World) -> Option<Cand> {
    let (sight, half) = (t.f(fp::SIGHT_RADIUS), (view_angle(t) * 0.5).to_radians());
    let me = t.pos;
    let c = t.closest(w, |c| dist_xz(me, c.pos) <= sight).filter(|c| t.calc_target_angle(c.pos) <= half);
    if c.is_some() {
        t.reg[ALERT_T] = 0.0;
    }
    c
}

/// `Obj::isAttackableTarget`: someone in a narrow lane ahead of it.
fn attackable(t: &Teki, w: &World) -> bool {
    let dir = t.forward();
    let (sight, fov) = (t.f(fp::SIGHT_RADIUS), t.f(25));
    t.candidates(w).iter().any(|c| {
        let d = c.pos - t.pos;
        let along = dir.dot(d);
        let side = (d - dir * along).with_y(0.0).length();
        d.y.abs() < fov && side < 15.0 && along > 15.0 && along < sight
    })
}

fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

fn flicking(t: &Teki) -> bool {
    t.damage_count > t.i(SHAKE_BLOWS) as f32
}

/// The common start of a state's tick: dead, or about to shake the Pikmin off.
fn check(t: &mut Teki) -> bool {
    hurt(t);
    if t.hp <= 0.0 {
        t.next_override = Some(DEAD);
        return true;
    }
    false
}

fn set_rand_target(t: &mut Teki, w: &mut World) {
    let r = t.f(fp::TERRITORY) * w.rng.f32();
    let a = TAU * w.rng.f32();
    t.target_pos = t.nest + Vec3::new(r * a.sin(), 0.0, r * a.cos());
}

fn change_when_done(t: &mut Teki) -> bool {
    if t.animation_finished() {
        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { WAIT });
        return true;
    }
    false
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(6, WAIT);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.reg[ALERT_T] = t.f(ALERT_DURATION);
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE);
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
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

    // StateWait: stands, picks a spot to go to, and after three seconds or on seeing someone turns.
    s.set(
        WAIT,
        vec![Action::new(WAIT)
            .on_start(|t, w| {
                t.reg[TIMER] = 0.0;
                set_rand_target(t, w);
                t.action_state = -1;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_WAIT);
            })
            .on_act(|t, w| {
                if check(t) {
                    return true;
                }
                if t.action_state < 0 {
                    if flicking(t) {
                        t.action_state = FLICK;
                        t.finish_motion();
                    } else if t.reg[TIMER] > 3.0 || searched(t, w).is_some() {
                        t.action_state = TURN;
                        t.finish_motion();
                    }
                }
                t.reg[TIMER] += w.dt;
                t.reg[ALERT_T] += w.dt;
                change_when_done(t)
            })
            .build()],
    );

    // StateTurn: pivots toward whoever it sees (and attacks if he is in line), else toward its wander goal.
    s.set(
        TURN,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.action_state = -1;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_PIVOT);
            })
            .on_act(|t, w| {
                if check(t) {
                    return true;
                }
                if t.action_state < 0 {
                    if flicking(t) {
                        t.action_state = FLICK;
                        t.finish_motion();
                    } else if let Some(c) = searched(t, w) {
                        turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                        if attackable(t, w) {
                            t.action_state = ATTACK;
                            t.finish_motion();
                        }
                    } else {
                        let goal = t.target_pos;
                        let left = turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                        if left.abs() <= std::f32::consts::PI / 6.0 {
                            t.action_state = MOVE;
                            t.finish_motion();
                        }
                    }
                }
                t.reg[ALERT_T] += w.dt;
                change_when_done(t)
            })
            .build()],
    );

    // StateMove: walks (while the walk clip's legs are down) to the goal; sees someone: turns or attacks.
    s.set(
        MOVE,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.flags[0] = false;
                t.action_state = -1;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_MOVE);
            })
            .on_act(|t, w| {
                if check(t) {
                    return true;
                }
                if t.action_state < 0 {
                    if flicking(t) {
                        t.action_state = FLICK;
                        t.finish_motion();
                    } else if searched(t, w).is_some() {
                        t.action_state = if attackable(t, w) { ATTACK } else { TURN };
                        t.finish_motion();
                    } else {
                        let goal = t.target_pos;
                        if t.reg[TIMER] > 6.0 || dist_xz(t.pos, goal) < 25.0 {
                            t.action_state = WAIT;
                            t.finish_motion();
                        } else {
                            let left = turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                            if left.abs() <= std::f32::consts::PI / 6.0 {
                                if t.flags[0] {
                                    set_target_speed(t, t.f(fp::MOVE_SPEED));
                                }
                            } else {
                                t.action_state = TURN;
                                t.finish_motion();
                            }
                        }
                    }
                }
                if t.anim.is_finished() {
                    t.flags[0] = false;
                    t.target_vel = Vec3::ZERO;
                }
                t.reg[TIMER] += w.dt;
                t.reg[ALERT_T] += w.dt;
                if t.key(key::LOOP_START) {
                    t.flags[0] = true;
                }
                change_when_done(t)
            })
            .build()],
    );

    // StateFlick: shakes the Pikmin and everyone near it off.
    s.set(
        FLICK,
        vec![Action::new(ATTACK)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.action_state = -1;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION0) {
                    let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCKBACK), t.f(SHAKE_DAMAGE));
                    w.sim.shake_off(t.creature);
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                    for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                    }
                    t.damage_count = 0.0;
                }
                hurt(t);
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { ATTACK });
                    return true;
                }
                false
            })
            .build()],
    );

    // StateAttack: at the clip's key the rock leaves its mouth.
    s.set(
        ATTACK,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.reg[ALERT_T] = 0.0;
                t.action_state = -1;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_ATTACK);
            })
            .on_act(|t, w| {
                if check(t) {
                    return true;
                }
                if t.key(key::ACTION0) {
                    let dir = t.forward();
                    w.out.push(Out::Spawn {
                        kind: IWAGON,
                        pos: t.pos + dir * (t.coll_radius + 20.0) + Vec3::Y * 25.0,
                        yaw: t.yaw,
                        parent: t.id,
                        velocity: dir * STONE_SPEED,
                    });
                    w.out.push(Out::Shake { pos: t.pos, strength: 1.5 });
                }
                if t.animation_finished() {
                    t.next_override = Some(if flicking(t) {
                        FLICK
                    } else if searched(t, w).is_some() {
                        TURN
                    } else {
                        WAIT
                    });
                    return true;
                }
                false
            })
            .build()],
    );
    s
}
