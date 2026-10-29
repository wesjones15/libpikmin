//! The Unmarked Spectralid of Pikmin 2 (`ShijimiChou::Obj : EnemyBase`, plugProjectMorimuraU/shijimiChou.cpp, shijimiChouState.cpp): a
//! butterfly that flutters about its place and cannot be hurt. A Pikmin that lands on it brings it down ([`FALL`]); when it
//! reaches the ground it dies ([`DEAD`]) and may leave nectar, and its small carcass.
//!
//! States (`ShijimiChou::StateID`): [`WAIT`], [`FLY`], [`FALL`], [`DEAD`]; ids 4 and 5 (`Leave`, `Rest`) belong to the flock. Registers:
//! `reg[0]` whether it has been lifted to its flight height, `reg[1]` its pitch phase (`mPitchRate`), `reg[2]` the fall's sway phase, `reg[3]` the
//! tick count of the state, `reg[4]` where the fall began (x), `reg[5]` (z); `target_pos` is `mGoalPosition`, `nest` its home.
//!
//! Host adaptations: the disc's butterflies come in flocks with a leader (`mGroupLeader`), settle on a Bulblax as it dies (`Rest`) and fly
//! off when someone nears (`Leave`); here each is alone and, when its flight time is up, goes back to hovering and flies again when
//! someone comes near. The red and purple kinds, which only drop sprays once those have been made, are the yellow one.

use std::sync::Arc;

use glam::Vec3;

use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const WAIT: i32 = 0;
pub const FLY: i32 = 1;
pub const FALL: i32 = 2;
pub const DEAD: i32 = 3;

/// `ShijimiChou::AnimID`.
pub const ANIM_DEAD: usize = 1;
pub const ANIM_MOVE: usize = 2;

const LIFTED: usize = 0;
const PITCH: usize = 1;
const SWAY: usize = 2;
const TICKS: usize = 3;
const FALL_X: usize = 4;
const FALL_Z: usize = 5;

// Its own block (`ProperParms`).
const MAX_FLY_TIME: usize = fp::own::BASE + 1;
const NECTAR_RATE: usize = fp::own::BASE + 2;
const FLIGHT_HEIGHT: usize = fp::own::BASE + 3;
const PITCH_RATE: usize = fp::own::BASE + 4;
const PITCH_AMP_RATE: usize = fp::own::BASE + 5;

/// Someone within this of a waiting one makes it take off (`getNearestPikmin(180, 150)`; any side, and players too here).
const TAKE_OFF_RANGE: f32 = 150.0;
/// `Parms::mMaxFallSpeed` stand-in: how fast it drops at most.
const MAX_FALL_SPEED: f32 = 60.0;

fn clinging(t: &Teki, w: &World) -> bool {
    w.sim.pikis().iter().any(|p| p.state.latched_to() == Some(t.creature))
}

/// `Obj::setNextGoal`: somewhere in its territory, a little above or below its flight height.
fn next_goal(t: &mut Teki, w: &mut World) {
    let radius = t.f(fp::TERRITORY) * (0.5 + 0.5 * w.rng.f32());
    let a = std::f32::consts::TAU * w.rng.f32();
    t.target_pos = Vec3::new(t.nest.x + radius * a.sin(), t.nest.y + 50.0 * a.sin(), t.nest.z + radius * a.cos());
}

/// `Obj::fly`: bobs on its pitch phase, turns for the goal and goes at it.
fn fly(t: &mut Teki, w: &mut World) {
    t.reg[PITCH] += t.f(PITCH_RATE);
    t.pos.y += (std::f32::consts::TAU * t.reg[PITCH]).sin() * t.f(PITCH_AMP_RATE);
    if t.reg[PITCH] > 1.0 {
        t.reg[PITCH] -= 1.0;
    }
    let goal = t.target_pos;
    if dist_xz(t.pos, goal).powi(2) < 1000.0 {
        next_goal(t, w);
        return;
    }
    turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
    set_target_speed(t, t.f(fp::MOVE_SPEED));
    t.pos.y += 0.01 * (goal.y - t.pos.y);
}

/// The host's body died (a blow nothing survives): it dies where it is.
fn host_kill() -> Act {
    Action::new(DEAD)
        .on_act(|t, _| {
            if t.stored_damage > 0.0 {
                t.make_damaged();
            }
            t.hp <= 0.0
        })
        .build()
}

/// A Pikmin has landed on it.
fn brought_down() -> Act {
    Action::new(FALL).on_act(|t, w| clinging(t, w)).build()
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(6, WAIT);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.action_state = -1;
        // `damageCallBack` answers no to every blow: only a Pikmin on its back harms it.
        t.set_option(opt::INVINCIBLE | opt::ATARI);
        t.start_flying();
    }));

    s.set(
        WAIT,
        vec![
            host_kill(),
            brought_down(),
            Action::new(FLY)
                .on_start(|t, w| {
                    if t.reg[LIFTED] == 0.0 {
                        // It is born on the ground and flies at its flight height above it.
                        t.reg[LIFTED] = 1.0;
                        t.pos.y += t.f(FLIGHT_HEIGHT);
                        t.nest.y = t.pos.y;
                        t.reg[PITCH] = w.rng.f32();
                        next_goal(t, w);
                    }
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_MOVE);
                    t.anim.counter = w.rng.f32() * 7.0;
                    t.reg[TICKS] = 0.0;
                })
                .on_act(|t, w| {
                    t.reg[TICKS] += 1.0;
                    if t.reg[TICKS] > 10.0 {
                        let me = t.pos;
                        if t.closest(w, |c| dist_xz(me, c.pos) <= TAKE_OFF_RANGE).is_some() {
                            return true;
                        }
                    }
                    fly(t, w);
                    false
                })
                .build(),
        ],
    );

    // StateFly: flutters between goals in its territory for its flight time (`mMaxFlyTime` ticks), then hovers again.
    s.set(
        FLY,
        vec![
            host_kill(),
            brought_down(),
            Action::new(WAIT)
                .on_start(|t, w| {
                    next_goal(t, w);
                    t.reg[TICKS] = 0.0;
                })
                .on_act(|t, w| {
                    t.reg[TICKS] += 1.0;
                    if t.reg[TICKS] > t.f(MAX_FLY_TIME) {
                        return true;
                    }
                    fly(t, w);
                    false
                })
                .build(),
        ],
    );

    // StateFall: drops swaying to the ground, pulled out of its flight.
    s.set(
        FALL,
        vec![
            host_kill(),
            Action::new(DEAD)
            .on_start(|t, w| {
                t.finish_flying();
                t.set_option(opt::GRAVITATABLE);
                t.target_vel = Vec3::ZERO;
                t.reg[FALL_X] = t.pos.x;
                t.reg[FALL_Z] = t.pos.z;
                t.reg[SWAY] = 0.0;
                t.reg[TICKS] = 0.0;
                w.out.push(Out::Sound { name: "BUTTERFLY_HIT", pos: t.pos });
            })
            .on_act(|t, _| {
                // `fallBehavior`: sways about where it started while it sinks, no faster than the cap.
                t.reg[SWAY] = (t.reg[SWAY] + 0.2) % std::f32::consts::TAU;
                let sway = t.reg[SWAY].sin();
                t.pos.x = t.reg[FALL_X] + 8.0 * sway;
                t.pos.z = t.reg[FALL_Z] + 4.0 * sway;
                t.action_vel.y = t.action_vel.y.max(-MAX_FALL_SPEED);
                t.reg[TICKS] += 1.0;
                // `isFallEnd`: near the ground, or it has been falling too long.
                let ground = t.ground_y.unwrap_or(t.pos.y - 100.0);
                t.pos.y < ground + 10.0 || t.reg[TICKS] > 100.0
            })
            .build(),
        ],
    );

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
                    // `genItem`: nectar, at the disc's rate.
                    if w.rng.f32() <= t.f(NECTAR_RATE) {
                        w.sim.spawn_nectar(t.pos + Vec3::Y * 2.0);
                    }
                    t.die(w);
                }
                false
            })
            .build()],
    );
    for st in [4, 5] {
        s.set(st, Vec::new());
    }
    s
}
