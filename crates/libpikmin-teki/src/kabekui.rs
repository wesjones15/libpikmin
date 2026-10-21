//! The Sheargrubs: `TAIkabekuiAStrategy` (the Female Sheargrub, which only wanders and eats), `TAIkabekuiBStrategy`
//! (the Male, which also bites) and `TAIkabekuiCStrategy` (the Shearwig, which takes to the air when hurt), in
//! plugPikiYamashita/TAIkabekui{A,B,C}.cpp. They sleep buried until a player comes near, rise, wander within
//! their territory, bite what they see, and dig in again after a while. Water kills them. The bridge-eating
//! states (`ChasingSetup`, `Chasing`, `EatingBridge`) belong to the stage's bridges; a host with none leaves
//! them empty, but their numbers are kept.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::TekiInfo;
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::tpf;

/// The float parameters that differ between the three (`TAIkabekuiAFloatParms`, ...).
struct Parms {
    time_until_burrow: usize,
    max_sleep_time: usize,
    piki_attack_range: usize,
    navi_attack_range: usize,
}

const A: Parms = Parms { time_until_burrow: 52, max_sleep_time: 53, piki_attack_range: 0, navi_attack_range: 0 };
const B: Parms = Parms { time_until_burrow: 52, max_sleep_time: 53, piki_attack_range: 54, navi_attack_range: 55 };
const C: Parms = Parms { time_until_burrow: 56, max_sleep_time: 57, piki_attack_range: 58, navi_attack_range: 59 };

// TAIkabekuiCFloatParms
const FLIGHT_ALERT_LIFE: usize = 52;
const MAX_FLIGHT_LIFE: usize = 53;
const FLIGHT_Y_VELOCITY: usize = 54;
const TAKEOFF_Y_VELOCITY: usize = 55;

/// `TAIAappearKabekui`: rise (or sink) with the dirt flying. At the action key it is up (the stay switch is on,
/// so it no longer sinks into the ground); a rising one stays drawn, a burrowing one is hidden at the end.
fn appear(next: i32, m: usize, keep_visible: bool) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.manual_speed = Some(30.0);
            t.flags[0] = false;
            if keep_visible {
                t.set_option(opt::VISIBLE);
            }
        })
        .on_act(move |t, w| {
            if t.key(key::ACTION0) {
                t.flags[0] = true;
                w.out.push(Out::Sound { name: "SE_WALLEAT_APPEAR", pos: t.pos });
            }
            if t.flags[0] && (t.anim.counter.round() as i32) % 7 == 0 {
                let at = t.part("cent").map_or(t.pos, |(p, _)| p);
                let at = Vec3::new(at.x, t.ground_y.unwrap_or(t.pos.y), at.z);
                w.out.push(Out::Effect { id: effects::DIRT_CLOUD, pos: at });
                w.out.push(Out::Effect { id: effects::DIRT_SPRAY, pos: at });
            }
            if t.key(key::FINISHED) {
                t.manual_speed = None;
                if !keep_visible {
                    t.clear_option(opt::VISIBLE);
                }
                return true;
            }
            false
        })
        .build()
}

/// `TAIAbiteForKabekui`: the bite. Between the first two action keys a Pikmin within `piki_range` of the mouth
/// is taken in, and a player is hurt; when it finishes it chews (state `chew`) if something is in its mouth, else
/// goes back to wandering (`miss`).
fn bite(chew: i32, miss: i32, m: usize, p: &'static Parms) -> Act {
    Action::new(chew)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.flags[1] = false;
            t.flags[2] = false;
            t.held_piki = false;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::FINISHED) {
                if !t.held_piki {
                    t.next_override = Some(miss);
                }
                return true;
            }
            if t.key(key::ACTION0) {
                t.flags[1] = true;
            } else if t.key(key::ACTION1) {
                t.flags[1] = false;
            }
            if t.flags[1] && !t.held_piki {
                let Some((mouth, _)) = t.part("slot") else { return false };
                if w.sim.swallow_pikis(t.creature, 0, mouth, t.f(p.piki_attack_range), 1) > 0 {
                    t.held_piki = true;
                } else if !t.flags[2] {
                    let reach = t.f(p.navi_attack_range);
                    let power = t.f(tpf::ATTACK_POWER);
                    for n in w.navis.iter().filter(|n| n.alive && mouth.distance(n.pos) < reach + n.size) {
                        w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: power, from: t.pos, knock: 0.0 });
                        t.flags[2] = true;
                    }
                }
            }
            false
        })
        .on_finish(|t, w| {
            if t.held_piki && !t.alive() {
                w.sim.spit_out(t.creature);
            }
        })
        .build()
}

/// `TAIAeatPiki`: at the action key the Pikmin in its mouth is swallowed.
fn eat_piki(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if reserve_act(t, m) && t.key(key::ACTION0) {
                w.out.push(Out::Sound { name: "SE_WALLEAT_NIP", pos: t.pos });
                kill_mouth_pikis(t, w);
                return true;
            }
            false
        })
        .build()
}

/// `TAIAlessLife` / `TAIAmoreLife`: its life as a fraction of the full life against a parameter.
fn life_below(next: i32, param: usize) -> Act {
    Action::new(next).on_act(move |t, _| t.hp / t.f(tpf::LIFE).max(1.0) < t.f(param)).build()
}
fn life_above(next: i32, param: usize) -> Act {
    Action::new(next).on_act(move |t, _| t.hp / t.f(tpf::LIFE).max(1.0) > t.f(param)).build()
}

/// `TAIAtakeOffKabekuiC`: jump into the air with a hop whose height is the flight height.
fn take_off(next: i32, m: usize) -> Act {
    const SPEED: f32 = 0.7;
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.start_flying();
            t.target_vel = Vec3::ZERO;
            t.reg[0] = t.f(tpf::FLIGHT_HEIGHT) * 2.0 / (SPEED * SPEED);
            t.flags[3] = false;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.key(key::ACTION0) {
                let walk = t.f(tpf::WALK_VELOCITY);
                t.flags[3] = true;
                t.target_vel = Vec3::new(walk * t.yaw.sin(), t.reg[0] * SPEED, walk * t.yaw.cos());
                if t.in_water {
                    w.out.push(Out::Effect { id: effects::BUBBLES, pos: t.pos });
                }
            }
            if t.flags[3] {
                t.target_vel.y -= t.reg[0] * w.dt;
            }
            if t.key(key::FINISHED) {
                t.target_vel.y *= 0.1;
                return true;
            }
            false
        })
        .build()
}

/// `TAIAflyingMotionKabekuiC`: hover, with the sound.
fn flying_motion(m: usize) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            reserve_start(t, m);
            w.out.push(Out::Sound { name: "SE_SARAI_HOVER", pos: t.pos });
        })
        .on_act(move |t, _| {
            reserve_act(t, m);
            false
        })
        .build()
}

/// `TAIAlandingKabekuiC`: sink to the ground, dropping the last bit with a puff of smoke.
fn landing(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.start_flying();
            t.flags[3] = true;
            t.reg[0] = 0.0;
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            if t.flags[3] {
                t.reg[0] = (t.target_vel.y - t.height()) * 2.0;
                let walk = t.f(tpf::WALK_VELOCITY);
                t.target_vel = Vec3::new(walk * t.yaw.sin(), t.target_vel.y, walk * t.yaw.cos());
                t.flags[3] = false;
            }
            if t.flying {
                if t.height() < 15.0 {
                    let at = Vec3::new(t.pos.x, t.ground_y.unwrap_or(t.pos.y), t.pos.z);
                    w.out.push(Out::Effect { id: effects::SMOKE_RING_S, pos: at });
                    t.finish_flying();
                    t.target_vel.x *= 0.15;
                    t.target_vel.z *= 0.15;
                }
                t.target_vel.y += t.reg[0] * w.dt;
            }
            if t.key(key::FINISHED) {
                t.finish_flying();
                t.target_vel.y = 0.0;
                return true;
            }
            false
        })
        .build()
}

fn show(on: bool) -> Act {
    set_option(opt::SHADOW_VISIBLE, on)
}

fn invincible(on: bool) -> Act {
    set_option(opt::INVINCIBLE, on)
}

/// The Female Sheargrub (`TAIkabekuiAStrategy`).
pub fn female(_info: &TekiInfo) -> Strategy {
    let (dead_check, damage, stop) = (dead(0), yai_damage(true), stop_move());
    let water = in_water_damage(20.0, true);
    let press = hit_check_flying_piki(5);
    let mut s = Strategy::new(11, 1);
    s.set(0, vec![dying_with_dust(DEAD, effects::SMOKE_RING_S), stop.clone()]);
    s.set(1, vec![show(false), invincible(true), set_motion_speed(NO_TRANSIT, WAITACT1, 0.0), visible_navi(2)]);
    s.set(2, vec![show(true), appear(3, WAITACT1, true)]);
    s.set(3, vec![invincible(false), set_target_circle_random(4)]);
    s.set(4, vec![
        water,
        press,
        damage,
        dead_check,
        go_target_face(3, MOVE1),
        timer_reaction(9, |t| t.f(A.time_until_burrow)),
    ]);
    s.set(5, vec![dying_with_dust(DAMAGE, effects::SMOKE_RING_S), stop.clone()]);
    s.set(9, vec![invincible(true), appear(10, WAITACT2, false), stop]);
    s.set(10, vec![show(false), timer_reaction(1, |t| t.f(A.max_sleep_time))]);
    s
}

/// The Male Sheargrub (`TAIkabekuiBStrategy`).
pub fn male(_info: &TekiInfo) -> Strategy {
    let (dead_check, damage, stop) = (dead(0), yai_damage(true), stop_move());
    let water = in_water_damage(20.0, true);
    let press = hit_check_flying_piki(8);
    let mut s = Strategy::new(14, 1);
    s.set(0, vec![dying_with_dust(DEAD, effects::SMOKE_RING_S), stop.clone()]);
    s.set(1, vec![show(false), invincible(true), set_motion_speed(NO_TRANSIT, WAITACT1, 0.0), visible_navi(2)]);
    s.set(2, vec![show(true), appear(3, WAITACT1, true)]);
    s.set(3, vec![invincible(false), set_target_circle_random(4)]);
    s.set(4, vec![
        water.clone(),
        press.clone(),
        damage.clone(),
        dead_check.clone(),
        go_target_face(3, MOVE1),
        visible_piki(5),
        visible_navi(5),
        timer_reaction(12, |t| t.f(B.time_until_burrow)),
    ]);
    s.set(5, vec![
        water.clone(),
        press.clone(),
        damage,
        dead_check,
        approach_target_face(NO_TRANSIT, MOVE1),
        attackable_target(6, true),
        unvisible_target(3),
        visible_piki(5),
        visible_navi(5),
    ]);
    s.set(6, vec![water.clone(), press.clone(), bite(7, 3, TYPE1, &B)]);
    s.set(7, vec![water, stop.clone(), press, eat_piki(3, TYPE2)]);
    s.set(8, vec![dying_with_dust(DAMAGE, effects::SMOKE_RING_S), stop.clone()]);
    s.set(12, vec![invincible(true), appear(13, WAITACT2, false), stop]);
    s.set(13, vec![show(false), timer_reaction(1, |t| t.f(B.max_sleep_time))]);
    s
}

/// The Shearwig (`TAIkabekuiCStrategy`): as the Male, but badly hurt it takes off and hovers around its nest until
/// its life has come back.
pub fn shearwig(_info: &TekiInfo) -> Strategy {
    let (dead_check, damage, stop) = (dead(0), yai_damage(true), stop_move());
    let water = in_water_damage(20.0, true);
    let press = hit_check_flying_piki(11);
    let alert = life_below(8, FLIGHT_ALERT_LIFE);
    let mut s = Strategy::new(17, 1);
    s.set(0, vec![dying_with_dust(DEAD, effects::SMOKE_RING_S), stop.clone()]);
    s.set(1, vec![show(false), invincible(true), set_motion_speed(NO_TRANSIT, WAITACT1, 0.0), visible_navi(2)]);
    s.set(2, vec![show(true), appear(3, WAITACT1, true)]);
    s.set(3, vec![invincible(false), set_target_circle_random(4)]);
    s.set(4, vec![
        water.clone(),
        press.clone(),
        damage.clone(),
        dead_check.clone(),
        alert.clone(),
        go_target_face(3, MOVE1),
        visible_piki(5),
        visible_navi(5),
        timer_reaction(15, |t| t.f(C.time_until_burrow)),
    ]);
    s.set(5, vec![
        water.clone(),
        press.clone(),
        damage,
        dead_check.clone(),
        alert,
        approach_target_face(NO_TRANSIT, MOVE1),
        attackable_target(6, true),
        unvisible_target(3),
        visible_piki(5),
        visible_navi(5),
    ]);
    s.set(6, vec![water.clone(), press.clone(), bite(7, 3, TYPE1, &C)]);
    s.set(7, vec![water.clone(), stop.clone(), press, eat_piki(3, TYPE2)]);
    s.set(8, vec![dead_check.clone(), take_off(9, TYPE3), water.clone()]);
    s.set(9, vec![
        dead_check.clone(),
        life_above(10, MAX_FLIGHT_LIFE),
        flying_base(|t| if t.reg[6] > 0.5 { t.f(FLIGHT_Y_VELOCITY) } else { t.f(TAKEOFF_Y_VELOCITY) }),
        flying_in_territory(0.25),
        flying_motion(WAIT2),
        water.clone(),
    ]);
    s.set(10, vec![dead_check, landing(3, TYPE4), water]);
    s.set(11, vec![dying_with_dust(DAMAGE, effects::SMOKE_RING_S), stop.clone()]);
    s.set(15, vec![invincible(true), appear(16, WAITACT2, false), stop]);
    s.set(16, vec![show(false), timer_reaction(1, |t| t.f(C.max_sleep_time))]);
    s
}
