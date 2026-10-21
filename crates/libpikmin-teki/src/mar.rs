//! The Puffy Blowhog: `TAImarStrategy` (TAImar.cpp). It floats about its territory; seeing something it closes
//! to a set distance, faces it and blasts a cone of wind that sweeps Pikmin off their feet. Pikmin that cling on
//! are shaken off after a while, or weigh it down so that it lands, shakes, and takes off again. The wind pushes
//! (it is not damage); the decomp's particle callback is a cone in front of the mouth.

use std::f32::consts::PI;

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::{tpf, tpi};

// TAImarFloatParams / TAImarIntParams
const ATTACK_OFFSET: usize = 50;
const WIND_CONE_ANGLE: usize = 51;
const WIND_MOTION_SPEED: usize = 52;
const SHAKE_TIME: usize = 53;
const TAKEOFF_TIME: usize = 54;
const FORCED_DESCENT_TIME: usize = 55;
const VERTICAL_SPEED: usize = 56;
const HITS_BEFORE_AIR_SHAKE: usize = tpi::SPECIFIC;
const PIKIS_TO_DESCEND: usize = tpi::SPECIFIC + 1;
const PIKIS_TO_TAKE_OFF: usize = tpi::SPECIFIC + 2;

// TAImarStateID
const DEAD_GROUND: i32 = 0;
const DEAD_AIR: i32 = 1;
const INIT: i32 = 2;
const FLY: i32 = 3;
const AIR_FLICKING: i32 = 4;
const DESCENDING: i32 = 5;
const LANDING: i32 = 6;
const GROUNDED: i32 = 7;
const GROUND_FLICK: i32 = 8;
const AFTER_FLICK: i32 = 9;
const TAKING_OFF: i32 = 10;
const APPROACH: i32 = 11;
const BREATHING: i32 = 12;
const TO_TERRITORY: i32 = 13;
const WATCH: i32 = 14;

fn stickers(t: &Teki, w: &World) -> i32 {
    w.sim.pikis().iter().filter(|p| p.is_alive() && p.state.latched_to() == Some(t.creature)).count() as i32
}

/// `TAIAinitMar`: a wide collision circle.
fn init(next: i32) -> Act {
    Action::new(next).on_start(|t, _| t.coll_radius = 64.0).on_act(|_, _| true).build()
}

/// `TAIAstickingPiki`: at least `param` Pikmin are on it.
fn sticking(next: i32, param: usize) -> Act {
    Action::new(next).on_act(move |t, w| stickers(t, w) >= t.i(param)).build()
}

/// `TAIAflickCheck` with the limit from a parameter.
fn flick_check_param(next: i32, param: usize) -> Act {
    Action::new(next)
        .on_act(move |t, _| {
            let hit = t.damage_count >= t.i(param) as f32;
            if hit {
                t.damage_count = 0.0;
            }
            hit
        })
        .build()
}

/// `TAIAflickCheckTimerMar`: Pikmin have been on it for `SHAKE_TIME`.
fn flick_check_timer(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            if stickers(t, w) > 0 {
                t.flags[0] = true;
            } else {
                t.flags[0] = false;
                t.counter = 0.0;
            }
            if t.flags[0] {
                t.counter += w.dt;
                if t.counter > t.f(SHAKE_TIME) {
                    t.flags[0] = false;
                    return true;
                }
            }
            false
        })
        .build()
}

/// `TAIAflickingMar`: the shake; only what is on its back is flung off.
fn flicking_upper_motion(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            t.start_motion(m);
            t.stop_move();
            t.clear_option(opt::DAMAGE_COUNTABLE);
            w.out.push(Out::Sound { name: "SE_CHAPPY_SWING", pos: t.pos });
        })
        .on_act(|t, w| {
            if t.key(key::FINISHED) {
                t.set_option(opt::DAMAGE_COUNTABLE);
                t.damage_count = 0.0;
                return true;
            }
            if t.key(key::ACTION0) {
                t.flick_upper(w);
            }
            false
        })
        .build()
}

/// `TAIAflyingBaseMar`.
fn flying_base_mar() -> Act {
    flying_base(|t| t.f(VERTICAL_SPEED))
}

/// `TAIAflyingDistance` (Mar's version): float to a set distance from the target, turn to it, and settle; then, by
/// chance (likelier with fewer Pikmin about), act.
fn flying_distance(next: i32, dive_angle: f32, face_adjust: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_flying();
            t.speed = 0.0;
        })
        .on_act(move |t, w| {
            let vel = t.f(tpf::RUN_VELOCITY);
            let Some(c) = t.target(0, w) else { return true };
            let ratio = (t.pos.y - c.pos.y).abs() / dive_angle.tan();
            let offset = ratio;
            let mut dist = c.pos.distance(t.pos) - offset;
            let mut check = false;
            if dist.abs() < 0.25 * vel {
                t.speed *= 27.0 * w.dt;
                check = true;
            } else {
                let cap = 0.05 * vel;
                if dist.abs() > cap {
                    dist = dist.signum() * cap;
                }
                t.speed += w.dt * dist * 900.0 * w.dt;
            }
            if t.speed.abs() > vel {
                t.speed = t.speed.signum() * vel;
            }
            t.target_angle = (c.pos.x - t.pos.x).atan2(c.pos.z - t.pos.z);
            let sep = nearer_direction(t.direction(), t.target_angle) - t.direction();
            t.yaw += sep * face_adjust * w.dt * 10.0;
            t.target_vel = Vec3::new(t.yaw.sin() * t.speed, t.target_vel.y, t.yaw.cos() * t.speed);
            let mut done = false;
            if sep.abs() < t.f(tpf::ATTACKABLE_ANGLE).to_radians() && check && t.speed.abs() < 1.0 {
                t.target_vel = Vec3::ZERO;
                done = true;
            }
            if !done {
                return false;
            }
            let weight = 1 + w.sim.pikis().iter().filter(|p| t.pos.distance(p.pos) < t.f(tpf::DANGER_TERRITORY_RANGE)).count() as i32;
            w.rng.range(0.0, 100.0 / weight as f32) * w.rng.range(0.0, 30.0) <= 1.0
        })
        .build()
}

/// `TAIAflyingDistanceInTerritoryMar`: fly toward the target, but staying inside the territory: the goal is a
/// point `ATTACK_OFFSET` (and the dive's run) from the target on the side toward the nest.
fn flying_distance_in_territory(next: i32, m: usize, dive_angle: f32, face_adjust: f32) -> Act {
    fn goal(t: &mut Teki, w: &World, dive_angle: f32) -> bool {
        let Some(c) = t.target(0, w) else {
            t.target_pos = t.nest;
            return true;
        };
        let ratio = (t.pos.y - c.pos.y).abs() / dive_angle.tan();
        let vel = t.f(ATTACK_OFFSET) + ratio;
        let mut away = t.nest - c.pos;
        away = if away.length() > 0.0 { away.normalize() } else { Vec3::X };
        t.target_pos = c.pos + away * vel;
        if t.target_pos.distance(t.nest) > t.f(tpf::DANGER_TERRITORY_RANGE) {
            let d = (t.target_pos - t.nest).normalize_or_zero();
            t.target_pos = t.nest + d * t.f(tpf::DANGER_TERRITORY_RANGE);
        }
        true
    }
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            t.start_flying();
            goal(t, w, dive_angle);
        })
        .on_act(move |t, w| {
            // `BTeki::turnToTarget`: face the creature it flies toward.
            if let Some(c) = t.target(0, w) {
                let a = (c.pos.x - t.pos.x).atan2(c.pos.z - t.pos.z);
                let sep = nearer_direction(t.direction(), a) - t.direction();
                t.yaw += sep * face_adjust * w.dt * 10.0;
            }
            if !reserve_act(t, m) {
                return false;
            }
            let vel = t.f(tpf::RUN_VELOCITY);
            let y_speed = t.target_vel.y;
            let mut dir = (t.target_pos - t.pos).with_y(0.0);
            let dist = dir.length();
            if dist > vel * 0.1 {
                dir = dir / dist * (vel * 0.1);
            }
            t.target_vel += dir * (w.dt * 900.0 * w.dt);
            let sp = t.target_vel.length();
            if sp > vel {
                t.target_vel = t.target_vel / sp * vel;
            }
            t.target_vel.y = y_speed;
            if dist_xz(t.target_pos, t.pos) <= t.f(tpf::WALK_VELOCITY) {
                return goal(t, w, dive_angle);
            }
            false
        })
        .build()
}

/// `TAIAdescent`: sink toward the ground below; after `FORCED_DESCENT_TIME` it just drops.
fn descent(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.start_flying();
            t.target_pos = Vec3::new(t.pos.x, t.ground_y.unwrap_or(0.0), t.pos.z);
            t.counter = 0.0;
        })
        .on_act(move |t, w| {
            let height = t.height();
            if !reserve_act(t, m) {
                return false;
            }
            t.counter += w.dt;
            if t.counter > t.f(FORCED_DESCENT_TIME) {
                t.finish_flying();
            }
            if height > 15.0 {
                let vel = t.f(tpf::WALK_VELOCITY);
                let mut accel = Vec3::new(t.target_pos.x - t.pos.x, -vel * 0.05, t.target_pos.z - t.pos.z);
                let len = accel.length();
                if len > vel * 0.1 {
                    accel = accel / len * (vel * 0.1);
                }
                t.target_vel += accel;
                let sp = t.target_vel.length();
                if sp > vel {
                    t.target_vel = t.target_vel / sp * vel;
                }
                return false;
            }
            true
        })
        .build()
}

/// `TAIAlandingMar`.
fn landing(next: i32, m: usize) -> Act {
    fn effect(t: &Teki, w: &mut World) {
        let at = Vec3::new(t.pos.x, t.ground_y.unwrap_or(t.pos.y), t.pos.z);
        if t.in_water {
            for _ in 0..3 {
                w.out.push(Out::Effect { id: effects::ONYON_BUBBLES, pos: t.pos });
            }
            w.out.push(Out::Sound { name: "SE_FLOG_WATERJUMP", pos: t.pos });
        } else {
            w.out.push(Out::Effect { id: effects::BIG_DUST_RING, pos: at });
            w.out.push(Out::Sound { name: "SE_MAR_DROP", pos: t.pos });
        }
        w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
    }
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.finish_flying();
            t.target_vel = Vec3::ZERO;
            t.flags[1] = true;
        })
        .on_act(move |t, w| {
            if t.height() < t.coll_radius && t.flags[1] {
                effect(t, w);
                t.flags[1] = false;
            }
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::ACTION0) && t.height() < 1.0 {
                effect(t, w);
            }
            if t.anim.is_finishing() {
                t.damage_count = 0.0;
                return true;
            }
            false
        })
        .build()
}

/// `TAIAtakeOffMar`: rise toward the flight height at the action key.
fn take_off(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            t.start_flying();
            t.target_vel = Vec3::ZERO;
            t.flags[2] = false;
            if t.in_water {
                for _ in 0..3 {
                    w.out.push(Out::Effect { id: effects::ONYON_BUBBLES, pos: t.pos });
                }
                w.out.push(Out::Sound { name: "SE_FLOG_WATERJUMP", pos: t.pos });
            }
        })
        .on_act(move |t, _| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::ACTION0) {
                t.flags[2] = true;
            }
            if t.flags[2] {
                let vel = t.f(tpf::WALK_VELOCITY);
                let up = if t.height() < t.f(tpf::FLIGHT_HEIGHT) { 1.0 } else { -1.0 };
                t.target_vel.y = (t.target_vel.y + vel * 0.5 * up).clamp(-vel, vel);
            }
            t.key(key::FINISHED)
        })
        .build()
}

/// `TAIAfireBreathMar`: the attack motion at its own speed; from the action key until it ends a cone of wind
/// pushes everything inside it away.
fn wind_breath(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.manual_speed = Some(t.f(WIND_MOTION_SPEED));
            t.flags[3] = false;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            t.manual_speed = Some(t.f(WIND_MOTION_SPEED));
            if t.key(key::ACTION0) {
                t.flags[3] = true;
                if let Some((mouth, _)) = t.part("kuti") {
                    let (down, c) = ((-27.0f32).to_radians(), 0.0);
                    let _ = c;
                    let dir1 = Vec3::new(t.yaw.sin() * down.cos(), down.sin(), t.yaw.cos() * down.cos());
                    w.out.push(Out::EffectDir { id: effects::MAR_WIND_JET, pos: mouth, dir: dir1 });
                    w.out.push(Out::Sound { name: "SE_MAR_WIND", pos: mouth });
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
            }
            if t.flags[3] {
                if let Some((mouth, _)) = t.part("kuti") {
                    let a = (-(27.0f32 + 18.5)).to_radians();
                    let dir2 = Vec3::new(t.yaw.sin() * a.cos(), a.sin(), t.yaw.cos() * a.cos());
                    blow(t, w, mouth, dir2);
                }
            }
            if t.key(key::FINISHED) {
                t.manual_speed = None;
                t.flags[3] = false;
                return true;
            }
            false
        })
        .build()
}

/// What is in the wind cone (`TAIeffectAttackEventCallBackMar::hitCreature`): Pikmin are knocked away, a player is
/// shoved along.
fn blow(t: &mut Teki, w: &mut World, mouth: Vec3, dir: Vec3) {
    let half = (t.f(WIND_CONE_ANGLE) / 2.0).to_radians();
    let length = 300.0;
    let in_cone = |p: Vec3| {
        let d = p - mouth;
        let dist = d.length();
        dist < length && dist > 1e-3 && d.dot(dir) / dist > half.cos()
    };
    let ids: Vec<_> = w.sim.pikis().iter().filter(|p| p.is_alive() && in_cone(p.pos)).map(|p| (p.id, p.pos)).collect();
    for (_, pos) in ids {
        let force = w.rng.range(150.0, 400.0);
        w.sim.flick_pikis_in_sphere(pos, 4.0, force, mouth);
    }
    for n in w.navis.iter().filter(|n| n.alive && in_cone(n.pos)) {
        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: 0.0, from: mouth, knock: 300.0 });
    }
}

/// `TAIAdyingMar`: the dying motion, with smoke and a jet from the mouth.
fn dying_mar(m: usize) -> Act {
    let inner = yai_dying(m);
    let (i1, i2) = (inner.clone(), inner);
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| i1.start(t, w))
        .on_act(move |t, w| {
            if t.key(key::ACTION0) {
                if let Some((mouth, _)) = t.part("kuti") {
                    w.out.push(Out::Effect { id: effects::MAR_DEAD_SMOKE, pos: mouth });
                    w.out.push(Out::Effect { id: effects::MAR_DEAD_JET, pos: mouth });
                }
            }
            i2.act(t, w)
        })
        .build()
}

fn watch_navi(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, w| {
            let nearest = w.navis.iter().filter(|n| n.alive).min_by(|a, b| dist_xz(a.pos, t.pos).total_cmp(&dist_xz(b.pos, t.pos)));
            t.targets[0] = nearest.map(|n| Target::Navi(n.id));
        })
        .on_act(|_, _| true)
        .build()
}

pub fn puffy_blowhog(info: &TekiInfo) -> Strategy {
    let dive = 27.0f32.to_radians();
    let (stop, damage) = (stop_move(), yai_damage(true));
    let (dead_ground, dead_air) = (dead(DEAD_GROUND), dead(DEAD_AIR));
    let flying = flying_base_mar();
    let sticking_fly = sticking(DESCENDING, PIKIS_TO_DESCEND);
    let flick_timer = flick_check_timer(AIR_FLICKING);

    let mut s = Strategy::new(15, INIT);
    s.set(DEAD_GROUND, vec![dying_mar(DEAD), stop.clone()]);
    s.set(DEAD_AIR, vec![dying_mar(TYPE3), stop.clone()]);
    s.set(INIT, vec![init(FLY)]);
    s.set(FLY, vec![
        dead_air.clone(),
        flying.clone(),
        flying_in_territory(0.025),
        sticking_fly.clone(),
        damage.clone(),
        visible_piki(APPROACH),
        visible_navi(APPROACH),
        flick_check(WATCH, 1),
        reserve_motion(NO_TRANSIT, MOVE1),
    ]);
    s.set(WATCH, vec![watch_navi(APPROACH)]);
    s.set(AIR_FLICKING, vec![dead_air.clone(), damage.clone(), flicking_upper_motion(FLY, DAMAGE), flying.clone()]);
    s.set(DESCENDING, vec![dead_air, descent(LANDING, TYPE2), damage.clone()]);
    s.set(LANDING, vec![landing(GROUNDED, MOVE2)]);
    s.set(GROUNDED, vec![
        dead_ground.clone(),
        damage.clone(),
        timer_reaction(TAKING_OFF, |t| t.f(TAKEOFF_TIME)),
        flick_check_param(GROUND_FLICK, HITS_BEFORE_AIR_SHAKE),
        reserve_motion(NO_TRANSIT, WAIT1),
    ]);
    s.set(GROUND_FLICK, vec![dead_ground.clone(), damage.clone(), flicking_upper_motion(AFTER_FLICK, FLICK)]);
    s.set(AFTER_FLICK, vec![sticking(GROUNDED, PIKIS_TO_TAKE_OFF), no_reaction(TAKING_OFF)]);
    s.set(TAKING_OFF, vec![take_off(FLY, TYPE1)]);
    s.set(APPROACH, vec![
        dead_ground.clone(),
        flying_distance(BREATHING, dive, 0.1),
        flying.clone(),
        sticking_fly.clone(),
        damage.clone(),
        flick_timer.clone(),
        outside_danger_territory(TO_TERRITORY),
    ]);
    s.set(BREATHING, vec![dead_ground.clone(), damage.clone(), stop, wind_breath(FLY, ATTACK), flying.clone()]);
    s.set(TO_TERRITORY, vec![
        dead_ground,
        flying,
        flying_distance_in_territory(FLY, MOVE1, dive, 0.15),
        sticking_fly,
        damage,
        flick_timer,
    ]);
    let _ = info;
    s
}
