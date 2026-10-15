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
use crate::piki::{Happa, PikiColor, PikiState};
use crate::sim::Sim;
use crate::{CaptainId, Host, PikiId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OnionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SproutId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnionAnim {
    Idle,
    /// Host-controlled airborne following. Storage waits until grounded; deliveries are taken (seeds become stored Pikmin).
    Hover,
    /// `GOAL_Effect`: "suction" plays after something is sucked in.
    Suction {
        frame: f32,
    },
    /// "flowrot" looping; one seed per loop end (`GOAL_EmitPiki`).
    Emitting {
        frame: f32,
    },
    /// Flying off (`GoalItem::startTakeoff`, `UfoItem::startTakeoff`): the motion carries it
    /// up out of sight.
    TakeOff {
        frame: f32,
    },
    /// Gone from the field until it lands.
    Away,
    /// Coming back down (`GoalItem::startLand`). The ship has no landing motion in the
    /// game (it's simply back the next day); it takes as long as its take off, which hosts
    /// can play backwards.
    Landing {
        frame: f32,
    },
}

#[derive(Debug, Clone)]
pub struct Onion {
    pub id: OnionId,
    pub pos: Vec3,
    pub colors: Vec<PikiColor>,
    /// Pikmin inside (`mHeldPikis`), by colour index then leaf stage (`Happa as usize`).
    pub stored: [[u32; 3]; 3],
    /// Pikmin waiting to come out for a captain (`mPikisToExit`), in order.
    pub exits: VecDeque<(PikiColor, crate::CaptainId)>,
    pub(crate) exit_timer: f32,
    pub anim: OnionAnim,
    /// Optional host-timed transition (landing, normalized progress). The host moves the
    /// object; presentation removes the disc clip's large cinematic travel.
    pub flight_motion: Option<(bool, f32)>,
    /// Seeds waiting to be launched (`mSAICtx.mCurrAnimId`).
    pub queue: VecDeque<PikiColor>,
    /// Pending fireworks from negative seed counts (`mSAICtx.mCounter`).
    pub fireworks: u32,
    /// Set for the ship (`UfoItem`): it takes ship parts instead of pellets.
    pub ship: Option<Ship>,
    /// Host extension: the captain it belongs to. Its plucker's Pikmin join him, and its
    /// emergency Pikmin come for him (see [`Sim::set_onion_owner`]).
    pub owner: Option<CaptainId>,
    /// Seconds its owner has had no Pikmin anywhere (see [`Sim::set_emergency_recovery`]).
    pub(crate) extinct_for: f32,
    /// Host extension: disabled Onions fold shut, wait, then replay their disc startup.
    pub dormancy: Option<OnionDormancy>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnionDormancy {
    pub remaining: f32,
    /// 0 is the dormant pose, 1 the live end of the startup clip.
    pub progress: f32,
    pub reviving: bool,
}

/// The S.S. Dolphin as a carry goal (`UfoItem`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ship {
    /// Where carriers bring parts (`UfoItem::getGoalPos`, its 'gol1' sphere).
    pub goal: Vec3,
    /// Where parts are pulled up to (`getSuckPos`, 'suc1').
    pub suck: Vec3,
    /// Part indices on board, in the order they arrived.
    pub parts: Vec<u32>,
}

impl Onion {
    pub fn accepts(&self, color: PikiColor) -> bool {
        self.colors.contains(&color)
    }

    /// Pikmin of `color` inside, all leaf stages.
    pub fn stored_of(&self, color: PikiColor) -> u32 {
        self.stored[color.index()].iter().sum()
    }

    pub fn stored_total(&self) -> u32 {
        self.stored.iter().flatten().sum()
    }

    /// `GoalItem::getSuckPos` (`UfoItem::getSuckPos` for the ship).
    pub fn suck_pos(&self) -> Vec3 {
        match &self.ship {
            Some(s) => s.suck,
            None => self.pos + Vec3::new(0.0, 74.0, 0.0),
        }
    }

    /// On the ground: it can be used, delivered to and called from. Not while taking off,
    /// away, or landing.
    pub fn grounded(&self) -> bool {
        self.dormancy.is_none() && !matches!(self.anim, OnionAnim::Hover | OnionAnim::TakeOff { .. } | OnionAnim::Away | OnionAnim::Landing { .. })
    }

    /// Carriers can bring things to it: on the ground, or hovering after the host lifted it
    /// (a hovering Onion still takes pellets, but turns the seeds straight into stored Pikmin).
    pub fn takes_deliveries(&self) -> bool {
        self.dormancy.is_none() && (self.grounded() || self.anim == OnionAnim::Hover)
    }

    /// Where carriers bring things (`getGoalPos`).
    pub fn goal_pos(&self) -> Vec3 {
        self.ship.as_ref().map_or(self.pos, |s| s.goal)
    }

    /// `GoalItem::suckMe`
    pub(crate) fn receive(&mut self, color: PikiColor, seeds: i32) {
        if self.anim == OnionAnim::Hover {
            // Hovering, it has nowhere to plant: the new Pikmin stay inside as leaves.
            if seeds > 0 {
                self.stored[color.index()][Happa::Leaf as usize] += seeds as u32;
            }
            return;
        }
        if seeds < 0 {
            self.fireworks += 2;
        } else {
            self.queue
                .extend(std::iter::repeat_n(color, seeds as usize));
        }
        // Something finishing going in as it leaves: the seeds wait for it to land.
        if self.grounded() {
            self.anim = OnionAnim::Suction { frame: 0.0 };
        }
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
    pub(crate) air_resist: f32,
    pub state: SproutState,
    /// The Onion that shot it (`mParentOnion`): its sounds play on that Onion's emitter.
    pub onion: Option<OnionId>,
    /// A captain is pulling this one.
    pub claimed_by: Option<CaptainId>,
    /// Seconds it has stood pluckable with nobody pulling it (see [`Sim::set_auto_pluck`]).
    pub unplucked: f32,
}

impl Sprout {
    /// `PikiHeadItem::canPullout`
    pub fn pluckable(&self) -> bool {
        matches!(self.state, SproutState::Ready { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluckPhase {
    /// `NaviNukuAdjustState`: step to 6 units short of the sprout and face it. Gives up if the
    /// captain moves less than 4 units in 0.4 s (`mOdoMeter.start(0.4, 4.0)`).
    Adjust {
        approach: Vec3,
        face: f32,
        odo_timer: f32,
        odo_dist: f32,
    },
    /// `NaviNukuState`: "NUKU" (or "NUKU fast" when chaining). The Pikmin comes out at
    /// `KEY_Action0`. Letting go of the button and then pressing or holding it again asks for
    /// the next sprout (`mExtractKeyReleased`, `mWantsNextPluck`).
    Pull {
        frame: f32,
        popped: bool,
        let_go: bool,
        again: bool,
        fast: bool,
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
            stored: [[0; 3]; 3],
            exits: VecDeque::new(),
            exit_timer: 0.0,
            anim: OnionAnim::Idle,
            flight_motion: None,
            queue: VecDeque::new(),
            fireworks: 0,
            ship: None,
            owner: None,
            extinct_for: 0.0,
            dormancy: None,
        });
        id
    }

    /// Places the ship. `goal` and `suck` are world positions of its delivery point and of
    /// where parts are pulled in (the host knows its model: 'gol1' / 'suc1').
    pub fn add_ship(&mut self, pos: Vec3, goal: Vec3, suck: Vec3) -> OnionId {
        let id = OnionId(self.onions.len() as u32);
        self.onions.push(Onion {
            id,
            pos,
            colors: Vec::new(),
            stored: [[0; 3]; 3],
            exits: VecDeque::new(),
            exit_timer: 0.0,
            anim: OnionAnim::Idle,
            flight_motion: None,
            queue: VecDeque::new(),
            fireworks: 0,
            ship: Some(Ship { goal, suck, parts: Vec::new() }),
            owner: None,
            extinct_for: 0.0,
            dormancy: None,
        });
        id
    }

    pub fn onions(&self) -> &[Onion] {
        &self.onions
    }

    /// Host-controlled flight/landing. Takeoff resolves climbers before moving the goal;
    /// stock, queued exits and seeds survive the flight. Hosts supply terrain-safe poses.
    pub fn set_onion_pose(&mut self, onion: OnionId, pos: Vec3, flying: bool) {
        if !pos.is_finite() || self.onions.get(onion.0 as usize).is_none() { return; }
        if self.onions[onion.0 as usize].dormancy.is_some() { return; }
        if flying && self.onions[onion.0 as usize].grounded() {
            self.onion_take_off(onion);
            // Finish exits at the old feet before moving the Onion's airborne legs.
            for p in &mut self.pikis {
                if let PikiState::Exit { onion: on, leg, .. } = p.state
                    && on == onion {
                    p.pos = self.onions[onion.0 as usize].pos + crate::onion_menu::LEG_FEET[leg as usize % 3];
                    p.on_ground = true;
                    p.state = if p.captain.is_some() { PikiState::squad() } else { PikiState::Idle };
                }
            }
        }
        let o = &mut self.onions[onion.0 as usize];
        o.flight_motion = None;
        let delta = pos - o.pos;
        o.pos = pos;
        if let Some(s) = o.ship.as_mut() { s.goal += delta; s.suck += delta; }
        if flying {
            o.anim = OnionAnim::Hover;
        } else if !o.grounded() {
            o.anim = if !o.queue.is_empty() || o.fireworks > 0 {
                OnionAnim::Emitting { frame: 0.0 }
            } else { OnionAnim::Idle };
            self.events.push(Event::OnionLanded { onion });
        }
    }

    /// RoR2 extension. Stock survives; workers and unfinished deliveries are released.
    /// Visual reference: GoalAI::BootInit / GoalItem::startBoot, not a second model.
    pub fn make_onion_dormant(&mut self, onion: OnionId, seconds: f32, ground: Vec3) {
        if !seconds.is_finite() || !ground.is_finite() { return; }
        let Some(o) = self.onions.get_mut(onion.0 as usize) else { return };
        if o.ship.is_some() || o.dormancy.is_some() { return; }
        o.pos = ground;
        o.flight_motion = None;
        o.anim = OnionAnim::Idle;
        o.dormancy = Some(OnionDormancy { remaining: seconds.max(0.0), progress: 1.0, reviving: false });
        o.exits.clear(); // Queued exits have not yet been deducted from stock.
        for p in &mut self.pikis {
            if matches!(p.state, PikiState::Enter { onion: id, .. } | PikiState::Exit { onion: id, .. } if id == onion) {
                p.state = PikiState::Idle;
                p.on_ground = false;
            }
        }
        // Release all holders of jobs with no remaining live destination. Re-evaluate
        // automatically after revival; no Pikmin spends the downtime gripping a pellet.
        let jobs: Vec<_> = self.pellets.iter().filter(|p| !self.pellet_has_destination(p))
            .map(|p| p.id).collect();
        for id in jobs {
            let workers: Vec<_> = self.pikis.iter().filter(|p| matches!(p.state, PikiState::Carry(c) if c.pellet == id))
                .map(|p| p.id).collect();
            for worker in workers { self.leave_current(worker); self.pikis[worker.0 as usize].state = PikiState::Idle; }
        }
        for p in &mut self.pellets {
            if matches!(p.state, crate::PelletState::Sucking { onion: id, .. } if id == onion) {
                p.state = crate::PelletState::Idle;
                p.scale = 1.0;
            }
        }
    }

    /// Hovering host pickup after descending and dwelling. Loose pellets are allowed as
    /// an extinction recovery aid. Never steals a pellet carried towards another Onion.
    pub fn collect_ground_pellet(&mut self, onion: OnionId, pellet: crate::PelletId) -> bool {
        let Some(o) = self.onions.get(onion.0 as usize) else { return false };
        let Some(p) = self.pellets.get(pellet.0 as usize) else { return false };
        if o.ship.is_some() || !o.takes_deliveries() || o.anim != OnionAnim::Hover
            || p.desc.ship_part.is_some() || p.state != crate::PelletState::Idle
            || dist_xz(o.pos, p.pos) > 20.0 || (o.pos.y - p.pos.y).abs() > 90.0 { return false; }
        let color = p.desc.color.filter(|&c| o.accepts(c)).unwrap_or(o.colors[0]);
        let p = &mut self.pellets[pellet.0 as usize];
        let workers = std::mem::take(&mut p.carriers);
        p.slots.iter_mut().for_each(|s| *s = None);
        p.state = crate::PelletState::Sucking { onion, color, wait: 1.5, start: p.pos, progress: 0.0, speed: 0.0 };
        self.events.push(Event::PelletArrived { pellet, onion });
        for id in workers { self.pikis[id.0 as usize].state = PikiState::Idle; }
        true
    }

    /// The first accepted colour controls only the model/animation palette. Reordering
    /// leaves the accepted set and every colour's stock unchanged.
    pub fn set_onion_color(&mut self, onion: OnionId, color: PikiColor) {
        if let Some(o) = self.onions.get_mut(onion.0 as usize)
            && let Some(index) = o.colors.iter().position(|c| *c == color) {
            o.colors.swap(0, index);
        }
    }

    /// Select a disc transition while the host supplies a safe trajectory. This never
    /// makes an airborne goal usable: landing must finish through set_onion_pose.
    pub fn set_onion_flight_motion(&mut self, onion: OnionId, landing: bool, progress: f32) {
        if !progress.is_finite() { return; }
        if let Some(o) = self.onions.get_mut(onion.0 as usize)
            && o.anim == OnionAnim::Hover {
            o.flight_motion = Some((landing, progress.clamp(0.0, 1.0)));
        }
    }

    pub fn sprouts(&self) -> impl Iterator<Item = &Sprout> {
        self.sprouts.iter().flatten()
    }

    /// Hands an Onion `seeds` seeds of `color` to launch, as if something had been delivered
    /// (a host reward, or for testing). Plays the suction reaction first, like `suckMe`.
    pub fn give_onion_seeds(&mut self, onion: OnionId, color: PikiColor, seeds: u32) {
        if let Some(o) = self.onions.get_mut(onion.0 as usize) {
            o.receive(color, seeds as i32);
        }
    }

    /// Puts ship part `part` on `ship` directly (hosts restoring a save, tests).
    pub fn give_ship_part(&mut self, ship: OnionId, part: u32) {
        if let Some(s) = self.onions.get_mut(ship.0 as usize).and_then(|o| o.ship.as_mut())
            && !s.parts.contains(&part)
        {
            s.parts.push(part);
        }
    }

    /// Plants a ready-to-pluck sprout directly (handy for hosts and tests).
    pub fn plant_sprout(&mut self, color: PikiColor, pos: Vec3) -> SproutId {
        let timer = self.sprout_stage.unwrap_or(self.params.piki.grow_up_time);
        self.insert_sprout(color, pos, Vec3::ZERO, 0.0, SproutState::Ready { timer }, None)
    }

    fn insert_sprout(
        &mut self,
        color: PikiColor,
        pos: Vec3,
        vel: Vec3,
        air_resist: f32,
        state: SproutState,
        onion: Option<OnionId>,
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
            onion,
            claimed_by: None,
            unplucked: 0.0,
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
        self.pikis.iter().filter(|p| p.on_field()).count()
            + self.sprouts.iter().flatten().count()
            + self.onions.iter().map(|o| o.exits.len()).sum::<usize>()
    }

    /// Sends an Onion (or the ship) off the field, as at the end of the day
    /// (`DemoEventOnion::Takeoff` / `DemoEventUFO::Takeoff`). Pikmin climbing in finish going
    /// in; ones sliding out finish getting off. Seeds and Pikmin waiting to come out stay
    /// inside until it lands.
    pub fn onion_take_off(&mut self, onion: OnionId) {
        let Some(o) = self.onions.get_mut(onion.0 as usize) else { return };
        if !o.grounded() {
            return;
        }
        o.anim = OnionAnim::TakeOff { frame: 0.0 };
        for pi in 0..self.pikis.len() {
            match self.pikis[pi].state {
                PikiState::Enter { onion: on, .. } if on == onion => {
                    // `GoalItem::enterGoal`
                    let p = &mut self.pikis[pi];
                    let (color, happa, id) = (p.color, p.happa, p.id);
                    p.state = PikiState::Dead;
                    self.onions[onion.0 as usize].stored[color.index()][happa as usize] += 1;
                    self.events.push(Event::PikiEnteredOnion { piki: id, onion });
                }
                PikiState::Exit { onion: on, leg, .. } if on == onion => {
                    let p = &mut self.pikis[pi];
                    p.state = PikiState::Exit { onion, leg, ratio: 0.0 };
                }
                _ => {}
            }
        }
    }

    /// Brings an Onion (or the ship) back down (`DemoEventOnion::Land`).
    pub fn onion_land(&mut self, onion: OnionId) {
        let Some(o) = self.onions.get_mut(onion.0 as usize) else { return };
        if o.anim != OnionAnim::Away {
            return;
        }
        o.anim = OnionAnim::Landing { frame: 0.0 };
    }

    pub(crate) fn update_onions(&mut self, dt: f32) {
        let anim = self.params.anim.clone();
        // Cooldown items (the host's growth rate) send seeds out sooner after feeding.
        let feed = dt * self.growth_rate;
        for oi in 0..self.onions.len() {
            let o = &mut self.onions[oi];
            if let Some(d) = o.dormancy.as_mut() {
                if d.reviving {
                    d.progress = (d.progress + dt / 3.0).min(1.0);
                    if d.progress >= 1.0 {
                        o.dormancy = None;
                        o.anim = if !o.queue.is_empty() || o.fireworks > 0 {
                            OnionAnim::Emitting { frame: 0.0 }
                        } else { OnionAnim::Idle };
                        self.events.push(Event::OnionLanded { onion: o.id });
                    }
                } else {
                    d.remaining = (d.remaining - dt).max(0.0);
                    d.progress = (d.progress - dt / 1.5).max(0.0);
                    if d.remaining <= 0.0 && d.progress <= 0.0 { d.reviving = true; }
                }
                continue;
            }
            let colour = o.colors.first().map_or(0, |c| c.index());
            match o.anim {
                OnionAnim::Idle | OnionAnim::Away | OnionAnim::Hover => {}
                OnionAnim::TakeOff { frame } => {
                    let f = frame + anim.fps * dt;
                    let end = if o.ship.is_some() { anim.ship_takeoff } else { anim.goal_takeoff[colour] };
                    o.anim = if f >= end { OnionAnim::Away } else { OnionAnim::TakeOff { frame: f } };
                }
                OnionAnim::Landing { frame } => {
                    let f = frame + anim.fps * dt;
                    let end = if o.ship.is_some() { anim.ship_takeoff } else { anim.goal_land[colour] };
                    if f >= end {
                        // Seeds it was holding come out now.
                        let busy = !o.queue.is_empty() || o.fireworks > 0;
                        o.anim = if busy { OnionAnim::Emitting { frame: 0.0 } } else { OnionAnim::Idle };
                        let onion = o.id;
                        self.events.push(Event::OnionLanded { onion });
                    } else {
                        o.anim = OnionAnim::Landing { frame: f };
                    }
                }
                OnionAnim::Suction { frame } => {
                    let f = frame + anim.fps * feed;
                    o.anim = if f >= anim.goal_suction.end {
                        OnionAnim::Emitting { frame: 0.0 }
                    } else {
                        OnionAnim::Suction { frame: f }
                    };
                }
                OnionAnim::Emitting { frame } => {
                    let mut f = frame + anim.fps * feed;
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
            self.onions[oi].stored[color.index()][Happa::Leaf as usize] += 1;
            self.events.push(Event::SeedStored { onion, color });
            return;
        }
        let dir = self.rng.f32() * TAU;
        let pos = self.onions[oi].pos + Vec3::new(0.0, 110.0, 0.0);
        let vel = Vec3::new(dir.sin() * 290.0, 800.0, dir.cos() * 290.0);
        // PikiHeadItem::startAI: enableAirResist(10 - 0.5 rand)
        let resist = 10.0 - 0.5 * self.rng.f32();
        let sprout = self.insert_sprout(color, pos, vel, resist, SproutState::Flying, Some(onion));
        self.events.push(Event::SeedLaunched {
            onion,
            sprout,
            color,
        });
    }

    /// `PikiHeadAI` states.
    pub(crate) fn update_sprouts(&mut self, host: &mut dyn Host, dt: f32) {
        let gravity = self.params.ai.gravity;
        let mut pp = self.params.piki.clone();
        let grow = dt * self.growth_rate;
        // Host-fixed stage and flower times replace the disc's, and their random extra.
        let fixed_stage = self.sprout_stage.is_some();
        let fixed_flower = self.sprout_flower.is_some();
        pp.grow_up_time = self.sprout_stage.unwrap_or(pp.grow_up_time);
        pp.wilt_time = self.sprout_flower.unwrap_or(pp.wilt_time);
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
                    let from_y = s.pos.y;
                    s.pos += s.vel * dt;
                    // From above both ends of the tick's fall, so a fast seed can't pass the
                    // floor between probes.
                    let probe = Vec3::new(s.pos.x, s.pos.y.max(from_y) + 1.0, s.pos.z);
                    if let Some(g) = host.ground_height(probe)
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
                    let t = timer - grow;
                    s.state = if t < 0.0 {
                        SproutState::Tane {
                            timer: pp.pluck_wait_time + r,
                        }
                    } else {
                        SproutState::Buried { timer: t }
                    };
                }
                SproutState::Tane { timer } => {
                    let t = timer - grow;
                    s.state = if t < 0.0 {
                        SproutState::Ready {
                            timer: pp.grow_up_time + if fixed_stage { 0.0 } else { r },
                        }
                    } else {
                        SproutState::Tane { timer: t }
                    };
                }
                SproutState::Ready { timer } => {
                    if s.claimed_by.is_none() {
                        s.unplucked += dt;
                    }
                    // Cooldown items hurry leaves and buds along, not a flower's wilting.
                    let t = timer - if s.happa == Happa::Flower { dt } else { grow };
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
                        let (next, fixed) = if s.happa == Happa::Flower {
                            (pp.wilt_time, fixed_flower)
                        } else {
                            (pp.grow_up_time, fixed_stage)
                        };
                        s.state = SproutState::Ready { timer: next + if fixed { 0.0 } else { r } };
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
        let near_onion = self.onions.iter().any(|o| o.ship.is_none() && dist_xz(o.pos, c.pos) <= 220.0);
        let reach = if c.fast_pluck {
            // Chaining: `mContinuousPluckDistance` (200).
            self.params.navi.continuous_pluck_distance
        } else if near_onion {
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
                odo_timer: 0.4,
                odo_dist: 0.0,
            },
        });
    }

    /// Runs a captain's pluck. `held` / `pressed`: the action button's level and edge.
    /// Returns true while plucking (movement is locked).
    pub(crate) fn update_pluck(&mut self, ci: usize, dt: f32, held: bool, pressed: bool) -> bool {
        let Some(pl) = self.captains[ci].pluck else {
            return false;
        };
        let alive = self
            .sprouts
            .get(pl.sprout.0 as usize)
            .is_some_and(Option::is_some);
        let fps = self.params.anim.fps;
        let cid = self.captains[ci].id;

        match pl.phase {
            PluckPhase::Adjust {
                approach,
                face,
                odo_timer,
                odo_dist,
            } => {
                if !alive {
                    return self.cancel_pluck(ci);
                }
                let fast = self.captains[ci].fast_pluck;
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
                            popped: false,
                            let_go: false,
                            again: false,
                            fast,
                        },
                        ..pl
                    });
                    return true;
                }
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
                // Odometer: abort if stuck (moved < 4 in 0.4 s) while still far off.
                let mut odo_timer = odo_timer - dt;
                let mut odo_dist = odo_dist + step.length();
                if odo_timer <= 0.0 {
                    if odo_dist < 4.0 && len >= 1.0 && ang.abs() < PI / 10.0 {
                        return self.cancel_pluck(ci);
                    }
                    odo_timer = 0.4;
                    odo_dist = 0.0;
                }
                let c = &mut self.captains[ci];
                c.pluck = Some(Pluck {
                    phase: PluckPhase::Adjust {
                        approach,
                        face,
                        odo_timer,
                        odo_dist,
                    },
                    ..pl
                });
                true
            }
            PluckPhase::Pull {
                frame,
                mut popped,
                let_go,
                again,
                fast,
            } => {
                let anim = if fast {
                    self.params.anim.nuku_fast
                } else {
                    self.params.anim.nuku
                };
                let let_go = let_go || !held;
                let hold_chains = self.params.extras.hold_to_chain_pluck;
                let again = again || pressed || (held && (let_go || hold_chains));
                let f = frame + fps * dt * self.captains[ci].attack_rate;
                if !popped && f >= anim.action0 {
                    popped = true;
                    if let Some(s) = self.sprouts[pl.sprout.0 as usize].take()
                        && let Some(piki) = self.spawn_plucked(s.color, s.happa, s.pos, cid)
                    {
                        self.events.push(Event::Plucked {
                            captain: cid,
                            piki,
                            sprout: s.id,
                        });
                    }
                }
                let c = &mut self.captains[ci];
                c.vel = Vec3::ZERO;
                c.target_vel = Vec3::ZERO;
                if f >= anim.end {
                    c.pluck = None;
                    // `KEY_Finished` with `mWantsNextPluck`: look again, wider, and pull faster.
                    c.fast_pluck = again;
                    if again {
                        match self.find_pluckable(ci) {
                            Some(next) => self.start_pluck(ci, next),
                            None => self.captains[ci].fast_pluck = false,
                        }
                    }
                } else {
                    c.pluck = Some(Pluck {
                        phase: PluckPhase::Pull {
                            frame: f,
                            popped,
                            let_go,
                            again,
                            fast,
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
