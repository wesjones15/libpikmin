//! The Skitter Leaf of Pikmin 2 (`Sokkuri::Obj : EnemyBase`, plugProjectNishimuraU/Sokkuri.cpp, SokkuriState.cpp): a bug
//! under a leaf. It sits hidden (`Stay`) until a player comes within its sight, pops up (`Appear`), skitters over the
//! ground or paddles over water in short legs with pauses between (`MoveGround`, `MoveWater`, `Wait`), shakes off the
//! Pikmin that keep hitting it (`Flick`) and goes back under its leaf when nobody is near and it is home
//! (`Disappear`). It never attacks. Killed or squashed it dies and leaves a corpse (`Dead`, `Press`).
//!
//! States (`SokkuriStateID`): [`DEAD`] .. [`FLICK`]. Registers: `reg[0]` the timer (`mTimer`), `reg[1]` the move speed
//! (`mMoveVelocity`), `action_state` the state to go to when the clip ends (`mNextState`, -1 none), `target_pos` where it
//! skitters to; `nest` is its home.
//!
//! Host adaptations: `in_water` stands for `mWaterBox`; the buoyancy push in the water (`velocity.y += 5`) is the host
//! body's; a wall turns it around; the flick counts the hits it took (`damage_count`) against `ip07` where the game keeps
//! a flick timer of the Pikmin's blows.

use std::sync::Arc;

use glam::Vec3;

use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const PRESS: i32 = 1;
pub const STAY: i32 = 2;
pub const APPEAR: i32 = 3;
pub const DISAPPEAR: i32 = 4;
pub const WAIT: i32 = 5;
pub const MOVE_GROUND: i32 = 6;
pub const MOVE_WATER: i32 = 7;
pub const FLICK: i32 = 8;

/// `SokkuriAnimID` (the `enemyAnimMgr.txt` order).
pub const ANIM_RUN_GROUND: usize = 0;
pub const ANIM_APPEAR: usize = 1;
pub const ANIM_WAIT: usize = 2;
pub const ANIM_HIDE: usize = 3;
pub const ANIM_DEAD: usize = 4;
pub const ANIM_PRESS_DEAD: usize = 5;
pub const ANIM_RUN_WATER: usize = 6;
pub const ANIM_FLICK: usize = 7;

const TIMER: usize = 0;
const SPEED: usize = 1;

// `ProperParms` (`fp01`..): travel time, move angle, waiting, underwater movement.
const MAX_TRAVEL: usize = fp::own::BASE + 1;
const MIN_TRAVEL: usize = fp::own::BASE + 2;
const MAX_ANGLE: usize = fp::own::BASE + 3;
const MIN_ANGLE: usize = fp::own::BASE + 4;
const WAIT_CHANCE: usize = fp::own::BASE + 11;
const MAX_WAIT: usize = fp::own::BASE + 12;
const MIN_WAIT: usize = fp::own::BASE + 13;
const WATER_SPEED: usize = fp::own::BASE + 21;
const WATER_TURN: usize = fp::own::BASE + 22;
const WATER_MAX_TURN: usize = fp::own::BASE + 23;

/// `EnemyParmsBase`'s `ip07` (`mShakeOffBlowD`): how many hits make it shake the Pikmin off.
const FLICK_BLOWS: usize = 7;
/// `mHomeRadius` (`fp10`) and `mTerritoryRadius` (`fp09`).
const HOME_RADIUS: usize = 10;
const TERRITORY: usize = fp::TERRITORY;

fn rand_weight(w: &mut World, x: f32) -> f32 {
    w.rng.f32() * x
}

/// `approach(cur, target, step)`.
fn approach(cur: f32, target: f32, step: f32) -> f32 {
    if (target - cur).abs() <= step { target } else { cur + step * (target - cur).signum() }
}

/// `Obj::getSearchedTarget`: the nearest player in sight (the Pikmin only in the Piklopedia).
fn searched_target(t: &Teki, w: &World) -> bool {
    let sight = t.f(fp::SIGHT_RADIUS);
    w.navis.iter().filter(|n| n.alive).any(|n| dist_xz(t.pos, n.pos) <= sight)
}

/// `Obj::isDisappear`: home and nobody in sight.
fn is_disappear(t: &Teki, w: &World) -> bool {
    dist_xz(t.pos, t.nest) < t.f(HOME_RADIUS) && !searched_target(t, w)
}

/// `Obj::resetMoveVelocity`.
fn reset_move_velocity(t: &mut Teki) {
    t.reg[SPEED] = if t.in_water { t.f(WATER_SPEED) } else { t.f(fp::MOVE_SPEED) };
}

/// `Obj::setNextMoveInfo`: how long this leg runs and where it heads (1000 units off, at a random angle from its facing).
fn set_next_move_info(t: &mut Teki, w: &mut World) {
    let span = t.f(MAX_TRAVEL) - t.f(MIN_TRAVEL);
    t.reg[TIMER] = rand_weight(w, span);
    let angle = (rand_weight(w, t.f(MAX_ANGLE) - t.f(MIN_ANGLE)) + t.f(MIN_ANGLE)).to_radians();
    let angle = if w.rng.f32() < 0.5 { angle + t.yaw } else { angle - t.yaw };
    t.target_pos = t.pos + dir_vec(angle) * 1000.0;
}

/// `Obj::updateMoveState`: back toward home when it has strayed; the speed eases to the ground's or the water's.
fn update_move_state(t: &mut Teki) {
    if dist_xz(t.pos, t.nest) > t.f(TERRITORY) {
        t.target_pos = t.nest;
    }
    t.reg[SPEED] = if t.in_water {
        approach(t.reg[SPEED], t.f(WATER_SPEED), 10.0)
    } else {
        approach(t.reg[SPEED], t.f(fp::MOVE_SPEED), 25.0)
    };
}

/// `EnemyFunc::isStartFlick`: it has taken enough hits.
fn is_start_flick(t: &Teki) -> bool {
    t.damage_count > t.i(FLICK_BLOWS) as f32
}

fn dead_check(t: &mut Teki) -> bool {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
    if t.hp <= 0.0 {
        t.next_override = Some(DEAD);
        return true;
    }
    false
}

fn change_when_done(t: &mut Teki, default: i32) -> bool {
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
        t.set_option(opt::DAMAGE_COUNTABLE);
    }));

    // StateDead / StatePress: the death motion, then gone (the host's body leaves the corpse).
    let dying = |anim: usize, pressed: bool| {
        Action::new(NO_TRANSIT)
            .on_start(move |t, w| {
                if pressed {
                    t.hp = 0.0;
                }
                t.clear_option(opt::ATARI);
                t.target_vel = Vec3::ZERO;
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(anim);
            })
            .on_act(|t, w| {
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()
    };
    s.set(DEAD, vec![dying(ANIM_DEAD, false)]);
    s.set(PRESS, vec![dying(ANIM_PRESS_DEAD, true)]);

    // StateStay: under its leaf, unseen and untouchable, the Appear clip held on its first frame.
    s.set(
        STAY,
        vec![Action::new(APPEAR)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.action_state = -1;
                reset_move_velocity(t);
                t.clear_option(opt::ATARI);
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_APPEAR);
                t.manual_speed = Some(0.0);
            })
            .on_act(|t, w| searched_target(t, w))
            .on_finish(|t, _| {
                t.set_option(opt::ATARI);
                t.manual_speed = None;
            })
            .build()],
    );

    // StateAppear: pops up, then runs.
    s.set(
        APPEAR,
        vec![
            Action::new(FLICK).on_act(|t, _| is_start_flick(t)).build(),
            Action::new(MOVE_GROUND)
                .on_start(|t, _| {
                    t.reg[TIMER] = 0.0;
                    t.action_state = -1;
                    reset_move_velocity(t);
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_APPEAR);
                })
                .on_act(|t, _| {
                    if dead_check(t) {
                        return true;
                    }
                    t.animation_finished()
                })
                .build(),
        ],
    );

    // StateDisappear: back under the leaf.
    s.set(
        DISAPPEAR,
        vec![Action::new(STAY)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.action_state = -1;
                reset_move_velocity(t);
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_HIDE);
            })
            .on_act(|t, _| dead_check(t) || t.animation_finished())
            .build()],
    );

    // StateWait: a pause, then off again (or away, if it has nobody to watch for and is home).
    s.set(
        WAIT,
        vec![Action::new(MOVE_GROUND)
            .on_start(|t, w| {
                t.reg[TIMER] = 0.0;
                t.action_state = -1;
                reset_move_velocity(t);
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_WAIT);
                let _ = w;
            })
            .on_act(|t, w| {
                if dead_check(t) {
                    return true;
                }
                if is_start_flick(t) {
                    t.next_override = Some(FLICK);
                    return true;
                }
                if t.reg[TIMER] > t.f(MAX_WAIT) && t.action_state < 0 {
                    t.action_state = if is_disappear(t, w) { DISAPPEAR } else if t.in_water { MOVE_WATER } else { MOVE_GROUND };
                    t.finish_motion();
                }
                t.reg[TIMER] += w.dt;
                change_when_done(t, MOVE_GROUND)
            })
            .build()],
    );

    // StateMoveGround: skitters toward a point; on water it switches to paddling.
    s.set(
        MOVE_GROUND,
        vec![Action::new(MOVE_GROUND)
            .on_start(|t, w| {
                t.action_state = -1;
                set_next_move_info(t, w);
                t.reg[TIMER] = 0.0;
                t.start_motion(ANIM_RUN_GROUND);
            })
            .on_act(|t, w| {
                update_move_state(t);
                if dead_check(t) {
                    return true;
                }
                if is_start_flick(t) {
                    t.next_override = Some(FLICK);
                    return true;
                }
                if t.action_state < 0 {
                    if t.reg[TIMER] > t.f(MAX_TRAVEL) {
                        t.action_state = if is_disappear(t, w) {
                            DISAPPEAR
                        } else if w.rng.f32() < t.f(WAIT_CHANCE) {
                            WAIT
                        } else if t.in_water {
                            t.target_vel = Vec3::ZERO;
                            MOVE_WATER
                        } else {
                            MOVE_GROUND
                        };
                        t.finish_motion();
                    } else if t.in_water {
                        t.target_vel = Vec3::ZERO;
                        t.action_state = MOVE_WATER;
                        t.finish_motion();
                    } else {
                        let (target, speed) = (t.target_pos, t.reg[SPEED]);
                        turn_to_target(t, target, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                        set_target_speed(t, speed);
                    }
                }
                t.reg[TIMER] += w.dt;
                change_when_done(t, MOVE_GROUND)
            })
            .on_event(|t, w, e| {
                // `wallCallback`: it turns away from the wall.
                if matches!(e, Event::Wall) {
                    t.target_pos = t.pos - t.forward() * 1000.0;
                    let _ = w;
                }
                false
            })
            .build()],
    );

    // StateMoveWater: the same on the water, slowly turning.
    s.set(
        MOVE_WATER,
        vec![Action::new(MOVE_WATER)
            .on_start(|t, w| {
                t.action_state = -1;
                set_next_move_info(t, w);
                t.reg[TIMER] = 0.0;
                t.start_motion(ANIM_RUN_WATER);
            })
            .on_act(|t, w| {
                update_move_state(t);
                if dead_check(t) {
                    return true;
                }
                if is_start_flick(t) {
                    t.next_override = Some(FLICK);
                    return true;
                }
                if t.action_state < 0 {
                    if is_disappear(t, w) {
                        t.action_state = DISAPPEAR;
                        t.finish_motion();
                    } else if t.reg[TIMER] > t.f(MAX_TRAVEL) {
                        if t.in_water {
                            t.reg[TIMER] = 0.0;
                            set_next_move_info(t, w);
                            t.reg[TIMER] = 0.0;
                        } else {
                            t.target_vel = Vec3::ZERO;
                            t.action_state = MOVE_GROUND;
                            t.finish_motion();
                        }
                    } else if !t.in_water {
                        t.target_vel = Vec3::ZERO;
                        t.action_state = MOVE_GROUND;
                        t.finish_motion();
                    } else {
                        let (target, speed) = (t.target_pos, t.reg[SPEED]);
                        turn_to_target(t, target, t.f(WATER_TURN), t.f(WATER_MAX_TURN));
                        set_target_speed(t, speed);
                    }
                }
                t.reg[TIMER] += w.dt;
                change_when_done(t, MOVE_WATER)
            })
            .build()],
    );

    // StateFlick: shakes the Pikmin off, then goes on.
    s.set(
        FLICK,
        vec![Action::new(MOVE_GROUND)
            .on_start(|t, w| {
                t.reg[TIMER] = 0.0;
                t.action_state = -1;
                reset_move_velocity(t);
                t.target_vel = Vec3::ZERO;
                t.damage_count = 0.0;
                t.start_motion(ANIM_FLICK);
                w.sim.shake_off(t.creature);
            })
            .on_act(|t, w| {
                if dead_check(t) {
                    return true;
                }
                // `KEY_2`: the knockback, `fp19` out, `fp17` hard, `fp18` damage.
                if t.key(crate::anim::key::ACTION0) {
                    w.sim.shake_off(t.creature);
                    let (range, damage) = (t.f(19), t.f(18));
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                }
                t.action_state = if t.in_water { MOVE_WATER } else { MOVE_GROUND };
                if !t.animation_finished() {
                    t.finish_motion();
                }
                change_when_done(t, MOVE_GROUND)
            })
            .build()],
    );
    // `pressCallBack`: squashed in any state but dying.
    for st in [APPEAR, DISAPPEAR, WAIT, MOVE_GROUND, MOVE_WATER, FLICK] {
        s.states[st as usize].insert(0, Action::new(PRESS).on_event(|_, _, e| matches!(e, Event::Pressed)).build());
    }
    s
}
