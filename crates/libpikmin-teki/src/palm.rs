//! The Pellet Posy: `TaiPalmStrategy` (taipalm.cpp). A plant that does nothing but grow through three sizes, its
//! flower cycling through the three pellet colors; hit, it flinches, and when it is killed a fully grown one drops
//! a pellet of the flower's color. The size is `count[0]` (a host may start it larger), the color `count[1]`
//! (blue, red, yellow); `count[2]` = 1 holds the color still.

use crate::bulborb::*;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use libpikmin_assets::teki::tpf;

// TaiPalmFloatParams
const CHANGING_COLOR_PERIOD: usize = 50;
const CHANGING_COLOR_RANDOM: usize = 51;
const GROWING_PERIOD: usize = 52;

// TaiPalmStateID
const DEAD_STATE: i32 = 0;
const DAMAGE_STATE: i32 = 1;
const GROW: i32 = 2;
const NORMAL: i32 = 3;

/// `TaiPalmStrategy::translateMotionIndex`: the model has a set of motions for each size.
fn motion_for(t: &Teki, m: usize) -> usize {
    let size = t.count[0].clamp(0, 2) as usize;
    match (m, size) {
        (DEAD, s) => s * 3,
        (DAMAGE, s) => s * 3 + 1,
        (WAIT1, s) => s * 3 + 2,
        (WAITACT1, 0) => 9,
        (WAITACT1, 1) => 10,
        _ => 0,
    }
}

fn palm_motion(next: i32, m: usize) -> Act {
    Action::new(next).on_start(move |t, _| t.start_motion(motion_for(t, m))).on_act(|t, _| t.animation_finished()).build()
}

/// `TaiPalmDyingAction`: a grown posy drops its pellet.
fn palm_dying() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.start_motion(motion_for(t, DEAD));
            t.clear_option(opt::ALIVE);
            t.hp = 0.0;
            if t.count[0] >= 2 {
                w.out.push(Out::SpawnPellets { pos: t.pos + glam::Vec3::Y * 10.0, number: 1, color: t.count[1].rem_euclid(3) as u32, count: 1 });
            }
            w.out.push(Out::Dying { teki: t.id });
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

/// `TaiPalmDamagingAction`.
fn palm_damaging(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            t.start_motion(motion_for(t, DAMAGE));
            t.make_damaged();
        })
        .on_act(|t, _| t.animation_finished())
        .build()
}

/// `TaiPalmGrowingAction`: one size bigger.
fn growing() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| t.count[0] += 1)
        .build()
}

/// `TaiPalmGrowAction`: time to grow, until it is full size.
fn grow_action(next: i32) -> Act {
    Action::new(next).on_act(|t, _| t.count[0] < 2 && t.timer_elapsed(0)).build()
}

/// `TaiPalmChangingColorAction`: the flower goes to the next color now and then.
fn changing_color() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.count[2] == 1 || !t.timer_elapsed(1) {
                return false;
            }
            t.count[1] = (t.count[1] + 1).rem_euclid(3);
            let p = t.f(CHANGING_COLOR_PERIOD);
            t.timers[1] = p * w.rng.range(1.0 - t.f(CHANGING_COLOR_RANDOM), 1.0 + t.f(CHANGING_COLOR_RANDOM));
            false
        })
        .build()
}

pub fn pellet_posy(info: &TekiInfo) -> Strategy {
    let dead_check = dead(DEAD_STATE);
    let mut s = Strategy::new(4, NORMAL);
    s.on_start = Some(std::sync::Arc::new(|t: &mut Teki| {
        t.timers[0] = t.f(GROWING_PERIOD);
        t.timers[1] = t.f(CHANGING_COLOR_PERIOD);
        if t.count[2] != 1 {
            t.count[1] = (t.id % 3) as i32;
        }
        t.target_pos = t.pos;
    }));
    s.set(DEAD_STATE, vec![palm_dying()]);
    s.set(DAMAGE_STATE, vec![dead_check.clone(), palm_damaging(NORMAL), changing_color()]);
    s.set(GROW, vec![palm_motion(NORMAL, WAITACT1), growing(), timer(NO_TRANSIT, 0, info.floats[GROWING_PERIOD], 0.0)]);
    s.set(NORMAL, vec![dead_check, damage(DAMAGE_STATE), palm_motion(NO_TRANSIT, WAIT1), changing_color(), grow_action(GROW)]);
    let _ = tpf::LIFE;
    s
}
