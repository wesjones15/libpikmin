//! Nectar: a falling drop that splashes into a puddle; Pikmin without a flower that walk into
//! the puddle drink it and bloom.
//!
//! - The drop is `MizuItem` with `FallWaterAI` (itemAI.cpp): "leave" plays, then "fall"
//!   loops until it touches the ground; it then plays on from frame 29 and spawns the puddle
//!   at `KEY_Action0`, disappearing when the motion ends.
//! - The puddle is `MizuItem` with `WaterAI`: "swing" (appearing) → "wait" loop; a captain
//!   touching it plays "touch"; the first drink message while waiting or touched starts
//!   "dead" (draining), after which it is no longer alive and is removed when that ends.
//! - Pikmin: `PikiNormalState::procCollideMsg` (pikiState.cpp) sends a non-flower Pikmin
//!   that isn't carrying into free mode and `PikiAbsorbState` (MIZUNOMI). While its loop
//!   plays it drinks (`AS_Drinking`); at the loop end it stops if the puddle is gone. When
//!   the motion finishes it goes to `PikiGrowupState` if it drank: GROWUP1 the first time,
//!   GROWUP2 after, and `KEY_Action0` turns it into a flower.

use glam::Vec3;

use crate::event::Event;
use crate::host::Host;
use crate::piki::{AttackMode, Happa, PikiState};
use crate::sim::{Sim, integrate};

/// `Creature::getSize` default: the puddle's collision radius.
pub const PUDDLE_RADIUS: f32 = 15.0;
/// The drop is a `MizuItem` too, with the same `getSize`.
const DROP_RADIUS: f32 = PUDDLE_RADIUS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NectarId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropState {
    /// "leave" then the "fall" loop, under gravity.
    Falling { frame: f32, leaving: bool },
    /// Hit the ground; "fall" plays on from frame 29. The puddle appears at `KEY_Action0`.
    Splash { frame: f32, emitted: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NectarDrop {
    pub id: NectarId,
    pub pos: Vec3,
    pub vel: Vec3,
    pub state: DropState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PuddleState {
    /// "swing": forming. Drinking doesn't drain it yet.
    Appear { frame: f32 },
    /// "wait" loop.
    Wait { frame: f32 },
    /// "touch": a captain stepped in it.
    Touched { frame: f32 },
    /// "dead": draining away. Not alive any more.
    Drain { frame: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Puddle {
    pub id: NectarId,
    pub pos: Vec3,
    pub state: PuddleState,
}

impl Puddle {
    /// `MizuItem::isAlive`
    pub fn alive(&self) -> bool {
        !matches!(self.state, PuddleState::Drain { .. })
    }
}

/// Frame timings from `objects/water/anims.bin` and `objects/fl_water/anims.bin`
/// (GPIE01_01 values). Motions play at 30 frames per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NectarTimings {
    /// "leave" (fl_water motion 1).
    pub leave_end: f32,
    /// "fall" (fl_water motion 0): loop, `KEY_Action0` (puddle appears) and end.
    pub fall_loop: (f32, f32),
    pub fall_emit: f32,
    pub fall_end: f32,
    /// water motions: 0 "wait", 1 "touch", 2 "swing" (appear), 3 "dead" (drain).
    pub wait_end: f32,
    pub touch_end: f32,
    pub appear_end: f32,
    pub drain_end: f32,
}

impl Default for NectarTimings {
    fn default() -> Self {
        Self {
            leave_end: 8.0,
            fall_loop: (19.0, 23.0),
            fall_emit: 30.0,
            fall_end: 37.0,
            wait_end: 40.0,
            touch_end: 50.0,
            appear_end: 42.0,
            drain_end: 50.0,
        }
    }
}

impl Sim {
    /// Drops nectar from `pos` (it falls, splashes and leaves a puddle), as a Honeywisp does
    /// when hit (`TaiMizinko`, taimizinko.cpp).
    pub fn spawn_nectar(&mut self, pos: Vec3) -> NectarId {
        let id = self.next_nectar_id();
        self.nectar_drops.push(NectarDrop {
            id,
            pos,
            vel: Vec3::ZERO,
            state: DropState::Falling {
                frame: 0.0,
                leaving: true,
            },
        });
        id
    }

    /// Places a nectar puddle directly.
    pub fn add_nectar_puddle(&mut self, pos: Vec3) -> NectarId {
        let id = self.next_nectar_id();
        self.puddles.push(Puddle {
            id,
            pos,
            state: PuddleState::Appear { frame: 0.0 },
        });
        id
    }

    pub fn nectar_drops(&self) -> &[NectarDrop] {
        &self.nectar_drops
    }

    pub fn puddles(&self) -> &[Puddle] {
        &self.puddles
    }

    fn next_nectar_id(&mut self) -> NectarId {
        self.next_nectar += 1;
        NectarId(self.next_nectar)
    }

    pub(crate) fn update_nectar(&mut self, host: &mut dyn Host, dt: f32) {
        let t = self.params.nectar;
        let step = 30.0 * dt;
        let gravity = self.params.ai.gravity;
        let mut new_puddles = Vec::new();
        self.nectar_drops.retain_mut(|d| {
            match &mut d.state {
                DropState::Falling { frame, leaving } => {
                    *frame += step;
                    if *leaving && *frame >= t.leave_end {
                        *leaving = false;
                        *frame = 0.0;
                    } else if !*leaving && *frame >= t.fall_loop.1 {
                        *frame -= t.fall_loop.1 - t.fall_loop.0;
                    }
                    let mut on_ground = false;
                    let target = d.vel;
                    integrate(&*host, &mut d.pos, &mut d.vel, target, &mut on_ground, DROP_RADIUS, 1.0, gravity, dt);
                    if on_ground {
                        // `CollideInit`: finishMotion(29)
                        d.state = DropState::Splash {
                            frame: 29.0,
                            emitted: false,
                        };
                        self.events.push(Event::NectarSplash { pos: d.pos });
                    }
                }
                DropState::Splash { frame, emitted } => {
                    *frame += step;
                    if !*emitted && *frame >= t.fall_emit {
                        *emitted = true;
                        new_puddles.push(d.pos);
                    }
                    if *frame >= t.fall_end {
                        return false;
                    }
                }
            }
            true
        });
        for pos in new_puddles {
            self.add_nectar_puddle(pos);
        }

        // Captains stepping in it play "touch" (`WaterAI::CollideChar`).
        let captains: Vec<Vec3> = self.captains.iter().filter(|c| !c.down).map(|c| c.pos).collect();
        let navi_radius = self.params.navi.body_collision_radius;
        for p in &mut self.puddles {
            let touched = captains
                .iter()
                .any(|c| (*c - p.pos).with_y(0.0).length() < PUDDLE_RADIUS + navi_radius);
            p.state = match p.state {
                PuddleState::Appear { frame } if frame + step >= t.appear_end => {
                    PuddleState::Wait { frame: 0.0 }
                }
                PuddleState::Appear { frame } => PuddleState::Appear { frame: frame + step },
                PuddleState::Wait { .. } if touched => PuddleState::Touched { frame: 0.0 },
                PuddleState::Wait { frame } => PuddleState::Wait {
                    frame: (frame + step) % t.wait_end.max(1.0),
                },
                PuddleState::Touched { frame } if frame + step >= t.touch_end => {
                    PuddleState::Wait { frame: 0.0 }
                }
                PuddleState::Touched { frame } => PuddleState::Touched { frame: frame + step },
                PuddleState::Drain { frame } => PuddleState::Drain { frame: frame + step },
            };
        }
        let before = self.puddles.len();
        self.puddles
            .retain(|p| !matches!(p.state, PuddleState::Drain { frame } if frame >= t.drain_end));
        for _ in self.puddles.len()..before {
            self.events.push(Event::NectarGone);
        }
    }

    /// `PikiNormalState::procCollideMsg`: a Pikmin touching a puddle starts drinking.
    pub(crate) fn nectar_contacts(&mut self) {
        if self.puddles.is_empty() {
            return;
        }
        for pi in 0..self.pikis.len() {
            let p = &self.pikis[pi];
            let normal = matches!(
                p.state,
                PikiState::Idle | PikiState::Squad { .. }
            ) || matches!(p.state, PikiState::Attack(a) if a.mode == AttackMode::Approach);
            if !normal || p.happa == Happa::Flower || !p.on_ground {
                continue;
            }
            let reach = PUDDLE_RADIUS + self.params.piki.collision_radius;
            let Some(puddle) = self
                .puddles
                .iter()
                // `Creature::collisionCheck` (creatureCollision.cpp) skips creatures that aren't
                // alive, so a draining puddle takes no new drinkers.
                .find(|n| n.alive() && (n.pos - p.pos).with_y(0.0).length() < reach)
                .map(|n| (n.id, n.pos))
            else {
                continue;
            };
            let id = p.id;
            // changeMode(FreeMode): it leaves the squad but remembers its captain.
            self.leave_current(id);
            let p = &mut self.pikis[pi];
            p.face = crate::math::yaw_of(puddle.1 - p.pos);
            p.state = PikiState::Absorb {
                puddle: puddle.0,
                frame: 0.0,
                drinking: false,
                drank: false,
                finishing: false,
            };
        }
    }

    /// `PikiAbsorbState` and `PikiGrowupState`.
    pub(crate) fn piki_nectar(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        let step = self.params.anim.fps * dt;
        let a = self.params.anim.clone();
        {
            let p = &mut self.pikis[pi];
            p.target_vel = Vec3::ZERO;
            p.vel.x = 0.0;
            p.vel.z = 0.0;
        }
        self.physics(pi, host, dt);
        let id = self.pikis[pi].id;
        match self.pikis[pi].state {
            PikiState::Absorb {
                puddle,
                mut frame,
                mut drinking,
                mut drank,
                mut finishing,
            } => {
                let (loop_start, loop_end) = a.mizunomi.loop_range.unwrap_or((10.0, 30.0));
                frame += step;
                let alive = self.puddles.iter().any(|n| n.id == puddle && n.alive());
                if !finishing && frame >= loop_start {
                    if !drinking {
                        self.events.push(Event::PikiDrinking { piki: id });
                    }
                    drinking = true;
                }
                if drinking && !finishing {
                    // The drink message drains a waiting or touched puddle.
                    if let Some(n) = self.puddles.iter_mut().find(|n| n.id == puddle)
                        && matches!(n.state, PuddleState::Wait { .. } | PuddleState::Touched { .. })
                    {
                        n.state = PuddleState::Drain { frame: 0.0 };
                    }
                    drank = true;
                }
                if !finishing && frame >= loop_end {
                    if alive {
                        frame -= loop_end - loop_start;
                    } else {
                        finishing = true;
                    }
                }
                let p = &mut self.pikis[pi];
                p.state = if frame >= a.mizunomi.end {
                    if drank {
                        PikiState::GrowUp {
                            frame: 0.0,
                            second: p.blooms > 0,
                            bloomed: false,
                        }
                    } else {
                        PikiState::Idle
                    }
                } else {
                    PikiState::Absorb {
                        puddle,
                        frame,
                        drinking,
                        drank,
                        finishing,
                    }
                };
            }
            PikiState::GrowUp {
                mut frame,
                second,
                mut bloomed,
            } => {
                let t = if second { a.grow_up2 } else { a.grow_up1 };
                frame += step;
                let p = &mut self.pikis[pi];
                if !bloomed && frame >= t.action0 {
                    bloomed = true;
                    p.happa = Happa::Flower;
                    p.blooms = p.blooms.saturating_add(1);
                    self.events.push(Event::Bloomed { piki: id });
                }
                p.state = if frame >= t.end {
                    PikiState::Idle
                } else {
                    PikiState::GrowUp {
                        frame,
                        second,
                        bloomed,
                    }
                };
            }
            _ => {}
        }
    }
}
