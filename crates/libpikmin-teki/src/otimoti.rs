//! The Wollywogs: `TaiOtimotiStrategy` (taiotimoti.cpp). They sit and wash, notice something, turn to it,
//! crouch and jump in a parabola, hang at the top, drop and press everything underneath, then guard
//! themselves for a moment before going back to waiting. Too much damage on them shakes Pikmin off with a
//! hop in place. The yellow one is the same strategy with its own numbers.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
#[allow(unused_imports)]
use crate::tai::animation_key_action;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use libpikmin_assets::teki::tpf;

// OTIMOTIPF_*
const DROP_VELOCITY: usize = 50;
const ATTACK_START_HEIGHT: usize = 51;
const WASH_PERIOD: usize = 52;
const WASH_CHANCE: usize = 53;
const TURN_PERIOD: usize = 54;
const IMPASSABLE_PERIOD: usize = 56;
const IMPASSABLE_DISTANCE: usize = 57;
const MISS_MIN_COUNT: usize = 58;
const MISS_MAX_COUNT: usize = 59;
const MISS_MIN_CHANCE: usize = 60;
const MISS_MAX_CHANCE: usize = 61;

/// `TaiOtimotiFlickAction`: enough hits for the Pikmin on it; it forgets its target.
fn otimoti_flick(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::LOWER_RANGE);
            let n = t.count_pikis_in_range(w, range);
            if t.damage_count >= t.flick_damage_count(n) as f32 {
                t.target_pos = t.pos;
                t.targets[0] = None;
                return true;
            }
            false
        })
        .build()
}

/// `TaiOtimotiFailToJumpAction`: the more Pikmin close underneath, the likelier it hops badly.
fn fail_to_jump(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::LOWER_RANGE);
            let n = t.count_pikis_in_range(w, range) as f32;
            let (c0, c1) = (t.f(MISS_MIN_COUNT), t.f(MISS_MAX_COUNT));
            let (p0, p1) = (t.f(MISS_MIN_CHANCE), t.f(MISS_MAX_CHANCE));
            let k = if c1 > c0 { ((n - c0) / (c1 - c0)).clamp(0.0, 1.0) } else { 1.0 };
            w.rng.occurred(p0 + (p1 - p0) * k)
        })
        .build()
}

/// `TaiParabolaAction` / `TaiAccelerationAction` (`PeveAccelerationEvent::update`).
fn accel_event() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.accel_on {
                t.pos += t.vel3 * w.dt;
                t.vel3 += t.accel3 * w.dt;
            }
            false
        })
        .build()
}

/// `TaiOtimotiJumpingAction`: crouch over, off it goes toward the target, not further than its reach.
fn jumping(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, w| {
            t.start_flying();
            t.flick_lower(w);
            if let Some(c) = t.target(0, w) {
                t.target_pos = c.pos;
            }
            let pos = t.pos;
            let mut dir = (t.target_pos - pos).with_y(0.0);
            let reach = t.f(tpf::ATTACKABLE_RANGE);
            if dir.length() > reach {
                dir = dir.normalize() * reach;
            }
            dir *= 0.7;
            let g = t.gravity;
            let up = (2.0 * g * t.f(tpf::FLIGHT_HEIGHT)).sqrt();
            let time_up = up / g;
            let speed = dir.length() / time_up.max(1e-3);
            let mut v = if dir.length() > 1e-4 { dir.normalize() * speed } else { Vec3::ZERO };
            v.y = up;
            t.vel3 = v;
            t.accel3 = Vec3::new(0.0, -g, 0.0);
            t.accel_on = true;
            t.action_vel = Vec3::ZERO;
            w.out.push(Out::Sound { name: "SE_FLOG_HIJUMP", pos });
        })
        .on_act(|t, _| {
            // Up to the top of the jump.
            if t.vel3.y <= 0.0 {
                t.action_vel = Vec3::new(t.vel3.x, 0.0, t.vel3.z);
                return true;
            }
            false
        })
        .build()
}

/// `TaiOtimotiAirWaitingAction`: it slows in the air and hangs there until its motion loops.
fn air_waiting(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            let v = t.action_vel;
            t.vel3 = v;
            let a = if v.length() > 1e-4 { -v.normalize() * 1000.0 } else { Vec3::ZERO };
            t.accel3 = a;
            t.accel_on = true;
        })
        .on_act(|t, _| {
            let moving = t.action_vel.length() > 1e-4;
            let still_forward = moving && t.vel3.length() > 1e-4 && t.vel3.normalize().dot(t.action_vel.normalize()) > 0.1;
            if !still_forward {
                t.vel3 = Vec3::ZERO;
                t.accel3 = Vec3::ZERO;
                t.stop_move();
            }
            t.key(key::LOOP_END)
        })
        .build()
}

/// `TaiOtimotiDroppingAction`: falls at its drop speed until it is low enough to attack.
fn dropping() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| {
            t.finish_flying();
            t.accel_on = false;
            t.action_vel = Vec3::new(0.0, -t.f(DROP_VELOCITY), 0.0);
            t.set_option(opt::GRAVITATABLE);
        })
        .on_act(|t, _| t.height() <= t.f(ATTACK_START_HEIGHT))
        .on_event(|t, _, e| {
            if matches!(e, Event::Ground) {
                t.finish_flying();
                t.clear_option(opt::GRAVITATABLE);
            }
            false
        })
        .build()
}

/// `TaiOtimotiPressingAction`: the attack motion, and on landing everything underneath is pressed.
fn pressing() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| t.start_motion(ATTACK))
        .on_act(|t, _| t.height() <= 1.0)
        .on_event(|t, w, e| {
            if !matches!(e, Event::Ground) {
                return false;
            }
            t.finish_flying();
            t.clear_option(opt::GRAVITATABLE);
            let (centre, range, damage) = (t.pos, t.f(tpf::ATTACK_RANGE) + t.info.collision_radius, t.f(tpf::ATTACK_POWER));
            w.sim.crush_pikis_in_sphere(centre, range, damage);
            for n in w.navis.iter().filter(|n| n.alive && dist_xz(centre, n.pos) - n.size <= range) {
                w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: centre, knock: 60.0 });
            }
            t.flick_upper(w);
            t.damage_count = 0.0;
            w.out.push(Out::Shake { pos: centre, strength: 3.0 });
            true
        })
        .build()
}

/// `TaiOtimotiAttackingAction`: down and in the attack motion until it is done.
fn attacking(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, _| {
            if t.height() <= 1.0 {
                if let Some(g) = t.ground_y {
                    t.pos.y = g;
                }
                t.finish_flying();
                t.clear_option(opt::GRAVITATABLE);
            }
            t.animation_finished() || t.anim.is_finished()
        })
        .build()
}

/// `TaiOtimotiAttackingEffectAction`: the landing's rings.
fn landing_effect() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            let at = Vec3::new(t.pos.x, t.ground_y.unwrap_or(t.pos.y), t.pos.z);
            w.out.push(Out::Effect { id: 81, pos: at });
            w.out.push(Out::Effect { id: 82, pos: at });
            w.out.push(Out::Sound { name: "SE_FLOG_LAND", pos: at });
        })
        .build()
}

/// `TaiOtimotiStartDroppingWaterAction`: water flies off it after the jump.
fn dropping_water() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            let at = t.pos;
            w.out.push(Out::Effect { id: 83, pos: at });
        })
        .build()
}

/// `TaiStartingTimerAction`: a timer that, once out, only counts if a chance comes up (else it restarts).
fn starting_timer(next: i32, idx: usize, len: f32, rate: f32, chance: f32) -> Act {
    Action::new(next)
        .on_start(move |t, w| t.timers[idx] = len + len * rate * w.rng.range(-1.0, 1.0))
        .on_act(move |t, w| {
            if t.timer_elapsed(idx) {
                if w.rng.occurred(chance) {
                    return true;
                }
                t.timers[idx] = len + len * rate * w.rng.range(-1.0, 1.0);
            }
            false
        })
        .build()
}

/// `TaiWaitTurningAction`: a turn to a random direction while idling.
fn wait_turning(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            t.start_motion(m);
            t.target_angle = w.rng.range(0.0, std::f32::consts::TAU);
        })
        .on_act(|t, w| {
            let face = t.direction();
            let near = nearer_direction(face, t.target_angle);
            if (near - face).abs() <= 0.1 * std::f32::consts::PI {
                return true;
            }
            let (a, s) = (t.target_angle, t.f(tpf::TURN_VELOCITY));
            t.turn_toward(a, s, w.dt);
            false
        })
        .build()
}

/// `TaiTurningAction`: turns in place toward its target while its motion plays.
fn turning(m: usize, speed: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, _| {
            let cur = t.current_motion();
            if cur < 0 || t.anim.is_finishing() {
                t.start_motion(m);
            } else if cur != m as i32 {
                t.finish_motion();
            }
        })
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let Some(c) = t.target(0, w) else { return true };
            if t.anim.counter < t.key_value(0) {
                return false;
            }
            t.target_angle = t.calc_target_direction(c.pos);
            let a = t.target_angle;
            t.turn_toward(a, speed, w.dt)
        })
        .build()
}

/// `TaiOutsideKeyStopMoveAction`: it only moves between two keys of its hop.
fn outside_key_stop_move(start_key: u8, end_key: u8) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, _| {
            let (a, b) = (t.key_value(start_key), t.key_value(end_key));
            if !(a <= t.anim.counter && t.anim.counter <= b) {
                t.stop_move();
            }
            false
        })
        .build()
}

/// `TaiImpassableAction` as the Wollywog uses it (timer 0).
fn impassable(next: i32, period: f32, distance: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.target_pos = t.pos;
            t.timers[0] = period;
        })
        .on_act(move |t, _| {
            if dist_xz(t.target_pos, t.pos) >= distance {
                t.target_pos = t.pos;
                t.timers[0] = period;
                return false;
            }
            t.timer_elapsed(0)
        })
        .build()
}

/// `TaiOtimotiStrategy`.
pub fn wollywog(info: &TekiInfo) -> Strategy {
    let danger = info.floats[tpf::DANGER_TERRITORY_RANGE];
    let safety = info.floats[tpf::SAFETY_TERRITORY_RANGE];
    let run = info.floats[tpf::RUN_VELOCITY];
    let turn = info.floats[tpf::TURN_VELOCITY];
    let unguarded = info.floats[tpf::UNGUARDED_PERIOD];

    let stop = stop_move();
    let dead0 = dead(0);
    let flick6 = otimoti_flick(6);
    let assail = target_nearest_assailant(NO_TRANSIT);
    let coll1 = target_nearest_collision(NO_TRANSIT);
    let coll2 = target_nearest_collision(5);
    let vis1 = target_visible_navi_piki(NO_TRANSIT);
    let vis2 = target_visible_navi_piki(5);
    let lost = target_lost(2);
    let attackable = attackable_navi_piki(6);
    let sim_damage = simultaneous_damage(NO_TRANSIT);
    let counter = counterattack_simultaneous_damage(5);
    let key_stop = outside_key_stop_move(2, 3);

    let mut s = Strategy::new(17, 2);
    s.set(0, vec![stop.clone(), start_dying(), dying(DEAD)]);
    s.set(1, vec![dead0.clone(), flick6.clone(), sim_damage.clone(), damaging(2, DAMAGE)]);
    s.set(2, vec![
        stop.clone(), dead0.clone(), flick6.clone(), outside_territory(14, danger), counter.clone(), coll2.clone(), vis2.clone(),
        starting_timer(3, 1, info.floats[TURN_PERIOD], 0.5, info.floats[WASH_CHANCE]),
        starting_timer(4, 0, info.floats[WASH_PERIOD], 0.5, info.floats[WASH_CHANCE]),
        motion(NO_TRANSIT, WAIT1),
    ]);
    s.set(4, vec![dead0.clone(), flick6.clone(), counter.clone(), coll2.clone(), vis2.clone(), motion(2, WAITACT2)]);
    s.set(3, vec![dead0.clone(), flick6.clone(), counter.clone(), coll2.clone(), vis2.clone(), wait_turning(2, WAITACT1)]);
    s.set(5, vec![
        dead0.clone(), stop.clone(), flick6.clone(), warn(), assail.clone(), coll1, vis1, sim_damage.clone(), attackable, lost,
        turning(WAITACT1, turn),
    ]);
    s.set(6, vec![stop.clone(), dead0.clone(), sim_damage.clone(), fail_to_jump(7), once(8)]);
    s.set(7, vec![dead0.clone(), sim_damage.clone(), motion(2, DAMAGE)]);
    s.set(8, vec![sim_damage.clone(), motion(NO_TRANSIT, FLICK), animation_key_action(9, key::ACTION0)]);
    s.set(9, vec![sim_damage.clone(), accel_event(), jumping(10), dropping_water()]);
    s.set(10, vec![sim_damage.clone(), motion(NO_TRANSIT, WAIT2), accel_event(), air_waiting(11)]);
    s.set(11, vec![
        stop.clone(), sim_damage.clone(), motion(NO_TRANSIT, TYPE1), serial(12, vec![dropping(), pressing(), once(NO_TRANSIT)]),
    ]);
    s.set(12, vec![dead0.clone(), sim_damage.clone(), attacking(13), landing_effect()]);
    s.set(13, vec![dead0.clone(), flick6.clone(), sim_damage.clone(), timer(2, 0, unguarded, 0.0), motion(NO_TRANSIT, WAIT1)]);
    s.set(14, vec![
        dead0.clone(), stop, flick6.clone(), sim_damage.clone(), target_nest(), turning_to_target_position(15, WAITACT1, turn),
    ]);
    s.set(15, vec![
        dead0.clone(), flick6.clone(), sim_damage.clone(), inside_territory(16, safety), going_home(MOVE1, run),
        impassable(16, info.floats[IMPASSABLE_PERIOD], info.floats[IMPASSABLE_DISTANCE]), key_stop.clone(),
    ]);
    s.set(16, vec![dead0, flick6, sim_damage, finish_motion(2), key_stop]);
    s.initial = 2;
    let _ = MISS_MAX_CHANCE;
    s
}
