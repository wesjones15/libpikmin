//! The Cloaking Burrow-nit of Pikmin 2 (`Armor::Obj : EnemyBase`, plugProjectNishimuraU/Armor.cpp, ArmorState.cpp): a burrowing
//! beetle in a rocky shell. It lies buried (`Stay`) until a player or Pikmin is in its sight, bursts out (`Appear`), hunts what it saw
//! (`Move`), bites it (`Attack2`) and eats what it caught (`Eat`) or snaps at nothing (`Fail`), shakes off the Pikmin that cling to it
//! (`Flick`), and when it has lost its prey or strayed from its territory goes home and dives again (`GoHome`, `Dive`). Killed it leaves a
//! corpse (`Dead`).
//!
//! States (`ArmorStateID`, in the order the FSM registers them): [`DEAD`], [`STAY`], [`APPEAR`], [`DIVE`], [`MOVE`], [`GO_HOME`], [`ATTACK2`],
//! [`EAT`], [`FLICK`], [`FAIL`]; ids 5-7 (`MoveSide`, `MoveCentre`, `MoveTop`) and `Attack1` are its bridge-gnawing in the story mode
//! (`isBreakBridge`), which has no bridges to work on here and is left out. Registers: `reg[0]` the attack loop time, `action_state` the
//! state its clip hands over to, `flags[0]` it caught a Pikmin in this bite; `targets[0]` the target.
//!
//! Host adaptations: the Pikmin it bites die at once rather than sit in a mouth slot until the `Eat` key; its bite winds up for
//! at most a second where the disc's attack loop time is 300 seconds (a story-mode setting).

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const STAY: i32 = 1;
pub const APPEAR: i32 = 2;
pub const DIVE: i32 = 3;
pub const MOVE: i32 = 4;
pub const GO_HOME: i32 = 8;
pub const ATTACK2: i32 = 10;
pub const EAT: i32 = 11;
pub const FLICK: i32 = 12;
pub const FAIL: i32 = 13;

/// `ArmorAnimID`.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_APPEAR: usize = 1;
pub const ANIM_DIVE: usize = 2;
pub const ANIM_MOVE: usize = 3;
pub const ANIM_ATTACK2: usize = 5;
pub const ANIM_EAT: usize = 6;
pub const ANIM_FLICK: usize = 7;
pub const ANIM_FAIL: usize = 8;

const LOOP_T: usize = 0;
const HOME_RADIUS: usize = 10;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
const BLOWS: usize = 7;
/// Longest the bite winds up for.
const MAX_WIND_UP: f32 = 1.0;

fn sighted(t: &Teki, w: &World) -> Option<Cand> {
    let (sight, half) = (t.f(fp::SIGHT_RADIUS), (t.f(fp::VIEW_ANGLE) * 0.5).to_radians());
    let me = t.pos;
    t.closest(w, |c| dist_xz(me, c.pos) <= sight).filter(|c| t.calc_target_angle(c.pos) <= half)
}

fn in_reach(t: &Teki, c: &Cand) -> bool {
    (c.pos - t.pos).length() < t.f(fp::MAX_ATTACK_RANGE) && t.calc_target_angle(c.pos) <= t.f(fp::MAX_ATTACK_ANGLE).to_radians()
}

fn check(t: &mut Teki, w: &World) -> bool {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
    if t.hp <= 0.0 {
        t.next_override = Some(DEAD);
        return true;
    }
    let _ = w;
    false
}

fn flick_due(t: &Teki) -> bool {
    t.damage_count > t.i(BLOWS) as f32
}

fn done(t: &mut Teki, default: i32) -> bool {
    if t.animation_finished() {
        t.next_override = Some(if t.action_state >= 0 { t.action_state } else { default });
        return true;
    }
    false
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(14, STAY);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
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

    // StateStay: buried, the Appear clip held on its first frame.
    s.set(
        STAY,
        vec![Action::new(APPEAR)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.clear_option(opt::ATARI | opt::VISIBLE);
                t.start_motion(ANIM_APPEAR);
                t.manual_speed = Some(0.0);
            })
            .on_act(|t, w| match sighted(t, w) {
                Some(c) => {
                    t.targets[0] = Some(c.target);
                    true
                }
                None => false,
            })
            .on_finish(|t, _| {
                t.set_option(opt::ATARI | opt::VISIBLE);
                t.manual_speed = None;
            })
            .build()],
    );

    s.set(
        APPEAR,
        vec![Action::new(MOVE)
            .on_start(|t, w| {
                t.target_vel = Vec3::ZERO;
                t.set_option(opt::INVINCIBLE);
                t.start_motion(ANIM_APPEAR);
                w.out.push(Out::Shake { pos: t.pos, strength: 1.0 });
            })
            .on_act(|t, _| {
                if t.stored_damage > 0.0 {
                    t.stored_damage = 0.0;
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { MOVE });
                    return true;
                }
                false
            })
            .on_finish(|t, _| t.clear_option(opt::INVINCIBLE))
            .build()],
    );

    s.set(
        DIVE,
        vec![Action::new(STAY)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.set_option(opt::INVINCIBLE);
                t.start_motion(ANIM_DIVE);
            })
            .on_act(|t, _| t.animation_finished())
            .on_finish(|t, _| t.clear_option(opt::INVINCIBLE))
            .build()],
    );

    // StateMove: runs at the nearest target; bites when it is in reach, goes home when too far from it or with nothing to hunt.
    s.set(
        MOVE,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.action_state = -1;
                t.start_motion(ANIM_MOVE);
            })
            .on_act(|t, w| {
                if check(t, w) {
                    return true;
                }
                if flick_due(t) {
                    t.damage_count = 0.0;
                    t.next_override = Some(FLICK);
                    return true;
                }
                if t.action_state < 0 {
                    if let Some(c) = sighted(t, w) {
                        t.targets[0] = Some(c.target);
                        turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                        set_target_speed(t, t.f(fp::MOVE_SPEED));
                        if in_reach(t, &c) {
                            t.action_state = ATTACK2;
                            t.finish_motion();
                        } else if dist_xz(t.pos, t.nest) > t.f(fp::TERRITORY) {
                            t.action_state = GO_HOME;
                            t.finish_motion();
                        }
                    } else {
                        t.action_state = GO_HOME;
                        t.finish_motion();
                    }
                    if t.action_state >= 0 {
                        t.target_vel = Vec3::ZERO;
                    }
                }
                done(t, GO_HOME)
            })
            .build()],
    );

    // StateGoHome: walks home; bites anything that comes into reach; dives at home.
    s.set(
        GO_HOME,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.action_state = -1;
                t.start_motion(ANIM_MOVE);
            })
            .on_act(|t, w| {
                let home = t.nest;
                if t.action_state < 0 {
                    turn_to_target(t, home, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    set_target_speed(t, t.f(fp::MOVE_SPEED));
                    let me = t.pos;
                    let reach = t.f(fp::MAX_ATTACK_RANGE);
                    if t.closest(w, |c| dist_xz(me, c.pos) <= reach).is_some_and(|c| in_reach(t, &c)) {
                        t.action_state = ATTACK2;
                        t.finish_motion();
                    } else if dist_xz(t.pos, home) < t.f(HOME_RADIUS) {
                        t.action_state = DIVE;
                        t.finish_motion();
                    } else if sighted(t, w).is_some() {
                        t.action_state = MOVE;
                        t.finish_motion();
                    }
                    if t.action_state >= 0 {
                        t.target_vel = Vec3::ZERO;
                    }
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.action_state >= 0 { t.action_state } else { DIVE });
                    return true;
                }
                if check(t, w) {
                    return true;
                }
                if flick_due(t) {
                    t.damage_count = 0.0;
                    t.next_override = Some(FLICK);
                    return true;
                }
                false
            })
            .build()],
    );

    // StateAttack2: winds up, then bites at the clip's keys.
    s.set(
        ATTACK2,
        vec![Action::new(FAIL)
            .on_start(|t, _| {
                t.reg[LOOP_T] = 0.0;
                t.flags[0] = false;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_ATTACK2);
            })
            .on_act(|t, w| {
                let frame = t.anim.counter;
                if frame > 17.0 && frame < 27.0 {
                    // `attackPikmin`: what is in front of the mouth is caught.
                    let mouth = t.pos + t.forward() * (t.f(fp::MAX_ATTACK_RANGE) * 0.5);
                    if w.sim.attack_pikis_in_sphere(mouth, t.f(fp::ATTACK_RADIUS) * 0.5, 1000.0) > 0 {
                        t.flags[0] = true;
                    }
                }
                if t.reg[LOOP_T] > t.f(fp::own::BASE + 1).min(MAX_WIND_UP) {
                    t.finish_motion();
                }
                t.reg[LOOP_T] += w.dt;
                if t.key(key::ACTION1) {
                    let (radius, half, damage) = (t.f(fp::ATTACK_RADIUS), t.f(fp::ATTACK_HIT_ANGLE).to_radians(), t.f(fp::ATTACK_DAMAGE));
                    for n in w.navis.iter().filter(|n| n.alive) {
                        if dist_xz(t.pos, n.pos) - n.size <= radius && t.calc_target_angle(n.pos) <= half {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 0.0 });
                        }
                    }
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 {
                        DEAD
                    } else if t.flags[0] {
                        EAT
                    } else {
                        FAIL
                    });
                    return true;
                }
                false
            })
            .build()],
    );

    s.set(
        EAT,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_EAT);
            })
            .on_act(|t, w| check(t, w) || t.animation_finished())
            .build()],
    );

    s.set(
        FAIL,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FAIL);
            })
            .on_act(|t, w| check(t, w) || t.animation_finished())
            .build()],
    );

    s.set(
        FLICK,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
            })
            .on_act(|t, w| {
                if check(t, w) {
                    return true;
                }
                if t.key(key::ACTION0) {
                    let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                    w.sim.shake_off(t.creature);
                    w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                    for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                    }
                    t.damage_count = 0.0;
                }
                t.animation_finished()
            })
            .build()],
    );
    for st in [5, 6, 7, 9] {
        s.set(st, Vec::new());
    }
    s
}
