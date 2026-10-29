//! The Bulborbs of Pikmin 2 that Pikmin 1 does not have: the Orange Bulborb (`BlueChappy`), the Hairy Bulborb
//! (`YellowChappy`) and their dwarfs, the Dwarf Orange Bulborb (`BlueKochappy`) and the Snow Bulborb (`YellowKochappy`).
//! Spec: `ChappyBase::Obj` (plugProjectYamashitaU/ChappyBase.cpp, chappyState.cpp) for the large ones and
//! `KochappyBase::Obj` (KochappyBase.cpp, kochappyState.cpp) for the dwarfs; the four share the Red Bulborbs' model and
//! clips with another texture, and their own numbers from the disc.
//!
//! A Bulborb sleeps at home ([`SLEEP`]) until it is woken (the Orange one by a player within its wake radius, the others
//! by being hit or bumped), turns on the nearest Pikmin or player it notices ([`TURN`]), walks to it ([`WALK`]) and bites
//! ([`ATTACK`]); it shakes off Pikmin that cling to it ([`FLICK`]), gives up and goes home ([`TURN_HOME`], [`GO_HOME`])
//! when it has lost its target or strayed from its territory, and dies ([`DEAD`]), leaving a corpse.
//!
//! Registers: `reg[0]` the alert timer (`mAlertTimer`: it sees all round while it is below `mAlertDuration`), `reg[1]` the
//! view angle and `reg[2]` the search angle it is using now; `action_state` the state its clip hands over to; `targets[0]`
//! its target. The dwarfs are kinds [`crate::p2::DWARF_ORANGE_BULBORB`] and [`crate::p2::SNOW_BULBORB`].
//!
//! Host adaptations: the dwarfs run the large Bulborbs' machine (the decomp has separate `KochappyBase` states: a `Wait` in place of
//! the sleep, a `Press` squash, a short absent-minded pause); a bite kills the Pikmin in front of the mouth instead of seating them in a
//! mouth slot and swallowing them (`eatPikmin`, `swallowPikmin`, with the White Pikmin's poison); the Hairy Bulborb's fading hair
//! (`TKechappyTest`) and the sleeping smoke are particle effects, so none is emitted.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

/// `ChappyStateID`.
pub const TURN: i32 = 0;
pub const DEAD: i32 = 1;
pub const FLICK: i32 = 2;
pub const WALK: i32 = 3;
pub const ATTACK: i32 = 4;
pub const TURN_HOME: i32 = 5;
pub const GO_HOME: i32 = 6;
pub const SLEEP: i32 = 7;

/// `ChappyAnimID` (`KochappyAnimID` is the same order).
pub const ANIM_ATTACK: usize = 0;
pub const ANIM_DEAD: usize = 1;
pub const ANIM_FLICK: usize = 2;
pub const ANIM_MOVE: usize = 3;
pub const ANIM_SLEEP: usize = 4;
pub const ANIM_WAIT: usize = 6;
pub const ANIM_TURN: usize = 7;
pub const ANIM_FAIL: usize = 8;

const ALERT: usize = 0;
const VIEW: usize = 1;
const SEARCH: usize = 2;

// `EnemyParmsBase` indices not named in `p2::fp`.
const HOME_RADIUS: usize = 10;
const PRIVATE_RADIUS: usize = 11;
const FOV: usize = 25;
const SEARCH_ANGLE: usize = 15;
const SEARCH_DISTANCE: usize = 14;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
const ALERT_LIFE: usize = 30;
const ALERT_DURATION: usize = 29;
const ATTACK_HIT_ANGLE: usize = fp::ATTACK_HIT_ANGLE;
// `ChappyBase::ProperParms`: `fp01` foot range, `fp03` wake radius (Orange).
const FOOT_RANGE: usize = fp::own::BASE + 1;
const WAKE_RADIUS: usize = fp::own::BASE + 3;
// `EnemyParmsBase` ip01..ip07: the blows (`ip01`, `ip03`, `ip05`, `ip07`) and Pikmin counts (`ip02`, `ip04`, `ip06`).
const BLOWS: [usize; 4] = [1, 3, 5, 7];
const STICKS: [usize; 3] = [2, 4, 6];

fn orange(t: &Teki) -> bool {
    t.kind == crate::p2::ORANGE_BULBORB
}

fn dwarf(t: &Teki) -> bool {
    matches!(t.kind, crate::p2::DWARF_ORANGE_BULBORB | crate::p2::SNOW_BULBORB)
}

/// `StateCautionBase::cautionProc`.
fn caution(t: &mut Teki, w: &World) {
    let near = |r: f32| w.navis.iter().any(|n| n.alive && (n.pos - t.pos).length() < r) || w.sim.pikis().iter().any(|p| p.is_alive() && (p.pos - t.pos).length() < r);
    let radius = if orange(t) { t.f(WAKE_RADIUS) } else { t.f(PRIVATE_RADIUS) };
    if near(radius) || t.hp < t.f(ALERT_LIFE) {
        t.reg[ALERT] = 0.0;
    }
    if t.reg[ALERT] < t.f(ALERT_DURATION) {
        t.reg[ALERT] += w.dt;
        t.reg[VIEW] = 180.0;
        t.reg[SEARCH] = 180.0;
    } else {
        t.reg[VIEW] = t.f(fp::VIEW_ANGLE);
        t.reg[SEARCH] = t.f(SEARCH_ANGLE);
    }
}

/// `EnemyFunc::getNearestPikminOrNavi(search angle, search distance)`.
fn nearest(t: &Teki, w: &World) -> Option<Cand> {
    let (dist, half) = (t.f(SEARCH_DISTANCE), (t.reg[SEARCH] * 0.5).to_radians());
    let me = t.pos;
    t.closest(w, |c| dist_xz(me, c.pos) <= dist).filter(|c| t.calc_target_angle(c.pos) <= half)
}

/// `isTargetAttackable`: within the bite's range and angle.
fn attackable(t: &Teki, c: &Cand) -> bool {
    (c.pos - t.pos).length() < t.f(fp::MAX_ATTACK_RANGE) && t.calc_target_angle(c.pos) <= t.f(fp::MAX_ATTACK_ANGLE).to_radians()
}

/// `isTargetOutOfRange`: beyond sight (or above/below its reach), or outside its view while not at its feet.
fn out_of_range(t: &Teki, c: &Cand) -> bool {
    let d = dist_xz(t.pos, c.pos);
    d > t.f(fp::SIGHT_RADIUS) || (c.pos.y - t.pos.y).abs() > t.f(FOV) * 4.0 || (d > t.f(PRIVATE_RADIUS) && t.calc_target_angle(c.pos) > (t.reg[VIEW] * 0.5).to_radians())
}

/// `EnemyFunc::isStartFlick`: enough hits for the number of Pikmin clinging.
fn start_flick(t: &Teki, w: &World) -> bool {
    let stuck = w.sim.pikis().iter().filter(|p| p.state.latched_to() == Some(t.creature)).count() as i32;
    let blow = if stuck < t.i(STICKS[0]) {
        BLOWS[0]
    } else if stuck < t.i(STICKS[1]) {
        BLOWS[1]
    } else if stuck < t.i(STICKS[2]) {
        BLOWS[2]
    } else {
        BLOWS[3]
    };
    t.damage_count > t.i(blow) as f32
}

fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

/// `StateTurnBase::turnToTarget`: slower the more Pikmin hang at its feet; true once it faces within the attack angle.
fn turn_base(t: &mut Teki, w: &World, target: Vec3) -> bool {
    let n = t.count_pikis_in_range(w, t.f(FOOT_RANGE)).min(10) as f32;
    let max = (1.0 - 0.9 * n / 10.0) * t.f(fp::MAX_TURN_ANGLE);
    let left = turn_to_target(t, target, t.f(fp::TURN_SPEED), max);
    left.abs() <= t.f(fp::MAX_ATTACK_ANGLE).to_radians()
}

fn hand_over(t: &mut Teki, next: i32, fast: bool) {
    t.action_state = next;
    t.manual_speed = fast.then_some(60.0);
    t.finish_motion();
}

fn go(t: &mut Teki) -> bool {
    if t.animation_finished() {
        t.manual_speed = None;
        t.next_override = Some(t.action_state.max(0));
        return true;
    }
    false
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(8, SLEEP);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.reg[ALERT] = t.f(ALERT_DURATION);
        t.reg[VIEW] = t.f(fp::VIEW_ANGLE);
        t.reg[SEARCH] = t.f(SEARCH_ANGLE);
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE);
    }));

    // StateSleep: curled up at home. The Orange one wakes when anyone comes within its wake radius, the others when they
    // are hit; a dwarf, which stands about instead, when it sees someone.
    s.set(
        SLEEP,
        vec![Action::new(TURN)
            .on_start(|t, _| {
                t.targets[0] = None;
                t.target_vel = Vec3::ZERO;
                t.action_state = -1;
                t.start_motion(if dwarf(t) { ANIM_WAIT } else { ANIM_SLEEP });
            })
            .on_act(|t, w| {
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                if t.action_state < 0 {
                    let wake = if dwarf(t) {
                        t.flash > 0.0 || {
                            let me = t.pos;
                            let sight = t.f(fp::SIGHT_RADIUS);
                            t.closest(w, |c| dist_xz(me, c.pos) <= sight).is_some()
                        }
                    } else if orange(t) {
                        let r = t.f(WAKE_RADIUS);
                        w.navis.iter().any(|n| n.alive && (n.pos - t.pos).length() < r) || w.sim.pikis().iter().any(|p| p.is_alive() && (p.pos - t.pos).length() < r)
                    } else {
                        t.flash > 0.0 || t.damage_count > 0.0
                    };
                    if wake {
                        hand_over(t, TURN, true);
                    }
                }
                go(t)
            })
            .build()],
    );

    // StateDead: the death clip (a dwarf pressed flat uses its press clip), then the dying notice and gone.
    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.clear_option(opt::ATARI);
                t.target_vel = Vec3::ZERO;
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(if dwarf(t) && t.flags[0] { ANIM_SLEEP } else { ANIM_DEAD });
            })
            .on_act(|t, w| {
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );

    // StateTurn: turns on the nearest target; attack if it can, walk if it faces him, home if he is out of range.
    s.set(
        TURN,
        vec![
            Action::new(DEAD).on_event(|t, _, e| {
                let pressed = matches!(e, Event::Pressed) && dwarf(t);
                if pressed {
                    t.flags[0] = true;
                    t.hp = 0.0;
                }
                pressed
            }).build(),
            Action::new(WALK)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.manual_speed = Some(40.0);
                    t.start_motion(ANIM_TURN);
                })
                .on_act(|t, w| {
                    caution(t, w);
                    hurt(t);
                    if t.hp <= 0.0 {
                        t.next_override = Some(DEAD);
                        return true;
                    }
                    if t.action_state < 0 {
                        if let Some(c) = nearest(t, w) {
                            t.targets[0] = Some(c.target);
                            if attackable(t, &c) {
                                hand_over(t, ATTACK, true);
                            } else if out_of_range(t, &c) {
                                hand_over(t, TURN_HOME, false);
                            } else if turn_base(t, w, c.pos) {
                                hand_over(t, WALK, false);
                            }
                        } else {
                            hand_over(t, TURN_HOME, false);
                        }
                        if start_flick(t, w) {
                            t.damage_count = 0.0;
                            t.next_override = Some(FLICK);
                            return true;
                        }
                    }
                    go(t)
                })
                .on_finish(|t, _| t.manual_speed = None)
                .build(),
        ],
    );

    // StateWalk: walks to the target; at its feet it bites, out of its sight it goes home, beside its view it turns.
    s.set(
        WALK,
        vec![
            Action::new(DEAD).on_event(|t, _, e| {
                let pressed = matches!(e, Event::Pressed) && dwarf(t);
                if pressed {
                    t.flags[0] = true;
                    t.hp = 0.0;
                }
                pressed
            }).build(),
            Action::new(ATTACK)
                .on_start(|t, _| {
                    t.action_state = -1;
                    t.manual_speed = Some(40.0);
                    t.start_motion(ANIM_MOVE);
                })
                .on_act(|t, w| {
                    caution(t, w);
                    hurt(t);
                    if t.hp <= 0.0 {
                        t.next_override = Some(DEAD);
                        return true;
                    }
                    if t.action_state < 0 {
                        if let Some(c) = nearest(t, w) {
                            t.targets[0] = Some(c.target);
                            let view = t.f(fp::VIEW_ANGLE).to_radians();
                            if attackable(t, &c) {
                                hand_over(t, ATTACK, true);
                                t.target_vel = Vec3::ZERO;
                            } else if out_of_range(t, &c) {
                                hand_over(t, TURN_HOME, false);
                                t.target_vel = Vec3::ZERO;
                            } else if t.calc_target_angle(c.pos) <= view {
                                turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                                set_target_speed(t, t.f(fp::MOVE_SPEED));
                            } else {
                                hand_over(t, TURN, false);
                                t.target_vel = Vec3::ZERO;
                            }
                        } else {
                            hand_over(t, TURN_HOME, false);
                        }
                        if t.action_state < 0 || t.action_state != TURN_HOME {
                            if dist_xz(t.pos, t.nest) > t.f(fp::TERRITORY) {
                                hand_over(t, TURN_HOME, false);
                                t.target_vel = Vec3::ZERO;
                            }
                        }
                        if start_flick(t, w) {
                            t.damage_count = 0.0;
                            t.target_vel = Vec3::ZERO;
                            t.next_override = Some(FLICK);
                            return true;
                        }
                    }
                    go(t)
                })
                .on_finish(|t, _| {
                    t.manual_speed = None;
                    t.target_vel = Vec3::ZERO;
                })
                .build(),
        ],
    );

    // StateAttack: the bite lands at the first key; with nothing in its mouth it shakes its head.
    s.set(
        ATTACK,
        vec![
            Action::new(DEAD).on_event(|t, _, e| {
                let pressed = matches!(e, Event::Pressed) && dwarf(t);
                if pressed {
                    t.flags[0] = true;
                    t.hp = 0.0;
                }
                pressed
            }).build(),
            Action::new(TURN)
                .on_start(|t, _| {
                    t.manual_speed = None;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_ATTACK);
                })
                .on_act(|t, w| {
                    caution(t, w);
                    if t.anim.motion == ANIM_ATTACK as i32 {
                        if t.key(key::ACTION0) {
                            let (radius, half) = (t.f(fp::ATTACK_RADIUS), t.f(ATTACK_HIT_ANGLE).to_radians());
                            let damage = t.f(fp::ATTACK_DAMAGE);
                            let mut hit = 0;
                            for n in w.navis.iter().filter(|n| n.alive) {
                                if dist_xz(t.pos, n.pos) - n.size <= radius && t.calc_target_angle(n.pos) <= half {
                                    w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 0.0 });
                                    hit += 1;
                                }
                            }
                            let ahead = t.pos + t.forward() * (radius * 0.6);
                            hit += w.sim.attack_pikis_in_sphere(ahead, radius * 0.7, 100.0);
                            if hit == 0 {
                                t.start_motion(ANIM_FAIL);
                            }
                            w.sim.shake_off(t.creature);
                        }
                        if t.key(key::ACTION2) || t.animation_finished() {
                            return true;
                        }
                    } else if t.animation_finished() {
                        return true;
                    }
                    hurt(t);
                    if t.hp <= 0.0 {
                        t.next_override = Some(DEAD);
                        return true;
                    }
                    false
                })
                .on_finish(|_, _| {})
                .build(),
        ],
    );
    // StateAttack::transitState: from the end of the bite, back to the nearest target, or home.
    // (Done as the action above's `next`: Turn; an attackable target there bites again at once.)

    // StateFlick: shakes everything off, then back to what it was doing.
    s.set(
        FLICK,
        vec![Action::new(RETURN_TRANSIT)
            .on_start(|t, _| {
                t.manual_speed = Some(40.0);
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
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
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                t.animation_finished()
            })
            .on_finish(|t, _| t.manual_speed = None)
            .build()],
    );

    // StateTurnToHome: at home it goes back to sleep; otherwise it turns toward home.
    s.set(
        TURN_HOME,
        vec![Action::new(GO_HOME)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.action_state = -1;
                t.targets[0] = None;
                if dist_xz(t.pos, t.nest) < t.f(HOME_RADIUS) {
                    t.next_override = Some(SLEEP);
                    t.action_state = SLEEP;
                }
                t.start_motion(ANIM_WAIT);
            })
            .on_act(|t, w| {
                caution(t, w);
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                if t.action_state == SLEEP {
                    t.next_override = Some(SLEEP);
                    return true;
                }
                if t.anim.motion == ANIM_WAIT as i32 {
                    if t.animation_finished() {
                        if t.flash > 0.0 {
                            t.next_override = Some(TURN);
                            return true;
                        }
                        t.start_motion(ANIM_TURN);
                    }
                    return false;
                }
                if start_flick(t, w) {
                    t.damage_count = 0.0;
                    t.next_override = Some(FLICK);
                    return true;
                }
                if t.action_state < 0 {
                    let home = t.nest;
                    if turn_base(t, w, home) {
                        hand_over(t, GO_HOME, false);
                    } else if let Some(c) = nearest(t, w) {
                        t.targets[0] = Some(c.target);
                        if attackable(t, &c) {
                            hand_over(t, ATTACK, true);
                        }
                    } else if t.flash > 0.0 {
                        hand_over(t, TURN, false);
                    }
                }
                go(t)
            })
            .on_finish(|t, _| t.manual_speed = None)
            .build()],
    );

    // StateGoHome: walks home; on the way it turns on whoever it notices.
    s.set(
        GO_HOME,
        vec![Action::new(SLEEP)
            .on_start(|t, _| {
                t.action_state = -1;
                t.start_motion(ANIM_MOVE);
            })
            .on_act(|t, w| {
                caution(t, w);
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                if start_flick(t, w) {
                    t.damage_count = 0.0;
                    t.next_override = Some(FLICK);
                    return true;
                }
                if t.action_state < 0 {
                    let home = t.nest;
                    turn_to_target(t, home, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    set_target_speed(t, t.f(fp::MOVE_SPEED));
                    if dist_xz(t.pos, home) < t.f(HOME_RADIUS) {
                        t.target_vel = Vec3::ZERO;
                        t.action_state = SLEEP;
                        t.finish_motion();
                    }
                    if let Some(c) = nearest(t, w) {
                        t.targets[0] = Some(c.target);
                        t.target_vel = Vec3::ZERO;
                        t.action_state = if attackable(t, &c) { ATTACK } else { WALK };
                        t.finish_motion();
                    } else if t.flash > 0.0 {
                        t.action_state = TURN;
                        t.finish_motion();
                    }
                }
                go(t)
            })
            .on_finish(|t, _| t.target_vel = Vec3::ZERO)
            .build()],
    );
    s
}
