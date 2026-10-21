//! The Pearly Clamclamp: `TaiShellStrategy` and its pearl `TaiPearlStrategy` (taishell.cpp). The shell sits with its
//! mouth shut; when anything comes near its pearl it opens, waits a while, flashes a warning and snaps shut, killing
//! the Pikmin inside. The pearl is a separate enemy that stays where it was put and can be broken (while the shell
//! is open) for what it holds. Set `count[0]` = 1 before the shell starts for a pearl that holds a ship part.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::{PEARL, ROCPE, TekiInfo};
use crate::object::*;
use crate::tai::*;
use libpikmin_assets::teki::{tpf, tpi};

// TaiShellIntParams / TaiShellFloatParams
const CLOSING_LOOP_COUNT: usize = tpi::SPECIFIC;
const OPENING_PERIOD: usize = 50;

// TaiShellStateID
const NORMAL: i32 = 0;
const OPEN: i32 = 1;
const WARN: i32 = 2;
const CLOSE: i32 = 3;
const EAT: i32 = 4;

/// `BTeki::outputSpawnPosition`: the pearl's place in the shell's mouth.
fn spawn_position(t: &Teki) -> Vec3 {
    let k = t.info.scale;
    t.pos + t.forward() * (t.f(tpf::SPAWN_DISTANCE) * k) + Vec3::Y * (t.f(tpf::SPAWN_HEIGHT) * k)
}

/// The pearl is made when the shell starts (`TaiShellStrategy::start`).
fn make_pearl() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.clear_option(opt::SHADOW_VISIBLE);
            if t.flags[0] {
                return;
            }
            t.flags[0] = true;
            let kind = if t.count[0] == 1 { ROCPE } else { PEARL };
            w.out.push(Out::Spawn { kind, pos: spawn_position(t), yaw: t.yaw, parent: t.id, velocity: Vec3::ZERO });
        })
        .build()
}

/// `TaiShellNaviPikiInsideAction`: something is at the pearl.
fn something_inside(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let at = spawn_position(t);
            let range = t.f(tpf::ATTACKABLE_RANGE);
            w.navis.iter().any(|n| n.alive && dist_xz(n.pos, at) < range)
                || w.sim.pikis().iter().any(|p| p.is_alive() && dist_xz(p.pos, at) < range)
        })
        .build()
}

/// `TaiShellEatAction`: the Pikmin at the pearl that are not clinging to the shell are killed.
fn eat(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let at = spawn_position(t);
            let range = t.f(tpf::ATTACKABLE_RANGE);
            let ids: Vec<_> = w
                .sim
                .pikis()
                .iter()
                .filter(|p| p.is_alive() && p.state.latched_to() != Some(t.creature) && dist_xz(p.pos, at) < range)
                .map(|p| p.id)
                .collect();
            for id in &ids {
                w.sim.kill_piki(*id);
            }
            let hit = w.navis.iter().any(|n| n.alive && dist_xz(n.pos, at) < range);
            !ids.is_empty() || hit
        })
        .build()
}

/// `TaiCountLoopAction`.
fn count_loop(next: i32, param: usize) -> Act {
    Action::new(next)
        .on_start(|t, _| t.loop_count = 0)
        .on_act(move |t, _| {
            if t.key(key::LOOP_END) {
                t.loop_count += 1;
                return t.loop_count >= t.i(param);
            }
            false
        })
        .build()
}

/// The shell's twinkle, which the decomp keeps as a particle generator on a joint.
fn twinkle() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            t.reg[0] -= w.dt;
            if t.reg[0] <= 0.0 {
                t.reg[0] = 1.0;
                w.out.push(Out::Effect { id: effects::SHELL_TWINKLE, pos: spawn_position(t) });
            }
            false
        })
        .build()
}

pub fn clamclamp(info: &TekiInfo) -> Strategy {
    let open_for = info.floats[OPENING_PERIOD];
    let mut s = Strategy::new(5, NORMAL);
    s.set(NORMAL, vec![make_pearl(), twinkle(), continuous_motion(NO_TRANSIT, WAIT1), something_inside(OPEN)]);
    s.set(OPEN, vec![twinkle(), continuous_motion(NO_TRANSIT, TYPE1), timer(WARN, 0, open_for, 0.0)]);
    s.set(WARN, vec![twinkle(), count_loop(CLOSE, CLOSING_LOOP_COUNT), continuous_motion(NO_TRANSIT, TYPE2)]);
    s.set(CLOSE, vec![eat(EAT), once(NORMAL)]);
    s.set(EAT, vec![continuous_motion(NORMAL, TYPE4)]);
    s
}

// ---- the pearl

const PEARL_DIE: i32 = 0;
const PEARL_DAMAGE: i32 = 1;
const PEARL_NORMAL: i32 = 2;

/// The pearl breaks: its treasure (a pellet here) falls out, and the shell's treasure sound plays.
fn pearl_die(treasure: bool) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            w.out.push(Out::Sound { name: "SE_SHELL_TRESURE", pos: t.pos });
            w.out.push(Out::Sound { name: "SE_KURIONE_HIT", pos: t.pos });
            if treasure {
                w.out.push(Out::SpawnPellets { pos: t.pos, number: 2, color: (t.id % 3) as u32, count: 1 });
            }
            w.out.push(Out::Dying { teki: t.id });
            t.start_motion(DEAD);
            t.clear_option(opt::ALIVE);
            t.hp = 0.0;
            t.clear_option(opt::SHADOW_VISIBLE);
            w.out.push(Out::Effect { id: effects::SHELL_BOOM, pos: t.pos });
        })
        .on_act(|t, w| {
            if t.animation_finished() {
                t.die(w);
                return true;
            }
            false
        })
        .build()
}

fn pearl_strategy(treasure: bool) -> Strategy {
    let dead_check = dead(PEARL_DIE);
    let mut s = Strategy::new(3, PEARL_NORMAL);
    s.set(PEARL_DIE, vec![pearl_die(treasure)]);
    s.set(PEARL_DAMAGE, vec![dead_check.clone(), damaging(PEARL_NORMAL, DAMAGE)]);
    s.set(PEARL_NORMAL, vec![dead_check, damage(PEARL_DAMAGE), motion(NO_TRANSIT, WAIT1)]);
    s
}

/// The plain pearl: a numbered pellet inside.
pub fn pearl(_info: &TekiInfo) -> Strategy {
    pearl_strategy(true)
}

/// The pearl that is a ship part (the host gives the part when it dies).
pub fn rocpe(_info: &TekiInfo) -> Strategy {
    pearl_strategy(false)
}
