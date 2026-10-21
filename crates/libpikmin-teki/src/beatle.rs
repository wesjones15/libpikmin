//! The Armored Cannon Beetle (`TAIbeatleStrategy`, TAIbeatle.cpp) and the Rolling Boulder it shoots
//! (`TaiIwagonStrategy`, taiiwagen.cpp). The beetle looks around, turns to what it sees, closes in until it is in
//! range, then sucks at the target with its nozzle: a Pikmin sucked in is held, the beetle overheats
//! (shaking everything off, steaming, backing away); with nothing sucked in it shoots a boulder from its
//! mouth. The boulder rolls straight until it hits a wall, another boulder or something, runs out of life or is hurt.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::{tpf, tpi};

// TAIbeatleFloatParms
const FLICKING_TIME: usize = 50;
const ATTACK_RANGE: usize = 51;
const SWALLOW_REACH: usize = 52;
const SUCTION_ANGLE: usize = 53;
const SUCTION_RADIUS: usize = 54;

// TAIbeatleStateID
const DYING: i32 = 0;
const INITIALIZING: i32 = 1;

fn init(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            t.coll_radius = 64.0;
            t.flags[3] = false;
        })
        .on_act(|_, _| true)
        .build()
}

fn invincible_off() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.clear_option(opt::INVINCIBLE)).build()
}

fn disable_stick() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.clear_option(opt::STICKABLE)).build()
}

/// `TAIAvisiblePikiBeatle`: a visible Pikmin not stuck on it and farther than 100 away.
fn visible_piki_far(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.closest(w, |c| c.piki.is_some() && !c.sticks_to_me && t.visible(c) && t.pos.distance(c.pos) > 100.0) {
            Some(c) => {
                t.targets[0] = Some(c.target);
                true
            }
            None => false,
        })
        .build()
}

/// `TAIAlookAround`: glances left and right while waiting.
fn look_around(m: usize, left: usize, right: usize) -> Act {
    fn aim(t: &mut Teki, w: &mut World) {
        let angle = 40f32.to_radians();
        let sign = if w.rng.f32() > 0.5 { 1.0 } else { -1.0 };
        t.target_angle = (angle + w.rng.range(0.0, angle)) * sign + t.direction();
    }
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            reserve_start(t, m);
            t.counter = 0.0;
            t.status = 2;
            aim(t, w);
        })
        .on_act(move |t, w| {
            match t.status {
                2 => {
                    if reserve_act(t, m) {
                        t.status = 1;
                    }
                }
                0 => {
                    let face = t.direction();
                    let mo = if nearer_direction(face, t.target_angle) - face > 0.0 { left } else { right };
                    if t.current_motion() != mo as i32 || t.anim.is_finished() {
                        if !t.anim.is_finished() {
                            t.finish_motion();
                        }
                        if t.anim.is_finishing() {
                            t.start_motion(mo);
                        }
                    } else {
                        let (a, s) = (t.target_angle, t.f(tpf::TURN_VELOCITY));
                        if t.turn_toward(a, s, w.dt) {
                            t.finish_motion();
                            t.counter = 0.0;
                            t.status = 1;
                        }
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
                        t.counter += w.dt;
                        if t.counter > 0.7 {
                            t.counter = 0.0;
                            t.status = 0;
                            aim(t, w);
                            t.finish_motion();
                        }
                    }
                }
            }
            false
        })
        .build()
}

/// `TAIAtimerReaction`.
fn timer_reaction(next: i32, secs: f32) -> Act {
    Action::new(next)
        .on_start(|t, _| t.counter = 0.0)
        .on_act(move |t, w| {
            t.counter += w.dt;
            t.counter > secs
        })
        .build()
}

/// `TAIAsetTargetPointCircle`: a point on the safety circle around the nest, a way round from where it is.
fn set_target_circle(next: i32, offset: f32) -> Act {
    fn point(t: &mut Teki, w: &mut World, offset: f32) {
        let from = t.pos - t.nest;
        let angle = offset + w.rng.range(0.0, offset) + from.x.atan2(from.z);
        let dist = t.f(tpf::SAFETY_TERRITORY_RANGE);
        t.target_pos = t.nest + Vec3::new(dist * angle.sin(), 0.0, dist * angle.cos());
    }
    Action::new(next)
        .on_start(move |t, w| point(t, w, offset))
        .on_act(move |t, w| {
            point(t, w, offset);
            true
        })
        .build()
}

/// `TAIAgoTarget`: walk to `target_pos`.
fn go_target(next: i32, m: usize) -> Act {
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
            t.move_toward(goal, walk, w.dt);
            false
        })
        .build()
}

/// `TAIAapproachTargetPriorityFaceDir`: close on the target, turning as it goes.
fn approach(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let Some(c) = t.target(0, w) else { return true };
            let run = t.f(tpf::RUN_VELOCITY);
            t.move_toward_face_priority(c.pos, run, w.dt);
            false
        })
        .build()
}

/// `TAIAinsideOptionalRangeBeatle`: near enough to attack.
fn inside_attack_range(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.target(0, w) {
            Some(c) => {
                t.target_pos = c.pos;
                t.target_pos.distance(t.pos) < t.f(ATTACK_RANGE)
            }
            None => true,
        })
        .build()
}

/// `TAIAturnHome`.
fn turn_home(next: i32, left: usize, right: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.target_pos = t.nest;
            t.target_angle = t.calc_target_direction(t.nest);
            let face = t.direction();
            let m = if nearer_direction(face, t.target_angle) - face > 0.0 { left } else { right };
            reserve_start(t, m);
            t.stop_move();
        })
        .on_act(move |t, w| {
            let face = t.direction();
            let m = if nearer_direction(face, t.target_angle) - face > 0.0 { left } else { right };
            if !reserve_act(t, m) {
                return false;
            }
            t.target_angle = t.calc_target_direction(t.target_pos);
            let (a, s) = (t.target_angle, t.f(tpf::TURN_VELOCITY));
            t.turn_toward(a, s, w.dt)
        })
        .build()
}

/// `TAIAgoingHome`: walk back at the run speed.
fn going_home(next: i32, m: usize) -> Act {
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
            t.move_toward(nest, run, w.dt);
            false
        })
        .build()
}

/// `TAIAcheckTurnAngle`: true once it has turned through `angle` since it started.
fn check_turn_angle(next: i32, angle: f32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            t.reg[0] = t.yaw;
            t.counter = 0.0;
        })
        .on_act(move |t, _| {
            let mut d = t.yaw - t.reg[0];
            if d.abs() > std::f32::consts::PI {
                d += if d > 0.0 { -std::f32::consts::TAU } else { std::f32::consts::TAU };
            }
            t.counter += d;
            t.reg[0] = t.yaw;
            t.counter.abs() > angle
        })
        .build()
}

/// `TAIArockAttack`: suck at the target; a Pikmin sucked in is held (the beetle then overheats); with none the
/// beetle fires a boulder from its mouth.
fn rock_attack(next_miss: i32, next_hit: i32, m: usize) -> Act {
    Action::new(next_miss)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.flags[0] = false;
            t.targets[1] = None;
            t.held_piki = false;
            t.next_override = None;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let mut done = false;
            let Some((nozzle, _)) = t.part("slot") else { return false };
            if t.key(key::ACTION0) {
                t.flags[0] = true;
            }
            if t.key(key::ACTION1) {
                t.flags[0] = false;
            }
            if t.flags[0] && !t.held_piki {
                let reach = t.f(SWALLOW_REACH);
                if w.sim.swallow_pikis(t.creature, 0, nozzle, reach, 1) > 0 {
                    t.held_piki = true;
                    w.out.push(Out::Effect { id: crate::effects::BEATLE_SHOOT_ROCK_HALO, pos: nozzle });
                    w.out.push(Out::Sound { name: "SE_KABUTO_TUMARI", pos: nozzle });
                    t.next_override = Some(next_hit);
                    done = true;
                }
            }
            if t.key(key::ACTION3) {
                t.flick(w);
            }
            if t.key(key::ACTION2) {
                if let Some((mouth, _)) = t.part("kuti") {
                    let dir = t.forward();
                    let at = mouth + dir * 60.0;
                    w.out.push(Out::EffectDir { id: crate::effects::BEATLE_ROCK_BLAST, pos: at, dir });
                    w.out.push(Out::Spawn {
                        kind: crate::info::IWAGON,
                        pos: t.pos + dir * (t.coll_radius + 20.0) + Vec3::Y * 53.0,
                        yaw: t.yaw,
                        parent: t.id,
                        velocity: dir * t.f(tpf::SPAWN_VELOCITY),
                    });
                    w.out.push(Out::Sound { name: "SE_KABUTO_SHOT", pos: t.pos });
                    w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
                }
            }
            if !t.held_piki && t.key(key::LOOP_END) {
                t.next_override = Some(next_miss);
                t.finish_motion();
            }
            if t.key(key::FINISHED) {
                done = true;
            }
            done
        })
        .build()
}

/// `TAIAflickingAfterMotionLoopBeatle`: overheated: shakes everything off, steams, then walks away from the
/// players for a while.
fn overheat(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            t.start_motion(m);
            t.clear_option(opt::DAMAGE_COUNTABLE);
            t.counter = 0.0;
            t.flags[1] = false;
            w.out.push(Out::Sound { name: "SE_KABUTO_OVERHEAT", pos: t.pos });
        })
        .on_act(move |t, w| {
            if t.key(key::ACTION1) {
                t.set_option(opt::STICKABLE);
                if let Some((mouth, _)) = t.part("kuti") {
                    w.out.push(Out::EffectDir { id: crate::effects::BEATLE_FLICK1, pos: mouth, dir: t.forward() });
                }
            }
            if t.key(key::ACTION2) {
                t.clear_option(opt::STICKABLE);
                t.flags[1] = false;
            }
            if t.key(key::ACTION0) {
                t.held_piki = false;
                w.sim.spit_out(t.creature);
            }
            if t.key(key::ACTION3) {
                w.out.push(Out::Sound { name: "SE_KABUTO_COOLDOWN", pos: t.pos });
                if let Some((mouth, _)) = t.part("kuti") {
                    w.out.push(Out::EffectDir { id: crate::effects::BEATLE_FLICK3, pos: mouth, dir: t.forward() });
                }
            }
            if t.key(key::LOOP_START) {
                t.flags[1] = true;
            }
            if t.key(key::ACTION0) {
                t.flick(w);
            }
            if t.flags[1] {
                // Runs away from the nearest player.
                if let Some(n) = w.navis.iter().min_by(|a, b| a.pos.distance(t.pos).total_cmp(&b.pos.distance(t.pos))) {
                    let away = (t.pos - n.pos).with_y(0.0);
                    let away = if away.length() > 1e-3 { away.normalize() } else { t.forward() };
                    let mut goal = t.pos + away * t.f(tpf::SAFETY_TERRITORY_RANGE);
                    if goal.distance(t.nest) > t.f(tpf::DANGER_TERRITORY_RANGE) {
                        goal = t.nest + (goal - t.nest).normalize() * t.f(tpf::DANGER_TERRITORY_RANGE);
                    }
                    let walk = t.f(tpf::WALK_VELOCITY) * 0.85;
                    t.move_toward_face_priority(goal, walk, w.dt);
                }
            }
            t.counter += w.dt;
            if t.key(key::FINISHED) && t.counter > t.f(FLICKING_TIME) * 0.0 {
                t.set_option(opt::DAMAGE_COUNTABLE);
                return true;
            }
            false
        })
        .build()
}

/// `TAIAflickingBeatle`: shakes everything off with a ring of dust.
fn flicking_beatle(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.stop_move();
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_act(|t, w| {
            if t.key(key::FINISHED) {
                t.set_option(opt::DAMAGE_COUNTABLE);
                t.damage_count = 0.0;
                return true;
            }
            if t.key(key::ACTION0) {
                t.flick(w);
                w.out.push(Out::Effect { id: crate::effects::BIG_DUST_RING, pos: t.pos });
            }
            false
        })
        .build()
}

/// `TAIbeatleStrategy`.
pub fn beatle(_info: &TekiInfo) -> Strategy {
    let dead_check = dead(DYING);
    let damage = yai_damage(true);
    let stop = stop_move();
    let inv = invincible_off();
    let vis_navi = visible_navi(6);
    let vis_piki = visible_piki_far(6);
    let look = look_around(WAIT1, TYPE1, TYPE2);
    let eff = eff_cloud();

    let mut s = Strategy::new(14, INITIALIZING);
    s.set(INITIALIZING, vec![init(2)]);
    s.set(DYING, vec![yai_dying(DEAD), stop.clone()]);
    s.set(3, vec![inv.clone(), set_target_circle(4, 0.698_131_7)]);
    s.set(4, vec![inv.clone(), stop.clone(), dead_check.clone(), damage.clone(), turn_to_target(5, TYPE1, TYPE2, true), vis_navi.clone(), vis_piki.clone(), eff.clone()]);
    s.set(5, vec![inv.clone(), stop.clone(), dead_check.clone(), damage.clone(), go_target(2, MOVE1), vis_navi.clone(), vis_piki.clone(), eff.clone()]);
    s.set(2, vec![
        disable_stick(), inv.clone(), stop.clone(), dead_check.clone(), damage.clone(), look, vis_navi, vis_piki, timer_reaction(3, 3.0), eff.clone(),
    ]);
    s.set(6, vec![
        dead_check.clone(), damage.clone(), turn_focus(8, TYPE1, TYPE2, true), unvisible_target(2), check_turn_angle(12, 1.570_796_4), eff.clone(),
    ]);
    s.set(7, vec![dead_check.clone(), damage.clone(), inside_attack_range(8), approach(NO_TRANSIT, MOVE1), outside_danger_territory(9), eff.clone()]);
    s.set(8, vec![stop.clone(), dead_check.clone(), damage.clone(), rock_attack(13, 11, ATTACK), eff.clone()]);
    s.set(9, vec![stop, dead_check.clone(), damage.clone(), turn_home(10, TYPE1, TYPE2), eff.clone()]);
    s.set(10, vec![dead_check.clone(), damage.clone(), going_home(2, MOVE1), visible_navi(6), visible_piki_far(6), eff.clone()]);
    s.set(11, vec![inv, dead_check, damage, overheat(2, FLICK), eff.clone()]);
    s.set(12, vec![flicking_beatle(13, 12), eff]);
    s.set(13, vec![visible_piki(2), visible_navi(2), no_reaction(2)]);
    s
}

// ---- the Rolling Boulder

/// `TaiIwagonRollingAction`: it keeps the velocity it was shot with.
fn rolling(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| t.reg[1] = 0.0)
        .on_act(|t, _| {
            t.target_vel = t.spawn_vel.with_y(0.0);
            false
        })
        .build()
}

/// `TaiBangingAction`: whatever it touches is pressed.
fn banging() -> Act {
    Action::new(NO_TRANSIT)
        .on_event(|t, w, e| {
            let Event::Entity { other, .. } = e else { return false };
            let Some(c) = t.cand_of(*other, w) else { return false };
            let power = t.f(tpf::ATTACK_POWER);
            match c.target {
                Target::Piki(_) => {
                    w.sim.crush_pikis_in_sphere(c.pos, c.size + 1.0, power);
                }
                Target::Navi(id) => w.out.push(Out::HitNavi { teki: t.id, navi: id, damage: power, from: t.pos, knock: 80.0 }),
                Target::Pellet(_) => {}
            }
            true
        })
        .build()
}

/// `TaiIwagonStrategy`.
pub fn iwagon(info: &TekiInfo) -> Strategy {
    let life = info.floats[tpf::LIFE_PERIOD];
    let mut s = Strategy::new(2, 1);
    s.set(0, vec![stop_move(), dying(DEAD), set_options(opt::SHADOW_VISIBLE, false)]);
    s.set(1, vec![
        damage(0),
        Action::new(0).on_event(|_, _, e| matches!(e, Event::Wall)).build(),
        timer(0, 0, life, 0.0),
        banging(),
        rolling(0),
        motion(NO_TRANSIT, MOVE1),
        play_sound("SE_ROCK_ROLL"),
    ]);
    let _ = tpi::SPAWN_TYPE;
    s
}
