//! The Snagrets of Pikmin 2: the Burrowing Snagret (`SnakeCrow::Obj : EnemyBase`, plugProjectNishimuraU/SnakeCrow.cpp, SnakeCrowState.cpp): a long-necked bird
//! that lives underground. When a Pikmin or player is in its territory it comes up beside them (`Stay`, `Appear1`, `Appear2`), watches
//! (`Wait`) and strikes with its head at whoever stands in one of five zones in front of it (`Attack`, one clip a zone), swallowing a
//! Pikmin (`Eat`) or hurting a player; Pikmin clinging to its head make it struggle (`Struggle`), and when it has had enough
//! blows, or nobody is in reach, it dives (`Disappear`) and waits underground again. Killed (`Dead`) it leaves a corpse.
//!
//! States (`SnakeCrow::StateID`): [`DEAD`], [`STAY`], [`APPEAR1`], [`APPEAR2`], [`DISAPPEAR`], [`WAIT`], [`ATTACK`], [`EAT`], [`STRUGGLE`]. Registers:
//! `reg[0]` the state's timer, `reg[1]` the zone it is attacking (`mAttackAnimIdx`).
//!
//! The Pileated Snagret (`SnakeWhole::Obj`, SnakeWhole.cpp, SnakeWholeState.cpp) is the same bird with its body above ground: bigger zones, and it hops after what it
//! sees ([`WALK`]) and back to its territory ([`HOME`]); [`whole`] builds it.
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
/// The Pileated Snagret's hops: out to a target, and back home.
pub const WALK: i32 = 9;
pub const HOME: i32 = 10;

/// `SnakeCrow::AnimID`: the five attack clips follow the dive, one for each zone.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_APPEAR1: usize = 1;
pub const ANIM_APPEAR2: usize = 2;
pub const ANIM_DIVE: usize = 3;
pub const ANIM_ATTACK: usize = 4;
pub const ANIM_WAIT: usize = 9;
pub const ANIM_EAT: usize = 10;
pub const ANIM_STRUGGLE: usize = 11;
/// The Pileated Snagret's hop (`run1`).
pub const ANIM_JUMP: usize = 12;

const TIMER: usize = 0;
const ZONE: usize = 1;
const FACE_STEP: usize = 2;
const HOME_RADIUS: usize = 10;

const PRIVATE_RADIUS: usize = 11;
const SHAKE_KNOCK: usize = 17;
const SHAKE_DAMAGE: usize = 18;
const SHAKE_RANGE: usize = 19;
// Its own block (`ProperParms`).
const FAST_APPEAR: usize = fp::own::BASE + 1;
const WAIT_TIME: usize = fp::own::BASE + 11;
const UNDERGROUND_TIME: usize = fp::own::BASE + 12;

/// What differs between the two birds: `setAttackPosition`'s centres of the zones (forward and sideways of the bird), and the boxes
/// `getAttackPiki` tests (a Pikmin in a box is bitten with that zone's clip); and whether the bird walks (the Pileated Snagret hops).
pub struct Variant {
    centres: [(f32, f32); 5],
    forward: [(f32, f32); 5],
    sideways: [(f32, f32); 5],
    whole: bool,
}

const CROW: Variant = Variant {
    centres: [(40.0, 0.0), (120.0, 0.0), (190.0, 0.0), (90.0, 80.0), (90.0, -80.0)],
    forward: [(0.0, 80.0), (80.0, 160.0), (160.0, 220.0), (50.0, 130.0), (50.0, 130.0)],
    sideways: [(-30.0, 30.0), (-30.0, 30.0), (-30.0, 30.0), (50.0, 110.0), (-110.0, -50.0)],
    whole: false,
};

const WHOLE: Variant = Variant {
    centres: [(60.0, 0.0), (150.0, 0.0), (220.0, 0.0), (120.0, 80.0), (120.0, -80.0)],
    forward: [(0.0, 120.0), (120.0, 180.0), (180.0, 260.0), (80.0, 160.0), (80.0, 160.0)],
    sideways: [(-30.0, 30.0), (-30.0, 30.0), (-30.0, 30.0), (50.0, 110.0), (-110.0, -50.0)],
    whole: true,
};
const HEIGHT: f32 = 40.0;
/// How far from a zone's centre the head catches a Pikmin.
const BITE_RADIUS: f32 = 40.0;
/// Where the bird comes up from its target.
const APPEAR_DISTANCE: f32 = 120.0;

/// `getAttackPiki` / `getAttackNavi`: the zone (of `only`, or any) that holds someone, and who. Pikmin come first.
fn in_zone(v: &Variant, t: &Teki, w: &World, only: Option<usize>) -> Option<(usize, Cand)> {
    let dir = dir_vec(t.yaw);
    let side = Vec3::new(-dir.z, 0.0, dir.x);
    let range = only.map_or(0..5, |i| i..i + 1);
    let find = |pikis: bool| {
        t.candidates(w).into_iter().filter(|c| c.piki.is_some() == pikis && !c.sticks_to_me).find_map(|c| {
            let sep = c.pos - t.pos;
            let (d, s) = (dir.dot(sep), side.dot(sep));
            range.clone().find(|&i| d > v.forward[i].0 && d < v.forward[i].1 && s > v.sideways[i].0 && s < v.sideways[i].1 && sep.y.abs() < HEIGHT).map(|i| (i, c))
        })
    };
    find(true).or_else(|| find(false))
}

fn centre(v: &Variant, t: &Teki, zone: usize) -> Vec3 {
    let dir = dir_vec(t.yaw);
    let side = Vec3::new(-dir.z, 0.0, dir.x);
    t.pos + dir * v.centres[zone].0 + side * v.centres[zone].1
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
fn decide(v: &Variant, t: &mut Teki, w: &World) -> i32 {
    if t.hp <= 0.0 {
        return DEAD;
    }
    if start_flick(t, w) {
        return DISAPPEAR;
    }
    if in_zone(v, t, w, None).is_some() {
        return if clinging(t, w) > 0 { STRUGGLE } else { ATTACK };
    }
    if v.whole {
        if dist_xz(t.pos, t.nest) > t.f(fp::TERRITORY) {
            return HOME;
        }
        if seen(t, w).is_some() {
            return WALK;
        }
    }
    WAIT
}

/// `setJumpMove`: the hop's turn, a 22nd of the angle a tick (at most a quarter turn's worth), and its speed if it already faces the goal.
fn jump_towards(t: &mut Teki, goal: Vec3) {
    let want = t.calc_target_direction(goal);
    let diff = nearer_direction(t.direction(), want) - t.direction();
    let step = if diff.abs() > std::f32::consts::FRAC_PI_2 { std::f32::consts::FRAC_PI_2 * diff.signum() } else { diff };
    t.reg[FACE_STEP] = step / 22.0;
    if diff.abs() <= 30.0f32.to_radians() {
        let flat = (goal - t.pos).with_y(0.0);
        let dist = flat.length();
        let speed = dist.min(t.f(fp::MOVE_SPEED)) * (11.0 / 15.0);
        t.target_vel = if dist > 1e-3 { flat / dist * speed } else { Vec3::ZERO };
    } else {
        t.target_vel = Vec3::ZERO;
    }
}

/// `getSearchedTarget`: the nearest in sight (all round while Pikmin cling to it).
fn seen(t: &Teki, w: &World) -> Option<Cand> {
    let (sight, angle) = (t.f(fp::SIGHT_RADIUS), if clinging(t, w) > 0 { 180.0f32 } else { t.f(fp::VIEW_ANGLE) });
    let (me, half) = (t.pos, (angle * 0.5).to_radians());
    t.closest(w, |c| dist_xz(me, c.pos) <= sight && t.calc_target_angle(c.pos) <= half)
}

/// The two ways up end the same way.
fn appear(v: &'static Variant, clip: usize, next_to_surface: u8) -> Act {
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
            t.next_override = Some(decide(v, t, w));
            true
        })
        .build()
}

/// The Burrowing Snagret.
pub fn strategy() -> Strategy {
    build(&CROW)
}

/// The Pileated Snagret (`SnakeWhole::Obj`): the same bird with its body out of the ground, hopping after what it sees and back home.
pub fn whole() -> Strategy {
    build(&WHOLE)
}

fn build(v: &'static Variant) -> Strategy {
    let mut s = Strategy::new(11, STAY);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.action_state = -1;
        t.set_option(opt::DAMAGE_COUNTABLE | opt::INVINCIBLE);
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(move |t, w| {
                t.target_vel = Vec3::ZERO;
                w.sim.spit_out(t.creature);
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(ANIM_DEAD);
            })
            .on_act(move |t, w| {
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
                .on_act(move |t, w| {
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

    s.set(APPEAR1, vec![dead_check(), appear(v, ANIM_APPEAR1, key::ACTION0)]);
    s.set(APPEAR2, vec![dead_check(), appear(v, ANIM_APPEAR2, key::ACTION0)]);

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
                .on_act(move |t, w| {
                    let (sight, half) = (t.f(fp::SIGHT_RADIUS), (t.f(fp::VIEW_ANGLE) * 0.5).to_radians());
                    let me = t.pos;
                    let seen = t.closest(w, |c| dist_xz(me, c.pos) <= sight && t.calc_target_angle(c.pos) <= half);
                    if let Some(c) = seen {
                        if !v.whole {
                            t.reg[TIMER] = 0.0;
                        }
                        turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    } else {
                        t.reg[TIMER] += w.dt;
                    }
                    if !t.anim.is_finished()
                        && (t.hp <= 0.0 || t.reg[TIMER] > t.f(WAIT_TIME) || start_flick(t, w) || in_zone(v, t, w, None).is_some())
                    {
                        t.finish_motion();
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(if t.reg[TIMER] > t.f(WAIT_TIME) { DISAPPEAR } else { decide(v, t, w) });
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
                .on_start(move |t, w| {
                    t.target_vel = Vec3::ZERO;
                    if let Some((z, _)) = in_zone(v, t, w, None) {
                        t.reg[ZONE] = z as f32;
                    }
                    t.start_motion(ANIM_ATTACK + t.reg[ZONE] as usize);
                })
                .on_act(move |t, w| {
                    let zone = t.reg[ZONE] as usize;
                    if t.hp <= 0.0 {
                        t.finish_motion();
                    }
                    if t.key(key::ACTION1) {
                        match in_zone(v, t, w, Some(zone)) {
                            Some((_, c)) if c.piki.is_some() => {
                                w.sim.swallow_pikis(t.creature, 0, centre(v, t, zone), BITE_RADIUS, 1);
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
                        decide(v, t, w)
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
                .on_act(move |t, w| {
                    if t.key(key::ACTION0) {
                        w.sim.chew(t.creature);
                    }
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(decide(v, t, w));
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
                .on_act(move |t, w| {
                    if t.reg[TIMER] > 1.5 && !t.anim.is_finished() {
                        t.finish_motion();
                    }
                    t.reg[TIMER] += w.dt;
                    if !t.animation_finished() {
                        return false;
                    }
                    t.next_override = Some(decide(v, t, w));
                    true
                })
                .build(),
        ],
    );

    // StateWalk / StateHome (the Pileated one): hops towards what it sees, or home. A hop starts at the clip's first key, aimed
    // (`setJumpMove`: it only goes if within 30 degrees of facing it; otherwise it turns a bit each tick of the hop) and ends at the second.
    if v.whole {
        for (state, home) in [(WALK, false), (HOME, true)] {
            s.set(
                state,
                vec![
                    dead_check(),
                    Action::new(state)
                        .on_start(|t, _| {
                            t.action_state = -1;
                            t.flags[0] = false;
                            t.reg[FACE_STEP] = 0.0;
                            t.target_vel = Vec3::ZERO;
                            t.start_motion(ANIM_JUMP);
                        })
                        .on_act(move |t, w| {
                            let mut redirect = |t: &mut Teki, to: i32| {
                                if t.action_state < 0 || to == DEAD {
                                    t.action_state = to;
                                    t.finish_motion();
                                }
                            };
                            if t.hp <= 0.0 {
                                redirect(t, DEAD);
                            } else if start_flick(t, w) {
                                redirect(t, DISAPPEAR);
                            } else if home && dist_xz(t.pos, t.nest) < t.f(HOME_RADIUS) {
                                t.target_vel = Vec3::ZERO;
                                redirect(t, WAIT);
                            } else if !home && dist_xz(t.pos, t.nest) > t.f(fp::TERRITORY) {
                                redirect(t, HOME);
                            } else if !home && in_zone(v, t, w, None).is_some() {
                                redirect(t, ATTACK);
                            }
                            if t.flags[0] {
                                t.yaw = (t.yaw + t.reg[FACE_STEP]).rem_euclid(std::f32::consts::TAU);
                            } else {
                                t.target_vel = Vec3::ZERO;
                            }
                            if t.key(key::ACTION0) {
                                t.flags[0] = true;
                                t.reg[FACE_STEP] = 0.0;
                                w.out.push(Out::Sound { name: "SNAKEWHOLE_JUMP", pos: t.pos });
                                let goal = if home { Some(t.nest) } else { seen(t, w).map(|c| c.pos) };
                                match goal {
                                    Some(goal) => jump_towards(t, goal),
                                    None => redirect(t, WAIT),
                                }
                            }
                            if t.key(key::ACTION1) {
                                t.flags[0] = false;
                            }
                            if t.animation_finished() {
                                t.next_override = Some(match t.action_state {
                                    ATTACK if clinging(t, w) > 0 => STRUGGLE,
                                    a if a >= 0 => a,
                                    _ => state,
                                });
                                return true;
                            }
                            false
                        })
                        .on_finish(|t, _| {
                            t.flags[0] = false;
                            t.target_vel = Vec3::ZERO;
                        })
                        .build(),
                ],
            );
        }
    }

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
                .on_act(move |t, w| {
                    if t.key(key::ACTION0) {
                        let (range, knock, damage) = (t.f(SHAKE_RANGE), t.f(SHAKE_KNOCK), t.f(SHAKE_DAMAGE));
                        w.sim.shake_off(t.creature);
                        w.sim.flick_pikis_in_sphere(t.pos, range, damage, t.pos);
                        for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range) {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock });
                        }
                    }
                    // Under: the Burrowing one at the clip's second key, the Pileated one at its fourth.
                    if t.key(if v.whole { key::ACTION3 } else { key::ACTION1 }) {
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
