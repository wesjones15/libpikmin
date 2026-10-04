//! Onions (decomp `GoalItem`, goalItem.cpp; `GoalAI`, itemAI.cpp) and sprouts
//! (`PikiHeadItem`, pikiheadItem.cpp; `PikiHeadAI`, itemAI.cpp), plus captains plucking them
//! (`Navi::procActionButton`, `NaviNukuAdjustState`, `NaviNukuState`).
//!
//! An Onion takes a *set* of colours. Pikmin 1 has one Onion per colour; giving one Onion
//! several colours makes a shared Onion (Pikmin 3 style).

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::event::Event;
use crate::math::{ang_dist, dir_of, dist_xz, round_ang, yaw_of};
use crate::piki::{Happa, PikiColor};
use crate::sim::Sim;
use crate::{CaptainId, Host, PikiId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OnionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SproutId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnionAnim {
    Idle,
    /// `GOAL_Effect`: "suction" plays after something is sucked in.
    Suction {
        frame: f32,
    },
    /// "flowrot" looping; one seed per loop end (`GOAL_EmitPiki`).
    Emitting {
        frame: f32,
    },
}

#[derive(Debug, Clone)]
pub struct Onion {
    pub id: OnionId,
    pub pos: Vec3,
    pub colors: Vec<PikiColor>,
    /// Pikmin kept inside because the field was full (`mHeldPikis`), by colour index.
    pub stored: [u32; 3],
    pub anim: OnionAnim,
    /// Seeds waiting to be launched (`mSAICtx.mCurrAnimId`).
    pub queue: VecDeque<PikiColor>,
    /// Pending fireworks from negative seed counts (`mSAICtx.mCounter`).
    pub fireworks: u32,
}

impl Onion {
    pub fn accepts(&self, color: PikiColor) -> bool {
        self.colors.contains(&color)
    }

    /// `GoalItem::getSuckPos`
    pub fn suck_pos(&self) -> Vec3 {
        self.pos + Vec3::new(0.0, 74.0, 0.0)
    }

    /// `GoalItem::suckMe`
    pub(crate) fn receive(&mut self, color: PikiColor, seeds: i32) {
        if seeds < 0 {
            self.fireworks += 2;
        } else {
            self.queue
                .extend(std::iter::repeat_n(color, seeds as usize));
        }
        self.anim = OnionAnim::Suction { frame: 0.0 };
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SproutState {
    /// Launched from an Onion (`PIKIHEAD_Flying`).
    Flying,
    /// Under the ground, not yet visible (`PIKIHEAD_Bury`).
    Buried { timer: f32 },
    /// Seed showing, not pluckable yet (`PIKIHEAD_Tane`).
    Tane { timer: f32 },
    /// Pluckable, growing (`PIKIHEAD_Wait` → `Growuped`).
    Ready { timer: f32 },
    /// A flower wilted (`PIKIHEAD_Kareta`); reburies as a leaf.
    Wilted { timer: f32 },
}

#[derive(Debug, Clone)]
pub struct Sprout {
    pub id: SproutId,
    pub color: PikiColor,
    pub happa: Happa,
    pub pos: Vec3,
    pub vel: Vec3,
    air_resist: f32,
    pub state: SproutState,
    /// A captain is pulling this one.
    pub claimed_by: Option<CaptainId>,
}

impl Sprout {
    /// `PikiHeadItem::canPullout`
    pub fn pluckable(&self) -> bool {
        matches!(self.state, SproutState::Ready { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluckPhase {
    /// `NaviNukuAdjustState`: step to 6 units short of the sprout and face it.
    Adjust {
        approach: Vec3,
        face: f32,
        timer: f32,
    },
    /// `NaviNukuState`: the "NUKU" animation. The Pikmin comes out at `KEY_Action0`.
    Pull {
        frame: f32,
        released: bool,
        again: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pluck {
    pub sprout: SproutId,
    pub phase: PluckPhase,
}

impl Sim {
    /// Places an Onion that accepts `colors` (one colour = Pikmin 1 style).
    pub fn add_onion(&mut self, pos: Vec3, colors: &[PikiColor]) -> OnionId {
        assert!(!colors.is_empty(), "an Onion needs at least one colour");
        let id = OnionId(self.onions.len() as u32);
        self.onions.push(Onion {
            id,
            pos,
            colors: colors.to_vec(),
            stored: [0; 3],
            anim: OnionAnim::Idle,
            queue: VecDeque::new(),
            fireworks: 0,
        });
        id
    }

    pub fn onions(&self) -> &[Onion] {
        &self.onions
    }

    pub fn sprouts(&self) -> impl Iterator<Item = &Sprout> {
        self.sprouts.iter().flatten()
    }

    /// Plants a ready-to-pluck sprout directly (handy for hosts and tests).
    pub fn plant_sprout(&mut self, color: PikiColor, pos: Vec3) -> SproutId {
        let timer = self.params.piki.grow_up_time;
        self.insert_sprout(color, pos, Vec3::ZERO, 0.0, SproutState::Ready { timer })
    }

    fn insert_sprout(
        &mut self,
        color: PikiColor,
        pos: Vec3,
        vel: Vec3,
        air_resist: f32,
        state: SproutState,
    ) -> SproutId {
        let slot = self
            .sprouts
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.sprouts.len());
        let id = SproutId(slot as u32);
        let s = Sprout {
            id,
            color,
            happa: Happa::Leaf,
            pos,
            vel,
            air_resist,
            state,
            claimed_by: None,
        };
        if slot == self.sprouts.len() {
            self.sprouts.push(Some(s));
        } else {
            self.sprouts[slot] = Some(s);
        }
        id
    }

    /// Pikmin plus sprouts, against `aiconst` p15 ("ピキ＋芽 リミット").
    pub(crate) fn field_count(&self) -> usize {
        self.pikis.iter().filter(|p| p.on_field()).count() + self.sprouts.iter().flatten().count()
    }

    pub(crate) fn update_onions(&mut self, dt: f32) {
        let anim = self.params.anim.clone();
        for oi in 0..self.onions.len() {
            let o = &mut self.onions[oi];
            match o.anim {
                OnionAnim::Idle => {}
                OnionAnim::Suction { frame } => {
                    let f = frame + anim.fps * dt;
                    o.anim = if f >= anim.goal_suction.end {
                        OnionAnim::Emitting { frame: 0.0 }
                    } else {
                        OnionAnim::Suction { frame: f }
                    };
                }
                OnionAnim::Emitting { frame } => {
                    let mut f = frame + anim.fps * dt;
                    let (start, end) = anim
                        .goal_flowrot
                        .loop_range
                        .unwrap_or((0.0, anim.goal_flowrot.end));
                    if f >= end {
                        // `GoalAI::EmitPiki`: fireworks first, then one seed.
                        o.fireworks = o.fireworks.saturating_sub(1);
                        let seed = o.queue.pop_front();
                        if o.queue.is_empty() && o.fireworks == 0 {
                            o.anim = OnionAnim::Idle;
                        } else {
                            f -= end - start;
                            o.anim = OnionAnim::Emitting { frame: f };
                        }
                        if let Some(color) = seed {
                            self.emit_seed(oi, color);
                        }
                    } else {
                        o.anim = OnionAnim::Emitting { frame: f };
                    }
                }
            }
        }
    }

    /// `GoalAI::EmitPiki`: launch a seed from 110 above the Onion, or keep it inside if the
    /// field is full.
    fn emit_seed(&mut self, oi: usize, color: PikiColor) {
        let onion = self.onions[oi].id;
        if self.field_count() >= self.params.ai.max_pikis_on_field.max(0) as usize {
            self.onions[oi].stored[color.index()] += 1;
            self.events.push(Event::SeedStored { onion, color });
            return;
        }
        let dir = self.rng.f32() * TAU;
        let pos = self.onions[oi].pos + Vec3::new(0.0, 110.0, 0.0);
        let vel = Vec3::new(dir.sin() * 290.0, 800.0, dir.cos() * 290.0);
        // PikiHeadItem::startAI: enableAirResist(10 - 0.5 rand)
        let resist = 10.0 - 0.5 * self.rng.f32();
        let sprout = self.insert_sprout(color, pos, vel, resist, SproutState::Flying);
        self.events.push(Event::SeedLaunched {
            onion,
            sprout,
            color,
        });
    }

    /// `PikiHeadAI` states.
    pub(crate) fn update_sprouts(&mut self, host: &mut dyn Host, dt: f32) {
        let gravity = self.params.ai.gravity;
        let pp = self.params.piki.clone();
        for si in 0..self.sprouts.len() {
            let r = self.rng.f32() * 2.0;
            let Some(s) = self.sprouts[si].as_mut() else {
                continue;
            };
            match s.state {
                SproutState::Flying => {
                    // Creature::moveVelocity gravity + air drag (creatureMove.cpp).
                    s.vel.y -= gravity * dt;
                    s.vel.y -= dt * s.air_resist * s.vel.y;
                    s.vel.x -= dt * s.air_resist * 0.2 * s.vel.x;
                    s.vel.z -= dt * s.air_resist * 0.2 * s.vel.z;
                    s.pos += s.vel * dt;
                    if let Some(g) = host.ground_height(s.pos)
                        && s.pos.y <= g
                        && s.vel.y <= 0.0
                    {
                        s.pos.y = g;
                        s.vel = Vec3::ZERO;
                        s.state = SproutState::Buried {
                            timer: pp.seed_up_time + r,
                        };
                        self.events.push(Event::SproutPlanted { sprout: s.id });
                    }
                }
                SproutState::Buried { timer } => {
                    let t = timer - dt;
                    s.state = if t < 0.0 {
                        SproutState::Tane {
                            timer: pp.pluck_wait_time + r,
                        }
                    } else {
                        SproutState::Buried { timer: t }
                    };
                }
                SproutState::Tane { timer } => {
                    let t = timer - dt;
                    s.state = if t < 0.0 {
                        SproutState::Ready {
                            timer: pp.grow_up_time + r,
                        }
                    } else {
                        SproutState::Tane { timer: t }
                    };
                }
                SproutState::Ready { timer } => {
                    let t = timer - dt;
                    if t >= 0.0 || s.claimed_by.is_some() {
                        s.state = SproutState::Ready { timer: t };
                    } else if s.happa == Happa::Flower {
                        s.happa = Happa::Leaf;
                        s.state = SproutState::Wilted {
                            timer: pp.rebirth_seed_time + r,
                        };
                        self.events.push(Event::SproutWilted { sprout: s.id });
                    } else {
                        s.happa = if s.happa == Happa::Leaf {
                            Happa::Bud
                        } else {
                            Happa::Flower
                        };
                        let next = if s.happa == Happa::Flower {
                            pp.wilt_time
                        } else {
                            pp.grow_up_time
                        };
                        s.state = SproutState::Ready { timer: next + r };
                        self.events.push(Event::SproutGrew {
                            sprout: s.id,
                            happa: s.happa,
                        });
                    }
                }
                SproutState::Wilted { timer } => {
                    let t = timer - dt;
                    s.state = if t < 0.0 {
                        SproutState::Buried {
                            timer: pp.seed_up_time + r,
                        }
                    } else {
                        SproutState::Wilted { timer: t }
                    };
                }
            }
        }
    }

    /// `Navi::procActionButton`'s pluck search: nearest pluckable sprout within reach
    /// (`action_radius` within 220 of an Onion, else `pluck_distance_outside_onyon`).
    pub(crate) fn find_pluckable(&self, ci: usize) -> Option<SproutId> {
        let c = &self.captains[ci];
        let near_onion = self.onions.iter().any(|o| dist_xz(o.pos, c.pos) <= 220.0);
        let reach = if near_onion {
            self.params.navi.action_radius
        } else {
            self.params.navi.pluck_distance_outside_onyon
        };
        self.sprouts
            .iter()
            .flatten()
            .filter(|s| s.pluckable() && s.claimed_by.is_none() && (s.pos.y - c.pos.y).abs() < 25.0)
            .map(|s| (s.id, dist_xz(s.pos, c.pos)))
            .filter(|&(_, d)| d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    pub(crate) fn start_pluck(&mut self, ci: usize, sprout: SproutId) {
        let cid = self.captains[ci].id;
        let Some(s) = self.sprouts[sprout.0 as usize].as_mut() else {
            return;
        };
        s.claimed_by = Some(cid);
        let c = &mut self.captains[ci];
        let to = s.pos - c.pos;
        let dir = Vec3::new(to.x, 0.0, to.z).normalize_or(dir_of(c.face));
        c.pluck = Some(Pluck {
            sprout,
            phase: PluckPhase::Adjust {
                approach: s.pos - dir * 6.0,
                face: yaw_of(dir),
                timer: 2.0,
            },
        });
    }

    /// Runs a captain's pluck. Returns true while plucking (movement is locked).
    pub(crate) fn update_pluck(&mut self, ci: usize, dt: f32, press_again: bool) -> bool {
        let Some(pl) = self.captains[ci].pluck else {
            return false;
        };
        let alive = self
            .sprouts
            .get(pl.sprout.0 as usize)
            .is_some_and(Option::is_some);
        let fps = self.params.anim.fps;
        let nuku = self.params.anim.nuku;
        let cid = self.captains[ci].id;

        match pl.phase {
            PluckPhase::Adjust {
                approach,
                face,
                timer,
            } => {
                if !alive || timer <= 0.0 {
                    return self.cancel_pluck(ci);
                }
                let c = &mut self.captains[ci];
                let ang = ang_dist(face, c.face);
                let to = Vec3::new(approach.x - c.pos.x, 0.0, approach.z - c.pos.z);
                let len = to.length();
                if ang.abs() < PI / 10.0 && len < 1.0 {
                    c.vel = Vec3::ZERO;
                    c.target_vel = Vec3::ZERO;
                    c.pluck = Some(Pluck {
                        phase: PluckPhase::Pull {
                            frame: 0.0,
                            released: false,
                            again: false,
                        },
                        ..pl
                    });
                } else {
                    let mut v = ang * 0.2;
                    if v.abs() < PI / 20.0 {
                        v = (PI / 20.0).copysign(ang);
                    }
                    if ang.abs() < PI / 20.0 {
                        v = ang;
                    }
                    c.face = round_ang(c.face + v);
                    // 100 u/s, slowing to 0.5 units per tick when close.
                    let speed = if dt * 100.0 > len { 0.5 / dt } else { 100.0 };
                    let step = (to.normalize_or_zero() * speed * dt).clamp_length_max(len);
                    c.pos += step;
                    c.vel = step / dt;
                    c.target_vel = Vec3::ZERO;
                    c.pluck = Some(Pluck {
                        phase: PluckPhase::Adjust {
                            approach,
                            face,
                            timer: timer - dt,
                        },
                        ..pl
                    });
                }
                true
            }
            PluckPhase::Pull {
                frame,
                mut released,
                again,
            } => {
                let again = again || press_again;
                let f = frame + fps * dt;
                if !released && f >= nuku.action0 {
                    released = true;
                    if let Some(s) = self.sprouts[pl.sprout.0 as usize].take() {
                        let piki = self.spawn_plucked(s.color, s.happa, s.pos, cid);
                        if let Some(piki) = piki {
                            self.events.push(Event::Plucked {
                                captain: cid,
                                piki,
                                sprout: s.id,
                            });
                        }
                    }
                }
                let c = &mut self.captains[ci];
                c.vel = Vec3::ZERO;
                c.target_vel = Vec3::ZERO;
                if f >= nuku.end {
                    c.pluck = None;
                    // Holding the button again plucks the next one (`mWantsNextPluck`).
                    if again && let Some(next) = self.find_pluckable(ci) {
                        self.start_pluck(ci, next);
                    }
                } else {
                    c.pluck = Some(Pluck {
                        phase: PluckPhase::Pull {
                            frame: f,
                            released,
                            again,
                        },
                        ..pl
                    });
                }
                true
            }
        }
    }

    fn cancel_pluck(&mut self, ci: usize) -> bool {
        if let Some(pl) = self.captains[ci].pluck.take()
            && let Some(Some(s)) = self.sprouts.get_mut(pl.sprout.0 as usize)
        {
            s.claimed_by = None;
        }
        false
    }

    fn spawn_plucked(
        &mut self,
        color: PikiColor,
        happa: Happa,
        pos: Vec3,
        captain: CaptainId,
    ) -> Option<PikiId> {
        let id = self.spawn_piki(color, pos)?;
        let p = &mut self.pikis[id.0 as usize];
        p.happa = happa;
        p.vel = Vec3::new(0.0, 120.0, 0.0);
        p.on_ground = false;
        self.join_squad(id, captain);
        Some(id)
    }
}
