//! Pikmin 1 Kogane/KoganeAi.cpp. BossMgr assets, not the P2 Kogane variant.
//! Disc own floats 50..64 follow KoganeProp::Parms. Route targets use local
//! random goals because foreign hosts have no Pikmin waypoint graph.
use crate::{anim::key, object::*, tai::*};
use glam::Vec3;
use std::sync::Arc;
pub const HIDDEN: i32 = 1;
pub const WALK: i32 = 2;
pub const WAIT: i32 = 3;
pub const FLIP: i32 = 4;
pub const BURROW: i32 = 0;
fn choose(t: &mut Teki, w: &mut World) {
    let angle = (t.f(55) + w.rng.f32() * (t.f(56) - t.f(55))) * std::f32::consts::PI / 360.0;
    let face = t.yaw + if w.rng.f32() > 0.5 { angle } else { -angle };
    let dist = t.f(57) + w.rng.f32() * (t.f(58) - t.f(57));
    t.target_pos = t.pos + Vec3::new(face.sin(), 0.0, face.cos()) * dist;
    t.reg[1] = 0.0;
}
fn grow(t: &mut Teki, w: &World) {
    t.fade = (t.fade + w.dt * t.f(51) / t.info.scale.max(0.001)).min(1.0);
}
fn dust(t: &Teki, w: &mut World) {
    w.out.push(Out::Effect {
        id: 120,
        pos: t.pos,
    });
}
fn drop(t: &mut Teki, w: &mut World) {
    if t.count[0] >= 8 {
        return;
    }
    let at = t.pos - t.forward() * 20.0 + Vec3::Y * 20.0;
    match t.count[0] % 4 {
        0 | 3 => {
            let color = (w.rng.f32() * 3.0) as u32 % 3;
            w.out.push(Out::SpawnPellets {
                pos: at,
                number: if t.count[0] % 4 == 0 { 0 } else { 1 },
                color,
                count: 1,
            });
            w.out.push(Out::Sound {
                name: "SE_KOGANE_DAMAGE",
                pos: t.pos,
            });
        }
        _ => {
            for sign in [-1.0f32, 1.0] {
                let a = t.yaw + sign * std::f32::consts::PI * 0.2;
                let back = -Vec3::new(a.sin(), 0.0, a.cos());
                w.sim.spawn_nectar_with_velocity(
                    t.pos + back * 20.0 + Vec3::Y * 20.0,
                    back * 50.0 + Vec3::Y * 200.0,
                );
            }
        }
    }
    t.count[0] += 1;
}
pub fn strategy() -> Strategy {
    let mut s = Strategy::new(5, HIDDEN);
    s.on_start = Some(Arc::new(|t| {
        t.set_option(opt::INVINCIBLE);
        t.fade = 0.0;
        t.count[0] = 0;
    }));
    s.set(
        HIDDEN,
        vec![
            Action::new(WALK)
                .on_start(|t, w| {
                    t.clear_option(opt::ATARI | opt::VISIBLE);
                    t.start_motion(6);
                    t.manual_speed = Some(0.0);
                    t.reg[0] = t.f(59) + w.rng.f32() * (t.f(60) - t.f(59));
                })
                .on_act(|t, w| {
                    let near = |at: Vec3| {
                        let d = at - t.pos;
                        d.x.hypot(d.z) < t.f(50) && d.y.abs() < t.f(50)
                    };
                    w.navis.iter().any(|n| n.alive && near(n.pos))
                        || w.sim.pikis().iter().any(|p| p.is_alive() && near(p.pos))
                })
                .on_finish(|t, w| {
                    t.set_option(opt::ATARI | opt::VISIBLE);
                    t.manual_speed = Some(30.0);
                    dust(t, w);
                })
                .build(),
        ],
    );
    for state in [WALK, WAIT] {
        let moving = state == WALK;
        s.set(
            state,
            vec![
                Action::new(BURROW)
                    .on_act(|t, _| t.reg[0] <= 0.0 || t.count[0] >= 8)
                    .build(),
                Action::new(FLIP)
                    .on_event(|_, _, e| {
                        matches!(
                            e,
                            Event::Pressed
                                | Event::Entity {
                                    other: Target::Piki(_),
                                    flying: true
                                }
                        )
                    })
                    .build(),
                Action::new(if moving { WAIT } else { WALK })
                    .on_start(move |t, w| {
                        t.start_motion(if moving { 6 } else { 2 });
                        choose(t, w);
                        t.reg[2] = t.f(61) + w.rng.f32() * (t.f(62) - t.f(61));
                        if !moving {
                            t.target_vel = Vec3::ZERO;
                        }
                    })
                    .on_act(move |t, w| {
                        grow(t, w);
                        t.reg[0] -= w.dt;
                        t.reg[1] += w.dt;
                        if moving {
                            t.turn_toward(t.calc_target_direction(t.target_pos), t.f(54), w.dt);
                            t.target_vel =
                                (t.target_pos - t.pos).with_y(0.0).normalize_or_zero() * t.f(53);
                            if t.key(key::ACTION0) {
                                w.out.push(Out::Effect { id: 19, pos: t.pos });
                            }
                            if (t.target_pos - t.pos).length() < 10.0 {
                                return true;
                            }
                            if t.reg[1] > t.f(63) {
                                choose(t, w);
                            }
                        } else if t.reg[1] > t.reg[2] {
                            return true;
                        }
                        false
                    })
                    .on_event(|t, w, e| {
                        if matches!(e, Event::Wall) {
                            choose(t, w);
                        }
                        false
                    })
                    .build(),
            ],
        );
    }
    s.set(
        FLIP,
        vec![
            Action::new(WALK)
                .on_start(|t, w| {
                    t.start_motion(1);
                    for (id, dir) in [
                        (89, t.forward()),
                        (88, Vec3::new(t.yaw.cos(), 0.0, -t.yaw.sin())),
                        (87, Vec3::new(-t.yaw.cos(), 0.0, t.yaw.sin())),
                    ] {
                        w.out.push(Out::EffectDir {
                            id,
                            pos: t.pos,
                            dir,
                        });
                    }
                })
                .on_act(|t, w| {
                    grow(t, w);
                    t.target_vel *= 0.95;
                    t.reg[0] -= w.dt;
                    if t.key(key::ACTION0) {
                        drop(t, w);
                    }
                    if t.key(key::ACTION1) {
                        dust(t, w);
                    }
                    if t.animation_finished() {
                        t.next_override = Some(if t.count[0] >= 8 || t.reg[0] <= 0.0 {
                            BURROW
                        } else {
                            WALK
                        });
                        return true;
                    }
                    false
                })
                .build(),
        ],
    );
    s.set(
        BURROW,
        vec![
            Action::new(NO_TRANSIT)
                .on_start(|t, _| {
                    t.target_vel = Vec3::ZERO;
                    t.clear_option(opt::ATARI);
                })
                .on_act(|t, w| {
                    t.fade = (t.fade - w.dt * t.f(52) / t.info.scale.max(0.001)).max(0.0);
                    if t.fade == 0.0 {
                        dust(t, w);
                        t.die(w);
                    }
                    false
                })
                .build(),
        ],
    );
    s
}
