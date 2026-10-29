//! The Bloysters of Pikmin 2 (`UmiMushi::Obj : EnemyBase`, plugProjectMorimuraU/umiMushi.cpp, umiMushiState.cpp): giant slugs that
//! ooze across the ground. The Ranging Bloyster ([`ranging`], `EnemyID_UmiMushi`) wanders its territory ([`WALK`]) until a player is
//! within its search range, takes note of him ([`FIND`]), turns to him ([`TURN`]) and creeps after him ([`SEARCH`]); in reach it lashes out
//! with its tongue ([`ATTACK`]), catching Pikmin it eats ([`EAT`]) and hurting a player, and it throws off those that cling ([`FLICK`]).
//! Losing the player it goes home ([`LOST`]). The Toady Bloyster ([`blind`], `EnemyID_UmiMushiBlind`) is the same slug without the sight: it
//! never marks a player, it ambles for a while and sits for a while, and only strikes at what comes within reach. Killed ([`DEAD`]) they
//! leave a corpse.
//!
//! States (`UmiMushi::StateID`): [`WAIT`], [`WALK`], [`FIND`], [`SEARCH`], [`TURN`], [`FLICK`], [`ATTACK`], [`EAT`], [`DEAD`], [`LOST`]. Registers:
//! `reg[0]` the state's timer, `reg[1]` the blind one's wait timer, `reg[2]` its move timer, `reg[3]` the goal's wobble phase; `flags[0]` the tongue is
//! out, `flags[1]` the tongue caught a Pikmin; `action_state` is `mNextState`, `targets[0]` the player it marked (`mTargetNavi`),
//! `target_pos` its goal.
//!
//! Host adaptations: no tail colouring while it marks a player (`changeColor`); the tongue's seven mouth slots are one swallow area ahead
//! of it; a Pikmin that clings to a part takes the full damage and one that does not takes `fp01` of it in the decomp, here the RoR2 body takes
//! whatever the host gives it; the cave territory is not used.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, turn_to_target};
use crate::tai::*;

pub const WAIT: i32 = 0;
pub const WALK: i32 = 1;
pub const FIND: i32 = 2;
pub const SEARCH: i32 = 3;
pub const TURN: i32 = 4;
pub const FLICK: i32 = 5;
pub const ATTACK: i32 = 6;
pub const EAT: i32 = 7;
pub const DEAD: i32 = 8;
pub const LOST: i32 = 9;

/// `UmiMushi::AnimID`.
pub const ANIM_ATTACK: usize = 0;
pub const ANIM_DEAD: usize = 1;
pub const ANIM_EAT: usize = 2;
pub const ANIM_FLICK: usize = 3;
pub const ANIM_RUN: usize = 4;
pub const ANIM_SEARCH: usize = 5;
pub const ANIM_SRUN: usize = 6;
pub const ANIM_STURN: usize = 7;
pub const ANIM_OUT_VIEW: usize = 9;
pub const ANIM_FSEARCH: usize = 10;

const TIMER: usize = 0;
const BLIND_WAIT: usize = 1;
const BLIND_MOVE: usize = 2;
const WOBBLE: usize = 3;

const HOME_RADIUS: usize = 10;
const SEARCH_DISTANCE: usize = 14;
const SEARCH_ANGLE: usize = 15;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
// Its own block (`ProperParms`).
const TURN_START: usize = fp::own::BASE + 2;
const TURN_END: usize = fp::own::BASE + 3;
const SEARCH_SPEED: usize = fp::own::BASE + 4;
const ROTATE_SPEED: usize = fp::own::BASE + 6;
const ROTATE_MAX: usize = fp::own::BASE + 7;
const BLIND_HEALTH: usize = fp::own::BASE + 12;
const BLIND_WAIT_TIME: usize = fp::own::BASE + 13;
const BLIND_MOVE_TIME: usize = fp::own::BASE + 14;
const WAIT_AFTER_ATTACK: usize = fp::own::BASE + 1;
/// `Parms::mBlindTurnRateReduction`.
const BLIND_TURN_SCALE: f32 = 0.3;
/// `Parms::mWalkAngleSpeed` and `mRotateAngleDelta`: the sway of its walk.
const TICK: f32 = 1.0 / 30.0;

fn players(t: &Teki, w: &World) -> Vec<Cand> {
    t.candidates(w).into_iter().filter(|c| c.piki.is_none()).collect()
}

/// `isChangeNavi`: a player has come within the search distance (a fifth more once it has marked one) and is not the one it has marked.
fn change_navi(t: &mut Teki, w: &World) -> bool {
    let mut dist = t.f(SEARCH_DISTANCE);
    if t.targets[0].is_some() {
        dist *= 1.2;
    }
    let near = players(t, w).into_iter().filter(|c| (c.pos - t.pos).length() < dist).min_by(|a, b| (a.pos - t.pos).length().total_cmp(&(b.pos - t.pos).length()));
    match near {
        Some(c) if t.targets[0] != Some(c.target) => {
            t.targets[0] = Some(c.target);
            t.target_pos = c.pos;
            true
        }
        _ => false,
    }
}

fn marked(t: &Teki, w: &World) -> Option<Cand> {
    t.target(0, w)
}

/// `isAttackStart`: the marked player in front and within reach, or a Pikmin there (the blind one also a player).
fn attack_start(t: &mut Teki, w: &World, blind: bool) -> bool {
    let (reach, half) = (t.f(fp::ATTACK_RADIUS), t.f(fp::ATTACK_HIT_ANGLE).to_radians());
    if let Some(c) = marked(t, w)
        && t.calc_target_angle(c.pos) <= half
        && dist_xz(t.pos, c.pos) < reach
    {
        return true;
    }
    let me = t.pos;
    let near = t.candidates(w).into_iter().filter(|c| (c.piki.is_some() || blind) && dist_xz(me, c.pos) < reach && t.calc_target_angle(c.pos) <= half).min_by(|a, b| dist_xz(me, a.pos).total_cmp(&dist_xz(me, b.pos)));
    if let Some(c) = near {
        t.targets[1] = Some(c.target);
        return true;
    }
    false
}

/// `isNeedTurn`: the goal, or the marked player, is more than the turn-start angle off its nose.
fn need_turn(t: &Teki, w: &World) -> bool {
    let limit = t.f(TURN_START).to_radians();
    t.calc_target_angle(t.target_pos) > limit || marked(t, w).is_some_and(|c| t.calc_target_angle(c.pos) > limit)
}

/// `turnFunc`: turns to the marked player (or the goal); returns the angle left.
fn turn_func(t: &mut Teki, w: &World, blind: bool) -> f32 {
    if let Some(c) = marked(t, w) {
        t.target_pos = c.pos;
    }
    let k = if blind { BLIND_TURN_SCALE } else { 1.0 };
    let goal = t.target_pos;
    turn_to_target(t, goal, k * t.f(ROTATE_SPEED), k * t.f(ROTATE_MAX)).abs()
}

fn next_goal(t: &mut Teki, w: &mut World) {
    let a = std::f32::consts::TAU * w.rng.f32();
    let r = t.f(fp::TERRITORY);
    t.target_pos = t.nest + Vec3::new(r * a.sin(), 0.0, r * a.cos());
}

/// `walkFunc`: towards the goal at the walking speed, swaying a little as it goes.
fn walk_func(t: &mut Teki) {
    let goal = t.target_pos;
    t.reg[WOBBLE] = (t.reg[WOBBLE] + 0.2) % std::f32::consts::TAU;
    turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
    let heading = t.yaw + 0.02 * t.reg[WOBBLE].sin();
    let d = dir_vec(heading);
    let speed = t.f(fp::MOVE_SPEED);
    t.target_vel = Vec3::new(d.x * speed, t.target_vel.y, d.z * speed);
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

fn clinging(t: &Teki, w: &World) -> i32 {
    w.sim.pikis().iter().filter(|p| p.state.latched_to() == Some(t.creature)).count() as i32
}

/// `EnemyFunc::isStartFlick`: enough blows for the number clinging (`ip01`..`ip07`).
fn start_flick(t: &Teki, w: &World) -> bool {
    let n = clinging(t, w);
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

/// The clip ended: on to the state a state chose.
fn hand_over(t: &mut Teki, default: i32) -> bool {
    if t.animation_finished() {
        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { default });
        return true;
    }
    false
}

/// Finish the clip, and take `to` next, unless something already has been chosen.
fn then(t: &mut Teki, to: i32) {
    t.action_state = to;
    t.finish_motion();
}

pub fn ranging() -> Strategy {
    build(false)
}

pub fn blind() -> Strategy {
    build(true)
}

fn build(blind: bool) -> Strategy {
    let mut s = Strategy::new(10, WAIT);
    s.on_start = Some(Arc::new(move |t: &mut Teki| {
        t.reg = [0.0; 8];
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE);
        if blind {
            // `onInit`: the Toady has its own life.
            t.hp = if t.f(BLIND_HEALTH) > 0.0 { t.f(BLIND_HEALTH) } else { t.hp };
        }
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.target_vel = Vec3::ZERO;
                w.sim.spit_out(t.creature);
                t.clear_option(opt::ATARI);
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(ANIM_DEAD);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION1) {
                    w.out.push(Out::Sound { name: "UMI_DEAD_GND", pos: t.pos });
                }
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );

    // StateWait: a moment's rest on the spot after something; then on to what it chooses.
    s.set(
        WAIT,
        vec![
            dead_check(),
            Action::new(WALK)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.reg[TIMER] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_SRUN);
                })
                .on_act(move |t, w| {
                    if !t.anim.is_finished() {
                        t.reg[TIMER] += 1.0;
                        if t.reg[TIMER] > t.f(WAIT_AFTER_ATTACK) {
                            t.finish_motion();
                        }
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(if t.action_state >= 0 {
                        t.action_state
                    } else if start_flick(t, w) {
                        t.action_state = SEARCH;
                        FLICK
                    } else if !blind && change_navi(t, w) {
                        FIND
                    } else if attack_start(t, w, blind) {
                        ATTACK
                    } else if marked(t, w).is_some() {
                        if need_turn(t, w) { TURN } else { SEARCH }
                    } else {
                        WALK
                    });
                    true
                })
                .build(),
        ],
    );

    // StateWalk: ambles to a goal on its territory's edge; the Toady stops now and then.
    s.set(
        WALK,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, w| {
                    t.action_state = -1;
                    t.reg[BLIND_WAIT] = 0.0;
                    t.reg[BLIND_MOVE] = 0.0;
                    t.reg[WOBBLE] = 0.0;
                    t.start_motion(ANIM_RUN);
                    if t.target_pos.length() == 0.0 {
                        next_goal(t, w);
                    }
                })
                .on_act(move |t, w| {
                    if t.action_state < 0 {
                        if dist_xz(t.pos, t.target_pos) < 50.0 {
                            let out = dist_xz(t.pos, t.nest) > t.f(fp::TERRITORY);
                            // `isFindTarget`: something in its sight keeps the goal where it is.
                            let me = t.pos;
                            let (range, half) = (t.f(SEARCH_DISTANCE), (t.f(SEARCH_ANGLE) * 0.5).to_radians());
                            let found = t.closest(w, |c| dist_xz(me, c.pos) < range && t.calc_target_angle(c.pos) <= half);
                            match found {
                                Some(c) if !out => t.target_pos = c.pos,
                                _ => next_goal(t, w),
                            }
                        }
                        if blind && t.reg[BLIND_WAIT] > 0.0 {
                            t.target_vel = Vec3::ZERO;
                            t.reg[BLIND_WAIT] += 1.0;
                            if t.reg[BLIND_WAIT] > t.f(BLIND_WAIT_TIME) {
                                t.reg[BLIND_WAIT] = 0.0;
                                t.reg[BLIND_MOVE] = 0.0;
                            }
                        } else {
                            t.reg[BLIND_MOVE] += 1.0;
                            walk_func(t);
                            if blind && t.reg[BLIND_MOVE] > t.f(BLIND_MOVE_TIME) {
                                t.reg[BLIND_MOVE] = 0.0;
                                t.reg[BLIND_WAIT] = 1.0;
                            }
                        }
                        if start_flick(t, w) {
                            t.target_vel = Vec3::ZERO;
                            then(t, FLICK);
                        } else if attack_start(t, w, blind) {
                            t.target_vel = Vec3::ZERO;
                            then(t, ATTACK);
                        } else if !blind && change_navi(t, w) {
                            t.target_vel = Vec3::ZERO;
                            t.next_override = Some(FIND);
                            return true;
                        }
                    }
                    if t.animation_finished() {
                        t.target_vel = Vec3::ZERO;
                        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { WAIT });
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateFind: it has marked a player: a look at him.
    s.set(
        FIND,
        vec![
            dead_check(),
            Action::new(SEARCH)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_SEARCH);
                })
                .on_act(move |t, w| {
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(if t.hp <= 0.0 {
                        DEAD
                    } else if need_turn(t, w) {
                        TURN
                    } else if attack_start(t, w, blind) {
                        ATTACK
                    } else {
                        SEARCH
                    });
                    true
                })
                .build(),
        ],
    );

    // StateSearch: creeps after the marked player.
    s.set(
        SEARCH,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.start_motion(ANIM_SRUN);
                })
                .on_act(move |t, w| {
                    if t.action_state < 0 {
                        if let Some(c) = marked(t, w) {
                            t.target_pos = c.pos;
                            // He has gone too far for it: it loses him.
                            if dist_xz(t.pos, c.pos) > t.f(SEARCH_DISTANCE) * 2.0 {
                                t.targets[0] = None;
                                t.target_vel = Vec3::ZERO;
                                then(t, LOST);
                                return false;
                            }
                        }
                        let speed = t.f(SEARCH_SPEED);
                        let d = dir_vec(t.yaw);
                        t.target_vel = Vec3::new(d.x * speed, t.target_vel.y, d.z * speed);
                        w.out.push(Out::Sound { name: "UMI_ZURUZURU", pos: t.pos });
                        if start_flick(t, w) {
                            t.target_vel = Vec3::ZERO;
                            then(t, FLICK);
                        } else if !blind && change_navi(t, w) {
                            t.target_vel = Vec3::ZERO;
                            then(t, FIND);
                        } else if attack_start(t, w, blind) {
                            t.target_vel = Vec3::ZERO;
                            then(t, ATTACK);
                        } else if need_turn(t, w) {
                            t.target_vel = Vec3::ZERO;
                            then(t, TURN);
                        }
                    }
                    if t.hp <= 0.0 {
                        t.target_vel = Vec3::ZERO;
                        then(t, DEAD);
                    }
                    if t.animation_finished() {
                        t.target_vel = Vec3::ZERO;
                        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { WAIT });
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateTurn: turns on the spot to face the marked player.
    s.set(
        TURN,
        vec![
            dead_check(),
            Action::new(SEARCH)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_STURN);
                })
                .on_act(move |t, w| {
                    if t.action_state < 0 {
                        if start_flick(t, w) {
                            then(t, FLICK);
                        } else if !blind && change_navi(t, w) {
                            then(t, FIND);
                        } else if turn_func(t, w, blind) < t.f(TURN_END).to_radians() {
                            let to = if attack_start(t, w, blind) { ATTACK } else { SEARCH };
                            then(t, to);
                        }
                    }
                    hand_over(t, SEARCH)
                })
                .build(),
        ],
    );

    // StateFlick: shakes off what clings to it and knocks back what is near.
    s.set(
        FLICK,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_FLICK);
                })
                .on_act(move |t, w| {
                    if t.key(key::ACTION0) {
                        let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                        w.sim.shake_off(t.creature);
                        w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                        for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                        }
                        t.damage_count = 0.0;
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(if t.hp <= 0.0 {
                        DEAD
                    } else if !blind && change_navi(t, w) {
                        FIND
                    } else if matches!(t.action_state, SEARCH | TURN | WALK) {
                        t.action_state
                    } else {
                        WAIT
                    });
                    true
                })
                .on_finish(|t, _| t.action_state = -1)
                .build(),
        ],
    );

    // StateAttack: the tongue. Out from the clip's third key, anything it touches is caught; at the fifth a player in front is hurt; at the sixth
    // it flicks and draws back in.
    s.set(
        ATTACK,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.flags[0] = false;
                    t.flags[1] = false;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_ATTACK);
                })
                .on_act(|t, w| {
                    let reach = t.f(fp::ATTACK_RADIUS);
                    if t.key(key::ACTION1) {
                        t.flags[0] = true;
                    }
                    if t.flags[0] {
                        // The sim keeps Pikmin out of its body sphere, so the tongue's area starts past it.
                        let body = t.coll_radius.max((t.info.hi.y - t.info.lo.y) * 0.4);
                        let mouth = t.pos + t.forward() * (body + reach * 0.35);
                        if w.sim.swallow_pikis(t.creature, 0, mouth, reach * 0.5 + 20.0, 7) > 0 {
                            t.flags[1] = true;
                        }
                    }
                    if t.key(key::ACTION3) {
                        let (half, damage) = (t.f(fp::ATTACK_HIT_ANGLE).to_radians(), t.f(fp::ATTACK_DAMAGE));
                        for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= reach && t.calc_target_angle(n.pos) <= half) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 0.0 });
                        }
                    }
                    // The clip's sixth key (past the four the animator names): the tongue flicks and draws back in.
                    if t.flags[0] && t.anim.counter >= t.key_value(6) {
                        let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                        w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                        w.sim.shake_off(t.creature);
                        for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                        }
                        t.flags[0] = false;
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(if t.flags[1] { EAT } else { WAIT });
                    true
                })
                .on_finish(|t, _| t.flags[0] = false)
                .build(),
        ],
    );

    // StateEat: what the tongue brought in is swallowed when the clip is over.
    s.set(
        EAT,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_EAT);
                })
                .on_act(|t, w| {
                    if t.animation_finished() {
                        w.sim.chew(t.creature);
                        t.next_override = Some(if t.hp <= 0.0 { DEAD } else { WAIT });
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateLost: lost sight of the player; the tail fades back, and it goes home.
    s.set(
        LOST,
        vec![
            dead_check(),
            Action::new(WALK)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_OUT_VIEW);
                })
                .on_act(|t, _| t.animation_finished())
                .on_finish(|t, _| t.target_pos = t.nest)
                .build(),
        ],
    );
    let _ = (HOME_RADIUS, ANIM_FSEARCH, TICK);
    s
}
