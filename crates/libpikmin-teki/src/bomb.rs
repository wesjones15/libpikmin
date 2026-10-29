//! The Bomb-rock of Pikmin 2 (`Bomb::Obj : EnemyBase`, plugProjectMorimuraU/bomb.cpp, bombState.cpp): a rock with a fuse.
//! It lies still (`Wait`, the first clip held on its first frame) until something sets it off; then the hit-start clip
//! plays and it hisses on the loop clip (`Bomb`) while its life, the fuse (4.5 seconds on the disc), burns down a second
//! a second. A moment after the fuse is out it blows: whoever is within the attack radius and the blast height takes the
//! blast, Pikmin are thrown and killed, and the rock is gone.
//!
//! States (`BombStateID`): [`WAIT`], [`BOMB`]. `hp` is the fuse. The host sets it off with any hit (a hit on its invulnerable
//! body reaches it as [`Event::Pressed`]), in place of the touch of a carrier or a thrown Pikmin (`isAnimStart`).
//!
//! Host adaptations: the damage to other enemies (`mDamageToEnemies`, `InteractBomb` on Teki) and the chain reaction with
//! other bombs (`mTriggerLimit`) are left out; the carry by Pikmin (`onStartCapture`) is the host's pellets, not this.

use std::sync::Arc;

use glam::Vec3;

use crate::object::*;
use crate::p2::fp;
use crate::tai::*;

pub const WAIT: i32 = 0;
pub const BOMB: i32 = 1;

/// `BombAnimID`: hit start, hit loop.
pub const ANIM_HIT_START: usize = 0;
pub const ANIM_HIT_LOOP: usize = 1;

/// `ProperParms::mBlastRangeHeight` (`fp02`): the blast reaches this far above and below it.
const BLAST_HEIGHT: usize = fp::own::BASE + 2;
/// Ticks between the fuse running out and the blast (`mExplodeDelayTimer < 10`).
const DELAY_TICKS: f32 = 10.0;
/// How hard the blast throws a player (`HitNavi::knock`).
const KNOCK: f32 = 200.0;

fn blast(t: &mut Teki, w: &mut World) {
    let radius = t.f(fp::ATTACK_RADIUS);
    let height = t.f(BLAST_HEIGHT);
    let damage = t.f(fp::ATTACK_DAMAGE);
    for n in w.navis.iter().filter(|n| n.alive) {
        if (n.pos.y - t.pos.y).abs() <= height && dist_xz(n.pos, t.pos) - n.size <= radius {
            w.out.push(Out::HitNavi { teki: t.id, navi: n.id, damage, from: t.pos, knock: KNOCK });
        }
    }
    // The Pikmin in the blast are thrown and killed.
    let victims: Vec<_> = w
        .sim
        .pikis()
        .iter()
        .filter(|p| p.is_alive() && (p.pos.y - t.pos.y).abs() <= height && dist_xz(p.pos, t.pos) <= radius)
        .map(|p| (p.id, p.health))
        .collect();
    for (id, health) in victims {
        w.sim.attack_piki(id, health.max(1.0));
    }
    w.out.push(Out::Sound { name: "PK_SE_BOMB", pos: t.pos });
    w.out.push(Out::Shake { pos: t.pos, strength: 1.0 });
}

pub fn strategy() -> Strategy {
    let mut s = Strategy::new(2, WAIT);
    s.on_start = Some(Arc::new(|t: &mut Teki| {
        t.set_option(opt::INVINCIBLE);
        t.reg = [0.0; 8];
    }));

    // StateWait: lying with the fuse not lit, the hit-start clip held on its first frame, until it is set off; then the
    // start clip plays with the fuse already burning and the loop follows it.
    s.set(
        WAIT,
        vec![Action::new(BOMB)
            .on_start(|t, _| {
                t.target_vel = Vec3::ZERO;
                t.start_motion(ANIM_HIT_START);
                t.manual_speed = Some(0.0);
                t.flags[0] = false;
            })
            .on_event(|t, w, e| {
                if matches!(e, Event::Pressed) && !t.flags[0] {
                    t.flags[0] = true;
                    t.manual_speed = None;
                    w.out.push(Out::Sound { name: "BOMB_START", pos: t.pos });
                }
                false
            })
            .on_act(|t, w| {
                if !t.flags[0] {
                    return false;
                }
                t.hp -= w.dt;
                t.animation_finished()
            })
            .build()],
    );

    // StateBomb: hissing; the fuse burns down and a moment after it is out the rock blows.
    s.set(
        BOMB,
        vec![Action::new(NO_TRANSIT)
            .on_start(|t, _| {
                t.reg[0] = 0.0;
                t.start_motion(ANIM_HIT_LOOP);
            })
            .on_act(|t, w| {
                t.hp -= w.dt;
                if t.animation_finished() {
                    t.start_motion(ANIM_HIT_LOOP);
                }
                if t.hp <= 0.0 {
                    t.reg[0] += 1.0;
                    if t.reg[0] >= DELAY_TICKS {
                        blast(t, w);
                        t.die(w);
                    }
                }
                false
            })
            .build()],
    );
    s
}
