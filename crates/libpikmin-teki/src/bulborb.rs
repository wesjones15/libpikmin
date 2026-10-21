//! The Bulborbs. Dwarf Bulborb and Bulbear: `TaiChappyStrategy` (taichappy.cpp). Spotty Bulborb and
//! Bulbear: `TaiSwallowStrategy` (taiswallow.cpp). State numbers are the decomp's `*STATE_Unk*`.

use crate::anim::key;
use crate::object::*;
use crate::info::TekiInfo;
use crate::tai::*;
use libpikmin_assets::teki::{tpf, tpi};

// TekiMotion (PaniAnimator.h): indices into the .key file.
pub const DEAD: usize = 0;
pub const DAMAGE: usize = 1;
pub const WAIT1: usize = 2;
pub const WAIT2: usize = 3;
pub const WAITACT1: usize = 4;
pub const WAITACT2: usize = 5;
pub const MOVE1: usize = 6;
pub const MOVE2: usize = 7;
pub const ATTACK: usize = 8;
pub const FLICK: usize = 9;
pub const TYPE1: usize = 10;
pub const TYPE2: usize = 11;
pub const TYPE3: usize = 12;
pub const TYPE4: usize = 13;

/// `CHAPPYPI_SmashType` and `CHAPPYPF_*`.
const CHAPPY_SMASH_TYPE: usize = 20;
const CHAPPY_PRESSED: usize = 50;
const CHAPPY_CRYING: usize = 51;
const CHAPPY_SMASH_DAMAGE: usize = 52;

/// `SWALLOWPF_*`.
const SW_NOTICE: usize = 50;
const SW_TURN_COUNT: usize = 53;
const SW_TURN_RATE: usize = 54;
const SW_FLICK_ANGLE: usize = 55;

fn is_spotty(kind: u8) -> bool {
    kind == crate::info::SPOTTY || kind == crate::info::SPOTTY_ORANGE
}

/// `TaiChappyCryAction`: a sleeping Spotty Bulborb within hearing.
fn chappy_cry() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            let range = t.f(tpf::MESSAGE_RANGE);
            w.others.iter().any(|o| o.alive && is_spotty(o.kind) && (o.state == 15 || o.state == 1) && dist_xz(o.pos, t.pos) <= range)
        })
        .build()
}

/// `TaiKeySendMessageAction`: at the cry's action key, wake the sleepers nearby.
fn key_send_message() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                w.out.push(Out::Message { from: t.id, pos: t.pos, range: t.f(tpf::MESSAGE_RANGE) });
            }
            false
        })
        .build()
}

fn chappy_smashed(next: i32) -> Act {
    Action::new(next).on_event(|t, _, e| t.i(CHAPPY_SMASH_TYPE) != 1 && matches!(e, Event::Entity { flying: true, .. })).build()
}

/// `TaiChappyStrategy`.
pub fn chappy(info: &TekiInfo) -> Strategy {
    let danger = info.floats[tpf::DANGER_TERRITORY_RANGE];
    let safety = info.floats[tpf::SAFETY_TERRITORY_RANGE];
    let run = info.floats[tpf::RUN_VELOCITY];
    let turn = info.floats[tpf::TURN_VELOCITY];
    let crying = info.floats[CHAPPY_CRYING];
    let pressed_period = info.floats[CHAPPY_PRESSED];
    let smash = info.floats[CHAPPY_SMASH_DAMAGE];

    let stop = stop_move();
    let stopping = stopping_move(MOVE1);
    let finish_stopping = finish_stopping_move();
    let dead1 = dead(0);
    let pr = pressed(2);
    let sm = chappy_smashed(13);
    let reset = reset_timer(NO_TRANSIT, 0, crying, 0.0);
    let cry = and(5, timer_elapsed(NO_TRANSIT, 0), chappy_cry());
    let outside_danger = outside_territory(12, danger);
    let flick = flick_when_hit(4);
    let attackable = attackable_navi_piki(8);
    let sim_damage = simultaneous_damage(NO_TRANSIT);
    let counter = counterattack_simultaneous_damage(10);
    let start_dying = start_dying();

    let mut s = Strategy::new(16, 15);
    s.set(0, vec![stop.clone(), pr.clone(), start_dying.clone(), dying(DEAD)]);
    s.set(13, vec![stop.clone(), pr.clone(), life_damage(smash), dead(1), once(14)]);
    s.set(14, vec![stop.clone(), sm.clone(), pr.clone(), motion(6, DAMAGE)]);
    s.set(1, vec![stop.clone(), pr.clone(), start_dying.clone(), dying(TYPE1)]);
    s.set(2, vec![stop.clone(), finish_motion(NO_TRANSIT), timer(3, 1, pressed_period, 0.0), life_zero(), being_pressed()]);
    s.set(3, vec![start_dying.clone(), dye()]);
    s.set(4, vec![stop.clone(), dead1.clone(), pr.clone(), sm.clone(), sim_damage.clone(), flicking(RETURN_TRANSIT, FLICK)]);
    s.set(5, vec![
        stopping.clone(), dead1.clone(), pr.clone(), sm.clone(), sim_damage.clone(), key_send_message(),
        continuous_motion(11, TYPE2), reset.clone(),
    ]);
    let outside_safety = outside_territory(12, safety);
    let target_vis1 = target_visible_navi_piki(10);
    let watch = watch_off_territory_center(7);
    let wait = continuous_motion(NO_TRANSIT, WAIT1);
    let sleepy_state = |extra: bool| {
        let mut v = vec![
            stopping.clone(), dead1.clone(), pr.clone(), sm.clone(), attackable.clone(), counter.clone(),
            target_vis1.clone(), watch.clone(), outside_safety.clone(), wait.clone(),
        ];
        if extra {
            v.push(random_set_animation_counter(0, 1));
        }
        v
    };
    s.set(15, sleepy_state(true));
    s.set(6, sleepy_state(false));
    s.set(10, vec![dead1.clone(), pr.clone(), sm.clone(), reset.clone(), once(11)]);
    s.set(7, vec![
        stopping.clone(), dead1.clone(), pr.clone(), sm.clone(), attackable.clone(), counter.clone(), target_vis1.clone(),
        target_nest(), turning_to_target_position(6, WAITACT1, turn),
    ]);
    s.set(11, vec![
        finish_stopping.clone(), tracing(MOVE1, run), dead1.clone(), pr.clone(), sm.clone(), flick.clone(), attackable.clone(),
        counter.clone(), outside_danger, target_visible_navi_piki(NO_TRANSIT), target_lost(12), cry,
    ]);
    s.set(8, vec![
        stop.clone(), dead1.clone(), pr.clone(), sm.clone(), sim_damage.clone(), animation_swallowing(9), action_state(9, DAMAGE as i32),
        motion(RETURN_TRANSIT, ATTACK),
    ]);
    s.set(9, vec![dead1.clone(), pr.clone(), sm.clone(), sim_damage.clone(), switch_motion(11, WAITACT2)]);
    s.set(12, vec![
        finish_stopping, going_home(MOVE1, run), dead1, pr, sm, flick, attackable, counter, inside_territory(6, safety),
    ]);
    s
}

/// `TaiSwallowNoticeAction`: something close enough to wake it.
fn swallow_notice(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let d = t.f(SW_NOTICE);
            match t.closest(w, |c| t.collision_distance(c) <= d) {
                Some(c) => {
                    t.targets[0] = Some(c.target);
                    true
                }
                None => false,
            }
        })
        .build()
}

fn wake_up_call(next: i32) -> Act {
    Action::new(next).on_event(|_, _, e| matches!(e, Event::WakeUp)).build()
}

/// `TaiSwallowTurningAction`: turns slower with many Pikmin hanging on underneath.
fn swallow_turning(next: i32, m: usize, speed: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            // TaiContinuousMotionAction::start
            let cur = t.current_motion();
            if cur < 0 || (cur == m as i32 && (t.anim.is_finished() || t.anim.is_finishing())) || (cur != m as i32 && t.anim.is_finishing()) {
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
            let range = t.f(tpf::LOWER_RANGE);
            let n = t.count_pikis_in_range(w, range) as f32;
            let (max_n, max_rate) = (t.f(SW_TURN_COUNT).max(1e-3), t.f(SW_TURN_RATE));
            let factor = 1.0 + (max_rate - 1.0) * (n / max_n).clamp(0.0, 1.0);
            t.target_angle = t.calc_target_direction(c.pos);
            let a = t.target_angle;
            t.turn_toward(a, speed * factor, w.dt)
        })
        .build()
}

/// `TaiSwallowFlickingAction`: the shake; at its action key flings what is on it and what is near in front.
fn swallow_flicking(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_finish(|t, _| t.set_option(opt::DAMAGE_COUNTABLE))
        .on_act(|t, w| {
            if t.animation_finished() {
                t.set_option(opt::DAMAGE_COUNTABLE);
                return true;
            }
            if t.key(key::ACTION0) {
                t.flick_upper(w);
                let range = t.f(tpf::LOWER_RANGE) + t.info.collision_radius;
                let half = (t.f(SW_FLICK_ANGLE) / 2.0).to_radians();
                let (pos, power, attack) = (t.pos, t.f(tpf::LOWER_FLICK_POWER), t.f(tpf::LOWER_ATTACK_POWER));
                let victims: Vec<_> = t
                    .candidates(w)
                    .into_iter()
                    .filter(|c| !c.sticking && dist_xz(pos, c.pos) - c.size <= range && t.calc_target_angle(c.pos) <= half)
                    .collect();
                for c in victims {
                    match c.target {
                        Target::Piki(_) => {
                            w.sim.flick_pikis_in_sphere(c.pos, 0.5, 0.0, pos);
                        }
                        Target::Navi(id) => w.out.push(Out::HitNavi { teki: t.id, navi: id, damage: attack, from: pos, knock: power }),
                        Target::Pellet(_) => {}
                    }
                }
                t.damage_count = 0.0;
            }
            false
        })
        .build()
}

/// `TaiSwallowStrategy`.
pub fn swallow(info: &TekiInfo) -> Strategy {
    let danger = info.floats[tpf::DANGER_TERRITORY_RANGE];
    let safety = info.floats[tpf::SAFETY_TERRITORY_RANGE];
    let run = info.floats[tpf::RUN_VELOCITY];
    let turn = info.floats[tpf::TURN_VELOCITY];
    let _ = tpi::SWALLOW_COUNT;

    let stop = stop_move();
    let stopping = stopping_move(MOVE2);
    let finish_stopping = finish_stopping_move();
    let dead0 = dead(0);
    let sim_damage = simultaneous_damage(NO_TRANSIT);
    let dmg = damage(3);
    let flick = flick_when_hit(10);
    let vis1 = target_visible_navi_piki(NO_TRANSIT);
    let vis2 = target_visible_navi_piki(7);
    let coll1 = target_nearest_collision(NO_TRANSIT);
    let coll2 = target_nearest_collision(4);
    let assail = target_nearest_assailant(NO_TRANSIT);
    let outside = outside_territory(11, danger);
    let lost = target_lost(5);
    let contact = contact_target(4);
    let sep = separate_target(5);
    let attackable = attackable_navi_piki(9);
    let start = start_dying();

    let mut s = Strategy::new(16, 15);
    s.set(0, vec![stop.clone(), start, dying(DEAD)]);
    let coll3 = target_nearest_collision(2);
    let sleeping = |extra: bool| {
        let mut v = vec![
            dead0.clone(), flick.clone(), assail.clone(), dmg.clone(), coll3.clone(), motion(NO_TRANSIT, FLICK),
            wake_up_call(2), swallow_notice(2),
        ];
        if extra {
            v.push(random_set_animation_counter(0, 1));
        }
        v
    };
    s.set(15, sleeping(true));
    s.set(1, sleeping(false));
    s.set(3, vec![warn(), stop.clone(), dead0.clone(), damaging(4, DAMAGE)]);
    s.set(2, vec![
        stopping.clone(), dead0.clone(), flick.clone(), warn(), assail.clone(), sim_damage.clone(), coll1.clone(), finish_motion(4),
    ]);
    s.set(4, vec![
        stopping.clone(), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(), attackable.clone(), coll1.clone(),
        sep, swallow_turning(5, WAITACT1, turn),
    ]);
    s.set(5, vec![
        stopping.clone(), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(), attackable.clone(), coll2.clone(),
        outside.clone(), vis2.clone(), once(6),
    ]);
    s.set(6, vec![
        stopping.clone(), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(), attackable.clone(), coll2.clone(),
        vis2.clone(), continuous_motion(11, WAIT2),
    ]);
    s.set(7, vec![
        stopping.clone(), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(), attackable.clone(), coll1.clone(),
        contact.clone(), lost.clone(), trace_turning(8, WAITACT1, turn),
    ]);
    s.set(8, vec![
        finish_stopping.clone(), tracing(MOVE2, run), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(),
        attackable.clone(), contact, coll1.clone(), outside, lost, vis1, out_of_trace_angle(7),
    ]);
    s.set(9, vec![
        stopping.clone(), dead0.clone(), assail.clone(), sim_damage.clone(), animation_swallowing(14),
        continuous_motion(5, ATTACK),
    ]);
    s.set(14, vec![dead0.clone(), assail.clone(), sim_damage.clone(), switch_motion(5, WAITACT2)]);
    s.set(10, vec![stop, dead0.clone(), assail.clone(), sim_damage.clone(), swallow_flicking(5, TYPE1)]);
    s.set(11, vec![
        stopping.clone(), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(), attackable.clone(), target_nest(),
        turning_to_target_position(12, WAITACT1, turn),
    ]);
    s.set(12, vec![
        finish_stopping, going_home(MOVE2, run), dead0.clone(), flick.clone(), assail.clone(), sim_damage.clone(),
        attackable.clone(), coll2.clone(), inside_territory(13, safety),
    ]);
    s.set(13, vec![
        stopping, dead0, flick, assail, sim_damage, attackable, coll2, vis2, continuous_motion(1, WAIT2),
    ]);
    s
}
