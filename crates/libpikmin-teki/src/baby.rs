//! The Bulborb Larva of Pikmin 2 (`Baby::Obj : EnemyBase`, plugProjectNishimuraU/Baby.cpp, BabyState.cpp). It hatches
//! from an egg (`Born`), then chases the nearest Pikmin or player in its sight (`Move`) and bites (`Attack`); it dies
//! when its health is gone or something lands on it (`Press`), possibly leaving a drop of nectar. It leaves no corpse
//! (`disableEvent(EB_LeaveCarcass)`).
//!
//! States (`BabyStateID`): [`DEAD`], [`PRESS`], [`BORN`], [`MOVE`], [`ATTACK`]. The target is `targets[0]`.
//!
//! Host adaptations: the bite hurts the Pikmin at its mouth instead of seating them in a mouth slot and swallowing them
//! with poison (`EnemyFunc::eatPikmin`, `swallowPikmin`); the nectar falls in the sim like the Honeywisp's.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const PRESS: i32 = 1;
pub const BORN: i32 = 2;
pub const MOVE: i32 = 3;
pub const ATTACK: i32 = 4;

/// `BabyAnimID`.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_DEAD_PRESS: usize = 1;
pub const ANIM_MOVE: usize = 2;
pub const ANIM_ATTACK: usize = 3;
pub const ANIM_ATTACK_FAIL: usize = 4;
pub const ANIM_BORN: usize = 5;

/// `ProperParms::mNectarChance` (`fp11`).
const NECTAR_CHANCE: usize = fp::own::BASE + 11;

/// Hurt this tick: the damage waits until the state looks at it (`mHealth` goes down at once in the game).
fn take_hits(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

/// `Obj::createHoney`: sometimes a drop of nectar.
fn create_honey(t: &Teki, w: &mut World) {
    if w.rng.f32() < t.f(NECTAR_CHANCE) {
        w.sim.spawn_nectar(t.pos + Vec3::Y * 4.0);
    }
}

/// `deathProcedure`: no longer alive to the world, though it plays out its death.
fn death_procedure(t: &mut Teki, w: &mut World) {
    t.clear_option(opt::ATARI);
    w.out.push(Out::Dying { teki: t.id });
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(5, BORN);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.targets = [None; 2];
    }));

    // StateDead / StatePress: a drop of nectar, the motion, then gone.
    let dying = |anim: usize, hit: bool| {
        Action::new(NO_TRANSIT)
            .on_start(move |t, w| {
                create_honey(t, w);
                if hit {
                    t.hp = 0.0;
                    w.out.push(Out::Sound { name: "BABY_PRESSED", pos: t.pos });
                }
                death_procedure(t, w);
                t.target_vel = Vec3::ZERO;
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
    s.set(PRESS, vec![dying(ANIM_DEAD_PRESS, true)]);

    // StateBorn: the hatching hop; once down it slows and the motion plays out.
    s.set(
        BORN,
        vec![Action::new(MOVE)
            .on_start(|t, _| t.start_motion(ANIM_BORN))
            .on_act(|t, _| {
                take_hits(t);
                if t.height() <= 0.5 {
                    t.target_vel.x *= 0.95;
                    t.target_vel.z *= 0.95;
                    t.finish_motion();
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { MOVE });
                    return true;
                }
                false
            })
            .build()],
    );

    // StateMove: head for the nearest Pikmin or player in sight, slowly while not facing it, and bite when close.
    s.set(
        MOVE,
        vec![
            Action::new(PRESS).on_event(|_, _, e| matches!(e, Event::Pressed)).build(),
            Action::new(ATTACK)
                .on_start(|t, _| {
                    t.targets[0] = None;
                    t.start_motion(ANIM_MOVE);
                })
                .on_act(|t, w| {
                    take_hits(t);
                    if t.hp <= 0.0 {
                        t.next_override = Some(DEAD);
                        return true;
                    }
                    let (sight, view) = (t.f(fp::SIGHT_RADIUS), t.f(fp::VIEW_ANGLE).to_radians());
                    let me = t.pos;
                    let target = t.closest(w, |c| dist_xz(me, c.pos) <= sight).filter(|c| t.calc_target_angle(c.pos) <= view);
                    let mut attack = false;
                    if let Some(c) = target {
                        t.targets[0] = Some(c.target);
                        let left = turn_to_target(t, c.pos, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                        let within = left.abs() <= t.f(fp::MAX_ATTACK_ANGLE).to_radians();
                        let speed = t.f(fp::MOVE_SPEED);
                        set_target_speed(t, if within { speed } else { 0.25 * speed });
                        attack = (c.pos - t.pos).length() < t.f(fp::MAX_ATTACK_RANGE) && within;
                    } else {
                        // `moveNoTarget`: it stands still.
                        t.target_vel = Vec3::ZERO;
                    }
                    if attack {
                        return true;
                    }
                    // The walk loops: when the clip ends it starts again.
                    if t.animation_finished() {
                        t.start_motion(ANIM_MOVE);
                    }
                    false
                })
                .build(),
        ],
    );

    // StateAttack: the bite lands at the first key and swallows at the second.
    s.set(
        ATTACK,
        vec![Action::new(MOVE)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_ATTACK);
            })
            .on_act(|t, w| {
                take_hits(t);
                if t.key(key::ACTION0) {
                    // `attackNavi`, `eatPikmin`: whoever is within the attack radius in front of it.
                    let (radius, half) = (t.f(fp::ATTACK_RADIUS), t.f(fp::ATTACK_HIT_ANGLE).to_radians());
                    let damage = t.f(fp::ATTACK_DAMAGE);
                    for n in w.navis.iter().filter(|n| n.alive) {
                        if dist_xz(t.pos, n.pos) - n.size <= radius && t.calc_target_angle(n.pos) <= half {
                            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 0.0 });
                        }
                    }
                    let ahead = t.pos + t.forward() * (radius * 0.5);
                    let bitten = w.sim.attack_pikis_in_sphere(ahead, radius, damage * 10.0);
                    if bitten == 0 {
                        t.start_motion(ANIM_ATTACK_FAIL);
                    }
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { MOVE });
                    return true;
                }
                false
            })
            .build()],
    );
    s
}
