//! Man-at-Legs (`Houdai::Obj`, plugProjectNishimuraU/Houdai.cpp, HoudaiState.cpp, HoudaiShotGun.cpp): the Long Legs' machine
//! ([`crate::longlegs`]) with a cannon on its back. It does not stamp (its collision tree has no foot spheres); it shakes
//! Pikmin off as it lands, and after standing a long while unhurt or after being shaken it opens the gun (`Shot`): the gun
//! swings round to the nearest player or Pikmin, fires bursts of shells that fly straight at 600 units a second until they
//! strike the ground, and folds away.
//!
//! States: the family's [`DEAD`]..[`WALK`] and [`SHOT`]. Registers: as [`crate::longlegs`], plus `reg[4]` the burst timer
//! of `mShotGunSearchTimer`, `reg[5]` the time since it was last hurt (`mShotGunBurstTimer`), `reg[6]` the gun's phase
//! (`mShotGunState`: 0 aiming, 1 firing).
//!
//! The gun (`HoudaiShotGunMgr`): `startRotation` swings it from where it rests toward its target at 0.05 radians a tick
//! (`searchShotGunRotation`), `finishRotation` returns it at 0.025 (`returnShotGunRotation`). The decomp turns the head
//! joint about its own axis and the gun joint about its Z; here the barrel's direction is kept as a heading and an
//! elevation and the host swings the head and gun joints onto it (`LegsView::gun`).
//!
//! Host adaptations: the ground a shell strikes is the one height the host reports under the enemy (the decomp traces the
//! map); a shell hurts what its path passes within the attack radius of, once each, and a hurt player is knocked from it
//! (`InteractBomb`); the story-mode cave's other target pattern is left out (it always aims at the nearest).

use glam::Vec3;

use crate::anim::key;
use crate::longlegs::*;
use crate::object::*;
use crate::p2::fp;
use crate::tai::*;

pub const SHOT: i32 = 6;

pub const ANIM_ATTACK: usize = 4;

const SHOT_TIMER: usize = 4;
const BURST: usize = 5;
const SHOT_STATE: usize = 6;

// `ProperParms`: shooting on/off times; `GeneralParms`: the gun's use of search height/angle as seconds.
const MAX_ON: usize = fp::own::BASE + 10;
const MIN_ON: usize = fp::own::BASE + 11;
const MAX_OFF: usize = fp::own::BASE + 12;
const MIN_OFF: usize = fp::own::BASE + 13;
const SEARCH_HEIGHT: usize = 26;
const SEARCH_ANGLE: usize = 15;
const SEARCH_DISTANCE: usize = 14;
const ATTACK_RADIUS: usize = 22;
const HIT_ANGLE: usize = 23;

/// A shell in flight (`HoudaiShotGunNode`).
#[derive(Debug, Clone)]
pub struct Shell {
    pub slot: u8,
    pub pos: Vec3,
    pub vel: Vec3,
    hit: Vec<u32>,
}

/// The gun (`HoudaiShotGunMgr`).
#[derive(Debug, Clone, Default)]
pub struct Gun {
    pub rotating: bool,
    pub locked_on: bool,
    pub finished: bool,
    sight_active: bool,
    /// The barrel's direction as a heading (`atan2(x, z)`) and an angle from straight up.
    pub heading: f32,
    pub elevation: f32,
    pub target: Vec3,
    pub shells: Vec<Shell>,
}

impl Gun {
    /// The barrel's direction.
    pub fn dir(&self) -> Vec3 {
        Vec3::new(self.elevation.sin() * self.heading.sin(), self.elevation.cos(), self.elevation.sin() * self.heading.cos())
    }
}

fn heading_of(d: Vec3) -> (f32, f32) {
    (d.x.atan2(d.z), d.x.hypot(d.z).atan2(d.y))
}

fn wrap(a: f32) -> f32 {
    let mut a = a % std::f32::consts::TAU;
    if a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    } else if a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

/// Moves `a` toward `to` by at most `step` (`searchShotGunRotation`'s 0.05, `returnShotGunRotation`'s 0.025).
fn approach(a: f32, to: f32, step: f32) -> f32 {
    let d = wrap(to - a);
    if d.abs() > step { a + step.copysign(d) } else { a + d }
}

/// The gun's place and its barrel at rest, in the world (the rig's `gun` joint in the landing pose).
fn rest(t: &Teki) -> Option<(Vec3, Vec3)> {
    let l = t.legs.as_ref()?;
    let g = l.rig.gun?;
    Some((l.trace + crate::legs::to_world(g.pos, l.face), crate::legs::to_world(g.dir, l.face)))
}

/// `Obj::setShotGunTargetPosition`: the nearest player or Pikmin in front (within the search distance), else, once a second
/// has gone by since it was last hurt, a random place round it.
fn aim_target(t: &mut Teki, w: &mut World) {
    let dist = t.f(SEARCH_DISTANCE);
    let me = t.pos;
    let target = t.closest(w, |c| !c.sticks_to_me && dist_xz(me, c.pos) <= dist);
    if let Some(c) = target {
        legs(t).gun.target = c.pos;
    } else if t.reg[BURST] > 1.0 {
        t.reg[BURST] = 0.0;
        let a = w.rng.f32() * std::f32::consts::TAU;
        let r = w.rng.f32() * dist;
        legs(t).gun.target = Vec3::new(r * a.sin() + t.pos.x, t.pos.y, r * a.cos() + t.pos.z);
    }
}

/// `HoudaiShotGunMgr::doUpdate` and the shells' `doUpdateCommon`, every tick.
pub fn update_gun(t: &mut Teki, w: &mut World) {
    // `updateShotGunTimer`: time since it was last hurt.
    if t.flash > 0.0 {
        t.reg[BURST] = 0.0;
    } else {
        t.reg[BURST] += w.dt;
    }
    let Some((pos, dir)) = rest(t) else { return };
    let (h0, e0) = heading_of(dir);
    let g = &mut legs(t).gun;
    if g.rotating {
        if g.finished {
            // `returnShotGunRotation`
            g.heading = approach(g.heading, h0, 0.025);
            g.elevation = approach(g.elevation, e0, 0.025);
            if wrap(g.heading - h0).abs() < 0.01 && (g.elevation - e0).abs() < 0.01 {
                g.rotating = false;
                g.locked_on = true;
            }
        } else {
            // `searchShotGunRotation`: it is locked on from the first tick it turns.
            let d = g.target - pos;
            let (h, e) = heading_of(d);
            g.heading = approach(g.heading, h, 0.05);
            g.elevation = approach(g.elevation, e, 0.05);
            g.locked_on = true;
        }
    } else {
        g.heading = h0;
        g.elevation = e0;
    }
    let key=t.id.wrapping_mul(64)|15;
    let g=&mut legs(t).gun;
    let sight=g.rotating && !g.finished && g.locked_on;
    if sight {
        w.out.push(Out::FollowEffect {key,id:0x9f,pos:g.target,dir:Vec3::Y,scale:1.0});
        w.out.push(Out::Sound {name:"HOUDAI_BEAM",pos:g.target});
    } else if g.sight_active {w.out.push(Out::StopEffect {key});}
    g.sight_active=sight;
    update_shells(t, w);
}

/// Each shell flies straight; it hurts what its path passes, and ends on the ground or far from the enemy
/// (`HoudaiShotGunNode::update`).
fn update_shells(t: &mut Teki, w: &mut World) {
    let radius = t.f(ATTACK_RADIUS);
    let damage = t.f(fp::ATTACK_DAMAGE);
    let ground = t.ground_y.unwrap_or(t.pos.y);
    let (me, id) = (t.pos, t.id);
    let water=t.in_water;
    let g = &mut legs(t).gun;
    let mut shells = std::mem::take(&mut g.shells);
    shells.retain_mut(|s| {
        let from = s.pos;
        s.pos += s.vel * w.dt;
        let key=id.wrapping_mul(64)|s.slot as u32;
        w.out.push(Out::FollowEffect {key,id:0x9b,pos:s.pos,dir:s.vel.normalize_or_zero(),scale:1.0});
        let seg = s.pos - from;
        let len = seg.length();
        // What the path passes within the attack radius of.
        let mid = from + seg * 0.5;
        let reach = len * 0.5 + radius;
        for n in w.navis.iter().filter(|n| n.alive) {
            if s.hit.contains(&n.id) {
                continue;
            }
            let c = n.pos + Vec3::Y * 5.0;
            let along = if len > 1e-4 { ((c - from).dot(seg) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
            if (c - (from + seg * along)).length() <= radius + n.size {
                s.hit.push(n.id);
                let away = (c - mid).with_y(0.0).normalize_or_zero();
                w.out.push(Out::HitNavi { teki: id, navi: n.id, damage: damage * 6.0, from: n.pos - away * 10.0, knock: 100.0 });
            }
        }
        w.sim.flick_pikis_in_sphere(mid - Vec3::Y * 10.0, reach + 6.0, damage, mid - Vec3::Y * 10.0);
        let struck = s.pos.y - 10.0 < ground;
        let far = (s.pos.x - me.x).abs() > 1500.0 || (s.pos.y - me.y).abs() > 1000.0 || (s.pos.z - me.z).abs() > 1500.0;
        if struck {
            let at = Vec3::new(s.pos.x, ground, s.pos.z);
            w.out.push(Out::Sound { name: if water {"HOUDAI_WATER_IMPACT"}else{"HOUDAI_IMPACT"}, pos: at });
            for fx in if water {&[0x97,0x98,0x99,0x9a][..]}else{&[0x8d,0x8e,0x8f,0x90][..]} {
                w.out.push(Out::Effect {id:0x8000|fx,pos:at});
            }
            w.out.push(Out::Shake { pos: at, strength: 1.0 });
        }
        if struck || far {w.out.push(Out::StopEffect {key});}
        !(struck || far)
    });
    legs(t).gun.shells = shells;
}

/// `HoudaiShotGunMgr::emitShotGun`: one shell out of the muzzle (45 units along the barrel), with a hair of spread.
fn emit(t: &mut Teki, w: &mut World) {
    let spread = t.f(HIT_ANGLE);
    let Some((pos, _)) = rest(t) else { return };
    let g = &mut legs(t).gun;
    let mut d = g.dir();
    d.x += w.rng.f32() * spread * 2.0 - spread;
    d.y += w.rng.f32() * spread * 2.0 - spread;
    d.z += w.rng.f32() * spread * 2.0 - spread;
    let d = d.normalize_or_zero();
    if g.shells.len() < 10 {
        let slot=(0..10).find(|slot|!g.shells.iter().any(|s|s.slot==*slot)).unwrap();
        g.shells.push(Shell { slot, pos: pos + d * 45.0, vel: d * 600.0, hit: Vec::new() });
        for id in [0x9d,0x9e,0x247] {w.out.push(Out::EffectDir {id:0x8000|id,pos:pos+d*45.0,dir:d});}
        w.out.push(Out::Sound { name: "HOUDAI_SHOT", pos });
    }
}

fn paused(t: &Teki) -> bool {
    t.manual_speed == Some(0.0)
}

fn pause(t: &mut Teki) {
    t.manual_speed = Some(0.0);
}

fn resume(t: &mut Teki) {
    t.manual_speed = None;
}

/// A random time in `0..max - min` (`randWeightFloat(max - min)`).
fn span(t: &Teki, w: &mut World, max: usize, min: usize) -> f32 {
    rand(w) * (t.f(max) - t.f(min))
}

pub fn strategy() -> Strategy {
    let mut s = crate::longlegs::strategy(Variant::Houdai);
    s.states.push(Vec::new());

    // StateStay: also comes down when it is hurt (`EB_TakingDamage`).
    s.set(
        STAY,
        vec![Action::new(LAND)
            .on_start(|t, _| {
                t.reg[NEXT] = -1.0;
                t.target_vel = Vec3::ZERO;
                t.clear_option(opt::VISIBLE | opt::ATARI);
                t.start_motion(ANIM_LANDING);
                pause(t);
            })
            .on_act(|t, w| {
                let hurt_now = t.flash > 0.0 || t.stored_damage > 0.0;
                hurt(t);
                let r = t.f(fp::SIGHT_RADIUS);
                hurt_now || someone_near(t, w, r)
            })
            .on_finish(|t, _| {
                resume(t);
                t.set_option(opt::VISIBLE | opt::ATARI);
            })
            .build()],
    );

    // StateLand: it shakes off what is on it at every key.
    s.set(
        LAND,
        vec![Action::new(WAIT)
            .on_start(|t, _| {
                t.reg[NEXT] = -1.0;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_LANDING);
            })
            .on_act(|t, w| {
                hurt(t);
                let at = t.pos;
                for k in [key::ACTION0, key::ACTION2, key::ACTION3] {
                    if t.key(k) {
                        shake_off(t, w);
                        w.out.push(Out::Sound { name: "HOUDAI_METALFOOT", pos: at });
                        w.out.push(Out::Shake { pos: at, strength: 1.5 });
                    }
                }
                if t.animation_finished() {
                    shake_off(t, w);
                    let next = if t.hp <= 0.0 {
                        DEAD
                    } else if start_flick(t, w) {
                        FLICK
                    } else {
                        WAIT
                    };
                    t.next_override = Some(next);
                    return true;
                }
                false
            })
            .on_finish(|t, _| {
                let (pos, yaw) = (t.pos, t.yaw);
                legs(t).start_programmed(pos, yaw);
            })
            .build()],
    );

    // StateWait: dead, flick, shoot (after standing unhurt a long while) or walk.
    s.set(
        WAIT,
        vec![Action::new(WALK)
            .on_start(|t, w| {
                t.reg[TIMER] = 0.0;
                t.reg[NEXT] = -1.0;
                t.reg[DURATION] = 1.5 + 1.5 * rand(w);
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
                } else if t.reg[BURST] > t.f(SEARCH_HEIGHT) {
                    t.reg[NEXT] = SHOT as f32;
                    t.finish_motion();
                } else if t.reg[TIMER] > t.reg[DURATION] {
                    t.reg[NEXT] = WALK as f32;
                    t.finish_motion();
                }
                t.animation_finished() && t.reg[NEXT] >= 0.0 && {
                    t.next_override = Some(t.reg[NEXT] as i32);
                    true
                }
            })
            .build()],
    );

    // StateFlick: shakes Pikmin off with the legs blended in, then opens the gun.
    s.set(
        FLICK,
        vec![Action::new(SHOT)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.reg[NEXT] = -1.0;
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_FLICK);
                legs(t).set_blend(true);
            })
            .on_act(|t, w| {
                hurt(t);
                if t.key(key::ACTION1) {
                    shake_off(t, w);
                    w.out.push(Out::Shake { pos: t.pos, strength: 2.0 });
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { SHOT });
                    return true;
                }
                false
            })
            .on_finish(|t, _| legs(t).set_blend(false))
            .build()],
    );

    // StateShot: opens the gun, aims, fires in bursts, folds it away and walks on.
    s.set(
        SHOT,
        vec![Action::new(WALK)
            .on_start(|t, _| {
                t.reg[TIMER] = 0.0;
                t.reg[NEXT] = -1.0;
                t.reg[SHOT_TIMER] = 0.0;
                t.reg[SHOT_STATE] = 0.0;
                t.target_vel = Vec3::ZERO;
                {
                    let g = &mut legs(t).gun;
                    g.rotating = false;
                    g.locked_on = false;
                    g.finished = false;
                }
                t.start_motion(ANIM_ATTACK);
                legs(t).set_blend(true);
            })
            .on_act(|t, w| {
                hurt(t);
                let dt = w.dt;
                let (max_on, min_on, max_off, min_off) = (MAX_ON, MIN_ON, MAX_OFF, MIN_OFF);
                if paused(t) {
                    let g = legs(t).gun.clone();
                    if t.reg[SHOT_STATE] != 0.0 {
                        // Firing, paused between bursts: carry on when the pause is over (or it was told to finish).
                        if t.anim.is_finished() || t.reg[SHOT_TIMER] > t.f(max_off) {
                            t.reg[SHOT_TIMER] = span(t, w, max_on, min_on);
                            resume(t);
                        }
                    } else if g.finished {
                        // Folded away: once it is back and a couple of seconds have passed, the clip goes on to its end.
                        if g.locked_on && t.reg[TIMER] > 2.0 {
                            t.reg[SHOT_STATE] = 0.0;
                            t.reg[TIMER] = 0.0;
                            resume(t);
                        }
                    } else if g.locked_on && t.reg[TIMER] > 2.0 {
                        // Aimed for two seconds: opens fire.
                        t.reg[SHOT_STATE] = 1.0;
                        t.reg[SHOT_TIMER] = 0.0;
                        t.reg[TIMER] = 0.0;
                        resume(t);
                    }
                }
                if legs(t).gun.rotating {
                    aim_target(t, w);
                    // After `searchAngle` seconds of aiming it stops and the clip is told to finish.
                    if t.reg[TIMER] > t.f(SEARCH_ANGLE) {
                        t.reg[TIMER] = 0.0;
                        t.finish_motion();
                    }
                }
                t.reg[SHOT_TIMER] += dt;
                t.reg[TIMER] += dt;
                if t.hp <= 0.0 {
                    if paused(t) {
                        resume(t);
                    }
                    t.finish_motion();
                }
                if t.key(key::ACTION0) {
                    // The gun is open: it stops here and starts to turn.
                    t.reg[TIMER] = 0.0;
                    pause(t);
                    let g = &mut legs(t).gun;
                    g.rotating = true;
                    g.locked_on = false;
                    g.finished = false;
                } else if t.key(key::ACTION1) {
                    if !t.anim.is_finished() {
                        shake_off(t, w);
                        emit(t, w);
                    }
                } else if t.key(key::ACTION2) {
                    if !t.anim.is_finished() && t.f(SEARCH_ANGLE) - t.reg[TIMER] > t.f(MAX_OFF) && t.reg[SHOT_TIMER] > t.f(MAX_ON) {
                        t.reg[SHOT_TIMER] = span(t, w, max_off, min_off);
                        pause(t);
                    }
                } else if t.key(key::ACTION3) {
                    t.reg[SHOT_STATE] = 0.0;
                    t.reg[TIMER] = 0.0;
                    pause(t);
                    let g = &mut legs(t).gun;
                    g.finished = true;
                    g.locked_on = false;
                }
                if t.animation_finished() {
                    t.next_override = Some(if t.hp <= 0.0 { DEAD } else { WALK });
                    return true;
                }
                false
            })
            .on_finish(|t, _| {
                resume(t);
                legs(t).set_blend(false);
                let g = &mut legs(t).gun;
                g.rotating = false;
                g.finished = false;
            })
            .build()],
    );
    s
}
