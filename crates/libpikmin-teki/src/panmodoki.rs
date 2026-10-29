//! The Giant Breadbug of Pikmin 2 (`OoPanModoki::Obj : PanModokiBase::Obj`, plugProjectMorimuraU/panModoki.cpp, panModokiState.cpp): a
//! huge bug that lives under a mound of its own. It comes up ([`APPEAR`]), ambles about ([`WALK`]), goes for a pellet it can pull away
//! from the Pikmin ([`STICK`]), backs home with it in its jaws ([`BACK`], [`CARRY_END`]) and goes under to eat it ([`HIDE`]) for a while,
//! then comes up again. A Purple Pikmin landing on it hurts ([`DAMAGE`]); everything else only makes it stop and look ([`WAIT`]).
//!
//! States (`PanModokiBase::StateID`): [`DEAD`], [`WALK`], [`BACK`], [`PULLED`], [`APPEAR`], [`HIDE`], [`DAMAGE`], [`WAIT`], [`STICK`], [`SUCKED`],
//! [`CARRY_END`]. Registers: `reg[0]` the state's timer, `reg[1]` what the hide waits out; `count[0]` the hide's clip has ended (the pellet is
//! eaten); `target_pos` the point it ambles to; `targets[0]` the pellet; `held` what it has in its jaws.
//!
//! Host adaptations: no route graph, so it walks straight to and from its mound; the pellet is the sim's `Held` one and no Pikmin
//! pull against the bug while it has it (it lets go when the Pikmin on the pellet would win the tug, as the Breadbug does); the treasures it is
//! to hold in its mound and the ship parts are never taken, so Ship Repair's scoring is left alone; the pellet it eats is not thrown up again
//! when it dies (`throwUpEatItem`); `Pulled` and `Sucked` (the Onion's and the ship's suction) have no counterpart here. Any blow a player lands
//! counts as the press, as well as a thrown Purple Pikmin.

use std::sync::Arc;

use glam::Vec3;
use libpikmin::{PelletId, PelletState, PikiColor};

use crate::anim::key;
use crate::object::*;
use crate::p2::{fp, set_target_speed, turn_to_target};
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const WALK: i32 = 1;
pub const BACK: i32 = 2;
pub const PULLED: i32 = 3;
pub const APPEAR: i32 = 4;
pub const HIDE: i32 = 5;
pub const DAMAGE: i32 = 6;
pub const WAIT: i32 = 7;
pub const STICK: i32 = 8;
pub const SUCKED: i32 = 9;
pub const CARRY_END: i32 = 10;

/// `PanModokiBase::AnimID`.
pub const ANIM_DEAD: usize = 0;
pub const ANIM_WALK: usize = 1;
pub const ANIM_BACK: usize = 2;
pub const ANIM_APPEAR: usize = 4;
pub const ANIM_HIDE: usize = 5;
pub const ANIM_DAMAGE: usize = 6;
pub const ANIM_CARRY: usize = 7;
pub const ANIM_WAIT: usize = 8;

const TIMER: usize = 0;

// General parms it reads.
const HOME_RADIUS: usize = 10;
const SEARCH_DISTANCE: usize = 14;
const SEARCH_ANGLE: usize = 15;
// Its own block (`ProperParms`).
const FAST_TURN: usize = fp::own::BASE + 2;
const MAX_FAST_TURN: usize = fp::own::BASE + 5;
const CARRY_SPEED: usize = fp::own::BASE + 3;
const PRESS_DAMAGE: usize = fp::own::BASE + 6;
const WAIT_TIME: usize = fp::own::BASE + 14;
const HIDE_TIME: usize = fp::own::BASE + 15;
/// `mCarrySizeDiff`: how far from a pellet's edge it takes hold.
const CARRY_SIZE: f32 = 20.0;
/// `StateStick`'s longest walk to a pellet, ticks.
const STICK_TICKS: f32 = 200.0;
const TICK: f32 = 1.0 / 30.0;

/// The tug of war (`PelletCarry::pullable(2, mCarryStrength)`): it wins while fewer Pikmin pull than its strength, the average of the carriers
/// the pellet asks for.
fn strength(w: &World, id: PelletId) -> f32 {
    let p = w.sim.pellet(id);
    (p.desc.min_carriers + p.desc.max_carriers) as f32 * 0.5
}

/// `Obj::isTargetable` and `findNearestPellet`'s conditions: a loose pellet that is not a ship part or treasure, no other bug holds it, and the
/// Pikmin on it do not outpull it.
fn takes(t: &Teki, w: &World, id: PelletId) -> bool {
    let p = w.sim.pellet(id);
    let mine = t.held == Some(id);
    if p.desc.ship_part.is_some() || !(p.state == PelletState::Idle || (mine && p.state == PelletState::Held)) {
        return false;
    }
    if !mine && w.others.iter().any(|o| o.id != t.id && o.holding == Some(id)) {
        return false;
    }
    (p.carriers.len() as f32) < strength(w, id)
}

/// `findNearestPellet`: the nearest one it can take within its search range and cone, at about its own height.
fn nearest_pellet(t: &Teki, w: &World) -> Option<PelletId> {
    let (range, half) = (t.f(SEARCH_DISTANCE), (t.f(SEARCH_ANGLE) * 0.5).to_radians());
    let mut best: Option<(f32, PelletId)> = None;
    for p in w.sim.pellets() {
        if !takes(t, w, p.id) || (p.pos.y - t.pos.y).abs() > 10.0 + p.desc.height {
            continue;
        }
        let d = dist_xz(t.pos, p.pos);
        if d > range || t.calc_target_angle(p.pos) > half {
            continue;
        }
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, p.id));
        }
    }
    best.map(|(_, id)| id)
}

fn let_go(t: &mut Teki, w: &mut World) {
    if let Some(id) = t.held.take() {
        let p = w.sim.pellet(id);
        let (pos, yaw) = (Vec3::new(p.pos.x, t.pos.y, p.pos.z), p.yaw);
        w.sim.release_pellet(id, pos, yaw);
    }
}

/// A blow that kills the body (the host's), or health gone.
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

/// `pressCallBack`: pressed by a Purple Pikmin, or by a blow of a player (the host's `Pressed`).
fn pressed() -> Act {
    Action::new(DAMAGE)
        .on_event(|_, w, e| match e {
            Event::Pressed => true,
            Event::Entity { other: Target::Piki(id), flying: true } => w.sim.piki(*id).color == PikiColor::Purple,
            _ => false,
        })
        .build()
}

fn amble(t: &mut Teki, w: &mut World) {
    let a = w.rng.range(0.0, std::f32::consts::TAU);
    let r = w.rng.range(0.3, 1.0) * t.f(fp::TERRITORY);
    t.target_pos = t.nest + dir_vec(a) * r;
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(11, APPEAR);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.count = [0; 4];
        t.action_state = -1;
        t.set_option(opt::INVINCIBLE);
    }));

    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                let_go(t, w);
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

    // StateAppear: up out of its mound.
    s.set(
        APPEAR,
        vec![
            dead_check(),
            Action::new(WALK)
                .on_start(|t, w| {
                    t.pos.x = t.nest.x;
                    t.pos.z = t.nest.z;
                    t.set_option(opt::VISIBLE | opt::ATARI);
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_APPEAR);
                    w.out.push(Out::Shake { pos: t.pos, strength: 0.5 });
                })
                .on_act(|t, _| t.animation_finished())
                .build(),
        ],
    );

    // StateWalk: ambles about its mound, on the lookout for a pellet.
    s.set(
        WALK,
        vec![
            dead_check(),
            pressed(),
            Action::new(STICK)
                .on_start(|t, w| {
                    t.targets[0] = None;
                    t.start_motion(ANIM_WALK);
                    amble(t, w);
                })
                .on_act(|t, w| {
                    if let Some(id) = nearest_pellet(t, w) {
                        t.targets[0] = Some(Target::Pellet(id));
                        return true;
                    }
                    let goal = t.target_pos;
                    if dist_xz(t.pos, goal) < 20.0 {
                        amble(t, w);
                    }
                    turn_to_target(t, goal, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    set_target_speed(t, t.f(fp::MOVE_SPEED));
                    false
                })
                .on_finish(|t, _| t.target_vel = Vec3::ZERO)
                .build(),
        ],
    );

    // StateStick: goes to the pellet at half pace, turning quickly, and takes hold.
    s.set(
        STICK,
        vec![
            dead_check(),
            pressed(),
            Action::new(BACK)
                .on_start(|t, _| {
                    t.reg[TIMER] = 0.0;
                    t.start_motion(ANIM_WALK);
                })
                .on_act(|t, w| {
                    let Some(Target::Pellet(id)) = t.targets[0] else {
                        t.next_override = Some(WALK);
                        return true;
                    };
                    if !takes(t, w, id) {
                        t.next_override = Some(WALK);
                        return true;
                    }
                    let p = w.sim.pellet(id);
                    let (at, radius, yaw) = (p.pos, p.desc.radius, p.yaw);
                    t.reg[TIMER] += 1.0;
                    if dist_xz(t.pos, at) < radius + 1.2 * CARRY_SIZE {
                        t.target_vel = Vec3::ZERO;
                        if w.sim.hold_pellet(id, at, yaw) {
                            t.held = Some(id);
                            t.targets[0] = None;
                            return true;
                        }
                        t.next_override = Some(WALK);
                        return true;
                    }
                    if t.reg[TIMER] > STICK_TICKS {
                        t.next_override = Some(WALK);
                        return true;
                    }
                    turn_to_target(t, at, t.f(FAST_TURN), t.f(MAX_FAST_TURN).max(t.f(fp::MAX_TURN_ANGLE)));
                    set_target_speed(t, 0.5 * t.f(fp::MOVE_SPEED));
                    false
                })
                .on_finish(|t, _| t.target_vel = Vec3::ZERO)
                .build(),
        ],
    );

    // StateBack: backs towards its mound with the pellet in its jaws; lets go if the Pikmin on it would win.
    s.set(
        BACK,
        vec![
            dead_check(),
            pressed(),
            Action::new(CARRY_END)
                .on_start(|t, _| {
                    t.start_motion(ANIM_BACK);
                })
                .on_act(|t, w| {
                    let Some(id) = t.held else {
                        t.next_override = Some(WAIT);
                        return true;
                    };
                    if w.sim.pellet(id).state == PelletState::Gone || !takes(t, w, id) {
                        let_go(t, w);
                        t.next_override = Some(WAIT);
                        return true;
                    }
                    let home = t.nest;
                    if dist_xz(t.pos, home) < t.f(HOME_RADIUS) {
                        return true;
                    }
                    // Facing away from the mound, walking backwards.
                    let away = t.pos * 2.0 - home;
                    turn_to_target(t, away, t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    let d = dir_vec(t.yaw);
                    let speed = t.f(CARRY_SPEED);
                    t.target_vel = Vec3::new(-d.x * speed, t.target_vel.y, -d.z * speed);
                    false
                })
                .on_finish(|t, _| t.target_vel = Vec3::ZERO)
                .build(),
        ],
    );

    // StateCarryEnd: at its mound, it settles over it.
    s.set(
        CARRY_END,
        vec![
            dead_check(),
            Action::new(HIDE)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_CARRY);
                })
                .on_act(|t, _| {
                    // `forceMovePosition(diff * 0.05)`: slides the rest of the way.
                    let diff = t.nest - t.pos;
                    t.pos.x += diff.x * 0.05;
                    t.pos.z += diff.z * 0.05;
                    t.key(key::LOOP_END) || t.animation_finished()
                })
                .build(),
        ],
    );

    // StateHide: down into the mound, eating what it brought; stays under for the hide time, then up again.
    s.set(
        HIDE,
        vec![
            dead_check(),
            Action::new(APPEAR)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.reg[TIMER] = 0.0;
                    t.count[0] = 0;
                    t.start_motion(ANIM_HIDE);
                })
                .on_act(|t, w| {
                    if t.count[0] == 0 && t.animation_finished() {
                        t.count[0] = 1;
                        // `endCarry`: whatever it held is eaten, and it is whole again.
                        if let Some(id) = t.held.take() {
                            w.sim.consume_pellet(id);
                        }
                        t.hp = t.info.life;
                        t.clear_option(opt::VISIBLE | opt::ATARI);
                    }
                    if t.count[0] == 1 {
                        t.reg[TIMER] += 1.0;
                        return t.reg[TIMER] > t.f(HIDE_TIME);
                    }
                    false
                })
                .build(),
        ],
    );

    // StateDamage: pressed; lets go, takes the press damage.
    s.set(
        DAMAGE,
        vec![
            dead_check(),
            Action::new(WAIT)
                .on_start(|t, w| {
                    let_go(t, w);
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_DAMAGE);
                    t.hp -= t.f(PRESS_DAMAGE);
                    t.flash = 0.08;
                })
                .on_act(|t, _| {
                    if t.hp <= 0.0 {
                        t.next_override = Some(DEAD);
                        return true;
                    }
                    t.animation_finished()
                })
                .build(),
        ],
    );

    // StateWait: a moment to look about, then on with walking.
    s.set(
        WAIT,
        vec![
            dead_check(),
            pressed(),
            Action::new(WALK)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.reg[TIMER] = 0.0;
                    t.start_motion(ANIM_WAIT);
                })
                .on_act(|t, w| {
                    t.reg[TIMER] += w.dt;
                    // `mWaitTime` ticks, then the clip is let finish.
                    if t.reg[TIMER] > t.f(WAIT_TIME) * TICK && !t.anim.is_finished() {
                        t.finish_motion();
                    }
                    t.animation_finished()
                })
                .build(),
        ],
    );
    for st in [PULLED, SUCKED] {
        s.set(st, Vec::new());
    }
    s
}
