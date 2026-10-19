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
//! - Host-controlled hovering Onions can also call Pikmin out: they slide to the midpoint
//!   of a leg, then let go into the normal ballistic/falling state. Putting away still
//!   requires a grounded Onion. This is a host extension, not the original game's exit.

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

pub(crate) fn leg_point(onion: Vec3, leg: u8, ratio: f32) -> Vec3 {
    let l = leg as usize % 3;
    onion + LEG_FEET[l].lerp(LEG_TOPS[l], ratio.clamp(0.0, 1.0))
}

pub(crate) fn leg_length(leg: u8) -> f32 {
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
                p.color == color && p.captain == Some(captain) && p.state.in_squad()
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

    /// Host extension: the Pikmin a captain left behind (he went down, or his owner died) that stand about
    /// join `to`'s squad instead. Returns how many.
    pub fn adopt_orphans(&mut self, from: CaptainId, to: CaptainId) -> u32 {
        if from == to || to.0 as usize >= self.captains.len() || self.captains[to.0 as usize].down {
            return 0;
        }
        let ids = self.orphans_of(from, None);
        for &id in &ids {
            self.leave_current(id);
            self.join_squad(id, to);
        }
        ids.len() as u32
    }

    /// Host extension: the Pikmin a captain left behind that stand about go into the landed `onion` (the
    /// ones of a colour it takes), walking to it. Returns how many were sent.
    pub fn send_orphans_home(&mut self, from: CaptainId, onion: OnionId) -> u32 {
        let Some(o) = self.onions.get(onion.0 as usize) else { return 0 };
        if o.ship.is_some() || !o.grounded() {
            return 0;
        }
        let ids = self.orphans_of(from, Some(onion));
        for &id in &ids {
            self.leave_current(id);
            let leg = (self.rng.f32() * 3.0) as u8 % 3;
            self.pikis[id.0 as usize].state = PikiState::Enter { onion, leg, climbing: false, ratio: 0.0 };
        }
        ids.len() as u32
    }

    /// Pikmin that were `captain`'s and are not doing anything else: standing about or still following
    /// (of a colour `onion` accepts, when one is given).
    fn orphans_of(&self, captain: CaptainId, onion: Option<OnionId>) -> Vec<PikiId> {
        self.pikis
            .iter()
            .filter(|p| p.is_alive() && p.captain == Some(captain) && matches!(p.state, PikiState::Idle | PikiState::Squad { .. }))
            .filter(|p| onion.is_none_or(|o| self.onions[o.0 as usize].accepts(p.color)))
            .map(|p| p.id)
            .collect()
    }

    /// Host extension: how long a dormant Onion still has to wait before it revives (it does not restart a
    /// revival already under way). 0 wakes it.
    pub fn set_onion_dormancy_seconds(&mut self, onion: OnionId, seconds: f32) {
        if let Some(d) = self.onions.get_mut(onion.0 as usize).and_then(|o| o.dormancy.as_mut())
            && !d.reviving
            && seconds.is_finite()
        {
            d.remaining = seconds.max(0.0);
        }
    }

    /// Asks the Onion to let up to `n` Pikmin of `color` out to join `captain`. Limited by the
    /// stock and the field limit. Returns how many will come out.
    pub fn onion_call_out(&mut self, captain: CaptainId, onion: OnionId, color: PikiColor, n: u32) -> u32 {
        if !self.onions[onion.0 as usize].accepts(color) || !self.onions[onion.0 as usize].takes_deliveries() { return 0; }
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

    /// Host extension: sacrifice stored Pikmin to heal a captain, leaves before buds before
    /// flowers (5%, 10%, 20% max health). Preserve one available Pikmin and every queued exit.
    /// Returns the number consumed and the health fraction restored, capped at the deficit.
    pub fn sacrifice_onion_stock(&mut self, onion: OnionId, missing: f32) -> (u32, f32) {
        if !missing.is_finite() || missing <= 0.0 { return (0, 0.0); }
        let Some(o) = self.onions.get_mut(onion.0 as usize) else { return (0, 0.0) };
        if o.ship.is_some() || !o.takes_deliveries() { return (0, 0.0); }
        let mut available = o.stored;
        for (colour, stock) in available.iter_mut().enumerate() {
            // Exits take flowers first, then buds and leaves. These are already promised.
            let mut pending = o.exits.iter().filter(|e| e.0.index() == colour).count() as u32;
            for stage in (0..3).rev() {
                let reserved = stock[stage].min(pending);
                stock[stage] -= reserved;
                pending -= reserved;
            }
        }
        let budget = available.iter().flatten().sum::<u32>().saturating_sub(1);
        let (mut killed, mut healing) = (0, 0.0f32);
        let missing = missing.min(1.0);
        for (stage, fraction) in [0.05, 0.1, 0.2].into_iter().enumerate() {
            for (colour, stock) in available.iter().enumerate() {
                for _ in 0..stock[stage] {
                    if killed == budget || healing + 1e-6 >= missing { return (killed, healing.min(missing)); }
                    o.stored[colour][stage] -= 1;
                    killed += 1;
                    healing += fraction;
                }
            }
        }
        (killed, healing.min(missing))
    }

    /// `GoalItem` dispensing: one Pikmin out every 0.04-0.06 s.
    pub(crate) fn update_onion_exits(&mut self, dt: f32) {
        for oi in 0..self.onions.len() {
            if self.onions[oi].exits.is_empty() || !self.onions[oi].takes_deliveries() {
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
            p.on_ground = false;
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
                let o = &self.onions[onion.0 as usize];
                let opos = o.pos;
                let hovering = o.anim == crate::onion::OnionAnim::Hover;
                let r = ratio - SLIDE_SPEED * dt / leg_length(leg);
                let p = &mut self.pikis[pi];
                if hovering && r <= 0.5 {
                    // Host flight: play the first half of ActExit, then fall from that
                    // exact point rather than following the moving leg to its foot.
                    p.pos = leg_point(opos, leg, 0.5);
                    p.vel = Vec3::NEG_Y * SLIDE_SPEED;
                    p.target_vel = p.vel;
                    p.on_ground = false;
                    p.state = PikiState::Flying { ground_frames: 0 };
                    return;
                }
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
