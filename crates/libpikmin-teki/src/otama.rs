//! The Wogpole: `TAIotamaStrategy` (TAIotama.cpp). It dawdles around its territory, waiting a random while
//! between ambles; one amble in twenty starts with a hop. Seeing a player it hops and runs for it.

use crate::anim::key;
use crate::bulborb::*;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::tpf;

// TAIotamaFloatParams
const MIN_WAIT_TIME: usize = 50;
const MAX_WAIT_TIME: usize = 51;

// TAIotamaStateID
const WAIT: i32 = 1;
const SET_TARGET: i32 = 2;
const GO_TARGET: i32 = 3;
const APPEAL_TARGET: i32 = 4;
const APPEAL_RUN: i32 = 5;
const RUN_AWAY: i32 = 6;

/// `TAIAwaitOtama`: stand for a random time, letting its slide die away first.
fn wait_otama(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            t.counter = 0.0;
            t.reg[0] = w.rng.range(t.f(MIN_WAIT_TIME), t.f(MAX_WAIT_TIME).max(t.f(MIN_WAIT_TIME)));
        })
        .on_act(move |t, w| {
            if reserve_act(t, m) {
                t.counter += w.dt;
            }
            t.target_vel.x *= 0.95;
            t.target_vel.z *= 0.95;
            t.counter > t.reg[0] && t.target_vel.x.abs() < 0.1 && t.target_vel.z.abs() < 0.1
        })
        .build()
}

/// `TAIAsetTargetOtama`: a random place in the territory; one time in twenty it hops first.
fn set_target_otama(normal: i32, rare: i32) -> Act {
    Action::new(normal)
        .on_start(move |t, w| {
            let a = w.rng.range(0.0, std::f32::consts::TAU);
            let d = w.rng.range(0.0, t.f(tpf::DANGER_TERRITORY_RANGE));
            t.target_pos = t.nest + glam::Vec3::new(d * a.sin(), 0.0, d * a.cos());
            t.target_pos.y = t.ground_y.unwrap_or(t.nest.y);
            t.next_override = Some(if w.rng.range(0.0, 100.0) > 5.0 { normal } else { rare });
        })
        .on_act(|_, _| true)
        .build()
}

/// `TAIAappealOtama`: the hop. At its action key it knocks away what is under it, with a splash in water.
fn appeal(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::ACTION0) {
                t.flick(w);
                if t.in_water {
                    w.out.push(Out::Effect { id: crate::effects::BUBBLES, pos: t.pos });
                    w.out.push(Out::Sound { name: "SE_OTAMA_WATERJUMP", pos: t.pos });
                } else {
                    w.out.push(Out::Sound { name: "SE_OTAMA_JUMP", pos: t.pos });
                }
            }
            t.key(key::FINISHED)
        })
        .build()
}

/// `TAIArunAwayOtama::setTargetPosition`: a place away from the nearest player; true once it is out of sight.
fn flee_target(t: &mut Teki, w: &World) -> bool {
    let Some(n) = w.navis.iter().filter(|n| n.alive).min_by(|a, b| dist_xz(a.pos, t.pos).total_cmp(&dist_xz(b.pos, t.pos))) else {
        return true;
    };
    let mut offset = (t.pos - n.pos).with_y(0.0);
    let dist = offset.length();
    if dist > t.f(tpf::VISIBLE_RANGE) {
        return true;
    }
    offset *= t.f(tpf::SAFETY_TERRITORY_RANGE) / dist.max(1e-3);
    t.target_pos = t.pos + offset;
    if t.target_pos.distance(t.nest) > t.f(tpf::DANGER_TERRITORY_RANGE) {
        let off = (t.target_pos - t.nest).normalize_or_zero() * t.f(tpf::DANGER_TERRITORY_RANGE);
        t.target_pos = t.nest + off;
    }
    false
}

/// `TAIArunAwayOtama`: scamper away from the player at a quickened pace.
fn run_away(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            flee_target(t, w);
            t.manual_speed = Some(50.0);
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let mut done = false;
            if flee_target(t, w) {
                t.manual_speed = None;
                done = true;
            }
            let (goal, run) = (t.target_pos, t.f(tpf::RUN_VELOCITY));
            t.move_toward_face_priority(goal, run, w.dt);
            done
        })
        .build()
}

pub fn wogpole(_info: &TekiInfo) -> Strategy {
    let (dead_check, damage, stop) = (dead(0), yai_damage(false), stop_move());
    let mut s = Strategy::new(7, WAIT);
    s.set(0, vec![yai_dying(DEAD), stop]);
    s.set(WAIT, vec![damage.clone(), dead_check.clone(), wait_otama(SET_TARGET, WAIT1), visible_navi(APPEAL_RUN)]);
    s.set(SET_TARGET, vec![damage.clone(), dead_check.clone(), set_target_otama(GO_TARGET, APPEAL_TARGET)]);
    s.set(APPEAL_TARGET, vec![damage.clone(), dead_check.clone(), appeal(GO_TARGET, WAITACT1)]);
    s.set(GO_TARGET, vec![
        damage.clone(),
        dead_check.clone(),
        go_target_face(WAIT, MOVE1),
        visible_navi(APPEAL_RUN),
        timer_reaction(SET_TARGET, |_| 3.0),
    ]);
    s.set(APPEAL_RUN, vec![damage.clone(), dead_check.clone(), appeal(RUN_AWAY, WAITACT1)]);
    s.set(RUN_AWAY, vec![damage, dead_check, run_away(WAIT, MOVE1)]);
    s
}
