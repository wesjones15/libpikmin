//! The Onion menu: calling stored Pikmin out and putting squad Pikmin away.
//!
//! - Opening: a captain within reach of the Onion's `cont` sphere presses the action button
//!   (`NaviWalkState::exec` → `NaviContainerState`, naviState.cpp). The game pauses while the
//!   window is up; hosts do the same by not advancing the sim.
//! - The window (`NaviContainerState::init`) shows the Onion's stock minus pending exits, the
//!   squad's Pikmin of that colour, and the field count (on the map plus pending exits)
//!   against `aiconst` p15 (100).
//! - Putting away (`NaviContainerState::enterPikis`): that many squad Pikmin of the colour
//!   switch to `EnterMode` (`ActEnter`, aiEnter.cpp): walk at half speed to one of the three
//!   legs, climb it at 15 units/s, shrink from 72% of the way, and are stored at 90%.
//! - Calling out (`GoalItem::exitPikis` / `exitPiki`, goalItem.cpp): one Pikmin every
//!   (0.2 + 0.1 rand) * 0.2 s, highest leaf stage first, appears at the top of a random leg and
//!   slides down it at 70 units/s (`ActExit`, aiExit.cpp), growing back from 72%. On the
//!   ground it joins the captain's squad (`TopAction::exec` with `mActionState` 0).

use glam::Vec3;

use crate::event::Event;
use crate::host::Host;
use crate::onion::OnionId;
use crate::piki::{Happa, PikiColor, PikiState};
use crate::sim::Sim;
use crate::{CaptainId, PikiId};

/// Leg ends measured on `objects/goal/goal.mod` in its bind pose: each leg mesh's lowest
/// vertex (the foot) and highest (where it meets the pod), relative to the Onion's position.
pub const LEG_FEET: [Vec3; 3] = [
    Vec3::new(2.3, 0.0, -115.2),
    Vec3::new(-100.9, 0.0, 55.7),
    Vec3::new(98.6, 0.0, 59.6),
];
pub const LEG_TOPS: [Vec3; 3] = [
    Vec3::new(0.7, 88.4, -19.5),
    Vec3::new(-18.4, 88.4, 9.3),
    Vec3::new(17.6, 88.3, 10.7),
];
/// Horizontal reach of the `cont` sphere plus the captain, from the Onion's centre. The
/// sphere lives in the Onion's collision data, which libpikmin doesn't load; this is a fit
/// to "standing under the Onion".
pub const CONTAINER_REACH: f32 = 70.0;
/// `ActEnter::climb`
pub const CLIMB_SPEED: f32 = 15.0;
/// `ActExit::exec`
pub const SLIDE_SPEED: f32 = 70.0;
/// Rope ratio where Pikmin start shrinking into / growing out of the Onion.
pub const SHRINK_FROM: f32 = 0.72;
/// `ActEnter::climb`: stored past this ratio.
pub const ENTER_AT: f32 = 0.9;

/// What the Onion window shows (`containerWindow->start` arguments).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnionCounts {
    pub color: PikiColor,
    /// Stored Pikmin of `color` not already on their way out.
    pub in_onion: u32,
    /// The captain's squad Pikmin of `color` that could be put away.
    pub in_squad: u32,
    /// Pikmin and sprouts on the field plus pending exits.
    pub on_field: u32,
    pub max_on_field: u32,
}

impl OnionCounts {
    /// How many could be called out right now.
    pub fn can_call_out(&self) -> u32 {
        self.in_onion.min(self.max_on_field.saturating_sub(self.on_field))
    }
}

fn leg_point(onion: Vec3, leg: u8, ratio: f32) -> Vec3 {
    let l = leg as usize % 3;
    onion + LEG_FEET[l].lerp(LEG_TOPS[l], ratio.clamp(0.0, 1.0))
}

fn leg_length(leg: u8) -> f32 {
    let l = leg as usize % 3;
    (LEG_TOPS[l] - LEG_FEET[l]).length()
}

impl Sim {
    /// The Onion a captain can open the menu of, if any.
    pub fn onion_in_reach(&self, captain: CaptainId) -> Option<OnionId> {
        let c = self.captains.get(captain.0 as usize)?;
        if c.down || c.pluck.is_some() {
            return None;
        }
        self.onions
            .iter()
            .filter(|o| o.ship.is_none() && o.grounded() && (o.pos - c.pos).with_y(0.0).length() < CONTAINER_REACH)
            .min_by(|a, b| {
                let da = (a.pos - c.pos).length();
                let db = (b.pos - c.pos).length();
                da.total_cmp(&db)
            })
            .map(|o| o.id)
    }

    fn squad_of_color(&self, captain: CaptainId, color: PikiColor) -> Vec<PikiId> {
        self.pikis
            .iter()
            .filter(|p| {
                p.color == color && p.captain == Some(captain) && matches!(p.state, PikiState::Squad { .. })
            })
            .map(|p| p.id)
            .collect()
    }

    pub fn onion_counts(&self, onion: OnionId, captain: CaptainId, color: PikiColor) -> OnionCounts {
        let o = &self.onions[onion.0 as usize];
        let pending = o.exits.iter().filter(|e| e.0 == color).count() as u32;
        OnionCounts {
            color,
            in_onion: o.stored_of(color).saturating_sub(pending),
            in_squad: self.squad_of_color(captain, color).len() as u32,
            on_field: self.field_count() as u32,
            max_on_field: self.params.ai.max_pikis_on_field.max(0) as u32,
        }
    }

    /// Sends up to `n` of the captain's squad Pikmin of `color` into the Onion. Returns how
    /// many were sent.
    pub fn onion_put_away(&mut self, captain: CaptainId, onion: OnionId, color: PikiColor, n: u32) -> u32 {
        if !self.onions[onion.0 as usize].accepts(color) || !self.onions[onion.0 as usize].grounded() {
            return 0;
        }
        let ids: Vec<_> = self.squad_of_color(captain, color).into_iter().take(n as usize).collect();
        for &id in &ids {
            self.leave_current(id);
            // `ActEnter::findLeg`: a random leg.
            let leg = (self.rng.f32() * 3.0) as u8 % 3;
            let p = &mut self.pikis[id.0 as usize];
            p.state = PikiState::Enter {
                onion,
                leg,
                climbing: false,
                ratio: 0.0,
            };
        }
        ids.len() as u32
    }

    /// Asks the Onion to let up to `n` Pikmin of `color` out to join `captain`. Limited by the
    /// stock and the field limit. Returns how many will come out.
    pub fn onion_call_out(&mut self, captain: CaptainId, onion: OnionId, color: PikiColor, n: u32) -> u32 {
        if !self.onions[onion.0 as usize].accepts(color) || !self.onions[onion.0 as usize].grounded() { return 0; }
        let k = self.onion_counts(onion, captain, color).can_call_out().min(n);
        let o = &mut self.onions[onion.0 as usize];
        for _ in 0..k {
            o.exits.push_back((color, captain));
        }
        k
    }

    /// Puts Pikmin straight into an Onion's stock (for hosts restoring a save, for example).
    pub fn store_in_onion(&mut self, onion: OnionId, color: PikiColor, happa: Happa, n: u32) {
        if let Some(o) = self.onions.get_mut(onion.0 as usize) {
            o.stored[color.index()][happa as usize] += n;
        }
    }

    /// `GoalItem` dispensing: one Pikmin out every 0.04-0.06 s.
    pub(crate) fn update_onion_exits(&mut self, dt: f32) {
        for oi in 0..self.onions.len() {
            if self.onions[oi].exits.is_empty() || !self.onions[oi].grounded() {
                continue;
            }
            self.onions[oi].exit_timer -= dt;
            if self.onions[oi].exit_timer > 0.0 {
                continue;
            }
            self.onions[oi].exit_timer = (self.rng.f32() * 0.1 + 0.2) * 0.2;
            let Some((color, captain)) = self.onions[oi].exits.pop_front() else {
                continue;
            };
            let o = &mut self.onions[oi];
            // Highest leaf stage first.
            let Some(happa) = [Happa::Flower, Happa::Bud, Happa::Leaf]
                .into_iter()
                .find(|&h| o.stored[color.index()][h as usize] > 0)
            else {
                continue;
            };
            o.stored[color.index()][happa as usize] -= 1;
            let (onion, opos) = (o.id, o.pos);
            let leg = (self.rng.f32() * 3.0) as u8 % 3;
            let Some(id) = self.spawn_piki(color, leg_point(opos, leg, 1.0)) else {
                // Field full after all: keep it inside.
                self.onions[oi].stored[color.index()][happa as usize] += 1;
                continue;
            };
            let p = &mut self.pikis[id.0 as usize];
            p.happa = happa;
            p.captain = Some(captain);
            p.state = PikiState::Exit {
                onion,
                leg,
                ratio: 1.0,
            };
            self.events.push(Event::PikiLeftOnion { piki: id, onion });
        }
    }

    /// `ActEnter` and `ActExit`.
    pub(crate) fn piki_onion_travel(&mut self, pi: usize, host: &mut dyn Host, dt: f32) {
        match self.pikis[pi].state {
            PikiState::Enter {
                onion,
                leg,
                climbing,
                ratio,
            } => {
                let opos = self.onions[onion.0 as usize].pos;
                if !climbing {
                    let foot = leg_point(opos, leg, 0.0);
                    let to = (foot - self.pikis[pi].pos).with_y(0.0);
                    if to.length() < 8.0 {
                        let p = &mut self.pikis[pi];
                        p.vel = Vec3::ZERO;
                        p.state = PikiState::Enter {
                            onion,
                            leg,
                            climbing: true,
                            ratio: 0.0,
                        };
                        return;
                    }
                    let speed = self.piki_speed(pi, 0.5);
                    self.pikis[pi].target_vel = to.normalize() * speed;
                    self.physics(pi, host, dt);
                    return;
                }
                let r = ratio + CLIMB_SPEED * dt / leg_length(leg);
                let p = &mut self.pikis[pi];
                p.pos = leg_point(opos, leg, r);
                p.vel = Vec3::ZERO;
                p.on_ground = false;
                let d = opos + LEG_TOPS[leg as usize % 3] - p.pos;
                p.face = d.x.atan2(d.z);
                if r > ENTER_AT {
                    // `GoalItem::enterGoal`
                    let (color, happa, id) = (p.color, p.happa, p.id);
                    p.state = PikiState::Dead;
                    self.onions[onion.0 as usize].stored[color.index()][happa as usize] += 1;
                    self.events.push(Event::PikiEnteredOnion { piki: id, onion });
                } else {
                    p.state = PikiState::Enter {
                        onion,
                        leg,
                        climbing: true,
                        ratio: r,
                    };
                }
            }
            PikiState::Exit { onion, leg, ratio } => {
                let opos = self.onions[onion.0 as usize].pos;
                let r = ratio - SLIDE_SPEED * dt / leg_length(leg);
                let p = &mut self.pikis[pi];
                p.pos = leg_point(opos, leg, r);
                p.vel = Vec3::ZERO;
                let d = p.pos - (opos + LEG_TOPS[leg as usize % 3]);
                p.face = d.x.atan2(d.z);
                if r > 0.0 {
                    p.state = PikiState::Exit {
                        onion,
                        leg,
                        ratio: r,
                    };
                    return;
                }
                p.on_ground = true;
                let id = p.id;
                match p.captain.filter(|c| !self.captains[c.0 as usize].down) {
                    Some(c) => self.join_squad(id, c),
                    None => self.pikis[pi].state = PikiState::Idle,
                }
            }
            _ => {}
        }
    }
}
