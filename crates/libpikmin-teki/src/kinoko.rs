//! The Puffstool: `TaiKinokoStrategy` (taikinoko.cpp). It stands about until a player comes near, then turns its back
//! and runs. Hit enough while running it trips and lies on its back, shaking off the Pikmin on it; whether
//! it was tripped or only chased for too long, it ends by puffing out a cloud of spores. Away from home it
//! turns back and walks there. Its spores are not the decomp's (which change Pikmin into Puffmin, a mode
//! the core has no use for): they hurt what they touch.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::tpf;

// TaiKinokoFloatParams, TaiKinokoIntParams
const TURNING_AWAY_PERIOD: usize = 50;
const RUNNING_AWAY_PERIOD: usize = 51;
const FLICK_PERIOD: usize = 52;
const STANDING_DAMAGE_RATE: usize = 54;
const WAITING_PERIOD: usize = 57;
const SLOW_TURN_VELOCITY: usize = 58;
const GOING_HOME_TERRITORY_RANGE: usize = 59;
const TURNING_OVER_DAMAGE_COUNT: usize = 20;
const FLICK_LOOP_COUNT: usize = 21;

// TaiKinokoStateID
const DEAD_UPRIGHT: i32 = 0;
const DEAD_FLIPPED: i32 = 1;
const WAIT: i32 = 2;
const TURNING_AWAY: i32 = 3;
const RUNNING_AWAY: i32 = 4;
const TURNING_OVER: i32 = 5;
const FLIPPED: i32 = 6;
const FLICK_STATE: i32 = 7;
const SPORE_ATTACK_FLIP: i32 = 8;
const UNUSED_MOTION: i32 = 9;
const TURN_TO_HOME: i32 = 10;
const GOING_HOME: i32 = 11;
const SPORE_ATTACK_RUN: i32 = 12;

/// `TaiDamageScaleAction`: the damage waiting is cut while it stands.
fn damage_scale(rate: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, _| {
            t.stored_damage *= rate;
            false
        })
        .build()
}

/// `TaiDamageCountAction`: hit often enough.
fn damage_count(next: i32, n: i32) -> Act {
    Action::new(next).on_act(move |t, _| t.damage_count >= n as f32).build()
}

/// `TaiTargetVisibleNaviAction`: the nearest player in sight becomes the target.
fn target_visible_navi(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.closest(w, |c| c.piki.is_none() && t.visible(c)) {
            Some(c) => {
                t.targets[0] = Some(c.target);
                true
            }
            None => false,
        })
        .build()
}

/// `TaiVisibleTargetAction`.
fn visible_target(next: i32) -> Act {
    Action::new(next).on_act(|t, w| t.target(0, w).is_some_and(|c| t.visible(&c))).build()
}

/// `TaiNotAction`: acts when the other does not.
fn not(next: i32, a: Act) -> Act {
    let (a1, a2, a3) = (a.clone(), a.clone(), a);
    Action::new(next)
        .on_start(move |t, w| a1.start(t, w))
        .on_finish(move |t, w| a2.finish(t, w))
        .on_act(move |t, w| !a3.act(t, w))
        .build()
}

/// `TaiTurningAwayAction`: turn to face away from the target; true once it does (or the target is gone).
fn turning_away(next: i32, m: usize, speed: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let Some(c) = t.target(0, w) else { return true };
            if !t.visible(&c) && dist_xz(t.pos, c.pos) > t.f(tpf::VISIBLE_RANGE) {
                return true;
            }
            if t.anim.counter < t.key_value(0) {
                return false;
            }
            t.target_angle = t.calc_target_direction(c.pos) + std::f32::consts::PI;
            let a = t.target_angle;
            t.turn_toward(a, speed, w.dt)
        })
        .build()
}

/// `TaiRunningAwayToTargetDirectionAction`: run along the direction decided.
fn running_away_dir(m: usize, speed: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let dir = dir_vec(t.target_angle);
            t.target_vel = dir * speed;
            face_velocity(t, 0.3);
            let _ = w;
            false
        })
        .build()
}

/// `TaiKinokoTerritoryRunningAwayAction`: far from home, running outward, it bends its way along the territory's
/// edge instead of out of it.
fn territory_running_away() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, _| {
            let dist = t.territory_distance();
            if dist < t.f(GOING_HOME_TERRITORY_RANGE) {
                return false;
            }
            let home = (t.pos - t.nest).with_y(0.0).normalize_or_zero();
            let dir = t.forward();
            let dot = dir.dot(home);
            if dot <= 0.0 {
                return false;
            }
            let side = home.cross(Vec3::Y);
            let along = side.dot(dir);
            let pick = |v: Vec3| v.x.atan2(v.z);
            if dist > t.f(tpf::DANGER_TERRITORY_RANGE) {
                t.target_angle = if along >= 0.0 { pick(side) } else { pick(-side) };
                return false;
            }
            if dot.abs() <= std::f32::consts::SQRT_2 / 2.0 {
                return false;
            }
            t.target_angle = if along >= 0.0 { pick(home + side) } else { pick(home - side) };
            false
        })
        .build()
}

/// `TaiKinokoTurningOverAction`: the trip: a shove forward, and what is on it is flung off.
fn turning_over() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.target_vel = t.forward() * 100.0;
            t.flick_upper(w);
        })
        .build()
}

/// `TaiKinokoChargingSporesAction`.
fn charging_spores() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.key(key::ACTION1) {
                w.out.push(Out::Effect { id: effects::KINOKO_CHARGE_SPORES, pos: t.pos });
            }
            false
        })
        .build()
}

/// `TaiKinokoDischargingSporesAction`: at the first key what is on it is flung off; at the third the spores go out.
fn discharging_spores() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.animation_finished() {
                return false;
            }
            if t.key(key::ACTION0) {
                t.flick_upper(w);
            } else if t.key(key::ACTION2) {
                let range = t.f(tpf::ATTACK_RANGE);
                let damage = t.f(tpf::ATTACK_POWER);
                w.sim.attack_pikis_in_sphere(t.pos, range + t.coll_radius, damage);
                for n in w.navis.iter().filter(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= range + t.coll_radius) {
                    w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: 0.0 });
                }
                w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
            }
            false
        })
        .build()
}

/// `TaiTimerAction` on one of the kind's own periods.
fn timer_of(info: &TekiInfo, next: i32, idx: usize, param: usize, rate: f32) -> Act {
    timer(next, idx, info.floats[param], rate)
}

/// `TaiCountLoopAction`.
fn count_loop(n: i32) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| t.loop_count = 0)
        .on_act(move |t, _| {
            if t.key(key::LOOP_END) {
                t.loop_count += 1;
                return t.loop_count >= n;
            }
            false
        })
        .build()
}

pub fn puffstool(info: &TekiInfo) -> Strategy {
    let stop = stop_move();
    let stopping = stopping_move(MOVE1);
    let finish_stopping = finish_stopping_move();
    let dead_up = dead(DEAD_UPRIGHT);
    let dead_flip = dead(DEAD_FLIPPED);
    let sim_damage = simultaneous_damage(NO_TRANSIT);
    let scale = damage_scale(info.floats[STANDING_DAMAGE_RATE]);
    let counter = |next| counterattack_simultaneous_damage(next);
    let target_navi = target_visible_navi(TURNING_AWAY);
    let flip_count = info.ints.get(FLICK_LOOP_COUNT).copied().unwrap_or(1);

    let mut s = Strategy::new(13, WAIT);
    s.set(DEAD_UPRIGHT, vec![stop.clone(), start_dying(), dying(DEAD)]);
    s.set(DEAD_FLIPPED, vec![stop.clone(), start_dying(), dying(DAMAGE)]);
    s.set(WAIT, vec![
        stopping.clone(),
        dead_up.clone(),
        scale.clone(),
        counter(TURNING_AWAY),
        target_navi.clone(),
        and(
            TURN_TO_HOME,
            timer_of(info, NO_TRANSIT, 0, WAITING_PERIOD, 0.4),
            outside_territory(NO_TRANSIT, info.floats[GOING_HOME_TERRITORY_RANGE]),
        ),
        continuous_motion(NO_TRANSIT, WAIT1),
    ]);
    s.set(TURNING_AWAY, vec![
        stopping.clone(),
        dead_up.clone(),
        scale.clone(),
        counter(RUNNING_AWAY),
        not(RUNNING_AWAY, visible_target(NO_TRANSIT)),
        timer_of(info, RUNNING_AWAY, 0, TURNING_AWAY_PERIOD, 0.0),
        turning_away(RUNNING_AWAY, WAITACT1, info.floats[tpf::TURN_VELOCITY]),
    ]);
    s.set(RUNNING_AWAY, vec![
        finish_stopping.clone(),
        territory_running_away(),
        running_away_dir(MOVE1, info.floats[tpf::RUN_VELOCITY]),
        dead_up.clone(),
        scale.clone(),
        sim_damage.clone(),
        damage_count(TURNING_OVER, info.ints.get(TURNING_OVER_DAMAGE_COUNT).copied().unwrap_or(5)),
        damage_count_reset(),
        timer_of(info, SPORE_ATTACK_RUN, 0, RUNNING_AWAY_PERIOD, 0.0),
    ]);
    s.set(TURNING_OVER, vec![
        dead_flip.clone(),
        scale.clone(),
        sim_damage.clone(),
        animation_key_action(FLIPPED, key::ACTION0),
        motion(NO_TRANSIT, TYPE1),
        turning_over(),
    ]);
    let flick_damage = simultaneous_damage(FLICK_STATE);
    s.set(FLIPPED, vec![dead_flip.clone(), stop.clone(), flick_damage.clone(), timer_of(info, SPORE_ATTACK_FLIP, 0, FLICK_PERIOD, 0.0)]);
    s.set(FLICK_STATE, vec![dead_flip, stop, flick_damage, timer_elapsed(SPORE_ATTACK_FLIP, 0)]);
    let spore_end = serial(WAIT, vec![count_loop(flip_count), finish_motion(NO_TRANSIT)]);
    s.set(SPORE_ATTACK_FLIP, vec![
        sim_damage.clone(),
        charging_spores(),
        discharging_spores(),
        spore_end.clone(),
        continuous_motion(NO_TRANSIT, FLICK),
    ]);
    s.set(SPORE_ATTACK_RUN, vec![
        stopping,
        sim_damage.clone(),
        charging_spores(),
        discharging_spores(),
        spore_end,
        continuous_motion(NO_TRANSIT, TYPE3),
    ]);
    s.set(UNUSED_MOTION, vec![sim_damage, serial(WAIT, vec![finish_motion(NO_TRANSIT), motion(NO_TRANSIT, TYPE2)])]);
    s.set(TURN_TO_HOME, vec![
        dead_up.clone(),
        scale.clone(),
        counter(TURNING_AWAY),
        target_navi.clone(),
        target_nest(),
        turning_to_target_position(GOING_HOME, WAITACT1, info.floats[SLOW_TURN_VELOCITY]),
    ]);
    s.set(GOING_HOME, vec![
        finish_stopping,
        going_home(MOVE1, info.floats[tpf::WALK_VELOCITY]),
        dead_up,
        scale,
        counter(TURNING_AWAY),
        target_navi,
        inside_territory(WAIT, info.floats[tpf::SAFETY_TERRITORY_RANGE]),
    ]);
    s
}
