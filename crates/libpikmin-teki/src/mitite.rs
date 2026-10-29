//! The Mitite of Pikmin 2 (`TamagoMushi::Obj : EnemyBase`, plugProjectMorimuraU/tamagoMushi.cpp, tamagoMushiState.cpp): a
//! swarm bug. A leader waits underground (`Appear`) until a player or Pikmin is within its appearance range, calls up ten
//! fellows, and they pop up, wander about their home in short walks with turns between (`Walk`, `Turn`), and dive away
//! when their survival time is out (`Hide`). They do no harm, only startling Pikmin (`InteractAstonish`, which the sim has
//! no panic for, so it is left out); hit or squashed they die (`Dead`), a drop of nectar comes out and they leave a corpse.
//!
//! States (`TamagoMushiStateID`): [`WALK`], [`TURN`], [`APPEAR`], [`HIDE`], [`DEAD`], [`WAIT`] (the ball a group falls in;
//! not used here). Registers: `reg[0]`/`reg[1]` the appear wait and its limit, `reg[2]`/`reg[3]` the walk timer and its
//! limit (all in ticks, as the game counts them), `reg[4]`/`reg[5]` the active counter and its limit, `reg[6]` the turn and
//! `reg[7]` the speed factor; `flags[0]` the fellows were called, `count[0]` 1 for a fellow (set by the host on the ones it
//! spawns for a leader), `target_pos` its goal.
//!
//! Host adaptations: no ball of fellows falling from the sky (`setTypeBall`), no wobble in the walk
//! (`walkFunc`'s `mMoveOffsetLevel` sway), and the fellows pop up in a ring around the leader.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::p2::{MITITE, fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const WALK: i32 = 0;
pub const TURN: i32 = 1;
pub const APPEAR: i32 = 2;
pub const HIDE: i32 = 3;
pub const DEAD: i32 = 4;
pub const WAIT: i32 = 5;

/// `TamagoAnimID` (the `enemyAnimMgr.txt` order): dead, dive, move, set, wait, carry.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_DIVE: usize = 1;
pub const ANIM_MOVE: usize = 2;
pub const ANIM_APPEAR: usize = 3;
pub const ANIM_WAIT: usize = 4;

const SURVIVAL: usize = fp::own::BASE + 1;
const APPEAR_RANGE: usize = fp::own::BASE + 2;
const HONEY_RATE: usize = fp::own::BASE + 3;
const MIN_WALK: usize = fp::own::BASE + 1;
const MAX_WALK: usize = fp::own::BASE + 2;
const MIN_APPEAR: usize = fp::own::BASE + 3;
const MAX_APPEAR: usize = fp::own::BASE + 4;
/// How many fellows a leader calls (`createGroup(this, 10, false)`).
const FELLOWS: u32 = 10;

fn in_ticks(w: &mut World, lo: i32, hi: i32) -> f32 {
    (((hi - lo).max(0) as f32) * w.rng.f32() + lo as f32).floor()
}

fn hurt_or_dead(t: &mut Teki) -> bool {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
    if t.hp <= 0.0 {
        t.next_override = Some(DEAD);
        return true;
    }
    false
}

/// `Obj::isFound`: a fellow goes with its leader; a leader needs a player or Pikmin within the appearance range.
fn is_found(t: &Teki, w: &World) -> bool {
    if t.count[0] == 1 {
        return true;
    }
    let r = t.f(APPEAR_RANGE);
    w.navis.iter().any(|n| (n.pos - t.pos).length() < r) || w.sim.pikis().iter().any(|p| p.is_alive() && (p.pos - t.pos).length() < r)
}

/// `Obj::setGoalRandom`: somewhere between half and all of the territory away from home.
fn set_goal_random(t: &mut Teki, w: &mut World) {
    let r = t.f(fp::TERRITORY) * (0.5 * w.rng.f32() + 0.5);
    let a = std::f32::consts::TAU * w.rng.f32();
    t.target_pos = t.nest + Vec3::new(r * a.sin(), 0.0, r * a.cos());
}

/// A drop of nectar at the honey rate (`genItem`).
fn gen_item(t: &Teki, w: &mut World) {
    if w.rng.f32() <= t.f(HONEY_RATE) {
        w.sim.spawn_nectar(t.pos + Vec3::Y * 2.0);
    }
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(6, APPEAR);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.flags = [false; 4];
        t.target_pos = t.pos;
    }));

    // StateAppear: underground until found, then a short wait and the pop-up (the clip held until then).
    s.set(
        APPEAR,
        vec![
            Action::new(DEAD).on_event(|_, _, e| matches!(e, Event::Pressed)).build(),
            Action::new(WALK)
                .on_start(|t, w| {
                    // `onInit`: how long it lives (80-100% of the survival time) and its own gait.
                    t.reg[5] = ((0.8 + 0.2 * w.rng.f32()) * t.f(SURVIVAL)).floor();
                    t.reg[4] = 0.0;
                    t.reg[6] = 0.7 + 0.3 * w.rng.f32();
                    t.reg[7] = 0.7 + 0.3 * w.rng.f32();
                    t.reg[1] = in_ticks(w, t.i(MIN_APPEAR), t.i(MAX_APPEAR));
                    t.reg[0] = 0.0;
                    t.flags[0] = false;
                    t.clear_option(opt::ATARI | opt::ALIVE);
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_APPEAR);
                    t.manual_speed = Some(0.0);
                })
                .on_act(|t, w| {
                    if !t.flags[0] && is_found(t, w) {
                        t.flags[0] = true;
                        if t.count[0] != 1 {
                            // `createFellow`: the leader calls ten fellows around it.
                            for i in 0..FELLOWS {
                                let a = std::f32::consts::TAU * (i as f32 + w.rng.f32()) / FELLOWS as f32;
                                let at = t.pos + Vec3::new(a.sin(), 0.0, a.cos()) * (t.f(APPEAR_RANGE) * (0.3 + 0.7 * w.rng.f32()));
                                w.out.push(Out::Spawn { kind: MITITE, pos: at, yaw: a, parent: t.id, velocity: Vec3::ZERO });
                            }
                        }
                    }
                    if t.flags[0] {
                        t.reg[0] += 1.0;
                    }
                    if t.flags[0] && t.reg[0] > t.reg[1] && t.manual_speed.is_some() {
                        t.manual_speed = None;
                        w.out.push(Out::Sound { name: "TAMAGOMUSHI_APPEAR", pos: t.pos });
                    }
                    if t.key(key::ACTION0) {
                        t.set_option(opt::ATARI | opt::ALIVE);
                    }
                    t.animation_finished() && t.manual_speed.is_none()
                })
                .on_finish(|t, _| {
                    t.set_option(opt::ATARI | opt::ALIVE);
                    t.manual_speed = None;
                })
                .build(),
        ],
    );

    // StateWalk: a leg of the wander, toward the goal; at its end the legs stop and it turns to the next.
    s.set(
        WALK,
        vec![
            Action::new(DEAD).on_event(|_, _, e| matches!(e, Event::Pressed)).build(),
            Action::new(TURN)
                .on_start(|t, w| {
                    t.start_motion(ANIM_MOVE);
                    t.reg[2] = 0.0;
                    t.reg[3] = in_ticks(w, t.i(MIN_WALK), t.i(MAX_WALK));
                })
                .on_act(|t, w| {
                    if hurt_or_dead(t) {
                        return true;
                    }
                    t.reg[2] += 1.0;
                    t.reg[4] += 1.0;
                    if t.reg[4] > t.reg[5] {
                        t.next_override = Some(HIDE);
                        return true;
                    }
                    let goal = t.target_pos;
                    if t.manual_speed.is_none() && !t.anim.is_finished() {
                        let speed = t.reg[7] * t.f(fp::MOVE_SPEED);
                        turn_to_target(t, goal, t.f(fp::TURN_SPEED) * 0.2, t.f(fp::MAX_TURN_ANGLE) * 0.2);
                        set_target_speed(t, speed);
                    }
                    if dist_xz(t.pos, goal) < 10.0 || t.reg[2] > t.reg[3] {
                        t.target_vel = Vec3::ZERO;
                        t.finish_motion();
                    }
                    if t.animation_finished() {
                        set_goal_random(t, w);
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateTurn: stands and turns to the new goal, then walks.
    s.set(
        TURN,
        vec![
            Action::new(DEAD).on_event(|_, _, e| matches!(e, Event::Pressed)).build(),
            Action::new(WALK)
                .on_start(|t, _| {
                    t.start_motion(ANIM_WAIT);
                    t.target_vel = Vec3::ZERO;
                })
                .on_act(|t, _| {
                    if hurt_or_dead(t) {
                        return true;
                    }
                    t.reg[4] += 1.0;
                    if t.reg[4] > t.reg[5] {
                        t.next_override = Some(HIDE);
                        return true;
                    }
                    let goal = t.target_pos;
                    let left = turn_to_target(t, goal, t.reg[6] * t.f(fp::TURN_SPEED), t.reg[6] * t.f(fp::MAX_TURN_ANGLE));
                    if left.abs() < 0.1 {
                        t.finish_motion();
                    }
                    t.animation_finished()
                })
                .build(),
        ],
    );

    // StateHide: dives back underground and is gone.
    s.set(
        HIDE,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_DIVE);
            })
            .on_act(|t, w| {
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );

    // StateDead: the death motion, then a drop of nectar and gone (the host's body leaves the corpse).
    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.hp = 0.0;
                t.clear_option(opt::ATARI);
                t.target_vel = Vec3::ZERO;
                w.out.push(Out::Dying { teki: t.id });
                t.start_motion(ANIM_DEAD);
            })
            .on_act(|t, w| {
                if t.animation_finished() {
                    gen_item(t, w);
                    t.die(w);
                }
                false
            })
            .build()],
    );
    s.set(WAIT, Vec::new());
    s
}
