//! The Smoky Progg (`TAIdororoStrategy`, TAIdororo.cpp) and the Giant Egg it hatches from (`TAItamagoStrategy`,
//! TAItamago.cpp). The egg waits for a player to come near, counts down and bursts. The Progg that comes out
//! ambles toward its goal (the Onion in the game; a host-given point here, else the nearest player), crushing the
//! Pikmin it touches, turning to chase Pikmin it sees, barking when sprouts are near, and shaking off what clings
//! to it.

use glam::Vec3;

use crate::anim::key;
use crate::bulborb::*;
use crate::effects;
use crate::info::{SMOKY_PROGG, TekiInfo};
use crate::object::*;
use crate::tai::*;
use crate::yai::*;
use libpikmin_assets::teki::tpf;

// TAIdororoFloatParams
const PIKI_NUM_FOR_MAX_DRAG: usize = 52;
const WALKING_SPEED_DRAG_FACTOR: usize = 53;
const WAIT_TIME: usize = 54;

// TAIdororoStateID
const DEAD: i32 = 0;
const INIT: i32 = 1;
const BIRTH: i32 = 2;
const GO_GOAL_PATH: i32 = 3;
const FLICK_STATE: i32 = 4;
const WAIT: i32 = 5;
const LOOK_AROUND: i32 = 6;
const GO_TARGET_PIKI: i32 = 7;
const TRANSFORM: i32 = 8;
const BARK_GOAL: i32 = 9;
const BARK: i32 = 10;
const IDLE: i32 = 11;

/// The glowing eyes, body smoke and trail (`setDororoEffect`): the decomp's particle generators follow the body;
/// here the same effects are emitted afresh from the parts a few times a second.
fn aura() -> Act {
    Action::new(NO_TRANSIT)
        .on_act(|t, w| {
            if !t.alive() {
                return false;
            }
            t.reg[5] -= w.dt;
            if t.reg[5] > 0.0 {
                return false;
            }
            t.reg[5] = 0.5;
            t.reg[4] += 1.0;
            for (id, part) in [(effects::DORORO_BODY_SMOKE, "kb01"), (effects::DORORO_BODY_FRAGMENTS, "kb01"), (effects::DORORO_BODY_SPECKS, "kb01"), (effects::DORORO_BODY_TRAIL, "mn01")] {
                if let Some((p, _)) = t.part(part) {
                    w.out.push(Out::Effect { id, pos: p });
                }
            }
            if t.reg[4] as i32 % 2 == 0 {
                for part in ["leye", "reye"] {
                    if let Some((p, _)) = t.part(part) {
                        w.out.push(Out::Effect { id: effects::DORORO_EYE, pos: p });
                    }
                }
            }
            false
        })
        .build()
}

/// `TAIAinitDororo`.
fn init(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            t.coll_radius = 64.0;
            t.clear_option(opt::ATARI);
            t.reg[3] = 0.0;
        })
        .on_act(|_, _| true)
        .build()
}

/// `TAIAgravityDororo`: a very slow sink to the ground.
fn gravity() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| t.reg[2] = w.dt * 0.98)
        .on_act(|t, _| {
            if t.height() > 0.0 {
                t.target_vel.y -= t.reg[2];
            } else {
                t.target_vel.y = 0.0;
            }
            false
        })
        .build()
}

/// `TAIAcheckBarkDororo`: sprouts near it build up the urge to bark.
fn check_bark(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::VISIBLE_RANGE);
            let n = w.sim.sprouts().filter(|s| s.pos.distance(t.pos) < range).count();
            t.reg[3] += w.dt * n as f32;
            if t.reg[3] > 4.0 {
                t.reg[3] = 0.0;
                return true;
            }
            false
        })
        .build()
}

/// `TAIAbarkDororo`: the howl motion for `secs`; at its key a ring of ripples and the sound go out.
fn bark(next: i32, m: usize, secs: f32) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            reserve_start(t, m);
            t.counter = 0.0;
        })
        .on_act(move |t, w| {
            let mut res = false;
            if reserve_act(t, m) {
                t.counter += w.dt;
                if t.counter > secs {
                    t.finish_motion();
                    if t.anim.is_finishing() {
                        res = true;
                    }
                }
            }
            if t.key(key::ACTION0) {
                if let Some((p, _)) = t.part("mn01") {
                    for id in [effects::DORORO_HOWL_ZIGZAGS, effects::DORORO_HOWL_RIPPLES, effects::DORORO_HOWL_HALO] {
                        w.out.push(Out::Effect { id, pos: p });
                    }
                }
                w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
                w.out.push(Out::Sound { name: "SE_DORORO_CRY", pos: t.pos });
            }
            res
        })
        .build()
}

/// `TAIAbirthDororo`: the emerging motion; when it ends the Progg is solid and walks.
fn birth(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, _| {
            if reserve_act(t, m) && t.anim.is_finishing() {
                t.set_option(opt::ATARI);
                t.start_flying();
                t.target_vel = Vec3::ZERO;
                return true;
            }
            false
        })
        .build()
}

/// `TAIAwaitDororo`: stand for between `WAIT_TIME` and twice that.
fn wait_dororo(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            t.stop_move();
            t.counter = 0.0;
            t.reg[0] = t.f(WAIT_TIME) + w.rng.range(0.0, t.f(WAIT_TIME));
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            t.counter += w.dt;
            t.counter > t.reg[0]
        })
        .build()
}

/// `TAIAflickingDororo`.
fn flicking_dororo(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| {
            t.start_motion(m);
            t.stop_move();
            t.clear_option(opt::DAMAGE_COUNTABLE);
        })
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                w.out.push(Out::Sound { name: "SE_DORORO_SWING", pos: t.pos });
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

/// What it goes for: the goal the host named, else the nearest player, else its nest.
fn goal_of(t: &Teki, w: &World) -> Vec3 {
    t.goal.unwrap_or_else(|| {
        w.navis
            .iter()
            .filter(|n| n.alive)
            .min_by(|a, b| dist_xz(a.pos, t.pos).total_cmp(&dist_xz(b.pos, t.pos)))
            .map_or(t.nest, |n| n.pos)
    })
}

/// `TAIAgoGoalPathDororo`: walk to the goal, slowed by the Pikmin clinging on.
fn go_goal_path(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, w| {
            reserve_start(t, m);
            t.target_pos = goal_of(t, w);
        })
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            t.target_pos = goal_of(t, w);
            if dist_xz(t.target_pos, t.pos) < t.f(tpf::DANGER_TERRITORY_RANGE).max(t.coll_radius) {
                return true;
            }
            let n = w.sim.pikis().iter().filter(|p| p.is_alive() && p.state.latched_to() == Some(t.creature)).count() as f32;
            let factor = (n / t.f(PIKI_NUM_FOR_MAX_DRAG).max(1.0)).min(1.0);
            let speed = t.f(tpf::WALK_VELOCITY) * (1.0 - t.f(WALKING_SPEED_DRAG_FACTOR) * factor);
            let goal = t.target_pos;
            t.move_toward_face_priority(goal, speed, w.dt);
            false
        })
        .build()
}

/// `TAIAgoTargetPiki`: chase the Pikmin; done when it is lost or out of sight.
fn go_target_piki(next: i32, m: usize) -> Act {
    Action::new(next)
        .on_start(move |t, _| reserve_start(t, m))
        .on_act(move |t, w| {
            if !reserve_act(t, m) {
                return false;
            }
            let Some(c) = t.target(0, w) else { return true };
            if t.pos.distance(c.pos) >= t.f(tpf::VISIBLE_RANGE) {
                return true;
            }
            let walk = t.f(tpf::WALK_VELOCITY);
            t.move_toward_face_priority(c.pos, walk, w.dt);
            false
        })
        .build()
}

/// `TAIAkillTouchPiki`: its body crushes the Pikmin it touches and hurts a player.
fn kill_touch_piki(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            for part in ["cent", "kb01", "mn01"] {
                let Some((p, r)) = t.part(part) else { continue };
                let range = r * 1.5;
                let ids: Vec<_> = w.sim.pikis().iter().filter(|q| q.is_alive() && q.pos.distance(p) < range).map(|q| q.id).collect();
                for id in ids {
                    w.sim.kill_piki(id);
                }
                for n in w.navis.iter().filter(|n| n.alive && p.distance(n.pos) < range) {
                    w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: t.f(tpf::ATTACK_POWER), from: t.pos, knock: 0.0 });
                }
            }
            false
        })
        .build()
}

/// `TAIAdyingDororo`: it collapses, leaving fragments and fumes; its body lingers a few seconds.
fn dying_dororo(m: usize) -> Act {
    Action::new(NO_TRANSIT)
        .on_start(move |t, w| {
            if let Some((p, _)) = t.part("kb01") {
                w.out.push(Out::Effect { id: effects::DORORO_DEAD_FRAGMENTS, pos: p });
            }
            if let Some((p, _)) = t.part("mn01") {
                w.out.push(Out::Effect { id: effects::DORORO_DEAD_FUMES, pos: p });
            }
            t.start_motion(m);
            t.clear_option(opt::ALIVE);
            t.hp = 0.0;
            t.create_dead_effect(w);
            w.out.push(Out::Dying { teki: t.id });
            t.clear_option(opt::SHADOW_VISIBLE);
            t.counter = 0.0;
        })
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                w.out.push(Out::Sound { name: "SE_DORORO_DEAD", pos: t.pos });
            }
            if t.animation_finished() && t.option(opt::ATARI) {
                w.out.push(Out::Sound { name: "SE_DORORO_CRASH", pos: t.pos });
                t.clear_option(opt::ATARI);
            }
            if !t.option(opt::ATARI) {
                t.counter += w.dt;
                if t.counter > 4.0 && t.animation_finished() {
                    t.die(w);
                }
            }
            true
        })
        .build()
}

pub fn smoky_progg(_info: &TekiInfo) -> Strategy {
    let (stop, damage) = (stop_move(), yai_damage(true));
    let dead_check = dead(DEAD);
    let kill_touch = kill_touch_piki(TRANSFORM);
    let gravity = gravity();
    let eff = aura();
    let flick_check = flick_check(FLICK_STATE, -1);
    let check_bark = check_bark(BARK);

    let mut s = Strategy::new(12, INIT);
    s.set(INIT, vec![init(BIRTH)]);
    s.set(DEAD, vec![dying_dororo(crate::bulborb::DEAD), stop.clone(), eff.clone(), gravity.clone()]);
    s.set(BIRTH, vec![kill_touch.clone(), eff.clone(), birth(GO_GOAL_PATH, TYPE1)]);
    s.set(GO_GOAL_PATH, vec![
        damage.clone(),
        dead_check.clone(),
        check_bark.clone(),
        kill_touch.clone(),
        go_goal_path(BARK_GOAL, MOVE1),
        flick_check.clone(),
        eff.clone(),
        gravity.clone(),
    ]);
    s.set(FLICK_STATE, vec![flicking_dororo(RETURN_TRANSIT, FLICK), eff.clone(), gravity.clone()]);
    s.set(WAIT, vec![
        stop.clone(),
        damage.clone(),
        dead_check.clone(),
        check_bark,
        kill_touch.clone(),
        wait_dororo(LOOK_AROUND, WAIT1),
        flick_check.clone(),
        eff.clone(),
        gravity.clone(),
    ]);
    s.set(LOOK_AROUND, vec![gravity.clone(), visible_piki(GO_TARGET_PIKI), no_reaction(IDLE)]);
    s.set(IDLE, vec![
        gravity.clone(),
        stop.clone(),
        damage.clone(),
        dead_check.clone(),
        kill_touch.clone(),
        reserve_motion(WAIT, TYPE3),
        eff.clone(),
    ]);
    s.set(GO_TARGET_PIKI, vec![
        gravity.clone(),
        damage.clone(),
        dead_check.clone(),
        kill_touch.clone(),
        go_target_piki(GO_GOAL_PATH, MOVE1),
        flick_check,
        eff.clone(),
    ]);
    s.set(TRANSFORM, vec![gravity.clone(), stop.clone(), eff.clone()]);
    s.set(BARK_GOAL, vec![
        gravity.clone(),
        damage.clone(),
        dead_check.clone(),
        kill_touch.clone(),
        bark(WAIT, TYPE2, 2.0),
        eff.clone(),
        stop.clone(),
    ]);
    s.set(BARK, vec![gravity, damage, dead_check, kill_touch, bark(RETURN_TRANSIT, TYPE2, 2.0), eff, stop]);
    s
}

// ---- the Giant Egg

const EGG_DEAD: i32 = 0;
const EGG_WAIT: i32 = 1;
const EGG_COUNT_DOWN: i32 = 2;
const EGG_HATCH: i32 = 3;

const MIN_EMERGENCE_TIME: usize = 50;
const MAX_EMERGENCE_TIME: usize = 51;

/// `TAIAcountStartTamago`: a player is near.
fn count_start(next: i32) -> Act {
    Action::new(next)
        .on_act(|t, w| {
            let range = t.f(tpf::VISIBLE_RANGE);
            w.navis.iter().any(|n| n.alive && dist_xz(n.pos, t.pos) < range)
        })
        .build()
}

/// `TAIAtimerReactionTamago`: a random time between the two emergence times.
fn timer_hatch(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, w| {
            t.counter = 0.0;
            t.reg[0] = t.f(MIN_EMERGENCE_TIME) + w.rng.range(0.0, (t.f(MAX_EMERGENCE_TIME) - t.f(MIN_EMERGENCE_TIME)).max(0.0));
        })
        .on_act(|t, w| {
            t.counter += w.dt;
            t.counter > t.reg[0]
        })
        .build()
}

/// `TAIAhatch`: the egg cracks; at its key a Smoky Progg comes out and whatever is near is knocked back.
fn hatch(next: i32) -> Act {
    Action::new(next)
        .on_start(|t, _| {
            t.start_motion(crate::bulborb::DEAD);
            t.clear_option(opt::STICKABLE);
            t.set_option(opt::INVINCIBLE);
        })
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                let at = t.pos + Vec3::Y * 35.0;
                w.out.push(Out::Effect { id: effects::TAMAGO_DEAD_FRAGMENTS, pos: at });
                w.out.push(Out::Effect { id: effects::TAMAGO_DEAD_SMOKE, pos: at });
                t.flick(w);
                t.clear_option(opt::ALIVE);
                t.clear_option(opt::ATARI);
                w.out.push(Out::Spawn { kind: SMOKY_PROGG, pos: t.pos, yaw: t.yaw, parent: t.id, velocity: Vec3::ZERO });
                w.out.push(Out::Sound { name: "SE_DORORO_EGG_CRASH", pos: t.pos });
                w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
            }
            if t.key(key::FINISHED) {
                t.die(w);
                return true;
            }
            false
        })
        .build()
}

/// `TAIAdyingTamago`: destroyed before it hatches.
fn dying_egg() -> Act {
    Action::new(NO_TRANSIT)
        .on_start(|t, w| {
            t.start_motion(crate::bulborb::DEAD);
            t.clear_option(opt::STICKABLE);
            t.set_option(opt::INVINCIBLE);
            t.clear_option(opt::ALIVE);
            w.out.push(Out::Dying { teki: t.id });
        })
        .on_act(|t, w| {
            if t.key(key::ACTION0) {
                let at = t.pos + Vec3::Y * 35.0;
                for id in [effects::TAMAGO_DEAD_FRAGMENTS, effects::TAMAGO_EYE_GLOW, effects::TAMAGO_DEAD_FUMES] {
                    w.out.push(Out::Effect { id, pos: at });
                }
                w.out.push(Out::Sound { name: "SE_DORORO_EGG_CRASH", pos: t.pos });
            }
            if t.key(key::FINISHED) {
                t.die(w);
                return true;
            }
            false
        })
        .build()
}

pub fn giant_egg(_info: &TekiInfo) -> Strategy {
    let (stop, damage) = (stop_move(), yai_damage(false));
    let dead_check = dead(EGG_DEAD);
    let mut s = Strategy::new(4, EGG_WAIT);
    s.set(EGG_DEAD, vec![dying_egg(), stop]);
    s.set(EGG_WAIT, vec![damage.clone(), dead_check.clone(), count_start(EGG_COUNT_DOWN)]);
    s.set(EGG_COUNT_DOWN, vec![damage, dead_check, timer_hatch(EGG_HATCH)]);
    s.set(EGG_HATCH, vec![hatch(NO_TRANSIT)]);
    s
}
