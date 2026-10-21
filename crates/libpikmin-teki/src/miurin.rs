//! The Mamuta: `TAImiurinStrategy` (TAImiurin.cpp). It ambles about its territory; a player who stands among its
//! flowers, or any hit, angers it. Angry it stamps (flinging off whatever is close), runs at its target and
//! punches with each hand, burying what it hits (here: hurting). It shakes off Pikmin that cling to it, and it
//! calms down when enough flowers are about. The decomp's `isNuking` (a player pulling up sprouts) is a player
//! standing within `SAFETY_TERRITORY_RANGE`; "buried" states (a buried player) never occur, so the ones that wait
//! for it are kept but never reached.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::tpf;

// TAImiurinFloatParams / TAImiurinIntParams
const RELAX_TIME: usize = 50;
const EYE_ROLL_TIME: usize = 51;
const CHECK_NAVI_CHANCE: usize = 52;
const ANGRY_ROTATION_SPEED: usize = 53;
const MAX_STICK_PIKI: usize = 20;
const SHAKE_OFF_HIT_COUNT: usize = 21;
const RELAXING_FLOWER_NUM: usize = 22;

// TAImiurinStateID
const DEAD_STATE: i32 = 0;
const INIT: i32 = 1;
const PAUSE: i32 = 2;
const IDLE: i32 = 3;
const WATCH: i32 = 4;
const TURN_TO_TARGET: i32 = 5;
const GO_TARGET: i32 = 6;
const ANGRY: i32 = 7;
const FACE_NAVI: i32 = 8;
const CHASE: i32 = 9;
const POSTURE: i32 = 10;
const PUNCH: i32 = 11;
const AFTER_PUNCH: i32 = 12;
const CALM_CHECK: i32 = 13;
const LOOK_BACK: i32 = 14;
const SATISFIED: i32 = 15;
const SHAKE: i32 = 16;
const SHAKE_HOME: i32 = 17;
const FACE_TARGET_2: i32 = 18;
const POSTURE_2: i32 = 19;
const PUNCH_2: i32 = 20;
const AFTER_PUNCH_2: i32 = 21;
const CALM_CHECK_2: i32 = 22;
const GROGGY: i32 = 23;

fn init(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            t.damage_count = 0.0;
            t.coll_radius = 50.0;
        })
        .on_act(|_, _| true)
        .build()
}

/// `TAIAresetAnimSpeedMiurin`.
fn reset_anim_speed() -> Act {
    Action::new(NO_TRANSIT).on_start(|t, _| t.manual_speed = None).build()
}

/// The footfalls (`TAIAeffCloudOfDust` on 'asi1', 'asi2'): smoke where a foot meets the ground.
fn foot_dust(effect: usize, sound: &'static str) -> Act {
    Action::new(NO_TRANSIT)
        .on_act(move |t, w| {
            t.reg[0] -= w.dt;
            if t.target_vel.length() > 5.0 && t.reg[0] <= 0.0 {
                t.reg[0] = 0.5;
                for part in ["asi1", "asi2"] {
                    if let Some((p, _)) = t.part(part) {
                        if p.y - t.ground_y.unwrap_or(0.0) < 15.0 {
                            w.out.push(Out::Effect { id: effect, pos: Vec3::new(p.x, t.ground_y.unwrap_or(p.y), p.z) });
                            w.out.push(Out::Sound { name: sound, pos: p });
                        }
                    }
                }
            }
            false
        })
        .build()
}

/// The nearest player (the decomp's single Navi).
fn nearest_navi(t: &Teki, w: &World) -> Option<Navi> {
    w.navis.iter().filter(|n| n.alive).min_by(|a, b| dist_xz(a.pos, t.pos).total_cmp(&dist_xz(b.pos, t.pos))).copied()
}

/// `TAIAwatchNaviMiurin`: hit, or a player among its flowers, angers it.
fn watch_navi(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let nuking = nearest_navi(t, w).is_some_and(|n| t.visible(&Cand { target: Target::Navi(n.id), pos: n.pos, size: n.size, sticks_to_me: false, sticking: false, piki: None }) && dist_xz(n.pos, t.pos) < t.f(tpf::SAFETY_TERRITORY_RANGE));
            if t.damage_count > 0.0 || nuking {
                t.targets[0] = nearest_navi(t, w).map(|n| Target::Navi(n.id));
                return true;
            }
            false
        })
        .build()
}

/// `TAIAcheckNaviMiurin`: now and then, a player in the territory draws its eye.
fn check_navi(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            if w.rng.range(0.0, 100.0) < t.f(CHECK_NAVI_CHANCE) {
                let range = t.f(tpf::DANGER_TERRITORY_RANGE);
                if let Some(n) = nearest_navi(t, w) {
                    if dist_xz(n.pos, t.nest) < range {
                        t.targets[0] = Some(Target::Navi(n.id));
                        return true;
                    }
                }
            }
            false
        })
        .build()
}

/// `TAIAinsideTerritoryRangeNavi`.
fn inside_territory_navi(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::DANGER_TERRITORY_RANGE);
            nearest_navi(t, w).is_some_and(|n| dist_xz(n.pos, t.nest) < range && {
                t.targets[0] = Some(Target::Navi(n.id));
                true
            })
        })
        .build()
}

/// `TAIAcheckFlowerPikmin`: enough flowers (grown sprouts) around make it content.
fn check_flowers(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::SAFETY_TERRITORY_RANGE);
            let n = w.sim.sprouts().filter(|s| s.happa == libpikmin::Happa::Flower && s.pos.distance(t.pos) < range).count() as i32;
            n >= t.i(RELAXING_FLOWER_NUM)
        })
        .build()
}

/// `TAIAangryMiurin`: the stamp: whatever is near is flung away, with dust and a shake.
fn angry(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            t.start_motion(m);
            t.counter = 0.0;
            t.flick(w);
            w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
        })
        .on_act(|t, w| {
            t.counter += w.dt;
            if let Some(n) = nearest_navi(t, w) {
                t.target_angle = t.calc_target_direction(n.pos);
            }
            if t.key(key::ACTION0) {
                let at = t.pos + Vec3::Y * 50.0;
                w.out.push(Out::Effect { id: effects::MIURIN_A01, pos: at });
                w.out.push(Out::Effect { id: effects::MIURIN_A02, pos: at });
            }
            if t.counter < 0.5 {
                let a = t.target_angle;
                t.turn_toward(a, 4.0 * PI / 3.0, w.dt);
            }
            t.key(key::FINISHED)
        })
        .build()
}

/// `TAIAsatisfyMiurin` / `TAIAgroggyMiurin`: a motion held for a time, then finished.
fn relax(next: i32, m: usize, param: usize, sing: bool) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.counter = 0.0;
            t.damage_count = 0.0;
        })
        .on_act(move |t, w| {
            let mut res = false;
            if reserve_act(t, m) {
                t.counter += w.dt;
                if t.counter > t.f(param) {
                    t.finish_motion();
                }
                if t.key(key::FINISHED) {
                    res = true;
                }
                if sing && t.key(key::LOOP_START) {
                    w.out.push(Out::Sound { name: "SE_MIURIN_SING", pos: t.pos });
                }
            }
            if sing && t.key(key::ACTION0) {
                let at = Vec3::new(t.pos.x, t.ground_y.unwrap_or(t.pos.y), t.pos.z);
                w.out.push(Out::Effect { id: effects::SMOKE_RING_M, pos: at });
                w.out.push(Out::Sound { name: "SE_CHAPPY_DOWN", pos: t.pos });
            }
            res
        })
        .build()
}

/// `TAIAattackPosture`: the wind-up, held still once reached.
fn posture(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, _| {
            let res = reserve_act(t, m);
            if res {
                t.manual_speed = Some(0.0);
            }
            res
        })
        .build()
}

/// `TAIAattackMiurin`: the punch. Which one depends on where the target is.
fn punch(next: i32, left_motion: usize, right_motion: usize, straight_motion: usize) -> Act {
    fn hit(t: &mut Teki, w: &mut World, part: &str) {
        let Some((p, r)) = t.part(part) else { return };
        let at = Vec3::new(p.x, t.ground_y.unwrap_or(p.y), p.z);
        w.out.push(Out::Effect { id: effects::SMOKE_RING_M, pos: at });
        w.out.push(Out::Effect { id: effects::MIURIN_ATTACK, pos: p });
        for n in w.navis.iter().filter(|n| n.alive && p.distance(n.pos) < r) {
            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: 20.0, from: p, knock: 0.0 });
        }
        w.sim.crush_pikis_in_sphere(p, r, 10.0);
        w.out.push(Out::Sound { name: "SE_MIURIN_PUNCH", pos: p });
        w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
    }
    Action::new(next)
        .on_start(move |t, w| {
            let motion = match nearest_navi(t, w) {
                Some(n) => {
                    let a = (n.pos.x - t.pos.x).atan2(n.pos.z - t.pos.z);
                    let mut d = a - t.yaw;
                    if d.abs() > PI {
                        d += if d > 0.0 { -TAU } else { TAU };
                    }
                    if d.abs() > 80f32.to_radians() {
                        straight_motion
                    } else if d > 50f32.to_radians() {
                        left_motion
                    } else if d > -(PI / 3.0) && d < PI / 6.0 {
                        straight_motion
                    } else {
                        right_motion
                    }
                }
                None => straight_motion,
            };
            t.index = motion as i32;
            reserve_start(t, motion);
        })
        .on_act(|t, w| {
            let m = t.index as usize;
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::ACTION0) {
                hit(t, w, "hndR");
            }
            if t.key(key::ACTION1) {
                hit(t, w, "hndL");
            }
            if t.key(key::LOOP_START) {
                t.finish_motion();
            }
            t.key(key::FINISHED)
        })
        .build()
}

/// `TAIAattackableTargetMiurin`: in range and angle of the target.
fn attackable(next: i32) -> Act {
    attackable_target(next, true)
}

/// `TAIAoutsideTerritoryMiurin`: it, and its target, are outside.
fn outside_territory_both(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            if t.territory_distance() <= t.f(tpf::DANGER_TERRITORY_RANGE) {
                return false;
            }
            t.target(0, w).is_some_and(|c| dist_xz(c.pos, t.nest) > t.f(tpf::DANGER_TERRITORY_RANGE))
        })
        .build()
}

/// `TAIAapproachTargetPriorityFaceDirMiurin`: at the run, quickened.
fn approach_run(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.manual_speed = Some(60.0);
        })
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

/// `TAIAturnFocusCreatureMiurin`: face the target at the angry rotation speed.
fn turn_focus_angry(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            t.stop_move();
            reserve_start(t, m);
            t.manual_speed = Some(60.0);
            t.target_angle = t.target(0, w).map_or(t.yaw, |c| t.calc_target_direction(c.pos));
        })
        .on_act(move |t, w| {
            let Some(c) = t.target(0, w) else { return true };
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
            t.target_angle = t.calc_target_direction(c.pos);
            let (a, s) = (t.target_angle, t.f(ANGRY_ROTATION_SPEED));
            t.turn_toward(a, s, w.dt)
        })
        .build()
}

/// `TAIAflickingMiurin`: the shake throws what is on it backward.
fn shaking(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.stop_move();
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::FINISHED) {
                t.set_option(opt::DAMAGE_COUNTABLE);
                t.damage_count = 0.0;
                return true;
            }
            if t.key(key::ACTION0) {
                t.flick(w);
            }
            false
        })
        .build()
}

fn sticking(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| w.sim.pikis().iter().filter(|p| p.is_alive() && p.state.latched_to() == Some(t.creature)).count() as i32 >= t.i(MAX_STICK_PIKI))
        .build()
}

fn flick_check_hits(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, _| {
            let hit = t.damage_count >= t.i(SHAKE_OFF_HIT_COUNT) as f32;
            if hit {
                t.damage_count = 0.0;
            }
            hit
        })
        .build()
}

/// `TAIAdyingMiurin`.
fn dying_miurin(m: usize) -> Act {
    let inner = yai_dying(m);
    let (i1, i2) = (inner.clone(), inner);
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            i1.start(t, w);
            t.counter = 0.0;
        })
        .on_act(move |t, w| {
            t.counter += w.dt;
            if t.counter > 1.5 {
                t.finish_motion();
            }
            if t.key(key::ACTION0) {
                let at = Vec3::new(t.pos.x, t.ground_y.unwrap_or(t.pos.y), t.pos.z);
                w.out.push(Out::Effect { id: effects::SMOKE_RING_M, pos: at });
                w.out.push(Out::Sound { name: "SE_CHAPPY_DOWN", pos: t.pos });
            }
            i2.act(t, w)
        })
        .build()
}

/// `TAIAsetTargetPointCircle`: a point on the ring around the nest, a third of a turn on from where it stands.
fn set_target_circle(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, w| {
            let a = PI / 3.0 + w.rng.range(0.0, PI / 3.0) + (t.pos.x - t.nest.x).atan2(t.pos.z - t.nest.z);
            let d = t.f(tpf::SAFETY_TERRITORY_RANGE);
            t.target_pos = Vec3::new(d * a.sin() + t.nest.x, t.ground_y.unwrap_or(t.nest.y), d * a.cos() + t.nest.z);
        })
        .on_act(|_, _| true)
        .build()
}

/// `TAIAgoTarget`: walk (straight, no turning first) to `target_pos`.
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
            face_velocity(t, 0.3);
            false
        })
        .build()
}

pub fn mamuta(_info: &TekiInfo) -> Strategy {
    let (stop, damage) = (stop_move(), yai_damage(true));
    let dead_check = dead(DEAD_STATE);
    let reset = reset_anim_speed();
    let dust_small = foot_dust(effects::SMOKE_RING_S, "SE_MIURIN_WALK");
    let dust_big = foot_dust(effects::SMOKE_RING_M, "SE_KABUTO_WALK");
    let watch = watch_navi(ANGRY);
    let sticking = sticking(SHAKE);
    let flick_check = flick_check_hits(SHAKE);
    let satisfied1 = Action::new(IDLE).on_act(|_, _| false).build();
    let satisfied2 = Action::new(SATISFIED).on_act(|_, _| false).build();
    let attackable1 = attackable(POSTURE);
    let attackable3 = attackable(POSTURE_2);

    let mut s = Strategy::new(24, INIT);
    s.set(INIT, vec![init(IDLE)]);
    s.set(DEAD_STATE, vec![reset.clone(), dying_miurin(crate::bulborb::DEAD), stop.clone()]);
    s.set(IDLE, vec![reset.clone(), stop.clone(), check_navi(WATCH), set_target_circle(TURN_TO_TARGET)]);
    s.set(WATCH, vec![
        reset.clone(),
        dust_small.clone(),
        damage.clone(),
        dead_check.clone(),
        watch.clone(),
        sticking.clone(),
        flick_check.clone(),
        turn_focus(PAUSE, WAITACT1, WAITACT1, false),
    ]);
    s.set(TURN_TO_TARGET, vec![
        reset.clone(),
        dust_small.clone(),
        damage.clone(),
        dead_check.clone(),
        watch.clone(),
        sticking.clone(),
        flick_check.clone(),
        turn_to_target(GO_TARGET, WAITACT1, WAITACT1, true),
    ]);
    s.set(GO_TARGET, vec![
        reset.clone(),
        dust_small.clone(),
        damage.clone(),
        dead_check.clone(),
        watch.clone(),
        sticking.clone(),
        flick_check.clone(),
        go_target(PAUSE, MOVE1),
        timer_reaction(IDLE, |_| 5.0),
    ]);
    s.set(PAUSE, vec![
        reset.clone(),
        dust_small.clone(),
        damage.clone(),
        dead_check.clone(),
        watch.clone(),
        sticking.clone(),
        flick_check.clone(),
        reserve_motion(NO_TRANSIT, WAIT1),
        timer_reaction(IDLE, |_| 1.5),
        stop.clone(),
    ]);
    s.set(ANGRY, vec![reset.clone(), dust_big.clone(), damage.clone(), angry(CHASE, WAIT2), stop.clone()]);
    s.set(FACE_NAVI, vec![
        reset.clone(),
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        watch.clone(),
        satisfied1.clone(),
        turn_focus_angry(CHASE, WAITACT1),
        attackable1.clone(),
        sticking.clone(),
        flick_check.clone(),
    ]);
    s.set(CHASE, vec![
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        attackable1.clone(),
        outside_territory_both(SHAKE_HOME),
        sticking.clone(),
        flick_check.clone(),
        satisfied1,
        approach_run(NO_TRANSIT, MOVE1),
    ]);
    s.set(POSTURE, vec![dust_big.clone(), damage.clone(), dead_check.clone(), posture(PUNCH, ATTACK), stop.clone()]);
    s.set(PUNCH, vec![
        reset.clone(),
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        punch(AFTER_PUNCH, TYPE3, TYPE1, TYPE2),
        stop.clone(),
    ]);
    s.set(AFTER_PUNCH, vec![attackable(PUNCH), no_reaction(CALM_CHECK)]);
    s.set(CALM_CHECK, vec![
        reset.clone(),
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        satisfied2.clone(),
        check_flowers(SATISFIED),
        reserve_motion(LOOK_BACK, TYPE4),
        stop.clone(),
    ]);
    s.set(LOOK_BACK, vec![inside_territory_navi(FACE_NAVI), no_reaction(PAUSE)]);
    s.set(SATISFIED, vec![
        reset.clone(),
        dust_small.clone(),
        damage.clone(),
        dead_check.clone(),
        watch,
        relax(IDLE, WAITACT2, RELAX_TIME, true),
        stop.clone(),
    ]);
    s.set(SHAKE, vec![damage.clone(), dead_check.clone(), reset.clone(), dust_big.clone(), shaking(POSTURE, FLICK), stop.clone()]);
    s.set(SHAKE_HOME, vec![reset.clone(), stop.clone(), dust_big.clone(), shaking(POSTURE_2, FLICK)]);
    s.set(FACE_TARGET_2, vec![
        reset.clone(),
        attackable3.clone(),
        turn_focus(NO_TRANSIT, MOVE2, MOVE2, false),
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        unvisible_target(IDLE),
        sticking.clone(),
        flick_check.clone(),
    ]);
    s.set(POSTURE_2, vec![reset.clone(), dust_big.clone(), damage.clone(), dead_check.clone(), posture(PUNCH_2, ATTACK), stop.clone()]);
    s.set(PUNCH_2, vec![
        reset.clone(),
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        punch(AFTER_PUNCH_2, TYPE3, TYPE1, TYPE2),
        stop.clone(),
    ]);
    s.set(AFTER_PUNCH_2, vec![attackable(PUNCH_2), no_reaction(CALM_CHECK_2)]);
    s.set(CALM_CHECK_2, vec![
        reset.clone(),
        dust_big.clone(),
        damage.clone(),
        dead_check.clone(),
        Action::new(SATISFIED).on_act(|_, _| false).build(),
        check_flowers(SATISFIED),
        reserve_motion(IDLE, TYPE4),
        stop.clone(),
    ]);
    s.set(GROGGY, vec![reset, dust_small, damage, dead_check, relax(IDLE, WAITACT2, EYE_ROLL_TIME, false), stop]);
    let _ = (FACE_NAVI, GROGGY, FACE_TARGET_2);
    s
}
