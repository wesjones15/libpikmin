//! The Swooping Snitchbug: `TaiNapkidStrategy` (tainapkid.cpp). It hovers about its territory; seeing a Pikmin
//! it gives chase, dives, and snatches one in its mouth to carry about and drop from a height. Pikmin
//! clinging to its body weigh it down: it falls, flails on the ground and, once up, shakes them off.
//! The wander follows no route graph here (the stage has none): it drifts between random points of its territory.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin::PikiState;
use libpikmin_assets::teki::{tpf, tpi};

// TaiNapkidIntParms / TaiNapkidFloatParms
const STRUGGLE_LOOP_COUNT: usize = tpi::SPECIFIC;
const CARRYING_VELOCITY: usize = 50;
const CARRY_FLIGHT_HEIGHT: usize = 51;
const TAKING_OFF_FLIGHT_HEIGHT: usize = 52;
const WANDERING_HOVER_PERIOD: usize = 53;
const WANDERING_HOVER_PROBABILITY: usize = 54;
const HOVERING_WANDER_PERIOD: usize = 55;
const HOVERING_WANDER_PROBABILITY: usize = 56;
const HOVERING_WASH_PERIOD: usize = 57;
const HOVERING_WASH_PROBABILITY: usize = 58;
const WASHING_WANDER_PERIOD: usize = 59;
const WASHING_WANDER_PROBABILITY: usize = 60;
const THROW_PERIOD: usize = 61;
const THROW_VELOCITY: usize = 62;

// TaiNapkidStateID
const DYING: i32 = 0;
const WANDERING: i32 = 1;
const IDLE_FLYING: i32 = 2;
const IDLE_CHATTING: i32 = 3;
const CHASING: i32 = 4;
const OUTRUNNING: i32 = 5;
const EVADING: i32 = 6;
const ATTACKING_SETUP: i32 = 7;
const ATTACKING: i32 = 8;
const CATCHING: i32 = 9;
const ATTACK_DECIDING: i32 = 10;
const CARRYING_SETUP: i32 = 11;
const ATTACK_MISSING: i32 = 12;
const CARRYING: i32 = 13;
const THROWING: i32 = 14;
const SHOCK_FALLING: i32 = 15;
const LOOP_FALLING: i32 = 16;
const LANDING: i32 = 17;
const STRUGGLING: i32 = 18;
const GETTING_UP: i32 = 19;
const HEIGHT_DECIDING: i32 = 20;
const FLICKING: i32 = 21;
const RISING: i32 = 22;

/// Pikmin clinging to its body, not those in its mouth (`Stickers`).
fn body_stickers(t: &Teki, w: &World) -> usize {
    w.sim.pikis().iter().filter(|p| p.is_alive() && p.state.latched_to() == Some(t.creature)).count()
}

/// Pikmin in its mouth.
fn mouth_stickers(t: &Teki, w: &World) -> usize {
    w.sim.pikis().iter().filter(|p| matches!(p.state, PikiState::Swallowed { creature, .. } if creature == t.creature)).count()
}

fn has_stickers_on_body(next: i32) -> Act {
    Action::new(next).on_act(|t, w| body_stickers(t, w) > 0).build()
}

fn has_stickers_in_mouth(next: i32) -> Act {
    Action::new(next).on_act(|t, w| mouth_stickers(t, w) > 0).build()
}

/// `TekiNapkidTargetPikiCondition`: a Pikmin in sight that nothing has hold of.
fn target_ok(t: &Teki, c: &Cand, w: &World) -> bool {
    c.piki.is_some() && !c.sticking && t.visible(c)
}

/// `TaiNapkidTargetPikiAction`.
fn target_piki(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.closest(w, |c| c.piki.is_some() && target_ok(t, c, w)) {
            Some(c) => {
                t.targets[0] = Some(c.target);
                true
            }
            None => false,
        })
        .build()
}

/// `TaiNapkidPikiLostAction`.
fn piki_lost(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.target(0, w) {
            None => true,
            Some(c) => {
                if !target_ok(t, &c, w) {
                    t.targets[0] = None;
                    return true;
                }
                false
            }
        })
        .build()
}

/// `TaiNapkidShortRangeAction`: the Pikmin it chases is within 130.
fn short_range(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.target(0, w) {
            None => true,
            Some(c) => c.piki.is_some() && !c.sticking && dist_xz(t.pos, c.pos) < 130.0,
        })
        .build()
}

/// `TaiNapkidApproachPikiAction`: the target is in the sector ahead of it.
fn approach_piki(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let Some(c) = t.target(0, w) else { return false };
            dist_xz(t.pos, c.pos) <= t.f(tpf::ATTACKABLE_RANGE) && t.calc_target_angle(c.pos) < t.f(tpf::ATTACKABLE_ANGLE).to_radians() / 2.0
        })
        .build()
}

/// `TaiStartingTimerAction`: when its timer runs out it acts with chance `chance`, else starts over.
fn starting_timer(next: i32, idx: usize, period: usize, rate: f32, chance: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| t.timers[idx] = t.f(period) * w.rng.range(1.0 - rate, 1.0 + rate))
        .on_act(move |t, w| {
            if t.timer_elapsed(idx) {
                if w.rng.occurred(t.f(chance)) {
                    return true;
                }
                t.timers[idx] = t.f(period) * w.rng.range(1.0 - rate, 1.0 + rate);
            }
            false
        })
        .build()
}

/// A velocity and acceleration to fly with (`PeveAccelerationEvent` made and reset).
fn start_accel(t: &mut Teki, vel: Vec3, accel: Vec3) {
    t.vel3 = vel;
    t.accel3 = accel;
    t.target_vel = vel;
}

/// `TaiAccelerationAction` + `TaiMakingNextVelocityAction`: velocity grows by the acceleration and carries it.
fn acceleration() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            t.vel3 += t.accel3 * w.dt;
            t.target_vel = t.vel3;
            false
        })
        .build()
}

/// `TaiParabolaAction`: falling under gravity.
fn parabola() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            t.vel3.y -= t.gravity * w.dt;
            t.target_vel = t.vel3;
            false
        })
        .build()
}

/// `TaiNapkidFlyingAction`: ease toward the hovering height.
fn flying_height(height: impl Fn(&Teki) -> f32 + Send + Sync + 'static) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, _| {
            let ground = t.ground_y.unwrap_or(t.pos.y - t.height());
            t.pos.y += 0.15 * (height(t) + ground - t.pos.y);
            false
        })
        .build()
}

fn clamp_min_height(next: i32, min: f32) -> Act {
    Action::new(next)
        .on_act(move |t, _| {
            let ground = t.ground_y.unwrap_or(0.0);
            if t.pos.y < ground + min {
                t.pos.y = ground + min;
                return true;
            }
            false
        })
        .build()
}

fn clamp_max_height(next: i32, max: impl Fn(&Teki) -> f32 + Send + Sync + 'static) -> Act {
    Action::new(next)
        .on_act(move |t, _| {
            let ground = t.ground_y.unwrap_or(0.0);
            let m = max(t);
            if t.pos.y > ground + m {
                t.pos.y = ground + m;
                return true;
            }
            false
        })
        .build()
}

fn clamp_min_velocity_y(next: i32, min: f32) -> Act {
    Action::new(next)
        .on_act(move |t, _| {
            if t.vel3.y < min {
                t.vel3.y = min;
                return true;
            }
            false
        })
        .build()
}

/// `TaiMakeVelocityDirectionAction`: face the way it flies.
fn face_velocity_action() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, _| {
            let v = t.vel3.with_y(0.0);
            if v.length() > 0.0 {
                t.yaw = v.x.atan2(v.z);
            }
            false
        })
        .build()
}

/// `TaiRotatingAction`: spin about its axis.
fn rotating(speed: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, w| {
            t.rotate(speed, w.dt);
            false
        })
        .build()
}

/// Creatures turn to face where they go unless a strategy says otherwise (`mFaceDirAdjust`).
fn auto_face() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, _| {
            face_velocity(t, 0.25);
            false
        })
        .build()
}

fn flying(on: bool) -> Act {
    Action::new(NO_TRANSIT).on_start(move |t, _| if on { t.start_flying() } else { t.finish_flying() }).build()
}

/// `TaiNapkidWanderingRouteAction`: drift between random points of the territory at `speed`.
fn wander_route(m: usize, speed: impl Fn(&Teki) -> f32 + Send + Sync + 'static) -> Act {
    fn pick(t: &mut Teki, w: &mut World) {
        let a = w.rng.range(0.0, TAU);
        let d = w.rng.range(0.0, t.f(tpf::DANGER_TERRITORY_RANGE));
        t.wander = t.nest + Vec3::new(d * a.sin(), 0.0, d * a.cos());
    }
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            reserve_start(t, m);
            pick(t, w);
        })
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let s = speed(t);
            let goal = t.wander;
            if t.move_toward(goal, s, w.dt) {
                pick(t, w);
            }
            face_velocity(t, 0.2);
            false
        })
        .build()
}

/// `TaiNapkidStraightFlyingAction`: run straight on until the Pikmin is `min` away.
fn straight_flying(next: i32, min: f32) -> Act {
    Action::new(next)
        .on_start(|t, _| t.target_vel = t.forward() * 150.0)
        .on_act(move |t, w| {
            t.target_vel = t.forward() * 150.0;
            match t.target(0, w) {
                None => true,
                Some(c) => dist_xz(c.pos, t.pos) >= min,
            }
        })
        .build()
}

/// `TaiNapkidCirclingAction`: a half circle of radius 50 at 150 a second, curling toward the Pikmin's side.
fn circling(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, w| {
            let sep = t.target(0, w).map_or(Vec3::ZERO, |c| (c.pos - t.pos).with_y(0.0));
            let cross = Vec3::Y.cross(t.forward());
            t.reg[0] = if cross.dot(sep) < 0.0 { -1.0 } else { 1.0 };
            t.reg[1] = 50.0 * PI / 150.0;
        })
        .on_act(|t, w| {
            let omega = PI / (50.0 * PI / 150.0);
            t.yaw += t.reg[0] * omega * w.dt;
            t.target_vel = t.forward() * 150.0;
            t.reg[1] -= w.dt;
            t.reg[1] <= 0.0
        })
        .build()
}

/// `TaiNapkidCatchDescendingAction`: drop at the Pikmin, nose first.
fn catch_descending() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| {
            let v = t.forward() * 160.0 + Vec3::Y * -240.0;
            start_accel(t, v, Vec3::new(0.0, -320.0, 0.0));
        })
        .build()
}

/// `TaiNapkidCatchTracingAction`: steer sideways toward the Pikmin while it falls on it.
fn catch_tracing() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            let Some(c) = t.target(0, w) else { return false };
            let sep = (c.pos - t.pos).with_y(0.0).normalize_or_zero();
            let cross = sep.cross(t.forward());
            let side = t.forward().cross(Vec3::Y).normalize_or_zero() * 300.0;
            if cross.y.abs() < 0.1 {
                t.accel3.x = 0.0;
                t.accel3.z = 0.0;
            } else if cross.y > 0.0 {
                t.accel3.x = side.x;
                t.accel3.z = side.z;
            } else {
                t.accel3.x = -side.x;
                t.accel3.z = -side.z;
            }
            false
        })
        .build()
}

/// `TaiNapkidCatchingAction`: between the attack's keys, the mouth grabs a Pikmin; after them it has decided.
fn catching(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let (counter, a, b) = (t.anim.counter, t.key_value(2), t.key_value(3));
            if counter < a {
                return false;
            }
            if counter > b {
                return true;
            }
            let at = t.pos + t.forward() * t.f(tpf::ATTACK_RANGE);
            let range = t.f(tpf::ATTACK_HIT_RANGE);
            if mouth_stickers(t, w) == 0 {
                w.sim.swallow_pikis(t.creature, 0, at, range, 1);
            }
            false
        })
        .build()
}

/// `TaiNapkid{Catch,Rising}AscendingAction`, `CatchFlyingAction`: a velocity and an acceleration to fly with.
fn set_flight(vel: impl Fn(&Teki) -> Vec3 + Send + Sync + 'static, accel: Vec3) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, _| {
            let v = vel(t);
            start_accel(t, v, accel);
        })
        .build()
}

/// `TaiNapkidTakingOffAscendingAction`: a leap whose height shrinks with the Pikmin on its back.
fn taking_off_ascending() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            let n = body_stickers(t, w) as f32;
            let k = 1.0 + (0.7 - 1.0) * (n / 10.0).clamp(0.0, 1.0);
            let v = t.forward() * 30.0 + Vec3::Y * (k * 270.0);
            start_accel(t, v, Vec3::new(0.0, -300.0, 0.0));
        })
        .build()
}

/// `TaiNapkidThrowingPikiAction`: at the action key it lets go of a Pikmin, which drops.
fn throwing_piki() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                let ids: Vec<_> = w
                    .sim
                    .pikis()
                    .iter()
                    .filter(|p| matches!(p.state, PikiState::Swallowed { creature, .. } if creature == t.creature))
                    .map(|p| p.id)
                    .collect();
                if let Some(first) = ids.first().copied() {
                    w.sim.spit_out(t.creature);
                    // Only one is thrown at a time: the others are taken back into the mouth.
                    let (down, v) = (t.f(THROW_VELOCITY), t.target_vel);
                    w.sim.piki_mut(first).vel = Vec3::new(v.x, -down, v.z);
                    if ids.len() > 1 {
                        let mouth = t.part("slot").map_or(t.pos, |(p, _)| p);
                        w.sim.swallow_pikis(t.creature, 0, mouth, 40.0, (ids.len() - 1) as u32);
                    }
                }
            }
            false
        })
        .build()
}

/// `TaiNapkidFlickAction`: with Pikmin on its back it tries to shake them off; more of them make it likelier.
fn napkid_flick(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let n = body_stickers(t, w) as f32;
            let chance = 0.1 + (0.7 - 0.1) * ((n - 1.0) / 4.0).clamp(0.0, 1.0);
            // The decomp returns false when the roll succeeds (the shake fails).
            !w.rng.occurred(chance)
        })
        .build()
}

/// `TaiNapkidFallingAction`: it lets go of what it carried and drops.
fn falling() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            start_accel(t, Vec3::new(0.0, -20.0, 0.0), Vec3::ZERO);
            w.sim.spit_out(t.creature);
            t.held_piki = false;
        })
        .build()
}

fn shock_falling() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.pos.y += 10.0).build()
}

/// `TaiNapkidFallingWaterEffectAction`.
fn falling_water_effect() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            let at = Vec3::new(t.pos.x, t.ground_y.unwrap_or(t.pos.y), t.pos.z);
            if t.in_water {
                w.out.push(Out::Effect { id: effects::FROG_BUBBLE_RING_S, pos: at });
                w.out.push(Out::Sound { name: "SE_FLOG_WATERLAND", pos: at });
            } else {
                w.out.push(Out::Effect { id: effects::SMOKE_RING_S, pos: at });
                w.out.push(Out::Sound { name: "SE_FLOG_LAND", pos: at });
            }
        })
        .build()
}

/// `TaiNapkidStartDroppingWaterAction`: water streams off it if it got up out of water.
fn start_dropping_water() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            if t.in_water {
                w.out.push(Out::Effect { id: effects::FROG_WATER_2, pos: t.pos });
            }
        })
        .build()
}

/// `TaiCountLoopAction`: true after `param` passes of the motion's loop.
fn count_loop(param: usize) -> Act {
    Action::new(NO_TRANSIT)
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

/// `TaiFlickingUpperAction`: the shake, flinging off only what is on its back.
fn flicking_upper(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_finish(|t, _| t.set_option(opt::DAMAGE_COUNTABLE))
        .on_act(|t, w| {
            if t.animation_finished() {
                return true;
            }
            if t.key(key::ACTION0) {
                t.flick_upper(w);
            }
            false
        })
        .build()
}

/// A timer on one of its own parameters (`TaiTimerAction`).
fn timer_param(next: i32, idx: usize, param: usize, rate: f32) -> Act {
    Action::new(next)
        .on_start(move |t, w| t.timers[idx] = t.f(param) * w.rng.range(1.0 - rate, 1.0 + rate))
        .on_act(move |t, _| t.timer_elapsed(idx))
        .build()
}

fn no_stickers_on_body(next: i32) -> Act {
    Action::new(next).on_act(|t, w| body_stickers(t, w) == 0).build()
}

fn no_stickers_in_mouth(next: i32) -> Act {
    Action::new(next).on_act(|t, w| mouth_stickers(t, w) == 0).build()
}

pub fn snitchbug(info: &TekiInfo) -> Strategy {
    let (stop, sim_damage) = (stop_move(), simultaneous_damage(NO_TRANSIT));
    let stickers_fall = has_stickers_on_body(SHOCK_FALLING);
    let midair_dying = dead(SHOCK_FALLING);
    let grounded_dying = dead(DYING);
    let target_chase = target_piki(CHASING);
    let hover = flying_height(|t| t.f(tpf::FLIGHT_HEIGHT));
    let carry_hover = flying_height(|t| t.f(CARRY_FLIGHT_HEIGHT));
    let accel = acceleration();
    let catch_min = clamp_min_height(NO_TRANSIT, 10.0);
    let catch_ascending = || set_flight(|t| t.vel3.with_y(0.0), Vec3::new(0.0, 200.0, 0.0));
    let face = face_velocity_action();

    let mut s = Strategy::new(23, WANDERING);
    s.set(DYING, vec![stop.clone(), start_dying(), dying(DEAD)]);
    s.set(WANDERING, vec![
        flying(true),
        sim_damage.clone(),
        stickers_fall.clone(),
        midair_dying.clone(),
        target_chase.clone(),
        hover.clone(),
        starting_timer(IDLE_FLYING, 0, WANDERING_HOVER_PERIOD, 0.5, WANDERING_HOVER_PROBABILITY),
        wander_route(MOVE1, |t| t.f(tpf::WALK_VELOCITY)),
    ]);
    s.set(IDLE_FLYING, vec![
        stop,
        sim_damage.clone(),
        stickers_fall.clone(),
        midair_dying.clone(),
        target_chase.clone(),
        hover.clone(),
        starting_timer(WANDERING, 0, HOVERING_WANDER_PERIOD, 0.5, HOVERING_WANDER_PROBABILITY),
        starting_timer(IDLE_CHATTING, 1, HOVERING_WASH_PERIOD, 0.5, HOVERING_WASH_PROBABILITY),
        continuous_motion(NO_TRANSIT, MOVE1),
    ]);
    s.set(IDLE_CHATTING, vec![
        sim_damage.clone(),
        stickers_fall.clone(),
        midair_dying.clone(),
        target_chase.clone(),
        hover.clone(),
        starting_timer(WANDERING, 0, WASHING_WANDER_PERIOD, 0.5, WASHING_WANDER_PROBABILITY),
        continuous_motion(NO_TRANSIT, WAIT1),
    ]);
    let approach = approach_piki(ATTACKING_SETUP);
    s.set(CHASING, vec![
        sim_damage.clone(),
        stickers_fall.clone(),
        midair_dying.clone(),
        approach.clone(),
        target_piki(NO_TRANSIT),
        short_range(OUTRUNNING),
        hover.clone(),
        piki_lost(WANDERING),
        tracing(MOVE1, info.floats[tpf::RUN_VELOCITY]),
        auto_face(),
    ]);
    s.set(OUTRUNNING, vec![
        sim_damage.clone(),
        stickers_fall.clone(),
        midair_dying.clone(),
        approach,
        hover.clone(),
        straight_flying(EVADING, 160.0),
        timer(EVADING, 0, 5.0, 0.0),
        continuous_motion(NO_TRANSIT, MOVE1),
    ]);
    s.set(EVADING, vec![sim_damage.clone(), stickers_fall.clone(), midair_dying.clone(), circling(WANDERING)]);
    s.set(ATTACKING_SETUP, vec![sim_damage.clone(), stickers_fall.clone(), midair_dying.clone(), hover, finish_motion(ATTACKING)]);
    s.set(ATTACKING, vec![
        sim_damage.clone(),
        catch_descending(),
        catch_tracing(),
        accel.clone(),
        clamp_min_height(CATCHING, 10.0),
        motion(NO_TRANSIT, ATTACK),
        catching(ATTACK_DECIDING),
        face.clone(),
    ]);
    s.set(CATCHING, vec![
        sim_damage.clone(),
        set_flight(|t| t.vel3.with_y(0.0), Vec3::ZERO),
        catch_tracing(),
        accel.clone(),
        catch_min.clone(),
        catching(ATTACK_DECIDING),
        face,
    ]);
    s.set(ATTACK_DECIDING, vec![accel.clone(), catch_min.clone(), has_stickers_in_mouth(CARRYING_SETUP), once(ATTACK_MISSING)]);
    s.set(ATTACK_MISSING, vec![
        sim_damage.clone(),
        accel.clone(),
        catch_min.clone(),
        serial(GETTING_UP, vec![switch_motion(NO_TRANSIT, TYPE4), motion(NO_TRANSIT, MOVE1)]),
        catch_ascending(),
        clamp_max_height(WANDERING, |t| t.f(tpf::FLIGHT_HEIGHT)),
    ]);
    s.set(CARRYING_SETUP, vec![
        sim_damage.clone(),
        continuous_motion(NO_TRANSIT, WAITACT2),
        catch_ascending(),
        accel.clone(),
        catch_min,
        clamp_max_height(CARRYING, |t| t.f(CARRY_FLIGHT_HEIGHT)),
    ]);
    s.set(CARRYING, vec![
        sim_damage.clone(),
        stickers_fall.clone(),
        midair_dying.clone(),
        timer_param(THROWING, 0, THROW_PERIOD, 0.5),
        wander_route(WAITACT2, |t| t.f(CARRYING_VELOCITY)),
        carry_hover,
    ]);
    s.set(THROWING, vec![
        stickers_fall,
        midair_dying,
        continuous_motion(CARRYING, WAITACT1),
        throwing_piki(),
        no_stickers_in_mouth(WANDERING),
    ]);
    let fall_motion = motion(NO_TRANSIT, TYPE1);
    let to_landing = clamp_min_height(LANDING, 10.0);
    s.set(SHOCK_FALLING, vec![
        sim_damage.clone(),
        shock_falling(),
        fall_motion.clone(),
        falling(),
        rotating(PI * 8.0),
        parabola(),
        to_landing.clone(),
    ]);
    s.set(LOOP_FALLING, vec![sim_damage.clone(), fall_motion, falling(), rotating(PI * 4.0), parabola(), to_landing]);
    s.set(LANDING, vec![
        stop_move(),
        flying(false),
        sim_damage.clone(),
        finish_motion(STRUGGLING),
        falling_water_effect(),
    ]);
    s.set(STRUGGLING, vec![
        grounded_dying,
        sim_damage.clone(),
        serial(GETTING_UP, vec![count_loop(STRUGGLE_LOOP_COUNT), finish_motion(NO_TRANSIT)]),
        continuous_motion(NO_TRANSIT, TYPE2),
    ]);
    s.set(GETTING_UP, vec![
        flying(true),
        sim_damage.clone(),
        taking_off_ascending(),
        accel.clone(),
        clamp_min_velocity_y(HEIGHT_DECIDING, 0.0),
        clamp_max_height(HEIGHT_DECIDING, |t| t.f(TAKING_OFF_FLIGHT_HEIGHT)),
        continuous_motion(NO_TRANSIT, TYPE3),
        start_dropping_water(),
    ]);
    s.set(HEIGHT_DECIDING, vec![no_stickers_on_body(RISING), napkid_flick(FLICKING), once(LOOP_FALLING)]);
    s.set(FLICKING, vec![sim_damage.clone(), flicking_upper(RISING, FLICK)]);
    s.set(RISING, vec![
        sim_damage,
        has_stickers_on_body(SHOCK_FALLING),
        dead(SHOCK_FALLING),
        set_flight(|t| t.forward() * 30.0, Vec3::new(0.0, 200.0, 0.0)),
        accel,
        clamp_max_height(WANDERING, |t| t.f(tpf::FLIGHT_HEIGHT)),
        continuous_motion(NO_TRANSIT, MOVE1),
    ]);
    s
}
