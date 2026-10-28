//! The Kogane family of Pikmin 2: the Iridescent Flint Beetle (`Koganemushi::Obj`), the Iridescent Glint Beetle
//! (`Wealthy::Obj`) and the Doodlebug (`Fart::Obj`), all `Kogane::Obj : EnemyBase` (plugProjectNishimuraU/Kogane.cpp,
//! KoganeState.cpp, Koganemushi.cpp, Wealthy.cpp, Fart.cpp). A harmless loot bug: it hides until a player or Pikmin is
//! close, pops up (growing from nothing), scuttles about for a while, flips over when hit (`pressCallBack`: a Pikmin
//! landing on it) and drops something each of its first three flips, then dives away. The Doodlebug also lets out a
//! gas cloud when it starts to scuttle (`Fart::interactFartGasAttack`).
//!
//! States (`KoganeStateID`): [`APPEAR`], [`DISAPPEAR`], [`MOVE`], [`WAIT`], [`PRESS`]. Registers on the [`Teki`]:
//! `reg[0]` the appear timer (`mAppearTimer`), `reg[1]` the move timer, `reg[2]` the scale timer, `reg[3]` the fart
//! timer, `count[0]` the flips so far (`mHitCount`), `target_pos` where it scuttles to (`mTargetPosition`), `wander`
//! the gas cloud's place (`mFartPosition`). `fade` is its size as a share of full: the host scales the model by it
//! (and hides it when the `VISIBLE` option is off, `EB_ModelHidden`).
//!
//! Host adaptations: a hit (`Tekis::damage` on an invincible Pikmin 2 enemy) reaches it as [`Event::Pressed`]; the
//! gas kills the Pikmin it touches instead of starting the panic state; a wall turns it around (the decomp turns it
//! along the wall's normal, which the sim does not report); the treasure it can hold in a cave
//! (`createTreasureItem`), the cave relocation (`transitDisappear`) and the bitter-spray stone state are left out.
//! The effects (`TKoganeDive`, `TKoganeHit`, `TOoganeKira`, `TBabaFly_ver01`, `TBabaHe`) are JPA2 particles, which
//! have no engine yet, so none is emitted.

use std::sync::Arc;

use glam::Vec3;

use crate::object::*;
use crate::p2::{FLINT_BEETLE, GLINT_BEETLE, fp};
use crate::tai::*;

pub const APPEAR: i32 = 0;
pub const DISAPPEAR: i32 = 1;
pub const MOVE: i32 = 2;
pub const WAIT: i32 = 3;
pub const PRESS: i32 = 4;

/// `KoganeAnimID` (the `enemyAnimMgr.txt` order): move, wait, damage.
pub const ANIM_MOVE: usize = 0;
pub const ANIM_WAIT: usize = 1;
pub const ANIM_DAMAGE: usize = 2;

/// `mAppearTimer = 12800.0f`: the enemy has given what it had and leaves after the flip.
const GIVEN_ALL: f32 = 12800.0;
/// How long the Doodlebug's gas hangs about (`mFartTimer < 2.5f`).
const FART_SECONDS: f32 = 2.5;
/// `mScaleTimer`'s floor (`0.0001f`).
const TINY: f32 = 0.0001;

const APPEAR_T: usize = 0;
const MOVE_T: usize = 1;
const SCALE_T: usize = 2;
const FART_T: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Flint,
    Glint,
    Doodlebug,
}

/// `randWeightFloat(weight)`: 0..weight.
fn rand_weight(w: &mut World, weight: f32) -> f32 {
    w.rng.f32() * weight
}

/// `Kogane::Obj::koganeScaleUp`: true the tick it reaches full size.
fn scale_up(t: &mut Teki, dt: f32) -> bool {
    let full = t.f(fp::own::SCALE).max(TINY);
    let mut done = false;
    if t.reg[SCALE_T] < full {
        t.reg[SCALE_T] += dt * 10.0;
        if t.reg[SCALE_T] >= full {
            t.reg[SCALE_T] = full;
            done = true;
        }
        t.fade = t.reg[SCALE_T] / full;
    }
    done
}

/// `Kogane::Obj::koganeScaleDown`: true the tick it is gone.
fn scale_down(t: &mut Teki, dt: f32) -> bool {
    let full = t.f(fp::own::SCALE).max(TINY);
    let mut done = false;
    if t.reg[SCALE_T] > TINY {
        t.reg[SCALE_T] -= dt * 10.0;
        if t.reg[SCALE_T] <= TINY {
            t.reg[SCALE_T] = TINY;
            done = true;
        }
        t.fade = t.reg[SCALE_T] / full;
    }
    done
}

/// `Kogane::Obj::setTargetPosition`: 1000 units away, `goal` being a direction (a wall's normal) or none (a random turn
/// within `mTurnAngle` of where it faces; not while it is still tiny).
fn set_target_position(t: &mut Teki, w: &mut World, goal: Option<Vec3>) {
    t.target_pos = if let Some(g) = goal {
        Vec3::new(g.x * 1000.0 + t.pos.x, t.pos.y, g.z * 1000.0 + t.pos.z)
    } else {
        let turn = t.f(fp::own::TURN_ANGLE);
        let mut angle = 0.0;
        if t.reg[SCALE_T] > 0.1 {
            angle = rand_weight(w, turn * 2.0) - turn;
        }
        let theta = angle.to_radians() + t.yaw;
        t.pos + dir_vec(theta) * 1000.0
    };
}

/// `Kogane::Obj::isAppear`: a player or a Pikmin within `mSightRadius` (`EnemyFunc::isThereOlimar` / `isTherePikmin`).
fn is_appear(t: &Teki, w: &World) -> bool {
    let r = t.f(fp::SIGHT_RADIUS);
    w.navis.iter().any(|n| (n.pos - t.pos).length() < r)
        || w.sim.pikis().iter().any(|p| p.is_alive() && (p.pos - t.pos).length() < r)
}

/// `EnemyFunc::walkToTarget(enemy, pos, speed, turnFactor, maxTurnSpeed)`: each tick turn by `factor` of the angle left
/// (at most `max_deg`), then head along the new facing at `speed`.
fn walk_to_target(t: &mut Teki, target: Vec3, speed: f32, factor: f32, max_deg: f32) {
    let want = t.calc_target_direction(target);
    let face = t.direction();
    let diff = nearer_direction(face, want) - face;
    let max = max_deg.to_radians();
    t.yaw = face + (diff * factor).clamp(-max, max);
    let d = dir_vec(t.yaw);
    t.target_vel = Vec3::new(speed * d.x, t.target_vel.y, speed * d.z);
}

fn sound(w: &mut World, name: &'static str, pos: Vec3) {
    w.out.push(Out::Sound { name, pos });
}

/// `createItem` of each variant: what the n-th flip (`mHitCount`) drops.
fn create_item(t: &mut Teki, w: &mut World, v: Variant) {
    let at = t.pos + Vec3::Y * 10.0;
    // (pellet number 0 = 1, 1 = 5; amount) or (nectar amount).
    enum Drop {
        Pellets(u32, u32),
        Nectar(u32),
        Nothing,
    }
    let drop = match (v, t.count[0]) {
        (Variant::Flint, 0) => Drop::Pellets(0, 1),
        (Variant::Flint, 1) => Drop::Nectar(2),
        (Variant::Flint, 2) => Drop::Nectar(3),
        (Variant::Glint, 0) => Drop::Pellets(1, 3),
        (Variant::Glint, 1 | 2) => Drop::Nectar(3),
        (Variant::Doodlebug, 0..=2) => Drop::Nectar(3),
        _ => Drop::Nothing,
    };
    match drop {
        // `createPellet`: each pellet takes a colour of the Pikmin the player has met.
        Drop::Pellets(number, amount) => {
            for _ in 0..amount {
                let color = (w.rng.f32() * 3.0) as u32 % 3;
                w.out.push(Out::SpawnPellets { pos: at, number, color, count: 1 });
            }
        }
        // `createDoping`: yellow nectar (`HONEY_Y`).
        Drop::Nectar(count) => w.out.push(Out::SpawnNectar { pos: at, count }),
        Drop::Nothing => {}
    }
    if t.count[0] == 2 {
        t.reg[APPEAR_T] = GIVEN_ALL;
    }
    t.count[0] += 1;
}

/// The gas cloud's damage (`Fart::Obj::interactFartGasAttack`): Pikmin in the sphere take `InteractGas`, which panics
/// all but White Pikmin (and Bulbmin) to death; here they die at once. Players are unaffected (`actNavi` is false).
fn gas(t: &mut Teki, w: &mut World) {
    if t.reg[FART_T] >= FART_SECONDS {
        return;
    }
    t.reg[FART_T] += w.dt;
    let radius = t.f(fp::ATTACK_RADIUS);
    let at = t.wander;
    let victims: Vec<_> = w
        .sim
        .pikis()
        .iter()
        .filter(|p| p.is_alive() && p.color != libpikmin::PikiColor::White && (p.pos - at).length() < radius)
        .map(|p| (p.id, p.health))
        .collect();
    for (id, health) in victims {
        w.sim.attack_piki(id, health.max(1.0));
    }
    w.out.push(Out::Sound { name: "OTAKARA_ATK_GAS", pos: at });
}

/// Builds the Kogane family's strategy for `kind`.
pub fn strategy(kind: u8) -> Strategy {
    let v = match kind {
        FLINT_BEETLE => Variant::Flint,
        GLINT_BEETLE => Variant::Glint,
        _ => Variant::Doodlebug,
    };
    let mut s = Strategy::new(5, APPEAR);
    // `Kogane::Obj::onInit`: invulnerable, tiny, nothing given yet.
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.set_option(opt::INVINCIBLE);
        t.reg = [0.0; 8];
        t.reg[SCALE_T] = TINY;
        t.reg[FART_T] = FART_SECONDS;
        t.fade = 0.0;
        t.count[0] = 0;
        t.wander = t.pos;
    }));

    // StateAppear: underground and unseen until someone comes close.
    s.set(
        APPEAR,
        vec![
            Action::new(MOVE)
                .on_start(|t, w| {
                    t.clear_option(opt::ATARI | opt::VISIBLE);
                    t.target_vel = Vec3::ZERO;
                    // `resetAppearTimer`, `resetMoveTimer(0, 0)`.
                    let span = t.f(fp::own::MAX_APPEAR) - t.f(fp::own::MIN_APPEAR);
                    t.reg[APPEAR_T] = rand_weight(w, span);
                    t.reg[MOVE_T] = 0.0;
                    // startMotion(Move); stopMotion(): the clip is held on its first frame.
                    t.start_motion(ANIM_MOVE);
                    t.manual_speed = Some(0.0);
                })
                .on_act(|t, w| is_appear(t, w))
                .on_finish(move |t, w| {
                    t.set_option(opt::ATARI | opt::VISIBLE);
                    t.manual_speed = None;
                    sound(w, "TAMAGOMUSHI_APPEAR", t.pos);
                })
                .build(),
        ],
    );

    // StateDisappear: shrinks away and is gone.
    s.set(
        DISAPPEAR,
        vec![
            Action::new(NO_TRANSIT)
                .on_start(|t, w| {
                    t.target_vel = Vec3::ZERO;
                    sound(w, "TAMAGOMUSHI_DIVE", t.pos);
                })
                .on_act(|t, w| {
                    if scale_down(t, w.dt) {
                        t.die(w);
                    }
                    false
                })
                .build(),
        ],
    );

    // StateMove: scuttles toward a point 1000 units off, turning by `mTurnAngle` at random each leg.
    s.set(
        MOVE,
        vec![
            Action::new(PRESS).on_event(|_, _, e| matches!(e, Event::Pressed)).build(),
            // A wall: turn away from it (the decomp's `wallCallback` aims along its normal).
            Action::new(NO_TRANSIT)
                .on_event(|t, w, e| {
                    if matches!(e, Event::Wall) {
                        let back = -t.forward();
                        set_target_position(t, w, Some(back));
                    }
                    false
                })
                .build(),
            Action::new(WAIT)
                .on_start(move |t, w| {
                    let span = t.f(fp::own::MAX_TRAVEL) - t.f(fp::own::MIN_TRAVEL);
                    t.reg[MOVE_T] = rand_weight(w, span);
                    set_target_position(t, w, None);
                    t.start_motion(ANIM_MOVE);
                    // `createFartEffect`: the Doodlebug lets one go behind it.
                    if v == Variant::Doodlebug {
                        t.reg[FART_T] = 0.0;
                        let behind = t.f(fp::own::SCALE) * t.f(fp::MAX_ATTACK_RANGE);
                        t.wander = t.pos + Vec3::Y * 8.0 - dir_vec(t.yaw) * behind;
                        sound(w, "FART_GAS", t.pos);
                    }
                })
                .on_act(|t, w| {
                    scale_up(t, w.dt);
                    walk_to_target(t, t.target_pos, t.f(fp::MOVE_SPEED), t.f(fp::TURN_SPEED), t.f(fp::MAX_TURN_ANGLE));
                    if t.reg[APPEAR_T] > t.f(fp::own::MAX_APPEAR) || t.reg[MOVE_T] > t.f(fp::own::MAX_TRAVEL) {
                        t.finish_motion();
                    }
                    t.reg[APPEAR_T] += w.dt;
                    t.reg[MOVE_T] += w.dt;
                    if t.animation_finished() {
                        t.next_override = Some(if t.reg[APPEAR_T] > t.f(fp::own::MAX_APPEAR) { DISAPPEAR } else { WAIT });
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );

    // StateWait: stands still for a while.
    s.set(
        WAIT,
        vec![
            Action::new(PRESS).on_event(|_, _, e| matches!(e, Event::Pressed)).build(),
            Action::new(MOVE)
                .on_start(|t, w| {
                    let span = t.f(fp::own::MAX_STOP) - t.f(fp::own::MIN_STOP);
                    t.reg[MOVE_T] = rand_weight(w, span);
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_WAIT);
                })
                .on_act(|t, w| {
                    scale_up(t, w.dt);
                    if t.reg[MOVE_T] > t.f(fp::own::MAX_STOP) {
                        t.finish_motion();
                    }
                    t.reg[APPEAR_T] += w.dt;
                    t.reg[MOVE_T] += w.dt;
                    t.animation_finished()
                })
                .build(),
        ],
    );

    // StatePress: flipped over; the item comes out at the clip's second key, and it recovers at the end.
    s.set(
        PRESS,
        vec![
            Action::new(MOVE)
                .on_start(move |t, w| {
                    // Host adaptation: it lies still while flipped.
                    t.target_vel = Vec3::ZERO;
                    t.start_motion(ANIM_DAMAGE);
                    // `createPressSENormal`.
                    match v {
                        Variant::Flint => sound(w, "KOGANE_HIT", t.pos),
                        Variant::Glint => sound(w, "OOGANE_HIT", t.pos),
                        Variant::Doodlebug => {}
                    }
                })
                .on_act(move |t, w| {
                    scale_up(t, w.dt);
                    t.reg[APPEAR_T] += w.dt;
                    if t.key(crate::anim::key::ACTION0) && v == Variant::Doodlebug {
                        // `Fart::createPressSESpecial`.
                        sound(w, "FART_HIT", t.pos);
                    }
                    if t.key(crate::anim::key::ACTION1) {
                        create_item(t, w, v);
                    }
                    if t.animation_finished() {
                        t.next_override = Some(if t.reg[APPEAR_T] > GIVEN_ALL { DISAPPEAR } else { MOVE });
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );
    s
}

/// `Fart::Obj::doUpdateCommon`: the Doodlebug's gas and buzzing run in every state.
pub fn update_common(t: &mut Teki, w: &mut World) {
    if t.kind != crate::p2::DOODLEBUG {
        return;
    }
    gas(t, w);
    // `getStateID() >= 2`: buzzing while it is out (Move, Wait, Press).
    if t.state >= MOVE {
        w.out.push(Out::Sound { name: "FART_BUZZ", pos: t.pos });
    }
}
