//! The Breadbug: `TaiCollecStrategy` (taicollec.cpp). It wanders near its nest, goes for a small pellet it
//! can beat the Pikmin for, takes hold of it, carries it to the hole it came from, eats it and
//! goes underground for a while. A Pikmin that kills it, shakes it off or hits it with a thrown
//! Pikmin makes it let go. Ship parts are never taken.
//!
//! Host difference: a held pellet is the sim's `Held` state (no Pikmin tug against the bug), and with no
//! route graph it walks straight to its nest.

use glam::Vec3;
use libpikmin::{PelletId, PelletState};

use crate::anim::key;
use crate::bulborb::{ATTACK, DAMAGE, DEAD, FLICK, MOVE1, MOVE2, TYPE1, TYPE2};
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use libpikmin_assets::teki::tpf;

const CARRY_POWER: usize = 20;
const BOUNCING_DAMAGE: usize = 50;
const PRESSED_DAMAGE: usize = 51;
const UNDERGROUND_DISTANCE: usize = 52;
const UNDERGROUND_PERIOD: usize = 53;
const UNDERGROUND_RANDOM: usize = 54;
const IMPASSABLE_PERIOD: usize = 58;
const IMPASSABLE_DISTANCE: usize = 59;
const TYPE3: usize = 12;

/// How far it roams from its nest when it has no waypoints to follow.
const WANDER_RADIUS: f32 = 260.0;

/// `TekiCollecTargetPelletCondition`: a pellet that is not a ship part, that its carry power wins against
/// the Pikmin on it, that needs no more carriers than that, and that no other Breadbug holds.
fn takes(t: &Teki, w: &World, id: PelletId) -> bool {
    let p = w.sim.pellet(id);
    let power = t.i(CARRY_POWER).max(0) as u32;
    let mine = t.held == Some(id);
    if p.desc.ship_part.is_some() || !(p.state == PelletState::Idle || (mine && p.state == PelletState::Held)) {
        return false;
    }
    if !mine && w.others.iter().any(|o| o.id != t.id && o.holding == Some(id)) {
        return false;
    }
    (p.carriers.len() as u32) < power.max(1) && p.desc.min_carriers <= power
}

fn pellet_cand(t: &Teki, w: &World, id: PelletId) -> Option<Cand> {
    t.cand_of(Target::Pellet(id), w)
}

/// `TaiCollecTargetPelletAction`.
fn target_pellet(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let skip = t.targets_lost;
            let mut best: Option<(f32, PelletId)> = None;
            for p in w.sim.pellets() {
                if Some(Target::Pellet(p.id)) == skip || !takes(t, w, p.id) {
                    continue;
                }
                let Some(c) = pellet_cand(t, w, p.id) else { continue };
                if !t.visible(&c) {
                    continue;
                }
                let d = t.collision_distance(&c);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, p.id));
                }
            }
            match best {
                Some((_, id)) => {
                    t.targets[0] = Some(Target::Pellet(id));
                    true
                }
                None => false,
            }
        })
        .build()
}

/// `TaiCollecVisibleHeightPelletLostAction`: the pellet is too high above it to reach.
fn visible_height_lost(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let Some(c) = t.target(0, w) else { return true };
            if c.pos.y > t.pos.y + t.f(tpf::VISIBLE_HEIGHT) {
                t.targets_lost = t.targets[0];
                t.targets[0] = None;
                return true;
            }
            false
        })
        .build()
}

fn visible_height_lost_timer() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, _| t.timers[1] = 15.0)
        .on_act(|t, _| {
            if t.timer_elapsed(1) {
                t.targets_lost = None;
            }
            false
        })
        .build()
}

/// `TaiCollecPelletLostAction`: no longer one it can take.
fn pellet_lost(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let Some(Target::Pellet(id)) = t.targets[0] else { return true };
            let ok = pellet_cand(t, w, id).is_some_and(|c| t.visible(&c)) && takes(t, w, id);
            if !ok {
                t.targets[0] = None;
            }
            !ok
        })
        .build()
}

/// `TaiCollecPelletDisappearedAction`.
fn pellet_disappeared(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| match t.held {
            None => true,
            Some(id) => {
                if w.sim.pellet(id).state == PelletState::Gone {
                    t.held = None;
                    true
                } else {
                    false
                }
            }
        })
        .build()
}

fn let_go_now(t: &mut Teki, w: &mut World) -> bool {
    if let Some(id) = t.held.take() {
        let p = w.sim.pellet(id);
        let (pos, yaw) = (glam::Vec3::new(p.pos.x, t.pos.y, p.pos.z), p.yaw);
        w.sim.release_pellet(id, pos, yaw);
    }
    true
}

/// `TaiCollecLetGoOfPelletAction`.
fn let_go() -> Act {
    Action::new(NO_TRANSIT).on_act(|t, w| let_go_now(t, w)).on_event(|t, w, _| let_go_now(t, w)).build()
}

/// `TaiCollecHoldPelletAction`: close enough, take hold.
fn hold_pellet(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let Some(Target::Pellet(id)) = t.targets[0] else { return false };
            let Some(c) = pellet_cand(t, w, id) else { return false };
            if t.collision_distance(&c) > 1.0 {
                return false;
            }
            let (pos, yaw) = (c.pos, w.sim.pellet(id).yaw);
            t.targets[0] = None;
            if !takes(t, w, id) || !w.sim.hold_pellet(id, pos, yaw) {
                return false;
            }
            t.held = Some(id);
            true
        })
        .build()
}

/// Where it takes a pellet: its nest, and a way along the way it faces (`outputUndergroundPosition`).
fn underground(t: &Teki) -> Vec3 {
    t.nest + t.forward_at_nest() * t.f(UNDERGROUND_DISTANCE)
}

/// `TaiCollecWinCarryingAction` / `DefeatCarryingAction`: whether it still can hold what it holds.
fn win_carrying() -> Act {
    Action::new(NO_TRANSIT).on_act(|t, w| t.held.is_some_and(|id| takes(t, w, id))).build()
}
fn defeat_carrying(next: i32) -> Act {
    Action::new(next).on_act(|t, w| t.held.is_some_and(|id| !takes(t, w, id))).build()
}

/// `TaiCollecPutAction`: at the hole.
fn put(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            if t.held.is_none() {
                return false;
            }
            let walk = t.f(tpf::WALK_VELOCITY);
            dist_xz(underground(t), t.pos) <= 2.0 * walk * w.dt
        })
        .build()
}

/// `TaiCollecCarryingToNestAction`: walk home with it (straight, with no waypoint graph).
fn carrying_to_nest(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            let cur = t.current_motion();
            if cur < 0 || (cur == m as i32 && t.anim.is_finishing()) || (cur != m as i32 && t.anim.is_finishing()) {
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
            let goal = underground(t);
            let speed = t.f(tpf::WALK_VELOCITY);
            t.move_toward(goal, speed, w.dt)
        })
        .build()
}

/// `TaiCollecPuttingPelletAction`: it lets the pellet go into its mouth and, when the motion is done, eats it.
fn putting_pellet() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            let at = t.pos;
            w.out.push(Out::Effect { id: crate::effects::COLLEC_HM_A, pos: at });
            w.out.push(Out::Effect { id: crate::effects::COLLEC_HM_B, pos: at });
        })
        .on_act(|t, w| {
            let Some(id) = t.held else { return false };
            if t.animation_finished() {
                t.flick_upper(w);
                w.sim.consume_pellet(id);
                t.held = None;
            }
            false
        })
        .build()
}

/// `moveNestPosition`: underground, it is at the nest.
fn move_nest_position() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, _| {
            t.pos = t.nest;
            false
        })
        .build()
}

fn get_out() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                w.out.push(Out::Effect { id: crate::effects::COLLEC_ST_A, pos: t.pos });
                w.out.push(Out::Effect { id: crate::effects::COLLEC_ST_B, pos: t.pos });
            }
            false
        })
        .build()
}

/// `TaiRandomWanderingRouteAction`: ambles between random points near its nest.
fn random_wandering(m: usize, speed: f32) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            let cur = t.current_motion();
            if cur < 0 || t.anim.is_finishing() {
                t.start_motion(m);
            } else if cur != m as i32 {
                t.finish_motion();
            }
            pick_wander_point(t, w);
        })
        .on_act(move |t, w| {
            continuous_act(t, m);
            if !motion_started(t, m) {
                return false;
            }
            let goal = t.wander;
            if t.move_toward(goal, speed, w.dt) {
                pick_wander_point(t, w);
            } else {
                let a = t.calc_target_direction(goal);
                t.turn_toward(a, 2.0 * t.f(tpf::TURN_VELOCITY), w.dt);
            }
            false
        })
        .build()
}

fn pick_wander_point(t: &mut Teki, w: &mut World) {
    let a = w.rng.range(0.0, std::f32::consts::TAU);
    let r = w.rng.range(40.0, WANDER_RADIUS);
    t.wander = t.nest + dir_vec(a) * r;
}

/// `TaiCollecImpassableAction`: stuck for a while, it goes underground and out at the nest.
fn impassable(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            if matches!(t.return_state, 3 | 4 | 6) {
                return;
            }
            t.target_pos = t.pos;
            t.timers[0] = t.f(IMPASSABLE_PERIOD);
        })
        .on_act(|t, _| {
            if dist_xz(t.target_pos, t.pos) >= t.f(IMPASSABLE_DISTANCE) {
                t.target_pos = t.pos;
                t.timers[0] = t.f(IMPASSABLE_PERIOD);
                return false;
            }
            t.timer_elapsed(0)
        })
        .build()
}

/// `TaiCollecStrategy`.
pub fn breadbug(info: &TekiInfo) -> Strategy {
    let walk = info.floats[tpf::WALK_VELOCITY];
    let run = info.floats[tpf::RUN_VELOCITY];
    let pressed = info.floats[PRESSED_DAMAGE];
    let _ = BOUNCING_DAMAGE;
    let under_period = info.floats[UNDERGROUND_PERIOD];
    let under_rand = info.floats[UNDERGROUND_RANDOM];

    let stop = stop_move();
    let dead0 = dead(0);
    let flick2 = flick_when_hit(2);
    let smashed1 = smashed(1);
    let sim_damage = simultaneous_damage(NO_TRANSIT);
    let disappeared = pellet_disappeared(3);
    let impassable8 = impassable(8);
    let lg = let_go();
    let dead_lg = dependence(0, lg.clone(), dead(0));
    let smashed_lg = dependence(1, lg.clone(), smashed(1));
    let flick_lg = dependence(2, lg.clone(), flick_when_hit(2));
    let disappear_lg = dependence(3, lg.clone(), disappeared.clone());
    let impassable_lg = dependence(8, lg.clone(), impassable(8));

    let mut s = Strategy::new(15, 3);
    s.set(0, vec![stop.clone(), start_dying(), dying(DEAD)]);
    s.set(1, vec![stop.clone(), life_damage(pressed), damaging(3, DAMAGE)]);
    s.set(2, vec![stop.clone(), dead0.clone(), sim_damage.clone(), flicking(3, FLICK)]);
    s.set(3, vec![
        dead0.clone(), smashed1.clone(), flick2.clone(), sim_damage.clone(), target_pellet(4), random_wandering(MOVE1, walk),
        impassable8.clone(), visible_height_lost_timer(),
    ]);
    s.set(4, vec![
        dead0.clone(), smashed1.clone(), flick2.clone(), sim_damage.clone(), visible_height_lost(3), pellet_lost(3),
        target_pellet(NO_TRANSIT), tracing(MOVE1, run), impassable8.clone(), hold_pellet(5),
    ]);
    s.set(5, vec![
        stop.clone(), dead_lg.clone(), smashed_lg.clone(), flick_lg.clone(), disappear_lg.clone(), sim_damage.clone(),
        animation_key(6, key::LOOP_START), motion(NO_TRANSIT, MOVE2),
    ]);
    s.set(6, vec![
        dead_lg.clone(), smashed_lg.clone(), flick_lg.clone(), disappear_lg.clone(), sim_damage.clone(), defeat_carrying(7),
        put(8), carrying_to_nest(8, MOVE2), impassable_lg,
    ]);
    s.set(7, vec![
        dead_lg, smashed_lg, flick_lg, disappear_lg, sim_damage.clone(),
        dependence(4, lg, win_carrying()), motion(NO_TRANSIT, TYPE1),
    ]);
    s.set(8, vec![stop.clone(), putting_pellet(), motion(9, TYPE3)]);
    s.set(9, vec![
        move_nest_position(), stop.clone(), set_options(opt::VISIBLE | opt::ATARI | opt::SHADOW_VISIBLE, false),
        timer(10, 1, under_period, under_rand),
    ]);
    s.set(10, vec![
        stop, dead0, motion(3, TYPE2), get_out(), set_options(opt::VISIBLE | opt::ATARI | opt::SHADOW_VISIBLE, true),
    ]);
    let _ = (ATTACK, TYPE2);
    s
}

/// `TaiAnimationKeyAction`: true when the key was passed this tick.
pub fn animation_key(next: i32, bit: u8) -> Act {
    Action::new(next).on_act(move |t, _| t.key(bit)).build()
}
