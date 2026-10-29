//! The Segmented Crawbster of Pikmin 2 (`DangoMushi::Obj : EnemyBase`, plugProjectNishimuraU/DangoMushi.cpp, DangoMushiState.cpp): a
//! boss that waits out of sight until someone comes near, crashes down from above (`Stay`, `Appear`), ambles after what it sees (`Wait`,
//! `Move`) and, when it has a target in reach, curls into a ball and rolls at it (`Attack`), crushing what it rolls over. If it rolls
//! hard into a wall it is thrown on its back (`Turn`), open to blows for the flip time, then rights itself (`Recover`) and swings its
//! arms about to flatten what is near (`Flick`). Killed (`Dead`) it leaves a corpse.
//!
//! States (`DangoMushi::StateID`): [`DEAD`], [`STAY`], [`APPEAR`], [`WAIT`], [`MOVE`], [`ATTACK`], [`TURN`], [`RECOVER`], [`FLICK`]. Registers:
//! `reg[0]` the state's timer, `reg[1]` the shadow's growth while it comes down; `flags[0]` it is rolling, `flags[1]` an arm is swinging;
//! `action_state` the state the clip hands over to (`mNextState`), `target_pos` the point it ambles to.
//!
//! Host adaptations: no blend animation (each clip simply starts); the rocks and the egg that fall when it crashes (`createCrashEnemy`), the
//! boss music and the camera shakes are left out; the wall crash is any wall met at speed (the host has no wall normal); the arm's
//! hit is a sphere in front of it for the swing's key windows (the decomp tests the arm's collision part), its wither a knock-back and damage.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const STAY: i32 = 1;
pub const APPEAR: i32 = 2;
pub const WAIT: i32 = 3;
pub const MOVE: i32 = 4;
pub const ATTACK: i32 = 5;
pub const TURN: i32 = 6;
pub const RECOVER: i32 = 7;
pub const FLICK: i32 = 8;

/// `DangoMushi::AnimID`.
pub const ANIM_FLY: usize = 0;
pub const ANIM_WAIT: usize = 1;
pub const ANIM_MOVE: usize = 2;
pub const ANIM_ATTACK: usize = 3;
pub const ANIM_FLICK: usize = 4;
pub const ANIM_TURN: usize = 5;
pub const ANIM_RECOVER: usize = 6;
pub const ANIM_DEAD: usize = 7;

const TIMER: usize = 0;
const SHADOW: usize = 1;

const HOME_RADIUS: usize = 10;
const PRIVATE_RADIUS: usize = 11;
const SHAKE_KNOCK: usize = 17;
// Its own block (`ProperParms`).
const ROLL_SPEED: usize = fp::own::BASE + 1;
const ROLL_TURN_ACCEL: usize = fp::own::BASE + 2;
const ROLL_TURN_MAX: usize = fp::own::BASE + 3;
const FLIP_TIME: usize = fp::own::BASE + 10;
/// `mFlipTime`'s default, which the disc does not override.
const DEFAULT_FLIP_TIME: f32 = 7.5;
/// The longest a roll lasts (`mStateTimer > 15`).
const ROLL_SECONDS: f32 = 15.0;
/// It rolled fast enough to crash (`speed > 100`).
const CRASH_SPEED: f32 = 100.0;

fn target(t: &Teki, w: &World, angle: f32, radius: f32) -> Option<Cand> {
    let me = t.pos;
    let half = (angle * 0.5).to_radians();
    t.closest(w, |c| dist_xz(me, c.pos) <= radius && (angle >= 360.0 || t.calc_target_angle(c.pos) <= half))
}

/// `isTargetAttackable`: in reach and within the attack cone.
fn attackable(t: &Teki, c: &Cand) -> bool {
    (c.pos - t.pos).length() < t.f(fp::MAX_ATTACK_RANGE) && t.calc_target_angle(c.pos) <= t.f(fp::MAX_ATTACK_ANGLE).to_radians()
}

/// `setRandTarget`: a point in the ring between its home radius and its territory.
fn rand_target(t: &mut Teki, w: &mut World) {
    let (home, territory) = (t.f(HOME_RADIUS), t.f(fp::TERRITORY));
    let r = home + w.rng.f32() * (territory - home).max(0.0);
    let a = w.rng.range(0.0, std::f32::consts::TAU);
    t.target_pos = t.nest + dir_vec(a) * r;
}

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

/// The clip ended: on to the state it chose, or `default`.
fn clip_done(t: &mut Teki, default: i32) -> bool {
    if t.animation_finished() {
        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { default });
        return true;
    }
    false
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(9, STAY);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.action_state = -1;
        t.flags = [false; 4];
        t.set_option(opt::INVINCIBLE);
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.flags[0] = false;
                t.target_vel = Vec3::ZERO;
                t.clear_option(opt::ATARI);
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(ANIM_DEAD);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION1) {
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );

    // StateStay: hidden, held still on the first frame of the fall; once someone is near its shadow grows and it comes down.
    s.set(
        STAY,
        vec![
            dead_check(),
            Action::new(APPEAR)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.set_option(opt::INVINCIBLE);
                    t.clear_option(opt::ATARI | opt::VISIBLE);
                    t.start_motion(ANIM_FLY);
                    t.manual_speed = Some(0.0);
                    t.reg[SHADOW] = 0.0;
                })
                .on_act(|t, w| {
                    if t.reg[SHADOW] > 0.0 {
                        t.reg[SHADOW] += 0.6 * w.dt;
                        return t.reg[SHADOW] >= 1.0;
                    }
                    let (me, r) = (t.pos, t.f(PRIVATE_RADIUS));
                    if t.closest(w, |c| dist_xz(me, c.pos) <= r).is_some() {
                        t.reg[SHADOW] = 0.6 * w.dt;
                        w.out.push(Out::Sound { name: "DANGO_FALL", pos: t.pos });
                    }
                    false
                })
                .on_finish(|t, _| {
                    t.manual_speed = None;
                    t.set_option(opt::VISIBLE | opt::ATARI);
                })
                .build(),
        ],
    );

    // StateAppear: crashes down; on landing it goes for what it sees.
    s.set(
        APPEAR,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_FLY);
                })
                .on_act(|t, w| {
                    if t.key(key::ACTION0) || t.key(key::ACTION2) {
                        w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    if let Some(c) = target(t, w, t.f(fp::VIEW_ANGLE), t.f(fp::SIGHT_RADIUS)) {
                        let attack = attackable(t, &c);
                        if !attack {
                            rand_target(t, w);
                        }
                        t.next_override = Some(if attack { ATTACK } else { MOVE });
                    }
                    true
                })
                .build(),
        ],
    );

    // StateWait: a look about; a target in sight starts the attack or the chase, otherwise it ambles after three seconds.
    s.set(
        WAIT,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, w| {
                    t.action_state = -1;
                    t.reg[TIMER] = 0.0;
                    rand_target(t, w);
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_WAIT);
                })
                .on_act(|t, w| {
                    if t.action_state < 0 {
                        if let Some(c) = target(t, w, t.f(fp::VIEW_ANGLE), t.f(fp::SIGHT_RADIUS)) {
                            t.action_state = if attackable(t, &c) { ATTACK } else { MOVE };
                            t.finish_motion();
                        } else if t.reg[TIMER] > 3.0 {
                            t.action_state = MOVE;
                            t.finish_motion();
                        }
                    }
                    t.reg[TIMER] += w.dt;
                    clip_done(t, MOVE)
                })
                .build(),
        ],
    );

    // StateMove: walks after its target, or to a point of its own; attacks when it can.
    s.set(
        MOVE,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[TIMER] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_MOVE);
                })
                .on_act(|t, w| {
                    if t.action_state < 0 {
                        let (turn, max_turn, speed) = (t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE), t.f(fp::MOVE_SPEED));
                        if let Some(c) = target(t, w, t.f(fp::VIEW_ANGLE), t.f(fp::SIGHT_RADIUS)) {
                            if attackable(t, &c) {
                                t.action_state = ATTACK;
                                t.finish_motion();
                            } else {
                                let toward = t.calc_target_angle(c.pos);
                                turn_to_target(t, c.pos, turn, max_turn);
                                if toward <= t.f(fp::MAX_ATTACK_ANGLE).to_radians() {
                                    set_target_speed(t, speed);
                                } else {
                                    t.target_vel = Vec3::ZERO;
                                }
                            }
                        } else if dist_xz(t.pos, t.target_pos) < 25.0 || t.reg[TIMER] > 10.0 {
                            t.action_state = WAIT;
                            t.finish_motion();
                        } else {
                            let goal = t.target_pos;
                            let toward = t.calc_target_angle(goal);
                            turn_to_target(t, goal, turn, max_turn);
                            if toward <= 30.0f32.to_radians() {
                                set_target_speed(t, speed);
                            } else {
                                t.target_vel = Vec3::ZERO;
                            }
                        }
                    }
                    if t.anim.is_finished() {
                        t.target_vel = Vec3::ZERO;
                    }
                    t.reg[TIMER] += w.dt;
                    clip_done(t, WAIT)
                })
                .on_finish(|t, _| t.target_vel = Vec3::ZERO)
                .build(),
        ],
    );

    // StateAttack: curls up and, at the clip's third key, rolls at its target until the time is up or it hits a wall.
    s.set(
        ATTACK,
        vec![
            dead_check(),
            // `wallCallback`: rolling fast, head on into a wall throws it on its back.
            Action::new(TURN)
                .on_event(|t, _, e| matches!(e, Event::Wall) && t.flags[0] && t.target_vel.length() > CRASH_SPEED)
                .build(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[TIMER] = 0.0;
                    t.flags[0] = false;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_ATTACK);
                })
                .on_act(|t, w| {
                    if t.reg[TIMER] > ROLL_SECONDS && t.action_state < 0 {
                        t.action_state = WAIT;
                        t.finish_motion();
                    }
                    if t.key(key::ACTION2) {
                        t.flags[0] = true;
                    }
                    if t.key(key::LOOP_END) && t.anim.is_finished() {
                        t.flags[0] = false;
                    }
                    if t.flags[0] && t.action_state < 0 {
                        // `rollingMove`: turns slowly towards the nearest player (or Pikmin), and goes at the rolling speed.
                        let goal = t.closest(w, |_| true).map(|c| c.pos);
                        if let Some(goal) = goal {
                            turn_to_target(t, goal, t.f(ROLL_TURN_ACCEL), t.f(ROLL_TURN_MAX));
                        }
                        set_target_speed(t, t.f(ROLL_SPEED));
                        t.reg[TIMER] += w.dt;
                        // Whatever is under it is crushed (`InteractPress` with the attack damage).
                        let damage = t.f(fp::ATTACK_DAMAGE);
                        let body = t.coll_radius.max((t.info.hi.y - t.info.lo.y) * 0.4);
                        w.sim.crush_pikis_in_sphere(t.pos, body + 20.0, damage);
                        if t.timer_elapsed(6) {
                            t.timers[6] = 0.5;
                            for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= body + 20.0) {
                                w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 200.0 });
                            }
                        }
                        w.out.push(Out::Sound { name: "DANGO_ROLL_GROUND", pos: t.pos });
                    } else if t.action_state >= 0 {
                        t.flags[0] = false;
                        t.target_vel = Vec3::ZERO;
                    }
                    clip_done(t, WAIT)
                })
                .on_finish(|t, _| {
                    t.flags[0] = false;
                    t.target_vel = Vec3::ZERO;
                })
                .build(),
        ],
    );
    // StateTurn: thrown on its back for the flip time, open to blows from the loop's start until it rights itself.
    s.set(
        TURN,
        vec![
            dead_check(),
            Action::new(RECOVER)
                .on_start(|t, w| {
                    t.action_state = -1;
                    t.reg[TIMER] = 0.0;
                    t.flags[0] = false;
                    t.target_vel = Vec3::ZERO;
                    t.set_option(opt::INVINCIBLE);
                    t.start_motion(ANIM_TURN);
                    w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
                })
                .on_act(|t, w| {
                    let flip = if t.f(FLIP_TIME) > 0.0 { t.f(FLIP_TIME) } else { DEFAULT_FLIP_TIME };
                    if t.reg[TIMER] > flip && !t.anim.is_finished() {
                        t.finish_motion();
                    }
                    t.reg[TIMER] += w.dt;
                    if t.key(key::LOOP_START) {
                        t.clear_option(opt::INVINCIBLE);
                    }
                    if t.key(key::ACTION1) {
                        t.set_option(opt::INVINCIBLE);
                    }
                    t.animation_finished()
                })
                .on_finish(|t, w| {
                    w.sim.shake_off(t.creature);
                    t.set_option(opt::INVINCIBLE);
                })
                .build(),
        ],
    );

    s.set(
        RECOVER,
        vec![
            dead_check(),
            Action::new(FLICK)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_RECOVER);
                })
                .on_act(|t, _| t.animation_finished())
                .on_finish(|t, _| t.yaw = (t.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU))
                .build(),
        ],
    );

    // StateFlick: three arm swings, each flattening what is in front of it.
    s.set(
        FLICK,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.flags[1] = false;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_FLICK);
                })
                .on_act(|t, w| {
                    if t.key(key::ACTION0) {
                        t.flags[1] = true;
                    }
                    if t.key(key::ACTION1) {
                        t.flags[1] = false;
                    }
                    if t.flags[1] {
                        let (reach, damage, knock) = (t.f(fp::ATTACK_RADIUS), t.f(fp::ATTACK_DAMAGE), t.f(SHAKE_KNOCK));
                        let hand = t.pos + t.forward() * reach;
                        w.sim.flick_pikis_in_sphere(hand, reach, damage, t.pos);
                        if t.timer_elapsed(6) {
                            t.timers[6] = 0.4;
                            for n in w.navis.iter().filter(|n| n.alive && dist_xz(hand, n.pos) - n.size <= reach) {
                                w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: knock.max(300.0) });
                            }
                            w.out.push(Out::Sound { name: "DANGO_ARM_GROUND", pos: hand });
                        }
                    }
                    t.animation_finished()
                })
                .on_finish(|t, _| t.flags[1] = false)
                .build(),
        ],
    );
    s
}
