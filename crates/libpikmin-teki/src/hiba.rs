//! The Fire Geyser: `TAIhibaAStrategy` (TAIhibaA.cpp). It stays where it is put, can't be hurt, waits, and
//! every so often shoots a jet of flame straight up for a while.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::tpf;

// TAIhibaAFloatParams
const WAIT_TIME: usize = 50;
const ATTACK_TIME: usize = 51;

// TAIhibaAStateID
const INIT: i32 = 0;
const WAIT_STATE: i32 = 1;
const ATTACK_STATE: i32 = 2;

/// `TAIhibaAStrategy::act`: it never leaves its nest.
fn pinned() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, _| {
            t.pos = t.nest;
            false
        })
        .build()
}

/// `TAIAinitHibaA`: not hittable, not organic.
fn init(next: i32) -> Act {
    Action::new(next).on_start(|t, _| t.clear_option(opt::ATARI)).on_act(|_, _| true).build()
}

/// `TAIAfireAttackHibaA`: the attack motion; at its loop start the jet begins and lasts `ATTACK_TIME`, then the
/// motion is told to finish.
fn fire_attack(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.counter = 0.0;
            t.fire = None;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            t.counter += w.dt;
            if t.key(key::LOOP_START) && t.fire.is_none() {
                if let Some((p, _)) = t.part("hblp") {
                    t.fire = Some(Fire { from: p, dir: Vec3::Y, length: 330.0, radius: 18.0, damage: t.f(tpf::ATTACK_POWER) });
                    w.out.push(Out::EffectDir { id: effects::HIBA_FIRE, pos: p, dir: Vec3::Y });
                    w.out.push(Out::Sound { name: "SE_TANK_FIRE", pos: p });
                }
            }
            if t.counter > t.f(ATTACK_TIME) {
                t.finish_motion();
                t.fire = None;
            }
            t.key(key::FINISHED)
        })
        .on_finish(|t, _| t.fire = None)
        .build()
}

pub fn fire_geyser(_info: &TekiInfo) -> Strategy {
    let mut s = Strategy::new(3, INIT);
    s.set(INIT, vec![init(WAIT_STATE)]);
    s.set(WAIT_STATE, vec![pinned(), stop_move(), timer_reaction(ATTACK_STATE, |t| t.f(WAIT_TIME))]);
    s.set(ATTACK_STATE, vec![pinned(), fire_attack(WAIT_STATE, ATTACK)]);
    s
}
