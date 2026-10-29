//! The Burrowing Snagret of Pikmin 2 (`SnakeCrow::Obj : EnemyBase`, plugProjectNishimuraU/SnakeCrow.cpp, SnakeCrowState.cpp): a long-necked bird
//! that lives underground. When a Pikmin or player is in its territory it comes up beside them (`Stay`, `Appear1`, `Appear2`), watches
//! (`Wait`) and strikes with its head at whoever stands in one of five zones in front of it (`Attack`, one clip a zone), swallowing a
//! Pikmin (`Eat`) or hurting a player; Pikmin clinging to its head make it struggle (`Struggle`), and when it has had enough
//! blows, or nobody is in reach, it dives (`Disappear`) and waits underground again. Killed (`Dead`) it leaves a corpse.
//!
//! States (`SnakeCrow::StateID`): [`DEAD`], [`STAY`], [`APPEAR1`], [`APPEAR2`], [`DISAPPEAR`], [`WAIT`], [`ATTACK`], [`EAT`], [`STRUGGLE`]. Registers:
//! `reg[0]` the state's timer, `reg[1]` the zone it is attacking (`mAttackAnimIdx`).
//!
//! Host adaptations: the neck is not bent to reach (`SnakeJointMgr`: the clip's own head path stands in), so the head may not meet the Pikmin it
//! bites; the bite is a swallow at the zone's centre; a bite does not repeat in place on the clip's last key; the boss music, joint shadows and the
//! items it throws up when it dies are left out.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const STAY: i32 = 1;
pub const APPEAR1: i32 = 2;
pub const APPEAR2: i32 = 3;
pub const DISAPPEAR: i32 = 4;
pub const WAIT: i32 = 5;
pub const ATTACK: i32 = 6;
pub const EAT: i32 = 7;
pub const STRUGGLE: i32 = 8;

/// `SnakeCrow::AnimID`: the five attack clips follow the dive, one for each zone.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_APPEAR1: usize = 1;
pub const ANIM_APPEAR2: usize = 2;
pub const ANIM_DIVE: usize = 3;
pub const ANIM_ATTACK: usize = 4;
pub const ANIM_WAIT: usize = 9;
pub const ANIM_EAT: usize = 10;
pub const ANIM_STRUGGLE: usize = 11;

const TIMER: usize = 0;
const ZONE: usize = 1;

const PRIVATE_RADIUS: usize = 11;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
// Its own block (`ProperParms`).
const FAST_APPEAR: usize = fp::own::BASE + 1;
const WAIT_TIME: usize = fp::own::BASE + 11;
const UNDERGROUND_TIME: usize = fp::own::BASE + 12;

/// `setAttackPosition`'s centres of the zones, forward and sideways of the bird, and the boxes `getAttackPiki` tests: the forward range and the
/// sideways range (a Pikmin in the box is bitten with that zone's clip).
const CENTRES: [(f32, f32); 5] = [(40.0, 0.0), (120.0, 0.0), (190.0, 0.0), (90.0, 80.0), (90.0, -80.0)];
const FORWARD: [(f32, f32); 5] = [(0.0, 80.0), (80.0, 160.0), (160.0, 220.0), (50.0, 130.0), (50.0, 130.0)];
const SIDEWAYS: [(f32, f32); 5] = [(-30.0, 30.0), (-30.0, 30.0), (-30.0, 30.0), (50.0, 110.0), (-110.0, -50.0)];
const HEIGHT: f32 = 40.0;
/// How far from a zone's centre the head catches a Pikmin.
const BITE_RADIUS: f32 = 40.0;
/// Where the bird comes up from its target.
const APPEAR_DISTANCE: f32 = 120.0;

/// `getAttackPiki` / `getAttackNavi`: the zone (of `only`, or any) that holds someone, and who. Pikmin come first.
fn in_zone(t: &Teki, w: &World, only: Option<usize>) -> Option<(usize, Cand)> {
    let dir = dir_vec(t.yaw);
    let side = Vec3::new(-dir.z, 0.0, dir.x);
    let range = only.map_or(0..5, |i| i..i + 1);
    let find = |pikis: bool| {
        t.candidates(w).into_iter().filter(|c| c.piki.is_some() == pikis && !c.sticks_to_me).find_map(|c| {
            let sep = c.pos - t.pos;
            let (d, s) = (dir.dot(sep), side.dot(sep));
            range.clone().find(|&i| d > FORWARD[i].0 && d < FORWARD[i].1 && s > SIDEWAYS[i].0 && s < SIDEWAYS[i].1 && sep.y.abs() < HEIGHT).map(|i| (i, c))
        })
    };
    find(true).or_else(|| find(false))
}

fn centre(t: &Teki, zone: usize) -> Vec3 {
    let dir = dir_vec(t.yaw);
    let side = Vec3::new(-dir.z, 0.0, dir.x);
    t.pos + dir * CENTRES[zone].0 + side * CENTRES[zone].1
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

/// After a strike or a wait: what the bird does next with the clip over.
fn decide(t: &mut Teki, w: &World) -> i32 {
    if t.hp <= 0.0 {
        return DEAD;
    }
    if start_flick(t, w) {
        return DISAPPEAR;
    }
    if in_zone(t, w, None).is_some() {
        if clinging(t, w) > 0 { STRUGGLE } else { ATTACK }
    } else {
        WAIT
    }
}

/// The two ways up end the same way.
fn appear(clip: usize, next_to_surface: u8) -> Act {
    Action::new(WAIT)
        .on_start(move |t, _| {
            t.set_option(opt::VISIBLE | opt::ATARI);
            t.clear_option(opt::INVINCIBLE);
            t.target_vel = Vec3::ZERO;
            t.manual_speed = None;
            t.start_motion(clip);
        })
        .on_act(move |t, w| {
            if t.key(next_to_surface) {
                w.out.push(Out::Shake { pos: t.pos, strength: 1.0 });
            }
            if !t.animation_finished() {
                return false;
            }
            t.next_override = Some(decide(t, w));
            true
        })
        .build()
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(9, STAY);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE | opt::INVINCIBLE);
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.target_vel = Vec3::ZERO;
                w.sim.spit_out(t.creature);
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

    // StateStay: underground, the Appear1 clip held on its first frame. After the underground time, someone in its territory brings it up beside them.
    s.set(
        STAY,
        vec![
            dead_check(),
            Action::new(APPEAR1)
                .on_start(|t, _| {
                    t.reg[TIMER] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.set_option(opt::INVINCIBLE);
                    t.clear_option(opt::ATARI | opt::VISIBLE);
                    t.start_motion(ANIM_APPEAR1);
                    t.manual_speed = Some(0.0);
                })
                .on_act(|t, w| {
                    t.reg[TIMER] += w.dt;
                    if t.reg[TIMER] <= t.f(UNDERGROUND_TIME) {
                        return false;
                    }
                    let (home, r) = (t.nest, t.f(fp::TERRITORY));
                    // Pikmin first, then players, within its territory.
                    let found = {
                        let c = t.candidates(w);
                        c.iter().find(|c| c.piki.is_some() && dist_xz(home, c.pos) < r).or_else(|| c.iter().find(|c| c.piki.is_none() && dist_xz(home, c.pos) < r)).copied()
                    };
                    let Some(c) = found else { return false };
                    t.targets[0] = Some(c.target);
                    // `appearNearByTarget`: comes up 120 from them, facing them; if that is outside its territory, from the side nearer home.
                    let mut face = w.rng.range(0.0, std::f32::consts::TAU);
                    let mut at = c.pos - dir_vec(face) * APPEAR_DISTANCE;
                    if dist_xz(home, at) > r {
                        let toward = (c.pos.x - home.x).atan2(c.pos.z - home.z);
                        face = toward + w.rng.range(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
                        at = c.pos - dir_vec(face) * APPEAR_DISTANCE;
                    }
                    t.pos.x = at.x;
                    t.pos.z = at.z;
                    t.yaw = face;
                    t.next_override = Some(if w.rng.f32() < t.f(FAST_APPEAR) { APPEAR1 } else { APPEAR2 });
                    true
                })
                .on_finish(|t, _| t.manual_speed = None)
                .build(),
        ],
    );

    s.set(APPEAR1, vec![dead_check(), appear(ANIM_APPEAR1, key::ACTION0)]);
    s.set(APPEAR2, vec![dead_check(), appear(ANIM_APPEAR2, key::ACTION0)]);

    // StateWait: watches and turns to the nearest; strikes, or dives when nobody is near for the wait time or it has had enough blows.
    s.set(
        WAIT,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.reg[TIMER] = 0.0;
                    t.targets[0] = None;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_WAIT);
                })
                .on_act(|t, w| {
                    let (sight, half) = (t.f(fp::SIGHT_RADIUS), (t.f(fp::VIEW_ANGLE) * 0.5).to_radians());
                    let me = t.pos;
                    let seen = t.closest(w, |c| dist_xz(me, c.pos) <= sight && t.calc_target_angle(c.pos) <= half);
                    if let Some(c) = seen {
                        t.reg[TIMER] = 0.0;
                        turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    } else {
                        t.reg[TIMER] += w.dt;
                    }
                    if !t.anim.is_finished()
                        && (t.hp <= 0.0 || t.reg[TIMER] > t.f(WAIT_TIME) || start_flick(t, w) || in_zone(t, w, None).is_some())
                    {
                        t.finish_motion();
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(if t.reg[TIMER] > t.f(WAIT_TIME) { DISAPPEAR } else { decide(t, w) });
                    if t.hp <= 0.0 {
                        t.next_override = Some(DEAD);
                    }
                    true
                })
                .build(),
        ],
    );

    // StateAttack: the zone's clip; at its bite key whoever is in the zone is swallowed (a Pikmin) or hurt (a player).
    s.set(
        ATTACK,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, w| {
                    t.target_vel = Vec3::ZERO;
                    if let Some((z, _)) = in_zone(t, w, None) {
                        t.reg[ZONE] = z as f32;
                    }
                    t.start_motion(ANIM_ATTACK + t.reg[ZONE] as usize);
                })
                .on_act(|t, w| {
                    let zone = t.reg[ZONE] as usize;
                    if t.hp <= 0.0 {
                        t.finish_motion();
                    }
                    if t.key(key::ACTION1) {
                        match in_zone(t, w, Some(zone)) {
                            Some((_, c)) if c.piki.is_some() => {
                                w.sim.swallow_pikis(t.creature, 0, centre(t, zone), BITE_RADIUS, 1);
                            }
                            Some((_, c)) => {
                                if let Target::Navi(id) = c.target {
                                    w.out.push(Out::HitNavi { teki: t.id, navi: id, damage: t.f(fp::ATTACK_DAMAGE), from: t.pos, knock: 0.0 });
                                }
                                t.finish_motion();
                            }
                            None => {}
                        }
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    let swallowed = w.sim.pikis().iter().any(|p| matches!(p.state, libpikmin::PikiState::Swallowed { creature, .. } if creature == t.creature));
                    t.next_override = Some(if t.hp <= 0.0 {
                        DEAD
                    } else if swallowed {
                        EAT
                    } else {
                        decide(t, w)
                    });
                    true
                })
                .build(),
        ],
    );

    // StateEat: swallows what it holds at the clip's key.
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
                    if t.key(key::ACTION0) {
                        w.sim.chew(t.creature);
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(decide(t, w));
                    true
                })
                .build(),
        ],
    );

    // StateStruggle: shakes about for a second and a half while Pikmin cling to its head.
    s.set(
        STRUGGLE,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, _| {
                    t.reg[TIMER] = 0.0;
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_STRUGGLE);
                })
                .on_act(|t, w| {
                    if t.reg[TIMER] > 1.5 && !t.anim.is_finished() {
                        t.finish_motion();
                    }
                    t.reg[TIMER] += w.dt;
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(decide(t, w));
                    true
                })
                .build(),
        ],
    );

    // StateDisappear: dives, throwing off whatever is near or clinging at the clip's first key, and is under at its second.
    s.set(
        DISAPPEAR,
        vec![
            dead_check(),
            Action::new(STAY)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_DIVE);
                })
                .on_act(|t, w| {
                    if t.key(key::ACTION0) {
                        let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                        w.sim.shake_off(t.creature);
                        w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                        for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                        }
                    }
                    if t.key(key::ACTION1) {
                        t.set_option(opt::INVINCIBLE);
                        t.damage_count = 0.0;
                    }
                    t.animation_finished()
                })
                .on_finish(|t, w| {
                    w.sim.shake_off(t.creature);
                })
                .build(),
        ],
    );
    let _ = PRIVATE_RADIUS;
    s
}
