//! The Emperor Bulblax of Pikmin 2 (`KingChappy::Obj : EnemyBase`, plugProjectMorimuraU/kingChappy.cpp, kingChappyState.cpp): a giant
//! Bulborb that lies buried in the ground (`HideWait`) until someone comes near, bursts out (`Appear`, throwing off whoever is close),
//! looks about (`Caution`) and goes after the nearest player or Pikmin (`Walk`, `Turn`), biting with a long tongue (`Attack`) and
//! swallowing what it caught (`Swallow`). It stamps on those at its feet and shakes off the ones clinging to it (`Flick`), roars
//! (`WarCry`), and when it has lost its prey or strayed from its territory goes home and buries itself again (`Hide`). Killed it
//! thrashes and falls (`Dead`) and leaves a corpse.
//!
//! States (`KingChappyStateID`): [`WALK`], [`ATTACK`], [`DEAD`], [`FLICK`], [`WAR_CRY`], [`TURN`], [`HIDE`], [`HIDE_WAIT`], [`APPEAR`],
//! [`CAUTION`], [`SWALLOW`]. (`Damage` and `Eat`, which follow its eating a Bomb-rock, are not used: nothing here feeds it one.)
//! Registers: `reg[0]` the walking timer (ticks), `reg[1]` the no-target timer, `reg[2]` the search delay, `reg[4]` the hide-wait timer;
//! `target_pos` its goal, `wander` where it was at the last walking check; `flags[0]` the bite is eating, `flags[1]` it caught a Pikmin,
//! `flags[2]` it has bitten a player this bite; `targets[0]` the target.
//!
//! Host adaptations: the tongue's reach is a sphere in front of the mouth for the length of the clip's eating keys (the decomp traces the
//! tongue joint against the map); the Pikmin it eats die at once (no mouth slots, no White-Pikmin poison); the roar's `InteractAstonish`
//! and the Bomb-rock handling are left out; leg IK, blended animations and the boss music are the host's.

use std::f32::consts::TAU;
use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const WALK: i32 = 0;
pub const ATTACK: i32 = 1;
pub const DEAD: i32 = 2;
pub const FLICK: i32 = 3;
pub const WAR_CRY: i32 = 4;
pub const DAMAGE: i32 = 5;
pub const TURN: i32 = 6;
pub const EAT: i32 = 7;
pub const HIDE: i32 = 8;
pub const HIDE_WAIT: i32 = 9;
pub const APPEAR: i32 = 10;
pub const CAUTION: i32 = 11;
pub const SWALLOW: i32 = 12;

/// `KingAnimID`.
pub const ANIM_ATTACK: usize = 0;
pub const ANIM_WAR_CRY: usize = 1;
pub const ANIM_DEAD: usize = 3;
pub const ANIM_DIVE: usize = 4;
pub const ANIM_FLICK: usize = 5;
pub const ANIM_MOVE: usize = 6;
pub const ANIM_SWALLOW: usize = 7;
pub const ANIM_APPEAR: usize = 9;
pub const ANIM_HIDE_WAIT: usize = 10;
pub const ANIM_TURN: usize = 11;
pub const ANIM_CAUTION: usize = 12;

const WALK_T: usize = 0;
const NO_TARGET: usize = 1;
const SEARCH_DELAY: usize = 2;
const HIDE_T: usize = 4;

// `ProperParms`.
const TURN_ANGLE: usize = fp::own::BASE + 1;
const SPAWN_DISTANCE: usize = fp::own::BASE + 2;
const ROAR_RANGE: usize = fp::own::BASE + 4;
const INVISIBLE: usize = fp::own::BASE + 6;
const TURN_END: usize = fp::own::BASE + 7;
const TRAMPLE: usize = fp::own::BASE + 8;
const SHAKE_RANGE: usize = fp::own::BASE + 9;
const SHAKE_POWER: usize = fp::own::BASE + 10;
const WALK_SPEED: usize = fp::own::BASE + 11;
const DEATH_RATE: usize = fp::own::BASE + 12;
const FLICK_SHOUT: usize = fp::own::BASE + 13;
const INCUBATION: usize = fp::own::BASE + 1;
const TIME_TO_APPEAR: usize = fp::own::BASE + 2;
const HOME_RADIUS: usize = 10;
const SEARCH_DISTANCE: usize = 14;
const SEARCH_ANGLE: usize = 15;
const SHAKE_KNOCK: usize = 17;
const GEN_SHAKE_RANGE: usize = 19;
const SHAKE_DAMAGE: usize = 18;
const LIFE_ALERT: usize = 30;

fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

fn stuck(t: &Teki, w: &World) -> i32 {
    w.sim.pikis().iter().filter(|p| p.state.latched_to() == Some(t.creature)).count() as i32
}

/// `EnemyFunc::isStartFlick`.
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

/// `Obj::searchTarget`: the nearest player in the search cone, or a Pikmin in it (above the invisible range) at its height.
fn search_target(t: &mut Teki, w: &World) {
    t.targets[0] = None;
    if t.reg[SEARCH_DELAY] > 0.0 {
        return;
    }
    let (dist, half) = (t.f(SEARCH_DISTANCE), (t.f(SEARCH_ANGLE) * 0.5).to_radians());
    let invisible = t.f(INVISIBLE);
    let me = t.pos;
    let c = t.closest(w, |c| {
        let d = dist_xz(me, c.pos);
        d <= dist && (c.piki.is_none() || (d > invisible && (c.pos.y - me.y).abs() <= 50.0))
    });
    if let Some(c) = c.filter(|c| t.calc_target_angle(c.pos) <= half) {
        t.targets[0] = Some(c.target);
        t.target_pos = c.pos;
    }
}

/// `Obj::setNextGoal`.
fn set_next_goal(t: &mut Teki, w: &mut World) {
    let rad = t.f(fp::TERRITORY);
    if dist_xz(t.pos, t.nest) > rad {
        t.target_pos = t.nest;
        return;
    }
    if let Some(c) = t.target(0, w) {
        t.target_pos = c.pos;
        return;
    }
    let r = rad * (0.3 + w.rng.f32());
    let a = TAU * w.rng.f32();
    t.target_pos = t.nest + Vec3::new(r * a.sin(), 0.0, r * a.cos());
}

/// The checks the moving states run (`checkDead`, `checkFlick`, `checkAttack`, `checkTurn`): sets the next state and returns true.
fn checks(t: &mut Teki, w: &mut World, turn: bool) -> bool {
    hurt(t);
    if t.hp <= 0.0 {
        t.next_override = Some(if w.rng.f32() < t.f(DEATH_RATE) { WAR_CRY } else { DEAD });
        return true;
    }
    // A player close to it winds the flick timer up (`mFlickTimer += 0.1`).
    let invisible = t.f(INVISIBLE);
    if w.navis.iter().any(|n| n.alive && dist_xz(t.pos, n.pos) < invisible) {
        t.damage_count += 0.1;
    }
    if start_flick(t, w) {
        t.next_override = Some(if t.hp < 0.5 * t.info.life && w.rng.f32() < t.f(FLICK_SHOUT) { WAR_CRY } else { FLICK });
        return true;
    }
    if let Some(c) = t.target(0, w) {
        let near = dist_xz(t.pos, c.pos) > invisible;
        let reach = (c.pos - t.pos).length() < t.f(fp::MAX_ATTACK_RANGE) && t.calc_target_angle(c.pos) <= t.f(fp::MAX_ATTACK_ANGLE).to_radians();
        if reach && near {
            t.targets[0] = None;
            t.next_override = Some(ATTACK);
            return true;
        }
    }
    if turn {
        let goal = t.target_pos;
        if t.calc_target_angle(goal) > t.f(TURN_ANGLE).to_radians() {
            t.next_override = Some(TURN);
            return true;
        }
    }
    false
}

fn end_to(t: &mut Teki, next: i32) -> bool {
    if t.animation_finished() {
        t.manual_speed = None;
        t.next_override = Some(next);
        return true;
    }
    false
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(13, HIDE_WAIT);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.flags = [false; 4];
        t.wander = t.pos;
        t.target_pos = t.pos;
        t.set_option(opt::DAMAGE_COUNTABLE);
    }));

    // StateHideWait: buried; comes up when a player or Pikmin is within the spawn distance (and the wait is over).
    s.set(
        HIDE_WAIT,
        vec![Action::new(APPEAR)
            .on_start(|t, _| {
                t.reg[HIDE_T] = 0.0;
                t.clear_option(opt::ATARI | opt::VISIBLE);
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_HIDE_WAIT);
            })
            .on_act(|t, w| {
                t.reg[HIDE_T] += 1.0;
                if t.animation_finished() {
                    t.start_motion(ANIM_HIDE_WAIT);
                }
                if t.reg[HIDE_T] > t.i(TIME_TO_APPEAR) as f32 {
                    let r = t.f(SPAWN_DISTANCE);
                    return w.navis.iter().any(|n| (n.pos - t.pos).length() < r) || w.sim.pikis().iter().any(|p| p.is_alive() && (p.pos - t.pos).length() < r);
                }
                false
            })
            .on_finish(|t, _| t.set_option(opt::ATARI | opt::VISIBLE))
            .build()],
    );

    // StateAppear: bursts out of the ground, throwing off what is near.
    s.set(
        APPEAR,
        vec![Action::new(CAUTION)
            .on_start(|t, w| {
                t.target_vel = Vec3::ZERO;
                t.set_option(opt::INVINCIBLE);
                search_target(t, w);
                t.start_motion(ANIM_APPEAR);
                w.out.push(Out::Sound { name: "KING_APPEAR", pos: t.pos });
            })
            .on_act(|t, w| {
                if t.key(key::ACTION1) {
                    let (range, power, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_POWER), t.f(SHAKE_DAMAGE));
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                    for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: power });
                    }
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                t.animation_finished()
            })
            .on_finish(|t, _| t.clear_option(opt::INVINCIBLE))
            .build()],
    );

    s.set(
        CAUTION,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_CAUTION);
            })
            .on_act(|t, _| {
                hurt(t);
                t.animation_finished()
            })
            .build()],
    );

    // StateWalk: heads for the goal; ends in a turn, a bite, a flick or its death.
    s.set(
        WALK,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.start_motion(ANIM_MOVE);
                t.manual_speed = Some(30.0 * t.f(WALK_SPEED));
                t.action_state = -1;
            })
            .on_act(|t, w| {
                if t.reg[SEARCH_DELAY] > 0.0 {
                    t.reg[SEARCH_DELAY] -= 1.0;
                }
                if t.action_state < 0 {
                    // `walkFunc`
                    search_target(t, w);
                    let goal = t.target_pos;
                    turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    set_target_speed(t, t.f(fp::MOVE_SPEED));
                    t.reg[WALK_T] += 1.0;
                    if t.reg[WALK_T] > 120.0 {
                        if dist_xz(t.pos, t.wander) < 30.0 {
                            t.reg[SEARCH_DELAY] = 120.0;
                            t.targets[0] = None;
                            t.target_pos = t.nest;
                        }
                        t.wander = t.pos;
                        t.reg[WALK_T] = 0.0;
                    }
                    if t.targets[0].is_none() {
                        t.reg[NO_TARGET] += 1.0;
                    } else {
                        t.reg[NO_TARGET] = 0.0;
                    }
                    let away = dist_xz(t.pos, t.nest) > t.f(fp::TERRITORY) || t.reg[NO_TARGET] > t.i(INCUBATION) as f32;
                    if away {
                        t.target_pos = t.nest;
                        t.reg[NO_TARGET] = t.i(INCUBATION) as f32;
                        if dist_xz(t.pos, t.nest) < t.f(HOME_RADIUS) {
                            t.next_override = Some(HIDE);
                            t.reg[NO_TARGET] = 0.0;
                            return true;
                        }
                    } else if dist_xz(t.pos, t.target_pos) < 20.0 {
                        set_next_goal(t, w);
                    }
                    if checks(t, w, true) {
                        t.target_vel = Vec3::ZERO;
                        return true;
                    }
                }
                false
            })
            .on_finish(|t, _| {
                t.manual_speed = None;
                t.target_vel = Vec3::ZERO;
            })
            .build()],
    );

    // StateTurn: pivots in place to the goal (or target), then walks.
    s.set(
        TURN,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_TURN);
            })
            .on_act(|t, w| {
                let target = t.target(0, w).map_or(t.target_pos, |c| c.pos);
                let left = turn_to_target(t, target, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                let end = if t.targets[0].is_some() { t.f(TURN_END).to_radians() } else { 0.5 };
                if left.abs() < end {
                    t.finish_motion();
                }
                hurt(t);
                if t.hp <= 0.0 || start_flick(t, w) {
                    return checks(t, w, false);
                }
                t.animation_finished()
            })
            .build()],
    );

    // StateAttack: the long tongue; between the eating keys whatever is in front of the mouth is caught.
    s.set(
        ATTACK,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.flags[0] = false;
                t.flags[1] = false;
                t.flags[2] = false;
                t.start_motion(ANIM_ATTACK);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION1) {
                    t.flags[0] = true;
                }
                if t.key(key::ACTION3) {
                    t.flags[0] = false;
                }
                if t.flags[0] {
                    let reach = t.f(fp::MAX_ATTACK_RANGE);
                    let mouth = t.pos + t.forward() * (reach * 0.6) + Vec3::Y * 8.0;
                    let radius = t.f(fp::ATTACK_RADIUS);
                    if w.sim.attack_pikis_in_sphere(mouth, radius, 1000.0) > 0 {
                        t.flags[1] = true;
                    }
                    if !t.flags[2] {
                        for n in w.navis.iter().filter(|n| n.alive && (n.pos - mouth).length() - n.size <= radius) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: t.f(fp::ATTACK_DAMAGE) * 6.0, from: t.pos, knock: 80.0 });
                            t.flags[2] = true;
                        }
                    }
                }
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                if t.animation_finished() {
                    t.flags[0] = false;
                    t.next_override = Some(if t.flags[1] { SWALLOW } else { WALK });
                    return true;
                }
                false
            })
            .build()],
    );

    s.set(
        SWALLOW,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_SWALLOW);
            })
            .on_act(|t, _| {
                hurt(t);
                t.animation_finished()
            })
            .build()],
    );

    // StateFlick: shakes and stamps.
    s.set(
        FLICK,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION0) {
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                if t.key(key::ACTION1) {
                    // The stamp: whoever is under its feet is trampled; everything near is thrown.
                    let trample = t.f(TRAMPLE);
                    let (range, knock, damage) = (t.f(GEN_SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                    w.sim.attack_pikis_in_sphere(t.pos, trample, 1000.0);
                    for n in w.navis.iter().filter(|n| n.alive) {
                        let d = dist_xz(t.pos, n.pos) - n.size;
                        if d <= trample {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: t.f(fp::ATTACK_DAMAGE) * 6.0, from: t.pos, knock: knock });
                        } else if d <= range {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                        }
                    }
                    w.sim.shake_off(t.creature);
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                    t.damage_count = 0.0;
                }
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                t.animation_finished()
            })
            .build()],
    );

    // StateWarCry: a roar that shakes off Pikmin.
    s.set(
        WAR_CRY,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_WAR_CRY);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION1) {
                    // `Roar`: the other enemies hear it (`requestTransit`).
                    w.out.push(Out::Message { from: t.id, pos: t.pos, range: t.f(ROAR_RANGE) * 3.0 });
                    w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
                }
                if t.key(key::ACTION2) {
                    let (range, knock, damage) = (t.f(GEN_SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                    w.sim.shake_off(t.creature);
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                    for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                    }
                    t.damage_count = 0.0;
                }
                hurt(t);
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { WALK });
                    return true;
                }
                false
            })
            .build()],
    );

    // StateHide: walks home and dives; then buried again.
    s.set(
        HIDE,
        vec![Action::new(HIDE_WAIT)
            .on_start(|t, w| {
                t.target_vel = Vec3::ZERO;
                t.set_option(opt::INVINCIBLE);
                t.start_motion(ANIM_DIVE);
                w.out.push(Out::Shake { pos: t.pos, strength: 1.5 });
            })
            .on_act(|t, w| {
                if t.key(key::ACTION0) {
                    w.out.push(Out::Sound { name: "KING_APPEAR", pos: t.pos });
                }
                t.animation_finished()
            })
            .build()],
    );

    // StateDead.
    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.clear_option(opt::ATARI);
                t.clear_option(opt::INVINCIBLE);
                t.target_vel = Vec3::ZERO;
                w.out.push(Out::Dying { teki: t.id });
                w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
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
    s.set(DAMAGE, Vec::new());
    s.set(EAT, Vec::new());
    let _ = (LIFE_ALERT, SEARCH_ANGLE);
    s
}
