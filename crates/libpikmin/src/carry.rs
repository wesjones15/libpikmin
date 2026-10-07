//! Carrying: pellets and corpses (decomp `Pellet`, pelletMgr.cpp / pelletState.cpp) and the
//! Pikmin side of it (`ActTransport`, aiTransport.cpp).
//!
//! A dead enemy in Pikmin 1 becomes a pellet (`BTeki::dieSoon` → `becomePellet`,
//! plugPikiNakata/tekibteki.cpp), so corpses and pellets share all of this. Hosts spawn one with
//! [`crate::Sim::spawn_pellet`] when a creature dies and drive their corpse object from
//! [`Pellet::pos`] / [`Pellet::yaw`] / [`Pellet::scale`] until [`crate::Event::PelletDelivered`].

use std::f32::consts::TAU;

use glam::Vec3;

use crate::event::Event;
use crate::host::CreatureId;
use crate::math::{ang_dist, dist_xz, rotate_y, round_ang, yaw_of};
use crate::onion::OnionId;
use crate::piki::{Happa, PikiColor, PikiState};
use crate::sim::Sim;
use crate::{Host, PikiId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PelletId(pub u32);

/// What a host provides when spawning something carryable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PelletDesc {
    pub pos: Vec3,
    pub yaw: f32,
    /// Carriers stand on a circle of this radius (`Pellet::getBottomRadius`).
    pub radius: f32,
    /// Height of the object (`getCylinderHeight`), for collision with thrown Pikmin.
    pub height: f32,
    /// `PelletConfig` p01 / p02.
    pub min_carriers: u32,
    pub max_carriers: u32,
    /// `PelletConfig` p06 / p07: seeds when delivered to an Onion of the pellet's colour, and
    /// otherwise. Negative = fireworks, no seeds (`GoalItem::suckMe`).
    pub seeds_matching: i32,
    pub seeds_other: i32,
    /// Numbered pellets have a colour; corpses don't and always use `seeds_matching`.
    pub color: Option<PikiColor>,
    /// The host creature this corpse came from, if any.
    pub creature: Option<CreatureId>,
    /// A ship part (its index in `_ufoIDTable`): carried to the ship, not an Onion
    /// (`ActTransport::decideGoal`: `PELTYPE_UfoPart` → `itemMgr->getUfo()`).
    pub ship_part: Option<u32>,
}

/// The numbered pellets' values, from the disc's `pelMgr.bin` (`p{b,r,y}{01,05,10,20}`).
pub const PELLET_NUMBERS: [u32; 4] = [1, 5, 10, 20];

/// Bottom radius of the 1, 5, 10 and 20 pellets: their models' 'cent' collision sphere at the
/// config's scale of 1 (`Pellet::getBottomRadius` → `getCentreSize`, `Pellet::initPellet`).
const PELLET_RADII: [f32; 4] = [10.0, 20.0, 35.0, 48.0];

impl PelletDesc {
    /// A numbered pellet: the largest of 1, 5, 10 and 20 not above `number`, with that pellet's
    /// disc config for `color` (carriers p01/p02, seeds p06/p07, height p11 as
    /// `Pellet::getCylinderHeight` without a view). The code defaults are the disc's values.
    pub fn numbered(params: &crate::Params, number: u32, color: PikiColor, pos: Vec3, yaw: f32) -> PelletDesc {
        let i = PELLET_NUMBERS.iter().rposition(|&n| n <= number).unwrap_or(0);
        let n = PELLET_NUMBERS[i];
        let letter = match color {
            PikiColor::Blue => 'b',
            PikiColor::Red => 'r',
            PikiColor::Yellow => 'y',
        };
        let cfg = crate::pellet_config::find(&params.pellets, &format!("p{letter}{n:02}"));
        let (max, seeds, height) = [(2, (2, 1), 7.6), (10, (5, 3), 14.0), (20, (10, 6), 27.0), (50, (20, 10), 36.5)][i];
        PelletDesc {
            pos,
            yaw,
            radius: PELLET_RADII[i],
            height: cfg.map_or(height, |c| c.carry_info_height),
            min_carriers: cfg.map_or(n, |c| c.min_carriers.max(1) as u32),
            max_carriers: cfg.map_or(max, |c| c.max_carriers.max(1) as u32),
            seeds_matching: cfg.map_or(seeds.0, |c| c.seeds_matching),
            seeds_other: cfg.map_or(seeds.1, |c| c.seeds_other),
            color: Some(color),
            creature: None,
            ship_part: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PelletState {
    /// On the ground, maybe with Pikmin trying to lift it.
    Idle,
    /// Lifted and moving to `onion` along `path` (from [`Host::carry_path`]).
    Carried {
        onion: OnionId,
        color: PikiColor,
        path: Vec<Vec3>,
        next: usize,
    },
    /// `PelletGoalState`: settled under the Onion, then pulled up into it.
    Sucking {
        onion: OnionId,
        color: PikiColor,
        wait: f32,
        start: Vec3,
        progress: f32,
        speed: f32,
    },
    /// Sucked in. The host should remove its object.
    Gone,
}

#[derive(Debug, Clone)]
pub struct Pellet {
    pub id: PelletId,
    pub desc: PelletDesc,
    pub pos: Vec3,
    pub yaw: f32,
    /// Shrinks to 0.25 while being sucked into the Onion.
    pub scale: f32,
    pub state: PelletState,
    /// Occupant of each carry slot (`mSlotFlags`), `max_carriers` long.
    pub slots: Vec<Option<PikiId>>,
    /// Carriers in the order they grabbed on (the first one is the original's "stick leader").
    pub carriers: Vec<PikiId>,
}

/// Height carriers lift the pellet by (`Pellet::startPick`, offset 8 / scale).
const PICK_OFFSET: f32 = 8.0;

impl Pellet {
    /// `Pellet::getSlotLocalPos` → world: slots evenly around the bottom circle.
    pub fn slot_world(&self, slot: u32) -> Vec3 {
        let n = self.desc.max_carriers.max(1) as f32;
        let a = TAU / n * slot as f32;
        let local = Vec3::new(self.desc.radius * a.sin(), 0.0, self.desc.radius * a.cos());
        self.pos + rotate_y(local, self.yaw)
    }

    pub fn can_take_carrier(&self) -> bool {
        matches!(self.state, PelletState::Idle | PelletState::Carried { .. })
            && self.slots.iter().any(Option::is_none)
    }

    /// `getRandomFreeSlotIndex`: the free slot nearest (by index) to a random one.
    fn random_free_slot(&self, r: f32) -> Option<u32> {
        let n = self.slots.len() as i32;
        let pick = (r * n as f32) as i32;
        (0..n)
            .filter(|&i| self.slots[i as usize].is_none())
            .min_by_key(|&i| (i - pick).abs())
            .map(|i| i as u32)
    }

    /// `getNearestFreeSlotIndex`
    fn nearest_free_slot(&self, from: Vec3) -> Option<u32> {
        (0..self.slots.len() as u32)
            .filter(|&i| self.slots[i as usize].is_none())
            .min_by(|&a, &b| {
                (self.slot_world(a) - from)
                    .length()
                    .total_cmp(&(self.slot_world(b) - from).length())
            })
    }

    fn release(&mut self, id: PikiId) {
        for s in &mut self.slots {
            if *s == Some(id) {
                *s = None;
            }
        }
        self.carriers.retain(|&c| c != id);
    }

    /// Pikmin hit this when thrown: within the footprint and below the top.
    pub(crate) fn touches(&self, p: Vec3, size: f32) -> bool {
        dist_xz(p, self.pos) < self.desc.radius + size
            && p.y >= self.pos.y - size
            && p.y <= self.pos.y + self.desc.height + size
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CarryPhase {
    /// `STATE_Go` (`gotoLiftPos`): walking to a slot. `progress` counts ticks near the pellet.
    Go { progress: u32 },
    /// `STATE_Lift`: holding the slot, playing PICKLOOP. `lifted` once its `KEY_Action0` hits.
    Lift { frame: f32, lifted: bool },
    /// `STATE_Move`: the pellet is moving; ride along at the slot.
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Carry {
    pub pellet: PelletId,
    pub slot: u32,
    pub phase: CarryPhase,
}

impl Sim {
    /// Spawns something carryable, e.g. a corpse when a host creature dies.
    pub fn spawn_pellet(&mut self, desc: PelletDesc) -> PelletId {
        let id = PelletId(self.pellets.len() as u32);
        self.pellets.push(Pellet {
            id,
            desc,
            pos: desc.pos,
            yaw: desc.yaw,
            scale: 1.0,
            state: PelletState::Idle,
            slots: vec![None; desc.max_carriers.max(1) as usize],
            carriers: Vec::new(),
        });
        id
    }

    pub fn pellets(&self) -> &[Pellet] {
        &self.pellets
    }

    pub fn pellet(&self, id: PelletId) -> &Pellet {
        &self.pellets[id.0 as usize]
    }

    /// `ActTransport::init`: head for a random free slot.
    pub(crate) fn start_carry(&mut self, pi: usize, pellet: PelletId) -> bool {
        let r = self.rng.f32();
        let Some(slot) = self.pellets[pellet.0 as usize].random_free_slot(r) else {
            return false;
        };
        self.pikis[pi].state = PikiState::Carry(Carry {
            pellet,
            slot,
            phase: CarryPhase::Go { progress: 0 },
        });
        true
    }

    /// Carrier weight for speed: leaf 1, bud 1.5, flower 2 (`ActTransport::getCarriers`).
    fn carrier_weight(&self, pellet: &Pellet) -> f32 {
        pellet
            .carriers
            .iter()
            .map(|id| {
                1.0 + 0.5
                    * match self.pikis[id.0 as usize].happa {
                        Happa::Leaf => 0.0,
                        Happa::Bud => 1.0,
                        Happa::Flower => 2.0,
                    }
            })
            .sum()
    }

    /// Lifted carriers (PICKLOOP `KEY_Action0` reached), as counted by `ActTransport::doLift`.
    fn lifted_carriers(&self, pellet: &Pellet) -> u32 {
        pellet
            .carriers
            .iter()
            .filter(|id| match self.pikis[id.0 as usize].state {
                PikiState::Carry(Carry {
                    phase: CarryPhase::Lift { lifted, .. },
                    ..
                }) => lifted,
                PikiState::Carry(Carry {
                    phase: CarryPhase::Move,
                    ..
                }) => true,
                _ => false,
            })
            .count() as u32
    }

    /// `ActTransport::decideGoal`: the carriers' majority colour (ties at random) picks the Onion.
    fn decide_goal(&mut self, pi: usize) -> Option<(OnionId, PikiColor)> {
        let pellet = &self.pellets[pi];
        if pellet.desc.ship_part.is_some() {
            let pos = pellet.pos;
            let color = pellet
                .carriers
                .first()
                .map_or(PikiColor::Red, |id| self.pikis[id.0 as usize].color);
            return self
                .onions
                .iter()
                .filter(|o| o.ship.is_some() && o.grounded())
                .min_by(|a, b| dist_xz(a.pos, pos).total_cmp(&dist_xz(b.pos, pos)))
                .map(|o| (o.id, color));
        }
        let mut counts = [0u32; 3];
        for id in &pellet.carriers {
            counts[self.pikis[id.0 as usize].color.index()] += 1;
        }
        let max = *counts.iter().max()?;
        let options: Vec<usize> = (0..3).filter(|&i| counts[i] == max).collect();
        let pick = ((self.rng.f32() * options.len() as f32) as usize).min(options.len() - 1);
        let color = PikiColor::from_index(options[pick]);

        let pos = self.pellets[pi].pos;
        let nearest = |accepting: bool| {
            self.onions
                .iter()
                .filter(|o| o.ship.is_none() && o.grounded())
                .filter(|o| !accepting || o.accepts(color))
                .min_by(|a, b| dist_xz(a.pos, pos).total_cmp(&dist_xz(b.pos, pos)))
                .map(|o| o.id)
        };
        let onion = nearest(true).or_else(|| nearest(false))?;
        let o = &self.onions[onion.0 as usize];
        // An Onion that doesn't take the majority colour makes its own.
        let color = if o.accepts(color) { color } else { o.colors[0] };
        Some((onion, color))
    }

    /// Whether the Onion still suits the carriers: it takes one of their majority colours.
    /// Pikmin 1 only decides at lift time (`ActTransport::doLift`); like Pikmin 2, whose
    /// `Pellet::onSlotStickStart` / `onSlotStickEnd` keep the colour counts that
    /// `Pellet::getPelletGoal` reads, we re-pick when Pikmin join or leave on the way.
    fn goal_still_majority(&self, pi: usize, onion: OnionId) -> bool {
        if !self.onions[onion.0 as usize].grounded() {
            return false;
        }
        if self.pellets[pi].desc.ship_part.is_some() {
            return true;
        }
        let mut counts = [0u32; 3];
        for id in &self.pellets[pi].carriers {
            counts[self.pikis[id.0 as usize].color.index()] += 1;
        }
        let max = counts.iter().copied().max().unwrap_or(0);
        let o = &self.onions[onion.0 as usize];
        (0..3).any(|c| counts[c] == max && o.accepts(PikiColor::from_index(c)))
            || !self.onions.iter().filter(|o| o.ship.is_none() && o.grounded()).any(|other| {
                (0..3).any(|c| counts[c] == max && other.accepts(PikiColor::from_index(c)))
            })
    }

    pub(crate) fn update_pellets(&mut self, host: &mut dyn Host, dt: f32) {
        for i in 0..self.pellets.len() {
            match self.pellets[i].state.clone() {
                PelletState::Idle => {
                    self.ground_pellet(i, host, 0.0);
                    let min = self.pellets[i].desc.min_carriers.max(1);
                    if self.lifted_carriers(&self.pellets[i]) >= min
                        && let Some((onion, color)) = self.decide_goal(i)
                    {
                        let mut path = Vec::new();
                        let from = self.pellets[i].pos;
                        let to = self.onions[onion.0 as usize].goal_pos();
                        if host.carry_path(from, to, &mut path) && !path.is_empty() {
                            let p = &mut self.pellets[i];
                            p.state = PelletState::Carried {
                                onion,
                                color,
                                path,
                                next: 0,
                            };
                            self.events.push(Event::PelletLifted {
                                pellet: p.id,
                                onion,
                            });
                            for id in p.carriers.clone() {
                                if let PikiState::Carry(c) = &mut self.pikis[id.0 as usize].state {
                                    c.phase = CarryPhase::Move;
                                }
                            }
                        }
                    }
                }
                PelletState::Carried {
                    mut onion,
                    mut color,
                    mut path,
                    mut next,
                } => {
                    if !self.goal_still_majority(i, onion)
                        && let Some((o, c)) = self.decide_goal(i)
                    {
                        let mut route = Vec::new();
                        let to = self.onions[o.0 as usize].goal_pos();
                        if host.carry_path(self.pellets[i].pos, to, &mut route) && !route.is_empty() {
                            (onion, color, path, next) = (o, c, route, 0);
                            self.pellets[i].state = PelletState::Carried {
                                onion,
                                color,
                                path: path.clone(),
                                next,
                            };
                            self.events.push(Event::PelletRerouted {
                                pellet: self.pellets[i].id,
                                onion,
                            });
                        }
                    }
                    self.carry_step(i, host, dt, onion, color, &path, &mut next)
                }
                PelletState::Sucking {
                    onion,
                    color,
                    wait,
                    start,
                    progress,
                    speed,
                } => self.suck_step(i, dt, onion, color, wait, start, progress, speed),
                PelletState::Gone => {}
            }
        }
    }

    fn ground_pellet(&mut self, i: usize, host: &mut dyn Host, lift: f32) {
        let p = &mut self.pellets[i];
        if let Some(g) = host.ground_height(p.pos + Vec3::new(0.0, 20.0, 0.0)) {
            p.pos.y = g + lift;
        }
    }

    /// `ActTransport::moveToWayPoint` / `STATE_Goal`, run once per pellet instead of by the
    /// stick leader.
    #[allow(clippy::too_many_arguments)]
    fn carry_step(
        &mut self,
        i: usize,
        host: &mut dyn Host,
        dt: f32,
        onion: OnionId,
        color: PikiColor,
        path: &[Vec3],
        next: &mut usize,
    ) {
        let min = self.pellets[i].desc.min_carriers.max(1);
        // Its Onion took off and none other is down to reroute to: put it down too, and lift
        // again once one lands.
        let gone = !self.onions[onion.0 as usize].grounded();
        if (self.pellets[i].carriers.len() as u32) < min || gone {
            // "小人数!!!": too few, put it down. The rest keep hold and struggle (Lift).
            let p = &mut self.pellets[i];
            p.state = PelletState::Idle;
            self.events.push(Event::PelletDropped { pellet: p.id });
            for id in p.carriers.clone() {
                if let PikiState::Carry(c) = &mut self.pikis[id.0 as usize].state {
                    c.phase = CarryPhase::Lift {
                        frame: 0.0,
                        lifted: false,
                    };
                }
            }
            self.ground_pellet(i, host, 0.0);
            return;
        }

        let goal = self.onions[onion.0 as usize].goal_pos();
        if dist_xz(self.pellets[i].pos, goal) < 10.0 {
            // `PelletGoalState`: carriers let go and the Onion takes over.
            let p = &mut self.pellets[i];
            let carriers = std::mem::take(&mut p.carriers);
            p.slots.iter_mut().for_each(|s| *s = None);
            p.state = PelletState::Sucking {
                onion,
                color,
                wait: 1.5,
                start: p.pos,
                progress: 0.0,
                speed: 0.0,
            };
            self.events.push(Event::PelletArrived {
                pellet: p.id,
                onion,
            });
            for id in carriers {
                self.pikis[id.0 as usize].state = PikiState::Idle;
            }
            return;
        }

        let pp = &self.params.piki;
        let max_speed = 0.7 * pp.max_leaf_move_speed;
        let min_speed = 0.5 * max_speed;
        let maxc = self.pellets[i].desc.max_carriers.max(1) as f32;
        let weight = self.carrier_weight(&self.pellets[i]);
        let speed = min_speed + (weight + 1.0 - min as f32) / maxc * (max_speed - min_speed);

        let p = &mut self.pellets[i];
        let mut target = path[(*next).min(path.len() - 1)];
        while *next + 1 < path.len() && dist_xz(p.pos, target) < 10.0 {
            *next += 1;
            target = path[*next];
        }
        let to = Vec3::new(target.x - p.pos.x, 0.0, target.z - p.pos.z);
        let step = (speed * dt).min(to.length());
        p.pos += to.normalize_or_zero() * step;
        // A carried pellet moves as a creature too (`Creature::moveVelocity`), so walls stop it.
        let centre = p.pos + Vec3::Y * p.desc.radius;
        let moved = host.walls(centre, p.desc.radius) - centre;
        p.pos += Vec3::new(moved.x, 0.0, moved.z);
        let n = *next;
        if let PelletState::Carried { next, .. } = &mut p.state {
            *next = n;
        }
        self.ground_pellet(i, host, PICK_OFFSET);
    }

    /// `PelletGoalState::exec`: wait 1.5 s, then accelerate (720/s²) up into the suck point,
    /// shrinking to a quarter size, then `GoalItem::suckMe`.
    #[allow(clippy::too_many_arguments)]
    fn suck_step(
        &mut self,
        i: usize,
        dt: f32,
        onion: OnionId,
        color: PikiColor,
        wait: f32,
        start: Vec3,
        progress: f32,
        speed: f32,
    ) {
        let suck_pos = self.onions[onion.0 as usize].suck_pos();
        let p = &mut self.pellets[i];
        if wait > 0.0 {
            p.state = PelletState::Sucking {
                onion,
                color,
                wait: wait - dt,
                start: p.pos,
                progress,
                speed,
            };
            return;
        }
        let dist = (suck_pos - start).length().max(1e-3);
        p.pos = start + (suck_pos - start) * progress.min(1.0);
        p.scale = 1.0 - progress.min(1.0) * 0.75;
        let progress = progress + speed * dt / dist;
        let speed = speed + dt * 720.0;
        if progress >= 1.0 {
            p.state = PelletState::Gone;
            if let Some(part) = p.desc.ship_part {
                // `UfoItem::suckMe`: the part goes on the ship (`PlayerState::getUfoParts`).
                let id = p.id;
                let ship = self.onions[onion.0 as usize].ship.as_mut().expect("ship parts go to ships");
                if !ship.parts.contains(&part) {
                    ship.parts.push(part);
                }
                let total = ship.parts.len() as u32;
                self.events.push(Event::ShipPartCollected {
                    pellet: id,
                    ship: onion,
                    part,
                    total,
                });
                return;
            }
            let seeds = match p.desc.color {
                Some(c) if c != color => p.desc.seeds_other,
                _ => p.desc.seeds_matching,
            };
            let id = p.id;
            self.events.push(Event::PelletDelivered {
                pellet: id,
                onion,
                color,
                seeds,
            });
            self.onions[onion.0 as usize].receive(color, seeds);
        } else {
            p.state = PelletState::Sucking {
                onion,
                color,
                wait,
                start,
                progress,
                speed,
            };
        }
    }

    /// Pikmin side of `ActTransport::exec`.
    pub(crate) fn piki_carry(&mut self, pi: usize, c: Carry, host: &mut dyn Host, dt: f32) {
        let pel = &self.pellets[c.pellet.0 as usize];
        if !matches!(pel.state, PelletState::Idle | PelletState::Carried { .. }) {
            return self.stop_carry(pi);
        }
        let id = self.pikis[pi].id;
        let slot_pos = pel.slot_world(c.slot);
        let centre = pel.pos;
        let carried = matches!(pel.state, PelletState::Carried { .. });

        match c.phase {
            CarryPhase::Go { mut progress } => {
                let p = &self.pikis[pi];
                let to = slot_pos - p.pos;
                let horiz = to.x.hypot(to.z);
                let vert = to.y;
                let mut slot = c.slot;
                // Stuck near the pellet for 60 ticks: try the nearest free slot instead.
                if horiz < 100.0 {
                    progress += 1;
                    if progress >= 60 {
                        match pel.nearest_free_slot(p.pos) {
                            Some(s) if s != slot => {
                                slot = s;
                                progress = 0;
                            }
                            _ => return self.stop_carry(pi),
                        }
                    }
                }
                if horiz > 6.0 {
                    let full = self.piki_speed(pi, 1.0);
                    let p = &mut self.pikis[pi];
                    p.target_vel = Vec3::new(to.x, 0.0, to.z).normalize_or_zero() * full;
                    p.state = PikiState::Carry(Carry {
                        slot,
                        phase: CarryPhase::Go { progress },
                        ..c
                    });
                    self.physics(pi, host, dt);
                    return;
                }
                if vert.abs() > 25.0 {
                    // Slot is out of reach vertically (the original jumps or gives up).
                    return self.stop_carry(pi);
                }
                if pel.slots[slot as usize].is_some() {
                    match pel.nearest_free_slot(p.pos) {
                        Some(s) => {
                            self.pikis[pi].state = PikiState::Carry(Carry {
                                slot: s,
                                phase: CarryPhase::Go { progress: 0 },
                                ..c
                            });
                        }
                        None => self.stop_carry(pi),
                    }
                    return;
                }
                // Grab on (`startStickObject(pellet, slot)`).
                let pel = &mut self.pellets[c.pellet.0 as usize];
                pel.slots[slot as usize] = Some(id);
                pel.carriers.push(id);
                self.events.push(Event::CarrierGrabbed {
                    piki: id,
                    pellet: c.pellet,
                });
                let p = &mut self.pikis[pi];
                p.pos = slot_pos;
                p.vel = Vec3::ZERO;
                p.target_vel = Vec3::ZERO;
                p.face = round_ang(yaw_of(centre - p.pos));
                p.state = PikiState::Carry(Carry {
                    slot,
                    phase: CarryPhase::Lift {
                        frame: 0.0,
                        lifted: false,
                    },
                    ..c
                });
            }
            CarryPhase::Lift { frame, lifted } => {
                let anim = self.params.anim.pickloop;
                let mut frame = frame + self.params.anim.fps * dt;
                let lifted = lifted || frame >= anim.action0;
                if let Some((start, end)) = anim.loop_range
                    && frame >= end
                {
                    frame -= end - start;
                }
                let phase = if carried && lifted {
                    CarryPhase::Move
                } else {
                    CarryPhase::Lift { frame, lifted }
                };
                let p = &mut self.pikis[pi];
                p.pos = slot_pos;
                p.vel = Vec3::ZERO;
                p.on_ground = true;
                p.state = PikiState::Carry(Carry { phase, ..c });
            }
            CarryPhase::Move => {
                let p = &mut self.pikis[pi];
                let face = yaw_of(slot_pos - p.pos);
                p.vel = (slot_pos - p.pos) / dt;
                p.pos = slot_pos;
                p.on_ground = true;
                if p.vel.x.hypot(p.vel.z) > 1.0 {
                    p.face = round_ang(p.face + 0.3 * ang_dist(face, p.face));
                }
            }
        }
    }

    /// Let go of the pellet and go idle.
    pub(crate) fn stop_carry(&mut self, pi: usize) {
        let p = &mut self.pikis[pi];
        if let PikiState::Carry(c) = p.state {
            let id = p.id;
            p.state = PikiState::Idle;
            p.vel = Vec3::ZERO;
            self.pellets[c.pellet.0 as usize].release(id);
        }
    }

    /// Thrown or landed near a pellet with a free slot (`graspSituation` pellet branch):
    /// nearest within `idle_work_search_range` of its edge.
    pub(crate) fn nearest_free_pellet(&self, pos: Vec3, size: f32) -> Option<(PelletId, f32)> {
        let range = self.params.piki.idle_work_search_range;
        self.pellets
            .iter()
            .filter(|p| p.can_take_carrier())
            .map(|p| (p.id, dist_xz(pos, p.pos)))
            .filter(|&(id, d)| d <= self.pellets[id.0 as usize].desc.radius + range + size)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FlatHost;
    use crate::{Params, Sim};

    /// Grabs `pellet` at its next free slot as if it had walked there and lifted.
    fn attach(sim: &mut Sim, color: PikiColor, pellet: PelletId) {
        let pos = sim.pellet(pellet).pos;
        let id = sim.spawn_piki(color, pos).unwrap();
        let p = &mut sim.pellets[pellet.0 as usize];
        let slot = p.slots.iter().position(Option::is_none).unwrap();
        p.slots[slot] = Some(id);
        p.carriers.push(id);
        let phase = if matches!(p.state, PelletState::Carried { .. }) {
            CarryPhase::Move
        } else {
            CarryPhase::Lift { frame: 30.0, lifted: true }
        };
        sim.pikis[id.0 as usize].state = PikiState::Carry(Carry { pellet, slot: slot as u32, phase });
    }

    #[test]
    fn carriers_joining_on_the_way_reroute_to_the_new_majority() {
        let mut sim = Sim::new(Params::default(), 3);
        let mut host = FlatHost;
        let red = sim.add_onion(Vec3::new(2000.0, 0.0, 2000.0), &[PikiColor::Red]);
        let blue = sim.add_onion(Vec3::new(2000.0, 0.0, -2000.0), &[PikiColor::Blue]);
        let pel = sim.spawn_pellet(PelletDesc {
            pos: Vec3::ZERO,
            yaw: 0.0,
            radius: 10.0,
            height: 10.0,
            min_carriers: 1,
            max_carriers: 4,
            seeds_matching: 1,
            seeds_other: 1,
            color: None,
            creature: None,
            ship_part: None,
        });
        let goal = |sim: &Sim| match sim.pellet(pel).state {
            PelletState::Carried { onion, .. } => Some(onion),
            _ => None,
        };
        attach(&mut sim, PikiColor::Blue, pel);
        sim.step(&mut host);
        assert_eq!(goal(&sim), Some(blue), "one blue carrier: blue Onion");

        attach(&mut sim, PikiColor::Red, pel);
        sim.step(&mut host);
        assert_eq!(goal(&sim), Some(blue), "a tie keeps the current goal");

        attach(&mut sim, PikiColor::Red, pel);
        sim.step(&mut host);
        assert_eq!(goal(&sim), Some(red), "two reds to one blue: red Onion");
        assert!(sim.drain_events().any(|e| matches!(e, Event::PelletRerouted { onion, .. } if onion == red)));
    }

    #[test]
    fn ship_parts_go_to_the_ship_not_the_onion() {
        let mut sim = Sim::new(Params::default(), 5);
        let mut host = FlatHost;
        let onion = sim.add_onion(Vec3::new(100.0, 0.0, 0.0), &[PikiColor::Red]);
        let ship = sim.add_ship(Vec3::new(-300.0, 0.0, 0.0), Vec3::new(-180.0, 0.0, 0.0), Vec3::new(-280.0, 60.0, 20.0));
        let pel = sim.spawn_pellet(PelletDesc {
            pos: Vec3::ZERO,
            yaw: 0.0,
            radius: 10.0,
            height: 10.0,
            min_carriers: 1,
            max_carriers: 4,
            seeds_matching: 1,
            seeds_other: 1,
            color: None,
            creature: None,
            ship_part: Some(4),
        });
        attach(&mut sim, PikiColor::Red, pel);
        sim.step(&mut host);
        assert!(matches!(sim.pellet(pel).state, PelletState::Carried { onion: o, .. } if o == ship));
        assert_ne!(Some(onion), Some(ship));
        let mut got = None;
        for _ in 0..30 * 30 {
            sim.step(&mut host);
            for e in sim.drain_events() {
                if let Event::ShipPartCollected { part, total, ship: s, .. } = e {
                    got = Some((part, total, s));
                }
            }
            if got.is_some() {
                break;
            }
        }
        assert_eq!(got, Some((4, 1, ship)));
        assert_eq!(sim.onions()[ship.0 as usize].ship.as_ref().unwrap().parts, vec![4]);
        assert!(sim.sprouts().next().is_none(), "no seeds from a ship part");
    }

    #[test]
    fn an_onion_taking_off_stops_deliveries_until_it_lands() {
        let mut sim = Sim::new(Params::default(), 7);
        let mut host = FlatHost;
        let red = sim.add_onion(Vec3::new(600.0, 0.0, 0.0), &[PikiColor::Red]);
        let pel = sim.spawn_pellet(PelletDesc {
            pos: Vec3::ZERO,
            yaw: 0.0,
            radius: 10.0,
            height: 10.0,
            min_carriers: 1,
            max_carriers: 4,
            seeds_matching: 1,
            seeds_other: 1,
            color: None,
            creature: None,
            ship_part: None,
        });
        attach(&mut sim, PikiColor::Red, pel);
        sim.step(&mut host);
        assert!(matches!(sim.pellet(pel).state, PelletState::Carried { onion, .. } if onion == red));

        sim.onion_take_off(red);
        sim.step(&mut host);
        assert_eq!(sim.pellet(pel).state, PelletState::Idle, "put down: nowhere to take it");
        for _ in 0..30 * 15 {
            sim.step(&mut host);
        }
        assert_eq!(sim.onions()[0].anim, crate::onion::OnionAnim::Away);
        assert_eq!(sim.pellet(pel).state, PelletState::Idle, "still waiting");

        sim.onion_land(red);
        let mut landed = false;
        for _ in 0..30 * 12 {
            sim.step(&mut host);
            landed |= sim.drain_events().any(|e| matches!(e, Event::OnionLanded { onion } if onion == red));
        }
        assert!(landed);
        assert!(sim.onions()[0].grounded());
        assert!(
            !matches!(sim.pellet(pel).state, PelletState::Idle),
            "lifted again once the Onion is back: {:?}",
            sim.pellet(pel).state
        );
    }
}
