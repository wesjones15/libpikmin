//! The gas pipe of Pikmin 2 (`GasHiba::Obj : EnemyBase`, plugProjectNishimuraU/GasHiba.cpp, GasHibaState.cpp): a pipe in
//! the ground that vents poison gas. It waits (`Wait`), vomits gas (`Attack`) and, if the wait time on the disc is zero,
//! never stops; Pikmin that fight it can wreck it (`Dead`), which stops it for good. The gas kills the Pikmin in the sphere
//! of `mAttackRadius` once `mAttackStartTime` of the vent has passed, unless they are White (or Bulbmin); players are
//! unaffected (`InteractGas::actNavi` is false).
//!
//! States (`GasHibaStateID`): [`DEAD`], [`WAIT`], [`ATTACK`]. `reg[0]` is the timer. It is a fixture: it does not walk, and
//! the host keeps it from being killed like the Fire Geyser unless its Pikmin damage is allowed through.
//!
//! Host adaptations: the Pikmin in the gas die at once rather than panicking first; the gas effect (`TGasuHiba`) is a JPA2
//! particle, so none is emitted; the bridge and gate it can be tied to in the story mode (`setInitLivingThing`) are not
//! modelled, it is always live.

use std::sync::Arc;

use glam::Vec3;

use crate::object::*;
use crate::p2::fp;
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const WAIT: i32 = 1;
pub const ATTACK: i32 = 2;

/// `GasHibaAnimID`: wait, attack.
pub const ANIM_WAIT: usize = 0;
pub const ANIM_ATTACK: usize = 1;

// `ProperParms`.
const ACTIVE_TIME: usize = fp::own::BASE + 1;
const WAIT_TIME: usize = fp::own::BASE + 2;
const ATTACK_START: usize = fp::own::BASE + 3;

const TIMER: usize = 0;
/// The vent sound's re-request timer.
const SOUND_TIMER: usize = 5;

fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

/// `Obj::interactGasAttack`: the Pikmin inside the sphere (and the height band) take the gas.
fn gas(t: &Teki, w: &mut World) {
    let radius = t.f(fp::ATTACK_RADIUS);
    // `mMaxAttackRange` above the pipe, `mMaxAttackAngle` (a height here) below it.
    let (above, below) = (t.f(fp::MAX_ATTACK_RANGE), t.f(fp::MAX_ATTACK_ANGLE));
    let victims: Vec<_> = w
        .sim
        .pikis()
        .iter()
        .filter(|p| {
            p.is_alive()
                && p.color != libpikmin::PikiColor::White
                && p.pos.y < t.pos.y + above
                && p.pos.y > t.pos.y - below
                && dist_xz(p.pos, t.pos) < radius
        })
        .map(|p| (p.id, p.health))
        .collect();
    for (id, health) in victims {
        w.sim.attack_piki(id, health.max(1.0));
    }
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(3, WAIT);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.reg = [0.0; 8];
        t.target_vel = Vec3::ZERO;
    }));

    // StateDead: stopped for good.
    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                t.clear_option(opt::ALIVE | opt::ATARI);
                t.set_option(opt::INVINCIBLE);
                t.start_motion(ANIM_WAIT);
                w.out.push(Out::Sound { name: "HIBA_STOP", pos: t.pos });
                w.out.push(Out::Dying { teki: t.id });
            })
            .build()],
    );

    // StateWait: a random share of the wait time at first, then the wait time; then it vents.
    s.set(
        WAIT,
        vec![Action::new(ATTACK)
            .on_start(|t, w| {
                t.reg[TIMER] = w.rng.f32() * t.f(WAIT_TIME);
                t.start_motion(ANIM_WAIT);
            })
            .on_act(|t, w| {
                t.reg[TIMER] += w.dt;
                hurt(t);
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                t.reg[TIMER] > t.f(WAIT_TIME)
            })
            .build()],
    );

    // StateAttack: vents until the active time is up (never, when the wait time is zero), the gas biting after a moment.
    s.set(
        ATTACK,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.start_motion(ANIM_ATTACK);
            })
            .on_act(|t, w| {
                hurt(t);
                if t.hp <= 0.0 || (t.f(WAIT_TIME) > 0.0 && t.reg[TIMER] > t.f(ACTIVE_TIME)) {
                    t.finish_motion();
                }
                t.reg[TIMER] += w.dt;
                if t.reg[TIMER] > t.f(ATTACK_START) {
                    gas(t, w);
                }
                // The game re-requests the held vent sound every frame; the host asks for the once-only flavour each second.
                if t.timer_elapsed(SOUND_TIMER) {
                    t.timers[SOUND_TIMER] = 1.0;
                    w.out.push(Out::Sound { name: "GAS_HIBA_VOMIT", pos: t.pos });
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { WAIT });
                    return true;
                }
                false
            })
            .build()],
    );
    s
}
