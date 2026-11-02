//! The actions Yamashita's enemies share (`TAIA*`, plugPikiYamashita/TAIA*.cpp): reserved motions that wait for the
//! current one to play out, waiting, patrolling, turning to face a target, stepping back, breathing fire,
//! flicking, random walks. Built like `tai.rs`'s, from closures.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::anim::key;
use crate::object::*;
use crate::tai::*;
use libpikmin_assets::teki::tpf;

/// `TAIAreserveMotion::start`: switch to `m` once the motion playing is nearly done, or at once if none.
pub fn reserve_start(t: &mut Teki, m: usize) {
    let total = |t: &Teki, i: usize| t.info.motion_data.get(i).map_or(0.0, |d| d.frames);
    let cur = t.current_motion();
    if cur != m as i32 {
        if cur < 0 {
            t.start_motion(m);
        } else if t.anim.counter >= total(t, cur as usize) - 2.0 {
            t.start_motion(m);
        } else {
            t.finish_motion();
        }
        return;
    }
    if t.anim.counter >= total(t, m) - 2.0 {
        t.start_motion(m);
    }
    if t.anim.is_finished() {
        let f = t.anim.counter;
        t.start_motion(m);
        t.anim.counter = f;
    }
}

/// `TAIAreserveMotion::act`: true once the motion wanted is the one playing.
pub fn reserve_act(t: &mut Teki, m: usize) -> bool {
    if t.current_motion() != m as i32 {
        if t.anim.is_finishing() {
            t.start_motion(m);
            return true;
        }
        return false;
    }
    true
}

pub fn reserve_motion(next: i32, m: usize) -> Act {
    Action::new(next).on_start(move |t, _| reserve_start(t, m)).on_act(move |t, _| reserve_act(t, m)).build()
}

/// `TAIAmotion`: start a motion; true when it has finished.
pub fn yai_motion(next: i32, m: usize) -> Act {
    Action::new(next).on_start(move |t, _| t.start_motion(m)).on_act(|t, _| t.key(key::FINISHED)).build()
}

/// `TAIAwait`: stand in a motion for `secs`.
pub fn wait(next: i32, m: usize, secs: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.stop_move();
            t.counter = 0.0;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            t.counter += w.dt;
            if t.counter > secs {
                t.counter = 0.0;
                return true;
            }
            false
        })
        .build()
}

/// `TAIAdying`: the death motion with its effects; `die` when it finishes.
pub fn yai_dying(m: usize) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            t.start_motion(m);
            t.clear_option(opt::ALIVE);
            t.hp = 0.0;
            t.create_dead_effect(w);
            w.out.push(Out::Dying { teki: t.id });
            t.clear_option(opt::SHADOW_VISIBLE);
        })
        .on_act(|t, w| {
            if t.animation_finished() {
                t.die(w);
                true
            } else {
                false
            }
        })
        .build()
}

/// `TAIAdamage`: damage waiting is taken, with a jiggle if `jiggle`.
pub fn yai_damage(jiggle: bool) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, _| {
            if t.stored_damage <= 0.0 {
                return false;
            }
            t.make_damaged();
            if jiggle {
                let p = t.f(tpf::DAMAGE_MOTION_PERIOD);
                t.start_damage_motion(p);
            }
            true
        })
        .build()
}

/// `TAIAvisiblePiki` / `TAIAvisibleNavi`: something in sight becomes the target.
pub fn visible_piki(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.closest(w, |c| c.piki.is_some() && !c.sticks_to_me && t.visible(c)) {
            Some(c) => {
                t.targets[0] = Some(c.target);
                true
            }
            None => false,
        })
        .build()
}

pub fn visible_navi(next: i32) -> Act {
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

/// `TAIAattackableAngleTarget`: the target is within the attackable angle (and the distance, if `distance`).
pub fn attackable_target(next: i32, distance: bool) -> Act {
    Action::new(next)
        .on_act(move |t, w| {
            let Some(c) = t.target(0, w) else { return true };
            t.target_pos = c.pos;
            let near = !distance || t.target_pos.distance(t.pos) < t.f(tpf::ATTACKABLE_RANGE);
            near && t.calc_target_angle(c.pos) < t.f(tpf::ATTACKABLE_ANGLE).to_radians() / 2.0
        })
        .build()
}

/// `TAIAunvisibleTarget`: no target, or one out of sight.
pub fn unvisible_target(next: i32) -> Act {
    Action::new(next).on_act(|t, w| t.target(0, w).is_none_or(|c| !t.visible(&c))).build()
}

/// `TAIAoutsideOptionalRange`: the target is further than `range`.
pub fn outside_range(next: i32, range_param: usize) -> Act {
    Action::new(next)
        .on_act(move |t, w| match t.target(0, w) {
            Some(c) => {
                t.target_pos = c.pos;
                t.target_pos.distance(t.pos) > t.f(range_param)
            }
            None => true,
        })
        .build()
}

/// `TAIAoutsideTerritory`.
pub fn outside_danger_territory(next: i32) -> Act {
    Action::new(next).on_act(|t, _| t.territory_distance() > t.f(tpf::DANGER_TERRITORY_RANGE)).build()
}

/// `TAIAflickCheck`: enough hits for the Pikmin on it (`limit` < 0), or `limit` hits.
pub fn flick_check(next: i32, limit: i32) -> Act {
    Action::new(next)
        .on_act(move |t, w| {
            let hit = if limit < 0 {
                let range = t.f(tpf::LOWER_RANGE);
                let n = t.count_pikis_in_range(w, range);
                t.damage_count >= t.flick_damage_count(n) as f32
            } else {
                t.damage_count >= limit as f32
            };
            if hit {
                t.damage_count = 0.0;
            }
            hit
        })
        .build()
}

/// `TAIAflicking`: the shake motion; at its action key everything on it and under it is flung away.
pub fn yai_flicking(next: i32, m: usize, backwards: bool) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.stop_move();
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_act(move |t, w| {
            if t.key(key::FINISHED) {
                t.set_option(opt::DAMAGE_COUNTABLE);
                t.damage_count = 0.0;
                return true;
            }
            if t.key(key::ACTION0) {
                let _ = backwards;
                t.flick(w);
            }
            false
        })
        .build()
}

/// `TAIAnoReaction`.
pub fn no_reaction(next: i32) -> Act {
    once(next)
}

/// `TAIAturnToTarget`: turn on the spot to face `target_pos`, playing a left or right turn motion.
pub fn turn_to_target(next: i32, left: usize, right: usize, finish_turn_motion: bool) -> Act {
    fn motion_for(t: &Teki, left: usize, right: usize) -> usize {
        let face = t.direction();
        if nearer_direction(face, t.target_angle) - face > 0.0 { left } else { right }
    }
    Action::new(next)
        .on_start(move |t, _| {
            t.target_angle = t.calc_target_direction(t.target_pos);
            let m = motion_for(t, left, right);
            reserve_start(t, m);
        })
        .on_act(move |t, w| {
            let m = motion_for(t, left, right);
            if t.current_motion() != m as i32 || t.anim.is_finished() {
                if !t.anim.is_finished() {
                    t.finish_motion();
                }
                if t.anim.is_finishing() {
                    t.start_motion(m);
                }
                return false;
            }
            t.stop_move();
            t.target_angle = t.calc_target_direction(t.target_pos);
            let (a, s) = (t.target_angle, t.f(tpf::TURN_VELOCITY));
            if t.turn_toward(a, s, w.dt) {
                if finish_turn_motion {
                    t.finish_motion();
                }
                return true;
            }
            false
        })
        .build()
}

/// `TAIAturnFocusCreature`: face the creature it has targeted.
pub fn turn_focus(next: i32, left: usize, right: usize, finish_turn_motion: bool) -> Act {
    let inner = turn_to_target(NO_TRANSIT, left, right, finish_turn_motion);
    let (i1, i2) = (inner.clone(), inner);
    Action::new(next)
        .on_start(move |t, w| {
            t.target_angle = t.target(0, w).map_or(t.yaw, |c| t.calc_target_direction(c.pos));
            t.stop_move();
            let face = t.direction();
            let m = if nearer_direction(face, t.target_angle) - face > 0.0 { left } else { right };
            reserve_start(t, m);
            let _ = &i1;
        })
        .on_act(move |t, w| match t.target(0, w) {
            Some(c) => {
                t.target_pos = c.pos;
                i2.act(t, w)
            }
            None => true,
        })
        .build()
}

/// `TAIAstepBack`: backs away from the target, still facing it.
pub fn step_back(next: i32, m: usize, speed_param: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let speed = t.f(speed_param);
            let Some(c) = t.target(0, w) else { return true };
            t.target_angle = t.calc_target_direction(c.pos);
            let diff = nearer_direction(t.direction(), t.target_angle) - t.direction();
            t.yaw += diff * t.f(tpf::TURN_VELOCITY) * w.dt * 10.0;
            t.target_vel = -t.forward() * speed;
            false
        })
        .build()
}

/// `TAIArandomWalk`: ambles to a random point near where it is.
pub fn random_walk(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            let a = w.rng.range(0.0, TAU);
            let d = w.rng.range(0.0, 200.0);
            t.target_pos = t.pos + Vec3::new(d * a.sin(), 0.0, d * a.cos());
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let goal = t.target_pos;
            let walk = t.f(tpf::WALK_VELOCITY);
            if goal.distance(t.pos) <= walk {
                return true;
            }
            t.move_toward_face_priority(goal, walk, w.dt);
            false
        })
        .build()
}

/// `TAIAgoingHomePriorityFaceDir`.
pub fn going_home_face(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.nest.distance(t.pos) <= t.f(tpf::SAFETY_TERRITORY_RANGE) {
                return true;
            }
            let (nest, run) = (t.nest, t.f(tpf::RUN_VELOCITY));
            t.move_toward_face_priority(nest, run, w.dt);
            false
        })
        .build()
}

/// `TAIApatrol`: turn to each point of a loop around the nest in turn and walk to it.
pub fn patrol(m: usize, left: usize, right: usize, offsets: Vec<Vec3>, timeout_param: usize) -> Act {
    let count = offsets.len();
    let (o1, o2) = (offsets.clone(), offsets);
    let point = move |t: &mut Teki, offs: &[Vec3]| {
        let o = offs[(t.index as usize).min(offs.len() - 1)];
        let r = t.f(tpf::DANGER_TERRITORY_RANGE);
        t.target_pos = t.nest + o * r;
        t.counter = 0.0;
    };
    let turn = turn_to_target(NO_TRANSIT, left, right, true);
    let (turn1, turn2) = (turn.clone(), turn);
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            t.index = 0;
            let mut best = f32::MAX;
            for i in 0..count {
                t.index = i as i32;
                point(t, &o1);
                let d = (t.pos - t.target_pos).abs().element_sum();
                if i == 0 || d < best {
                    best = d;
                }
            }
            // The decomp keeps the last point that was nearer than the first; start at the nearest.
            let mut nearest = 0usize;
            let mut nd = f32::MAX;
            for i in 0..count {
                t.index = i as i32;
                point(t, &o1);
                let d = (t.pos - t.target_pos).abs().element_sum();
                if d < nd {
                    nd = d;
                    nearest = i;
                }
            }
            t.index = nearest as i32;
            point(t, &o1);
            turn1.start(t, w);
            t.status = 1;
            t.stop_move();
        })
        .on_act(move |t, w| {
            match t.status {
                1 => {
                    if turn2.act(t, w) {
                        t.status = 0;
                    }
                }
                _ => {
                    if t.current_motion() != m as i32 || t.anim.is_finished() {
                        if !t.anim.is_finished() {
                            t.finish_motion();
                        }
                        if t.anim.is_finishing() {
                            t.start_motion(m);
                        }
                    } else {
                        let dist = t.target_pos.distance(t.pos);
                        t.counter += w.dt;
                        let walk = t.f(tpf::WALK_VELOCITY);
                        if dist <= walk || t.counter > t.f(timeout_param) {
                            t.index += 1;
                            if t.index as usize >= count {
                                t.index = 0;
                            }
                            point(t, &o2);
                            t.finish_motion();
                            t.status = 1;
                            t.stop_move();
                            let tp = t.target_pos;
                            t.target_angle = t.calc_target_direction(tp);
                            turn2.start(t, w);
                        } else {
                            let goal = t.target_pos;
                            t.move_toward(goal, walk, w.dt);
                        }
                    }
                }
            }
            false
        })
        .build()
}

/// `TAIAfireBreath`: the attack motion at its own speed; at the action key a flame leaves the mouth and burns
/// what it touches until the motion is done.
pub fn fire_breath(next: i32, m: usize, speed_param: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.manual_speed = Some(t.f(speed_param));
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            t.manual_speed = Some(t.f(speed_param));
            if t.key(key::ACTION0) {
                if let Some((mouth, _)) = t.part("kuti") {
                    let dir = t.forward();
                    let from = mouth - dir * 15.0;
                    t.fire = Some(Fire { from, dir, length: 200.0, radius: 20.0, damage: t.f(tpf::ATTACK_POWER) });
                    w.out.push(Out::EffectDir { id: 100, pos: mouth, dir });
                    w.out.push(Out::EffectDir { id: 99, pos: mouth, dir });
                    w.out.push(Out::Sound { name: "SE_TANK_FIRE", pos: mouth });
                }
            }
            if t.key(key::FINISHED) {
                t.manual_speed = None;
                t.fire = None;
                return true;
            }
            false
        })
        .build()
}

/// `TAIAeffCloudOfDust`: foot dust is not drawn here.
pub fn eff_cloud() -> Act {
    once(NO_TRANSIT)
}

/// A burning flame burns what is in it each tick (`TAIeffectAttackEventCallBackTank::hitCreature`, `InteractFire`).
pub fn burn(t: &mut Teki, w: &mut World) {
    let Some(f) = t.fire else { return };
    let dt = w.dt;
    // Pikmin: hurt along the stream.
    let steps = 6;
    for i in 0..=steps {
        let p = f.from + f.dir * (f.length * i as f32 / steps as f32);
        w.sim.burn_pikis_in_sphere(p, f.radius, f.damage * dt * 0.5);
    }
    for n in w.navis.iter().filter(|n| n.alive) {
        let rel = n.pos - f.from;
        let along = rel.dot(f.dir);
        if (0.0..=f.length).contains(&along) && (rel - f.dir * along).with_y(0.0).length() <= f.radius + n.size {
            // A negative knock marks a burn: the host applies it as damage over time.
            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: f.damage, from: t.pos, knock: -1.0 });
        }
    }
}

pub const PI_: f32 = PI;

// ---- the actions the Sheargrubs, Wogpoles and the other burrowers share (TAIAmove.cpp, TAIAreaction.cpp)

/// `TAIAsetMotionSpeed`: start `m` and hold the animation at `speed` frames a second.
pub fn set_motion_speed(next: i32, m: usize, speed: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.manual_speed = Some(speed);
        })
        .build()
}

/// `TAIAsetTargetPointCircleRandom`: a random place within the danger territory around the nest.
pub fn set_target_circle_random(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, w| {
            let angle = w.rng.range(0.0, TAU);
            let dist = w.rng.range(0.0, t.f(tpf::DANGER_TERRITORY_RANGE));
            let mut p = t.nest + Vec3::new(dist * angle.sin(), 0.0, dist * angle.cos());
            p.y = t.ground_y.unwrap_or(t.nest.y);
            t.target_pos = p;
        })
        .on_act(|_, _| true)
        .build()
}

/// `TAIAgoTargetPriorityFaceDir`: walk to `target_pos`, facing the way it goes first.
pub fn go_target_face(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let walk = t.f(tpf::WALK_VELOCITY);
            if t.target_pos.distance(t.pos) <= walk {
                return true;
            }
            let goal = t.target_pos;
            t.move_toward_face_priority(goal, walk, w.dt);
            false
        })
        .build()
}

/// `TAIAapproachTargetPriorityFaceDir`: close in on the creature targeted; true once it is gone.
pub fn approach_target_face(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let Some(c) = t.target(0, w) else { return true };
            let walk = t.f(tpf::WALK_VELOCITY);
            t.move_toward_face_priority(c.pos, walk, w.dt);
            false
        })
        .build()
}

/// `TAIAtimerReaction`: true once `secs` of the frame counter have run.
pub fn timer_reaction(next: i32, secs: impl Fn(&Teki) -> f32 + Send + Sync + 'static) -> Act {
    Action::new(next)
        .on_start(|t, _| t.counter = 0.0)
        .on_act(move |t, w| {
            t.counter += w.dt;
            t.counter > secs(t)
        })
        .build()
}

/// `TAIAhitCheckFlyingPiki`: a thrown Pikmin landed on it.
pub fn hit_check_flying_piki(next: i32) -> Act {
    Action::new(next).on_event(|_, _, e| matches!(e, Event::Entity { other: Target::Piki(_), flying: true } | Event::Pressed)).build()
}

/// `TAIAinWaterDamage`: water hurts it `damage` a second; a splash as it goes in or comes out.
pub fn in_water_damage(damage: f32, splash: bool) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, w| {
            let wet = t.in_water;
            if wet {
                t.hp -= w.dt * damage;
            }
            if splash {
                if wet != (t.reg[7] > 0.5) {
                    w.out.push(Out::Effect { id: crate::effects::BUBBLES, pos: t.pos });
                    w.out.push(Out::Sound { name: "SE_OTAMA_WATERJUMP", pos: t.pos });
                }
                t.reg[7] = if wet { 1.0 } else { 0.0 };
            }
            wet
        })
        .build()
}

/// `YTeki::killStickToMouthPiki`: what is in its mouth dies.
pub fn kill_mouth_pikis(t: &mut Teki, w: &mut World) {
    w.sim.chew(t.creature);
    t.held_piki = false;
}

/// `TAIAdyingKabekui`: the dying motion, with a puff of dust where it lands.
pub fn dying_with_dust(m: usize, effect: usize) -> Act {
    let inner = yai_dying(m);
    let (i1, i2) = (inner.clone(), inner);
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            i1.start(t, w);
            t.finish_flying();
        })
        .on_act(move |t, w| {
            let on_ground = t.ground_y.is_none_or(|g| (g - t.pos.y).abs() < 1.0);
            if on_ground && (t.action_vel.y < -2.0 * t.gravity * w.dt || t.key(key::ACTION0)) {
                w.out.push(Out::Effect { id: effect, pos: t.pos });
            }
            i2.act(t, w)
        })
        .build()
}

/// `YTeki::flyingStay`: climb or sink toward the flight height, at most `speed`.
pub fn flying_stay(t: &mut Teki, speed: f32, dt: f32) {
    let mut h = t.f(tpf::FLIGHT_HEIGHT) - t.height();
    let limit = 0.05 * speed;
    if h.abs() > limit {
        h = h.signum() * limit;
    }
    t.target_vel.y += h * 900.0 * dt * dt;
    if t.target_vel.y.abs() > speed {
        t.target_vel.y = t.target_vel.y.signum() * speed;
    }
}

/// `TAIAflyingBase`: hold the flight height; `speed` is the climb rate allowed (it changes once it has risen).
pub fn flying_base(speed: impl Fn(&Teki) -> f32 + Send + Sync + 'static) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| {
            t.start_flying();
            t.reg[6] = 0.0;
        })
        .on_act(move |t, w| {
            let s = speed(t);
            flying_stay(t, s, w.dt);
            if t.height() > t.f(tpf::FLIGHT_HEIGHT) {
                t.reg[6] = 1.0;
            }
            false
        })
        .build()
}

/// The turn toward where it flies (`mFaceDirAdjust`).
pub fn face_velocity(t: &mut Teki, adjust: f32) {
    let v = t.target_vel.with_y(0.0);
    if v.length() > 1.0 {
        let a = nearer_direction(t.direction(), v.x.atan2(v.z));
        t.yaw += (a - t.direction()) * adjust;
    }
}

/// `TAIAflyingInTerritory`: drift to random points around the nest at the flight height.
pub fn flying_in_territory(face_adjust: f32) -> Act {
    fn pick(t: &mut Teki, w: &mut World) {
        let mut o = Vec3::new(w.rng.range(-1.0, 1.0), 0.0, w.rng.range(-1.0, 1.0));
        o = o.normalize_or_zero() * t.f(tpf::SAFETY_TERRITORY_RANGE);
        t.target_pos = Vec3::new(o.x + t.nest.x, t.f(tpf::FLIGHT_HEIGHT), o.z + t.nest.z);
    }
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.start_flying();
            pick(t, w);
        })
        .on_act(move |t, w| {
            let speed = t.f(tpf::WALK_VELOCITY);
            let y_speed = t.target_vel.y;
            let here = Vec3::new(t.pos.x, t.height(), t.pos.z);
            if t.target_pos.distance(here) <= speed {
                pick(t, w);
            }
            let mut dir = (t.target_pos - t.pos).with_y(0.0);
            let dist = dir.length();
            if dist > speed * 0.1 {
                dir = dir / dist * (0.1 * speed);
            }
            t.target_vel += dir * (w.dt * 900.0 * w.dt);
            let len = t.target_vel.length();
            if len > speed {
                t.target_vel = t.target_vel / len * speed;
            }
            t.target_vel.y = y_speed;
            face_velocity(t, face_adjust);
            false
        })
        .build()
}
