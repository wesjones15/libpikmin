//! The Empress Bulblax of Pikmin 2 (`Queen::Obj : EnemyBase`, plugProjectNishimuraU/Queen.cpp, QueenState.cpp): a huge sleeping
//! Bulborb that rests in her chamber and lays Bulborb Larvae (`Born`). Hurt, she wakes and tries to shake off whoever clings to her
//! (`Damage`, `Flick`), then rolls from side to side across her territory (`Rolling`), crushing everything and crashing into the
//! walls at either end; she rests again between rolls (`Wait`, `Sleep`). Killed (`Dead`) she leaves a corpse.
//!
//! States (`QueenStateID`): [`DEAD`], [`SLEEP`], [`WAIT`], [`DAMAGE`], [`FLICK`], [`ROLLING`], [`BORN`]. Registers: `reg[0]` the wait timer,
//! `reg[1]` the hits she had when the state began (`mPrevHitNum`: more hits since is `isHitCounterUp`), `reg[2]` the birth timer;
//! `flags[0]` she is rolling now, `flags[1]` the roll goes left, `action_state` the state the clip hands over to.
//!
//! Host adaptations: the decomp lets her keep 25 to 50 larvae alive (`mMinBirths`, `mMaxBirths`); here a fifth of that, because a
//! RoR2 stage is not a cave. The crash's falling rocks (`createCrashFallRock`) and her boss music are left out. The wall-crash
//! is her territory's edge (`mTerritoryRadius - 50`), not the map's wall.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{BULBORB_LARVA, fp};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const SLEEP: i32 = 1;
pub const WAIT: i32 = 2;
pub const DAMAGE: i32 = 3;
pub const FLICK: i32 = 4;
pub const ROLLING: i32 = 5;
pub const BORN: i32 = 6;

/// `QueenAnimID`.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_SLEEP: usize = 1;
pub const ANIM_WAIT: usize = 2;
pub const ANIM_DAMAGE: usize = 3;
pub const ANIM_FLICK: usize = 4;
pub const ANIM_ROLL_LEFT: usize = 5;
pub const ANIM_ROLL_RIGHT: usize = 6;
pub const ANIM_BORN: usize = 7;

const WAIT_T: usize = 0;
const PREV_HITS: usize = 1;
const BIRTH_T: usize = 2;

// `ProperParms`: rolling time, birth interval, larva limits.
const ROLLING_TIME: usize = fp::own::BASE + 1;
const BIRTH_INTERVAL: usize = fp::own::BASE + 2;
const MAX_BIRTHS: usize = fp::own::BASE + 1;
const MIN_BIRTHS: usize = fp::own::BASE + 2;
const HOME_RADIUS: usize = 10;
const SEARCH_DISTANCE: usize = 14;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
/// The share of the disc's larva limits a stage gets.
const LARVA_SHARE: f32 = 0.2;
/// How hard the roll crushes (`rollingAttack`): damage to a player each half second.
const ROLL_DAMAGE: f32 = 30.0;

fn stuck(t: &Teki, w: &World) -> i32 {
    w.sim.pikis().iter().filter(|p| p.state.latched_to() == Some(t.creature)).count() as i32
}

fn hit_counter_up(t: &Teki) -> bool {
    t.damage_count > t.reg[PREV_HITS]
}

/// `isStartFlick`: enough blows for the number clinging (the disc's `ip01`..`ip07`).
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

/// `Obj::isCreateBaby`/`updateCreateBaby`: the larvae she has alive against her limits.
fn room_for_larva(t: &Teki, w: &World) -> bool {
    let alive = w.others.iter().filter(|o| o.kind == BULBORB_LARVA && o.alive).count() as f32;
    let (max, min) = (t.i(MAX_BIRTHS) as f32 * LARVA_SHARE, t.i(MIN_BIRTHS) as f32 * LARVA_SHARE);
    if t.flags[2] { alive < max } else { alive <= min }
}

fn create_baby(t: &Teki) -> bool {
    t.flags[2] && t.reg[BIRTH_T] > t.f(BIRTH_INTERVAL)
}

fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

fn change(t: &mut Teki) -> bool {
    if t.animation_finished() {
        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { WAIT });
        return true;
    }
    false
}

/// What a resting state heads for once its clip is told to finish.
fn pick_next(t: &mut Teki, w: &World, base: i32) {
    t.action_state = if t.hp <= 0.0 {
        DEAD
    } else if start_flick(t, w) {
        FLICK
    } else if stuck(t, w) != 0 {
        DAMAGE
    } else {
        base
    };
}

fn flick_all(t: &mut Teki, w: &mut World) {
    let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
    w.sim.shake_off(t.creature);
    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
    for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
    }
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(7, SLEEP);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.flags = [false, false, true, false];
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE);
    }));
    // Every state counts the time since she last laid a larva, and whether there is room for one.
    let counting = |inner: Act| -> Act {
        let (a1, a2, a3) = (inner.clone(), inner.clone(), inner.clone());
        Action::new(inner.next)
            .on_start(move |t, w| a1.start(t, w))
            .on_finish(move |t, w| a2.finish(t, w))
            .on_act(move |t, w| {
                t.reg[BIRTH_T] += w.dt;
                t.flags[2] = room_for_larva(t, w);
                a3.act(t, w)
            })
            .build()
    };

    // StateDead.
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
                if t.key(key::ACTION0) {
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );

    // StateSleep: curled in her chamber; wakes at a hit or a larva to lay.
    s.set(
        SLEEP,
        vec![counting(
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[WAIT_T] = 0.0;
                    t.reg[PREV_HITS] = t.damage_count;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_SLEEP);
                })
                .on_act(|t, w| {
                    hurt(t);
                    if t.action_state < 0 && (t.hp <= 0.0 || hit_counter_up(t) || create_baby(t)) {
                        pick_next(t, w, WAIT);
                        t.finish_motion();
                    }
                    change(t)
                })
                .build(),
        )],
    );

    // StateWait: awake and still; back to sleep after half a minute with no larva to lay.
    s.set(
        WAIT,
        vec![counting(
            Action::new(SLEEP)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[WAIT_T] = 0.0;
                    t.reg[PREV_HITS] = t.damage_count;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_WAIT);
                })
                .on_act(|t, w| {
                    hurt(t);
                    if t.action_state < 0 {
                        if !create_baby(t) && t.reg[WAIT_T] > 30.0 {
                            t.action_state = SLEEP;
                            t.finish_motion();
                        }
                        if hit_counter_up(t) {
                            t.action_state = DAMAGE;
                            t.finish_motion();
                        }
                        if create_baby(t) {
                            t.action_state = BORN;
                            t.finish_motion();
                        }
                        if start_flick(t, w) {
                            t.action_state = FLICK;
                            t.finish_motion();
                        }
                        if t.hp <= 0.0 {
                            t.action_state = DEAD;
                            t.finish_motion();
                        }
                    }
                    t.reg[WAIT_T] += w.dt;
                    change(t)
                })
                .build(),
        )],
    );

    // StateDamage: shuddering with Pikmin on her.
    s.set(
        DAMAGE,
        vec![counting(
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[WAIT_T] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_DAMAGE);
                })
                .on_act(|t, w| {
                    hurt(t);
                    if t.action_state < 0 {
                        if create_baby(t) {
                            t.action_state = BORN;
                            t.finish_motion();
                        }
                        if stuck(t, w) == 0 {
                            t.action_state = WAIT;
                            t.finish_motion();
                        }
                        if start_flick(t, w) {
                            t.action_state = FLICK;
                            t.finish_motion();
                        }
                        if t.hp <= 0.0 {
                            t.action_state = DEAD;
                            t.finish_motion();
                        }
                    }
                    change(t)
                })
                .build(),
        )],
    );

    // StateFlick: throws off what clings to her, then rolls.
    s.set(
        FLICK,
        vec![counting(
            Action::new(ROLLING)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[WAIT_T] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_FLICK);
                })
                .on_act(|t, w| {
                    hurt(t);
                    if t.key(key::ACTION0) {
                        flick_all(t, w);
                    }
                    if t.animation_finished() {
                        t.flags[1] = w.rng.f32() < 0.5;
                        t.next_override = Some(if t.hp <= 0.0 { DEAD } else { ROLLING });
                        return true;
                    }
                    false
                })
                .build(),
        )],
    );

    // StateRolling: from one end of her territory to the other and back, crushing.
    s.set(
        ROLLING,
        vec![counting(
            Action::new(ROLLING)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.flags[0] = false;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(if t.flags[1] { ANIM_ROLL_LEFT } else { ANIM_ROLL_RIGHT });
                })
                .on_act(|t, w| {
                    hurt(t);
                    let left = t.anim.motion == ANIM_ROLL_LEFT as i32;
                    let side = t.yaw + if left { FRAC_PI_2 } else { -FRAC_PI_2 };
                    let dir = dir_vec(side);
                    let territory = t.f(fp::TERRITORY);
                    if t.key(key::LOOP_START) {
                        t.flags[0] = true;
                    }
                    if t.flags[0] {
                        let off = (t.pos - t.nest).with_y(0.0);
                        let along = off.dot(dir);
                        if along > territory {
                            t.target_vel = Vec3::ZERO;
                        } else {
                            let toward = ((t.nest - t.pos) + dir * (territory + 10.0)).with_y(0.0).normalize_or_zero();
                            t.target_vel = toward * t.f(fp::MOVE_SPEED);
                        }
                        // `flickPikmin`, `rollingAttack`: whatever is under her is crushed.
                        let reach = t.f(fp::ATTACK_RADIUS);
                        // The sim keeps Pikmin out of her body sphere, so the crush reaches a little past it.
                        let body = t.coll_radius.max((t.info.hi.y - t.info.lo.y) * 0.4);
                        w.sim.attack_pikis_in_sphere(t.pos, (reach * 0.8).max(body + 30.0), 1000.0);
                        if t.timer_elapsed(6) {
                            t.timers[6] = 0.5;
                            for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= reach) {
                                w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: ROLL_DAMAGE, from: t.pos, knock: 250.0 });
                            }
                            w.out.push(Out::Shake { pos: t.pos, strength: 1.0 });
                        }
                        t.reg[WAIT_T] += w.dt;
                    } else {
                        t.target_vel = Vec3::ZERO;
                    }
                    if t.hp <= 0.0 && t.action_state != DEAD {
                        t.action_state = DEAD;
                        t.flags[0] = false;
                        t.target_vel = Vec3::ZERO;
                        t.finish_motion();
                    }
                    if t.key(key::ACTION0) && t.action_state != DEAD {
                        let off = (t.pos - t.nest).with_y(0.0);
                        let along = off.dot(dir);
                        if along > territory - 50.0 {
                            // The crash at the end: she turns and rolls back the other way.
                            if t.flags[0] {
                                w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
                                w.out.push(Out::Sound { name: "QUEEN_CRUSH", pos: t.pos });
                            }
                            t.flags[0] = false;
                            t.flags[1] = !t.flags[1];
                            t.action_state = ROLLING;
                            t.finish_motion();
                        } else if t.reg[WAIT_T] > t.f(ROLLING_TIME) && along > -(50.0 + t.f(HOME_RADIUS)) && along < 50.0 {
                            t.flags[0] = false;
                            t.action_state = WAIT;
                            t.finish_motion();
                        }
                    }
                    if t.animation_finished() {
                        t.target_vel = Vec3::ZERO;
                        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { WAIT });
                        return true;
                    }
                    false
                })
                .on_finish(|t, _| {
                    t.damage_count = 0.0;
                    t.reg[PREV_HITS] = 0.0;
                    t.flags[0] = false;
                    t.target_vel = Vec3::ZERO;
                })
                .build(),
        )],
    );

    // StateBorn: lays a larva at the clip's key.
    s.set(
        BORN,
        vec![counting(
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[WAIT_T] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_BORN);
                })
                .on_act(|t, w| {
                    hurt(t);
                    if t.key(key::ACTION0) {
                        let back = -t.forward();
                        w.out.push(Out::Spawn {
                            kind: BULBORB_LARVA,
                            pos: t.pos + back * 60.0 + Vec3::Y * 10.0,
                            yaw: t.yaw + std::f32::consts::PI,
                            parent: t.id,
                            velocity: back * t.f(SEARCH_DISTANCE),
                        });
                    }
                    if t.animation_finished() {
                        t.reg[BIRTH_T] = 0.0;
                        t.next_override = Some(if t.hp <= 0.0 { DEAD } else { WAIT });
                        return true;
                    }
                    false
                })
                .build(),
        )],
    );
    s
}
