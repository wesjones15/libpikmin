//! The Honeywisp: `TaiMizinkoStrategy` (taimizinko.cpp). It flies a bobbing path from its nest out to a
//! point, hides, flies back along a path beside it, hides again, and so on. Hit by a Pikmin (or hurt) it
//! drops a drop of nectar, then flies straight up and is gone.

use std::f32::consts::TAU;

use glam::Vec3;

use crate::bulborb::{DAMAGE, DEAD, WAIT1};
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use libpikmin_assets::teki::tpf;

// MIZINKOPF_*
const DROP_DISTANCE: usize = 50;
const PATH_DISTANCE: usize = 51;
const AWAY_VELOCITY: usize = 52;
const AWAY_ACCEL: usize = 53;
const FADE_PERIOD: usize = 54;
const HIDING_PERIOD: usize = 55;
const HIDING_RANDOM: usize = 56;
const HEIGHT_RANDOM: usize = 57;
const AMPLITUDE: usize = 58;
const AMPLITUDE_RANDOM: usize = 59;
const ANGULAR_VELOCITY: usize = 60;
const ANGULAR_RANDOM: usize = 61;
const CRY_PERIOD: usize = 62;
const CRY_RANDOM: usize = 63;

fn rate_random(w: &mut World, x: f32, rate: f32) -> f32 {
    x + x * rate * w.rng.range(-1.0, 1.0)
}

fn visible(t: &mut Teki, on: bool) {
    let bits = opt::VISIBLE | opt::ATARI | opt::SHADOW_VISIBLE;
    if on { t.set_option(bits) } else { t.clear_option(bits) }
}

/// `makeHorizontalSinWaveEvent` + `reset` + one `update`, then the enemy is put there.
fn start_wave(t: &mut Teki, w: &mut World, from: Vec3, dir: Vec3, height_base: f32) {
    let offset = rate_random(w, t.f(tpf::FLIGHT_HEIGHT) + height_base, t.f(HEIGHT_RANDOM));
    let amp = rate_random(w, t.f(AMPLITUDE), t.f(AMPLITUDE_RANDOM));
    let theta = w.rng.range(0.0, TAU);
    let omega = rate_random(w, t.f(ANGULAR_VELOCITY), t.f(ANGULAR_RANDOM));
    t.wave_pos = from;
    t.wave = Wave { on: true, dir, offset, amp, theta, omega };
    step_wave(t, w.dt);
    t.pos = t.wave_pos;
}

/// `PeveHorizontalSinWaveEvent::update`.
fn step_wave(t: &mut Teki, dt: f32) {
    if !t.wave.on {
        return;
    }
    let y = t.wave.amp * t.wave.theta.sin() + t.wave.offset;
    t.wave.theta += dt * t.wave.omega;
    t.wave_pos += t.wave.dir * dt;
    t.wave_pos.y = y;
}

/// `TaiMizinkoMovingTimerAction`: the time to cross the territory at the walking speed.
fn moving_timer(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| t.timers[0] = t.territory / t.f(tpf::WALK_VELOCITY).max(1e-3))
        .on_act(|t, _| t.timer_elapsed(0))
        .build()
}

/// `TaiMizinkoFadingAction`: solid mid-flight, fading in and out at the ends.
fn fading() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, _| {
            let travel = t.territory / t.f(tpf::WALK_VELOCITY).max(1e-3);
            let left = t.timers[0];
            let fade = t.f(FADE_PERIOD).max(1e-3);
            t.fade = if travel - left < fade {
                (travel - left) / fade
            } else if left < fade {
                left / fade
            } else {
                1.0
            };
            false
        })
        .build()
}

fn going() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.stop_move();
            visible(t, true);
            let (nest, target) = (t.nest, t.target_pos);
            let mut dir = (target - nest).with_y(0.0);
            if dir.length() < 1e-4 {
                dir = t.forward();
            } else {
                dir = dir.normalize();
            }
            t.yaw = dir.x.atan2(dir.z);
            let v = dir * t.f(tpf::WALK_VELOCITY);
            start_wave(t, w, nest, v, nest.y);
        })
        .build()
}

fn coming() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.stop_move();
            visible(t, true);
            let (mut target, mut nest) = (t.target_pos, t.nest);
            let dir = nest - target;
            let mut ortho = Vec3::Y.cross(dir).with_y(0.0);
            ortho = if ortho.length() < 1e-4 { Vec3::X } else { ortho.normalize() };
            ortho *= t.f(PATH_DISTANCE);
            target += ortho;
            nest += ortho;
            let mut dir2 = (nest - target).with_y(0.0);
            dir2 = if dir2.length() < 1e-4 { Vec3::X } else { dir2.normalize() };
            t.yaw = dir2.x.atan2(dir2.z);
            let v = dir2 * t.f(tpf::WALK_VELOCITY);
            start_wave(t, w, target, v, target.y);
        })
        .build()
}

fn waiting() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| {
            t.stop_move();
            visible(t, false);
        })
        .build()
}

/// `TaiHorizontalSinWaveAction` + `TaiMakingNextVelocityAction`: fly the path.
fn fly_wave() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            step_wave(t, w.dt);
            t.pos = t.wave_pos;
            false
        })
        .build()
}

fn cry_timer() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            let (p, r) = (t.f(CRY_PERIOD), t.f(CRY_RANDOM));
            t.timers[1] = rate_random(w, p, r);
        })
        .on_act(|t, w| {
            if t.timer_elapsed(1) {
                w.out.push(Out::Sound { name: "SE_KURIONE_FLYING", pos: t.pos });
                let (p, r) = (t.f(CRY_PERIOD), t.f(CRY_RANDOM));
                t.timers[1] = rate_random(w, p, r);
            }
            false
        })
        .build()
}

fn piki_collision(next: i32) -> Act {
    Action::new(next)
        .on_event(|t, w, e| {
            let Event::Entity { other: Target::Piki(_), .. } = e else { return false };
            let _ = (t, w);
            true
        })
        .build()
}

/// `TaiMizinkoDropWaterAction`: at the motion action key a drop of nectar falls.
fn drop_water() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.key(crate::anim::key::ACTION0) {
                let at = t.pos - Vec3::Y * t.f(DROP_DISTANCE);
                w.sim.spawn_nectar(at);
                t.count[0] += 1;
            }
            false
        })
        .build()
}

/// `TaiMizinkoFlyingAwayAction`: straight up, faster and faster.
fn flying_away() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| {
            t.flying = true;
            t.stop_move();
            visible_atari_off(t);
            t.wave.on = false;
            t.wave_pos = t.pos;
            t.vel3 = Vec3::new(0.0, t.f(AWAY_VELOCITY), 0.0);
            t.accel3 = Vec3::new(0.0, t.f(AWAY_ACCEL), 0.0);
            t.accel_on = true;
        })
        .build()
}

fn visible_atari_off(t: &mut Teki) {
    t.clear_option(opt::VISIBLE | opt::ATARI);
}

/// `TaiAccelerationAction`: `PeveAccelerationEvent::update`.
fn accelerate() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.accel_on {
                t.wave_pos += t.vel3 * w.dt;
                t.vel3 += t.accel3 * w.dt;
                t.pos = t.wave_pos;
            }
            false
        })
        .build()
}

/// `TaiMizinkoStrategy`.
pub fn honeywisp(info: &TekiInfo) -> Strategy {
    let hiding = info.floats[HIDING_PERIOD];
    let hiding_rand = info.floats[HIDING_RANDOM];
    let life_period = info.floats[tpf::LIFE_PERIOD];

    let pikis = piki_collision(4);
    let hurt = damage(4);
    let bob = motion(NO_TRANSIT, WAIT1);
    let wave = fly_wave();
    let fade = fading();
    let cry = cry_timer();
    let wait = waiting();

    let mut s = Strategy::new(7, 0);
    s.set(0, vec![pikis.clone(), hurt.clone(), bob.clone(), wave.clone(), moving_timer(1), going(), fade.clone(), cry.clone()]);
    s.set(1, vec![timer(2, 0, hiding, hiding_rand), wait.clone()]);
    s.set(2, vec![pikis, hurt, bob, wave, moving_timer(3), coming(), fade, cry]);
    s.set(3, vec![timer(0, 0, hiding, hiding_rand), wait]);
    s.set(4, vec![damaging(5, DAMAGE), drop_water(), Action::new(NO_TRANSIT).build()]);
    s.set(5, vec![timer(6, 0, life_period, 0.0), motion(NO_TRANSIT, DEAD), accelerate(), flying_away()]);
    s.set(6, vec![dye()]);
    s.on_start = Some(std::sync::Arc::new(|t| t.flying = true));
    s
}
