//! What the Long Legs bosses of Pikmin 2 share: Beady Long Legs (`Damagumo::Obj`, plugProjectNishimuraU/Damagumo.cpp,
//! DamagumoState.cpp), Raging Long Legs (`BigFoot::Obj`, BigFoot.cpp, BigFootState.cpp) and Man-at-Legs (`Houdai::Obj`).
//! Each is a body on four legs that cannot be hurt until it lands: it waits out of sight above its place (`Stay`), drops
//! when someone comes near (`Land`), and then alternates standing (`Wait`) with walking on its four legs (`Walk`, the gait
//! of [`crate::legs`]); the Pikmin clinging to it make it shake them off (`Flick`), and killed it falls apart (`Dead`).
//! Its damage is its feet: a foot coming down crushes what is under it (`collisionCallback`: `InteractPress` while the
//! leg is swinging down, `IKSystemMgr::isCollisionCheck`).
//!
//! States: [`DEAD`], [`STAY`], [`LAND`], [`WAIT`], [`FLICK`], [`WALK`] (the Man-at-Legs adds [`crate::houdai::SHOT`]).
//! Clips (`enemyAnimMgr.txt`): [`ANIM_DEAD`], [`ANIM_LANDING`], [`ANIM_WAIT`], [`ANIM_FLICK`] (and the Man-at-Legs' attack).
//! Registers: `reg[0]` the state timer (seconds), `reg[1]` how long the state lasts, `reg[2]` the state to go to once the
//! clip finishes (`mNextState`, -1 for none), `reg[3]` Raging Long Legs' walk time; `flags[0]` Raging Long Legs is enraged.
//!
//! Host adaptations: it lands when anyone is within its sight radius (the decomp's private radius is 70 and puts it
//! under the first thing that walks below it); it also goes after players (the decomp goes only for Pikmin); the foot
//! spheres are read from `enemyColl.txt`; the clip key that plays the last foot's landing (key type 6) is lost, as every
//! kind's keys above type 5 are; effects, the pinch smoke and the boss music are the host's. The shadow growing as it
//! lands, the material animation of the dying body and the Shijimichou it releases when it dies are left out.

use std::sync::Arc;

use glam::Vec3;

use crate::anim::key;
use crate::legs::{LegEvent, Legs};
use crate::object::*;
use crate::p2::fp;
use crate::tai::*;

pub const DEAD: i32 = 0;
pub const STAY: i32 = 1;
pub const LAND: i32 = 2;
pub const WAIT: i32 = 3;
pub const FLICK: i32 = 4;
pub const WALK: i32 = 5;

pub const ANIM_DEAD: usize = 0;
pub const ANIM_LANDING: usize = 1;
pub const ANIM_WAIT: usize = 2;
pub const ANIM_FLICK: usize = 3;

pub const TIMER: usize = 0;
pub const DURATION: usize = 1;
pub const NEXT: usize = 2;
pub const WALK_TIME: usize = 3;

// General parms (`EnemyParmsBase`).
const TERRITORY: usize = fp::TERRITORY;
const HOME_RADIUS: usize = 10;
const SIGHT_RADIUS: usize = fp::SIGHT_RADIUS;
const VIEW_ANGLE: usize = fp::VIEW_ANGLE;
const SHAKE_CHANCE: usize = 16;

// The legs' own parms (`ProperParms`, fp01..fp06).
const BASE: usize = fp::own::BASE + 1;
const RAISE: usize = fp::own::BASE + 2;
const DOWN: usize = fp::own::BASE + 3;
const MIN_DECEL: usize = fp::own::BASE + 4;
const MAX_DECEL: usize = fp::own::BASE + 5;
const SWING: usize = fp::own::BASE + 6;

/// Which of the two bosses with this machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Variant {
    Damagumo,
    BigFoot,
    Houdai,
}

/// Every Long Legs kind: the ones that have legs.
pub fn is_long_legs(kind: u8) -> bool {
    matches!(kind, crate::p2::BEADY_LONG_LEGS | crate::p2::RAGING_LONG_LEGS | crate::p2::MAN_AT_LEGS)
}

pub fn legs(t: &mut Teki) -> &mut Legs {
    t.legs.as_mut().expect("a Long Legs enemy has its legs").as_mut()
}

/// Applies the damage a hit stored (the decomp's `addDamage` lowers its health at once).
pub fn hurt(t: &mut Teki) {
    if t.stored_damage > 0.0 {
        t.make_damaged();
    }
}

/// `EnemyFunc::isStartFlick(this, false)`: enough hits taken for the number of Pikmin clinging to it.
pub fn start_flick(t: &Teki, w: &World) -> bool {
    crate::king::start_flick(t, w)
}

/// `IKSystemParms` from the enemy's own parms (`setIKParameter`): `enraged` picks Raging Long Legs' second set.
pub fn set_gait(t: &mut Teki, variant: Variant, enraged: bool) {
    let off = if variant == Variant::BigFoot && enraged { 10 } else { 0 };
    let (base, raise, down, min, max, swing) = (t.f(BASE + off), t.f(RAISE + off), t.f(DOWN + off), t.f(MIN_DECEL + off), t.f(MAX_DECEL + off), t.f(SWING + off));
    let (turn, speed) = (t.f(fp::MAX_TURN_ANGLE), t.f(fp::MOVE_SPEED));
    let p = &mut legs(t).parms;
    p.max_turn_angle = turn;
    p.move_speed = speed;
    p.bottom_move_speed = base;
    p.raise_slowdown = raise;
    p.downward_accel = down;
    p.max_decel = max;
    p.min_decel = min;
    p.height_offset = swing;
    match variant {
        Variant::Damagumo | Variant::Houdai => p.bend_factor = 0.67,
        Variant::BigFoot => p.bend_factor = 0.5,
    }
}

/// Anyone within `radius` of the enemy: a player or a Pikmin (`EnemyFunc::isThereOlimar`, `isTherePikmin`).
pub fn someone_near(t: &Teki, w: &World, radius: f32) -> bool {
    w.navis.iter().any(|n| n.alive && dist_xz(t.pos, n.pos) - n.size <= radius)
        || w.sim.pikis().iter().any(|p| p.is_alive() && dist_xz(t.pos, p.pos) - p.size <= radius)
}

/// Draws from the world's source (`randWeightFloat(1)`).
pub fn rand(w: &mut World) -> f32 {
    w.rng.f32()
}

/// The sound a foot landing makes (`createOnGroundEffect`).
fn foot_sound(variant: Variant) -> &'static str {
    match variant {
        Variant::Damagumo => "SPIDER_WALK",
        Variant::BigFoot => "BIGFOOT_WALK",
        Variant::Houdai => "HOUDAI_METALFOOT",
    }
}

/// What the gait did this tick, as sounds and a shake (`createOnGroundEffect`).
pub fn play_legs(t: &Teki, w: &mut World, events: &[LegEvent], sound: &'static str, shake: f32) {
    let Some(l) = t.legs.as_ref() else { return };
    for e in events {
        if let LegEvent::Landed(i) = e {
            let at = l.legs[*i].foot;
            w.out.push(Out::Sound { name: sound, pos: at });
            w.out.push(Out::Shake { pos: at, strength: shake });
            if matches!(t.kind,crate::p2::BEADY_LONG_LEGS|crate::p2::RAGING_LONG_LEGS) {
                let ids:&[u16]=if t.in_water {&[0x28,0x29,0x2a]}else if t.kind==crate::p2::RAGING_LONG_LEGS {&[0xf5,0xf6]}else{&[0x2b,0x2c]};
                for &id in ids {w.out.push(Out::Effect {id:0x8000|id as usize,pos:at});}
                for layer in 0..2 {w.out.push(Out::StopEffect {key:t.id.wrapping_mul(64)+16+(*i as u32)*2+layer});}
            }
        }
    }
}

/// The feet crush what they come down on (`collisionCallback`: `InteractPress` on players and Pikmin whose feet
/// are on the ground, while `isCollisionCheck` holds for the foot sphere).
pub fn stomp(t: &mut Teki, w: &mut World) {
    let damage = t.f(fp::ATTACK_DAMAGE);
    let Some(l) = t.legs.as_mut() else { return };
    let radius = l.foot_radius();
    if radius <= 0.0 {
        return;
    }
    for i in 0..crate::legs::LEGS {
        if !l.stomping(i) || l.stomped[i] {
            continue;
        }
        let at = l.foot_sphere(i);
        let mut hit = w.sim.attack_pikis_in_sphere(at, radius, 1000.0) > 0;
        for n in w.navis.iter().filter(|n| n.alive && (n.pos - at).length() - n.size <= radius) {
            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage: damage * 6.0, from: at, knock: 60.0 });
            hit = true;
        }
        l.stomped[i] = hit;
    }
}

/// `EnemyFunc::flickStickPikmin(this, chance, ..)`: shakes the Pikmin off, if the dice say so, and starts counting the hits again.
pub fn shake_off(t: &mut Teki, w: &mut World) {
    if w.rng.occurred(t.f(SHAKE_CHANCE)) {
        w.sim.shake_off(t.creature);
    }
    t.damage_count = 0.0;
}

/// The start every state of the family runs: the enemy's options and registers.
fn on_start(variant: Variant) -> Arc<dyn Fn(&mut Teki) + Send + Sync> {
    Arc::new(move |t: &mut Teki| {
        t.reg = [0.0; 8];
        t.reg[NEXT] = -1.0;
        t.flags = [false; 4];
        t.target_pos = t.nest;
        t.clear_option(opt::GRAVITATABLE);
        t.set_option(opt::DAMAGE_COUNTABLE);
        let (pos, yaw) = (t.pos, t.yaw);
        t.legs = t.legs.take().map(|l| Box::new(Legs::new(l.rig.clone(), pos, yaw)));
        set_gait(t, variant, false);
    })
}

fn next_state(t: &Teki) -> Option<i32> {
    (t.reg[NEXT] >= 0.0).then_some(t.reg[NEXT] as i32)
}

/// A state that waits for the clip to finish then goes where `reg[2]` says (`mNextState`).
fn go_next(t: &mut Teki) -> bool {
    if t.animation_finished()
        && let Some(n) = next_state(t)
    {
        t.next_override = Some(n);
        return true;
    }
    false
}

/// The checks Damagumo's and BigFoot's standing states (`Wait`, `Land`) run: dead, flick, or go on.
fn after_land(t: &mut Teki, w: &World) -> i32 {
    if t.hp <= 0.0 {
        DEAD
    } else if start_flick(t, w) {
        FLICK
    } else {
        WAIT
    }
}

pub fn strategy(variant: Variant) -> Strategy {
    let mut s = Strategy::new(6, STAY);
    s.on_start = Some(on_start(variant));
    let sound = foot_sound(variant);
    let shake = if variant == Variant::BigFoot { 1.5 } else { 1.0 };

    // StateStay: out of sight above its place until someone comes near.
    s.set(
        STAY,
        vec![Action::new(LAND)
            .on_start(|t, _| {
                t.reg[NEXT] = -1.0;
                t.target_vel = Vec3::ZERO;
                t.clear_option(opt::VISIBLE | opt::ATARI);
                t.start_motion(ANIM_LANDING);
                t.manual_speed = Some(0.0);
            })
            .on_act(|t, w| {
                hurt(t);
                let r = t.f(SIGHT_RADIUS);
                someone_near(t, w, r)
            })
            .on_finish(|t, _| {
                t.manual_speed = None;
                t.set_option(opt::VISIBLE | opt::ATARI);
            })
            .build()],
    );

    // StateLand: drops from above; the feet come down at the clip's keys.
    s.set(
        LAND,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[NEXT] = -1.0;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_LANDING);
            })
            .on_act(move |t, w| {
                hurt(t);
                let at = t.pos;
                if t.key(key::ACTION0) {
                    w.out.push(Out::Sound { name: sound, pos: at });
                    w.out.push(Out::Shake { pos: at, strength: shake * 2.0 });
                }
                for k in [key::ACTION1, key::ACTION2, key::ACTION3] {
                    if t.key(k) {
                        w.out.push(Out::Sound { name: sound, pos: at });
                    }
                }
                if t.animation_finished() {
                    t.next_override = Some(after_land(t, w));
                    return true;
                }
                false
            })
            .on_finish(|t, _| {
                // `startProgramedIK`: the legs stand on the feet the landing left.
                let (pos, yaw) = (t.pos, t.yaw);
                legs(t).start_programmed(pos, yaw);
            })
            .build()],
    );

    // StateWait: stands, then walks (or flicks, or dies).
    s.set(
        WAIT,
        vec![Action::new(WALK)
            .on_start(move |t, w| {
                t.reg[TIMER] = 0.0;
                t.reg[NEXT] = -1.0;
                t.reg[DURATION] = match variant {
                    Variant::Damagumo => 1.75 + 1.75 * rand(w),
                    Variant::Houdai => 1.5 + 1.5 * rand(w),
                    Variant::BigFoot => 5.0,
                };
                if variant == Variant::BigFoot {
                    // `resetFlickWalkTimeMax`, `setIKParameter`.
                    let travel = t.f(fp::own::BASE + 20);
                    t.reg[WALK_TIME] = travel * 0.5 + travel * rand(w);
                    t.flags[0] = false;
                    set_gait(t, variant, false);
                }
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_WAIT);
            })
            .on_act(|t, w| {
                hurt(t);
                t.reg[TIMER] += w.dt;
                if t.hp <= 0.0 {
                    t.reg[NEXT] = DEAD as f32;
                    t.finish_motion();
                } else if start_flick(t, w) {
                    t.reg[NEXT] = FLICK as f32;
                    t.finish_motion();
                } else if t.reg[TIMER] > t.reg[DURATION] {
                    t.reg[NEXT] = WALK as f32;
                    t.finish_motion();
                }
                go_next(t)
            })
            .build()],
    );

    // StateFlick: shakes the Pikmin off with its legs blended into the animation.
    s.set(
        FLICK,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.reg[NEXT] = -1.0;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
                legs(t).set_blend(true);
            })
            .on_act(|t, w| {
                hurt(t);
                if t.key(key::ACTION0) {
                    shake_off(t, w);
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { WALK });
                    return true;
                }
                false
            })
            .on_finish(move |t, w| {
                legs(t).set_blend(false);
                if variant == Variant::BigFoot {
                    // `setFlickWalkTimeMax`: after being shaken it rages: a short, wild walk with its second set of numbers.
                    let travel = t.f(fp::own::BASE + 21);
                    t.reg[WALK_TIME] = travel * 0.5 + travel * rand(w);
                    t.flags[0] = true;
                    set_gait(t, variant, true);
                }
            })
            .build()],
    );

    // StateWalk: its legs take it after the goal.
    s.set(
        WALK,
        vec![Action::new(WAIT)
            .on_start(move |t, w| {
                t.reg[TIMER] = 0.0;
                t.reg[NEXT] = -1.0;
                t.reg[DURATION] = match variant {
                    Variant::Damagumo => 3.25 + 3.25 * rand(w),
                    Variant::Houdai => 3.5 + 3.5 * rand(w),
                    Variant::BigFoot => t.reg[WALK_TIME],
                };
                t.target_vel = Vec3::ZERO;
                legs(t).start_motion();
                target(t, w, variant);
            })
            .on_act(move |t, w| {
                hurt(t);
                target(t, w, variant);
                t.reg[TIMER] += w.dt;
                if t.hp <= 0.0 {
                    t.next_override = Some(DEAD);
                    return true;
                }
                if start_flick(t, w) {
                    t.reg[NEXT] = FLICK as f32;
                    legs(t).finish_motion();
                } else if t.reg[TIMER] > t.reg[DURATION] {
                    t.reg[NEXT] = WAIT as f32;
                    legs(t).finish_motion();
                }
                if legs(t).finished()
                    && let Some(n) = next_state(t)
                {
                    t.next_override = Some(n);
                    return true;
                }
                false
            })
            .on_finish(move |t, _| {
                if variant == Variant::BigFoot {
                    t.flags[0] = false;
                }
            })
            .build()],
    );

    // StateDead: the legs stop, the body falls apart.
    s.set(
        DEAD,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, w| {
                legs(t).force_finish();
                t.clear_option(opt::ATARI);
                t.clear_option(opt::INVINCIBLE);
                t.target_vel = Vec3::ZERO;
                w.out.push(Out::Dying { teki: t.id });
                w.out.push(Out::Shake { pos: t.pos, strength: 3.0 });
                t.start_motion(ANIM_DEAD);
            })
            .on_act(|t, w| {
                if t.key(key::ACTION0) {
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                if t.animation_finished() {
                    t.die(w);
                }
                false
            })
            .build()],
    );
    s
}

/// `Obj::getTargetPosition` for the variant: Raging Long Legs, enraged, stomps about where it stands.
fn target(t: &mut Teki, w: &mut World, variant: Variant) {
    if variant == Variant::BigFoot && t.flags[0] && dist_xz(t.pos, t.nest) < t.f(TERRITORY) {
        // A random direction within a wide arc of where it faces, `movementOffset` away (`(rand(2a) - a) * DEG2RAD * PI`).
        let a = t.f(fp::own::BASE + 17);
        let spread = (rand(w) * 60.0 - 30.0) * std::f32::consts::PI / 180.0 * std::f32::consts::PI;
        let heading = t.yaw + spread;
        t.target_pos = Vec3::new(a * heading.sin() + t.pos.x, t.pos.y, a * heading.cos() + t.pos.z);
        let goal = t.target_pos;
        legs(t).set_goal(goal);
        return;
    }
    calm(t, w, variant);
}

/// `Obj::getTargetPosition` (calm): the nearest player or Pikmin in sight, else a point wandered to in the territory.
fn calm(t: &mut Teki, w: &mut World, variant: Variant) {
    // Draw the two randoms first, so the target code needs only the world's reading side.
    let draws = [rand(w), rand(w)];
    calm_target_with(t, w, variant, draws);
}

fn calm_target_with(t: &mut Teki, w: &World, variant: Variant, draws: [f32; 2]) {
    let territory = t.f(TERRITORY);
    if dist_xz(t.pos, t.nest) < territory {
        let (sight, view) = (t.f(SIGHT_RADIUS), (t.f(VIEW_ANGLE) * 0.5).to_radians());
        let me = t.pos;
        let near = t.closest(w, |c| !c.sticks_to_me && dist_xz(me, c.pos) <= sight);
        if let Some(c) = near.filter(|c| t.calc_target_angle(c.pos) <= view) {
            t.target_pos = c.pos;
        } else if dist_xz(t.pos, t.target_pos) < 25.0 {
            let range = territory - t.f(HOME_RADIUS);
            let dist = t.f(HOME_RADIUS) + range * draws[0];
            let from_home = (t.pos.x - t.nest.x).atan2(t.pos.z - t.nest.z);
            let a = from_home + std::f32::consts::PI * draws[1] + std::f32::consts::FRAC_PI_2;
            t.target_pos.x = dist * a.sin() + t.nest.x;
            t.target_pos.y = t.nest.y;
            t.target_pos.z = match variant {
                Variant::Damagumo | Variant::Houdai => dist * a.cos() + t.nest.z,
                Variant::BigFoot => dist * a.sin() + t.nest.z,
            };
        }
    } else {
        t.target_pos = t.nest;
    }
    let goal = t.target_pos;
    legs(t).set_goal(goal);
}

/// What every Long Legs enemy does after its state ran (`Obj::updateIKSystem`, the feet's damage, the Man-at-Legs' gun).
pub fn update_common(t: &mut Teki, w: &mut World) {
    if t.legs.is_none() {
        return;
    }
    let events = crate::legs::update_common(t, w.dt);
    let (sound, shake) = match t.kind {
        crate::p2::BEADY_LONG_LEGS => ("SPIDER_WALK", 1.0),
        crate::p2::RAGING_LONG_LEGS => ("BIGFOOT_WALK", 1.5),
        _ => ("HOUDAI_METALFOOT", 1.5),
    };
    play_legs(t, w, &events, sound, shake);
    // createOffGroundEffect's held dust follows a lifted foot until createOnGroundEffect.
    if matches!(t.kind,crate::p2::BEADY_LONG_LEGS|crate::p2::RAGING_LONG_LEGS) {
        let l=t.legs.as_ref().unwrap();
        for (i,leg) in l.legs.iter().enumerate().filter(|(i,_)|l.states[*i]==crate::legs::SWINGING) {
            let ids:&[u16]=if t.in_water {&[0x23]}else if t.kind==crate::p2::RAGING_LONG_LEGS {&[0xf1,0x25]}else{&[0x24,0x25]};
            for (layer,&id) in ids.iter().enumerate() {w.out.push(Out::FollowEffect {key:t.id.wrapping_mul(64)+16+i as u32*2+layer as u32,id,pos:leg.foot,dir:Vec3::Y,scale:1.0});}
        }
    }
    if t.kind==crate::p2::RAGING_LONG_LEGS {
        for e in &events {if let LegEvent::Lifted(i)=e {
            let ratio=t.hp/t.f(crate::p2::fp::LIFE).max(0.01);
            let name=if t.flags[0] {"BIGFOOT_RAISE_FAST"}else if ratio<0.175 {"BIGFOOT_RAISE3"}else if ratio<0.35 {"BIGFOOT_RAISE2"}else{"BIGFOOT_RAISE1"};
            w.out.push(Out::Sound {name,pos:t.legs.as_ref().unwrap().legs[*i].foot});
        }}
    }
    let pinch=t.alive() && t.hp<t.f(crate::p2::fp::LIFE)*0.35;
    if pinch && !t.flags[1] {w.out.push(Out::Sound {name:"DAMAGUMO_SMOKE",pos:t.pos});}
    t.flags[1]=pinch;
    stomp(t, w);
    if t.kind == crate::p2::MAN_AT_LEGS {
        crate::houdai::update_gun(t, w);
    }
}
