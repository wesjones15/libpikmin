//! Host extension, not Pikmin 1: hooks for a host's item system (Risk of Rain 2's items, see
//! `DOCS/olimar_pikmin_ror2_unified_ruleset.md`). The sim knows nothing of items; the host
//! turns them into these numbers each frame:
//!
//! - a captain's attack speed ([`Sim::set_captain_attack_rate`]) and the move speed his
//!   Pikmin inherit ([`Sim::set_squad_speed`]);
//! - cooldown items as a growth rate ([`Sim::set_growth_rate`]: seeds leave the Onion sooner,
//!   sprouts mature faster) and a shorter regroup ([`Sim::set_rejoin_delay`]);
//! - healing for Pikmin ([`Sim::heal_squad`], [`Sim::heal_pikis_in_sphere`]);
//! - moving a captain's whole squad with him when the host teleports him
//!   ([`Sim::teleport_squad`]);
//! - Pikmin coming back after the last one dies ([`Sim::set_emergency_recovery`]);
//! - squad Pikmin picking up pellets they pass ([`Sim::set_auto_carry_range`]).

use std::f32::consts::TAU;

use glam::Vec3;

use crate::event::Event;
use crate::onion::{OnionAnim, OnionId};
use crate::piki::{PikiColor, PikiState};
use crate::{CaptainId, PikiId, Sim};

/// Colours of emergency Pikmin, in turn (four Red, three Yellow, three Blue of ten).
const RECOVERY_COLOURS: [PikiColor; 3] = [PikiColor::Red, PikiColor::Yellow, PikiColor::Blue];

impl Sim {
    /// How fast `captain`'s punches and plucks and his Pikmin's attacks play, as a multiple
    /// of Pikmin's own speed; clamped to 0.1..10.
    pub fn set_captain_attack_rate(&mut self, captain: CaptainId, rate: f32) {
        if let Some(c) = self.captains.get_mut(captain.0 as usize) {
            c.attack_rate = if rate.is_finite() { rate.clamp(0.1, 10.0) } else { 1.0 };
        }
    }

    /// Multiplies the walk and run speeds of `captain`'s Pikmin; clamped to 0.1..10.
    pub fn set_squad_speed(&mut self, captain: CaptainId, scale: f32) {
        if let Some(c) = self.captains.get_mut(captain.0 as usize) {
            c.squad_speed = if scale.is_finite() { scale.clamp(0.1, 10.0) } else { 1.0 };
        }
    }

    /// Makes every Pikmin with a captain walk and run as fast as `pace` times that captain's
    /// full-tilt speed (his run speed with the host's speed scale), by stage: leaf, bud,
    /// flower. Above 1 a Pikmin catches up with him while he runs; it replaces the stage
    /// speeds and [`Sim::set_squad_speed`]. Any value 0 or less (or not a number) turns it
    /// off.
    pub fn set_squad_pace(&mut self, leaf: f32, bud: f32, flower: f32) {
        let ok = |v: f32| v.is_finite() && v > 0.0;
        self.pace = (ok(leaf) && ok(bud) && ok(flower)).then_some([leaf.min(10.0), bud.min(10.0), flower.min(10.0)]);
    }

    /// Mid-air jumps `captain` may make before landing (Hopoo Feather). His squad repeats
    /// each one where it jumped with him.
    pub fn set_captain_air_jumps(&mut self, captain: CaptainId, jumps: u32) {
        if let Some(c) = self.captains.get_mut(captain.0 as usize) {
            c.air_jumps = jumps.min(64);
        }
    }

    /// Whether jumping in mid-air with no jumps left stomps down (H3AD-5T v2).
    pub fn set_captain_stomp(&mut self, captain: CaptainId, on: bool) {
        if let Some(c) = self.captains.get_mut(captain.0 as usize) {
            c.can_stomp = on;
        }
    }

    /// Multiplies how fast sprouts grow underground and from leaf to bud to flower, and how
    /// soon a fed Onion sends its seeds out; clamped to 0.1..20. Flowers wilt at their own
    /// pace. 1 in Pikmin.
    pub fn set_growth_rate(&mut self, rate: f32) {
        self.growth_rate = if rate.is_finite() { rate.clamp(0.1, 20.0) } else { 1.0 };
    }

    /// Seconds a separated Pikmin stands before rejoining (with [`Sim::set_auto_rejoin`]).
    /// 0.5 by default.
    pub fn set_rejoin_delay(&mut self, seconds: f32) {
        self.rejoin_delay = if seconds.is_finite() { seconds.clamp(0.0, 30.0) } else { 0.5 };
    }

    /// Squad Pikmin following their captain (not swarming) and Pikmin standing about take
    /// a free pellet within `range` units of its edge and carry it home, as idle Pikmin do
    /// near a pellet in Pikmin 1. 0 (the default) is off.
    pub fn set_auto_carry_range(&mut self, range: f32) {
        self.auto_carry_range = if range.is_finite() { range.max(0.0) } else { 0.0 };
    }

    /// The captain Onion `onion` belongs to: its plucker's Pikmin join him and its emergency
    /// Pikmin come for him.
    pub fn set_onion_owner(&mut self, onion: OnionId, captain: Option<CaptainId>) {
        if let Some(o) = self.onions.get_mut(onion.0 as usize) {
            o.owner = captain;
        }
    }

    /// When an Onion's owner has had no Pikmin anywhere (on the field, in the Onion, as
    /// sprouts or seeds) for `seconds`, the Onion sends him `count`: down its legs when
    /// landed, dropped from it when flying. `count` 0 (the default) is off. Hosts shorten
    /// `seconds` for cooldown items as they go.
    pub fn set_emergency_recovery(&mut self, seconds: f32, count: u32) {
        self.recovery = (count > 0 && seconds.is_finite()).then(|| (seconds.max(0.0), count.min(100)));
    }

    /// Heals each living Pikmin of `captain` by `amount` (of `piki_max_health`), up to full.
    /// Returns the health actually restored, summed.
    pub fn heal_squad(&mut self, captain: CaptainId, amount: f32) -> f32 {
        self.heal_where(amount, |p| p.captain == Some(captain))
    }

    /// Heals each living Pikmin within `radius` of `centre` by `amount`, up to full. Returns
    /// the health actually restored, summed.
    pub fn heal_pikis_in_sphere(&mut self, centre: Vec3, radius: f32, amount: f32) -> f32 {
        let r2 = radius * radius;
        self.heal_where(amount, |p| p.pos.distance_squared(centre) <= r2)
    }

    fn heal_where(&mut self, amount: f32, want: impl Fn(&crate::Piki) -> bool) -> f32 {
        if amount <= 0.0 || !amount.is_finite() {
            return 0.0;
        }
        let max = self.params.piki.piki_max_health;
        let mut total = 0.0;
        for p in &mut self.pikis {
            if !p.is_alive() || p.health <= 0.0 || !want(p) {
                continue;
            }
            let healed = (max - p.health).clamp(0.0, amount);
            p.health += healed;
            total += healed;
        }
        total
    }

    /// The host moved `captain` somewhere far (a teleport): his squad comes with him,
    /// gathered loosely around `pos`, so it isn't left behind or sent running across the map.
    pub fn teleport_squad(&mut self, captain: CaptainId, pos: Vec3) {
        let Some(c) = self.captains.get_mut(captain.0 as usize) else { return };
        if !pos.is_finite() {
            return;
        }
        c.pos = pos;
        c.vel = Vec3::ZERO;
        c.jump = None;
        let members = c.plate.members.clone();
        let n = members.len().max(1) as f32;
        for (k, id) in members.into_iter().enumerate() {
            let p = &mut self.pikis[id.0 as usize];
            if !matches!(p.state, PikiState::Squad { .. } | PikiState::GoHang { .. }) {
                continue;
            }
            let a = k as f32 / n * TAU;
            let ring = 12.0 + 4.0 * (k / 8) as f32;
            p.pos = pos + Vec3::new(a.sin() * ring, 2.0, a.cos() * ring);
            p.vel = Vec3::ZERO;
            p.target_vel = Vec3::ZERO;
            p.jump = None;
            p.jump_queue.clear();
            p.on_ground = false;
        }
    }

    /// A Pikmin following its captain or standing about takes a free pellet in reach
    /// (see [`Sim::set_auto_carry_range`]). Checked every few ticks.
    pub(crate) fn try_auto_carry(&mut self, pi: usize) -> bool {
        if self.auto_carry_range <= 0.0 || !(self.tick + pi as u64).is_multiple_of(6) {
            return false;
        }
        let p = &self.pikis[pi];
        if !p.on_ground || p.jump.is_some() || !p.jump_queue.is_empty() {
            return false;
        }
        let (pos, size) = (p.pos, p.size);
        let range = self.auto_carry_range;
        let Some(pellet) = self
            .pellets
            .iter()
            .filter(|pl| pl.can_take_carrier() && matches!(pl.state, crate::carry::PelletState::Idle))
            .filter(|pl| (pl.pos.y - pos.y).abs() < 60.0)
            .map(|pl| (pl.id, crate::math::dist_xz(pos, pl.pos) - pl.desc.radius - size))
            .filter(|&(_, d)| d <= range)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
        else {
            return false;
        };
        let id = self.pikis[pi].id;
        let dismissed = self.pikis[pi].dismissed;
        self.leave_current(id);
        // Leaving the squad isn't a dismissal: it comes back on its own when it's done.
        self.pikis[pi].dismissed = dismissed;
        self.start_carry(pi, pellet)
    }

    /// Counts down each owned Onion's emergency Pikmin (see [`Sim::set_emergency_recovery`]).
    pub(crate) fn update_recovery(&mut self, dt: f32) {
        let Some((delay, count)) = self.recovery else {
            for o in &mut self.onions {
                o.extinct_for = 0.0;
            }
            return;
        };
        for oi in 0..self.onions.len() {
            let o = &self.onions[oi];
            let Some(owner) = o.owner.filter(|c| (c.0 as usize) < self.captains.len()) else {
                continue;
            };
            let id = o.id;
            let extinct = o.ship.is_none()
                && !self.captains[owner.0 as usize].down
                && o.stored_total() == 0
                && o.queue.is_empty()
                && o.exits.is_empty()
                && !self.pikis.iter().any(|p| p.is_alive() && p.captain == Some(owner))
                && !self.sprouts.iter().flatten().any(|s| s.onion == Some(id) || s.onion.is_none());
            let o = &mut self.onions[oi];
            if !extinct {
                o.extinct_for = 0.0;
                continue;
            }
            o.extinct_for += dt;
            if o.extinct_for < delay {
                continue;
            }
            o.extinct_for = 0.0;
            self.send_recovery(oi, owner, count);
        }
    }

    fn send_recovery(&mut self, oi: usize, owner: CaptainId, count: u32) {
        let onion = self.onions[oi].id;
        let flying = !self.onions[oi].grounded() || self.onions[oi].anim == OnionAnim::Hover;
        let mut sent = 0;
        for k in 0..count as usize {
            let color = RECOVERY_COLOURS[k % 3];
            if !self.onions[oi].accepts(color) {
                continue;
            }
            if !flying {
                // Down the legs like any called-out Pikmin.
                let o = &mut self.onions[oi];
                o.stored[color.index()][crate::Happa::Leaf as usize] += 1;
                o.exits.push_back((color, owner));
                sent += 1;
                continue;
            }
            // Dropped from the hovering Onion, tumbling as thrown Pikmin do.
            let from = self.onions[oi].pos;
            let Some(id) = self.spawn_piki(color, from) else { break };
            let dir = self.rng.f32() * TAU;
            let speed = 30.0 + 50.0 * self.rng.f32();
            let p = &mut self.pikis[id.0 as usize];
            p.captain = Some(owner);
            p.vel = Vec3::new(dir.sin() * speed, 40.0, dir.cos() * speed);
            p.target_vel = p.vel;
            p.on_ground = false;
            p.state = PikiState::Flying { ground_frames: 0 };
            sent += 1;
        }
        if sent > 0 {
            self.events.push(Event::EmergencyPikmin { onion, captain: owner, count: sent });
        }
    }

    /// Living Pikmin of `captain`, anywhere on the field.
    pub fn captain_piki_count(&self, captain: CaptainId) -> u32 {
        self.pikis.iter().filter(|p| p.is_alive() && p.captain == Some(captain)).count() as u32
    }

    /// Spawns `count` Pikmin straight into `captain`'s squad at `pos` (a host's revival
    /// squad), colours in turn. Returns how many fit under the field limit.
    pub fn spawn_squad(&mut self, captain: CaptainId, pos: Vec3, count: u32) -> u32 {
        let mut n = 0;
        for k in 0..count as usize {
            let a = k as f32 / count.max(1) as f32 * TAU;
            let at = pos + Vec3::new(a.sin(), 0.0, a.cos()) * (14.0 + 3.0 * (k / 8) as f32);
            if self.spawn_piki_in_squad(RECOVERY_COLOURS[k % 3], at, captain).is_none() {
                break;
            }
            n += 1;
        }
        n
    }

    /// Removes `captain`'s Pikmin from the field at once, with no ghosts (a host's temporary
    /// squad ending). Returns how many.
    pub fn remove_captain_pikis(&mut self, captain: CaptainId) -> u32 {
        let ids: Vec<PikiId> = self
            .pikis
            .iter()
            .filter(|p| p.on_field() && p.captain == Some(captain))
            .map(|p| p.id)
            .collect();
        for &id in &ids {
            self.leave_current(id);
            self.pikis[id.0 as usize].state = PikiState::Dead;
        }
        ids.len() as u32
    }
}

#[cfg(test)]
mod pace_tests {
    use crate::{Happa, Params, PikiColor, Sim, Vec3};

    #[test]
    fn flower_pikmin_outrun_the_captain_and_leaf_and_bud_are_a_little_slower() {
        let mut sim = Sim::new(Params::default(), 3);
        let c = sim.add_captain(Vec3::ZERO);
        let ids: Vec<_> = (0..3)
            .map(|k| sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(-30.0, 0.0, k as f32 * 8.0), c).unwrap())
            .collect();
        sim.set_captain_speed(c, 1.5);
        sim.set_squad_pace(0.92, 0.97, 1.2);
        sim.pikis[ids[1].0 as usize].happa = Happa::Bud;
        sim.pikis[ids[2].0 as usize].happa = Happa::Flower;
        let top = sim.params().navi.run_speed * 1.5;
        let speed = |sim: &Sim, i: usize| sim.piki_speed(ids[i].0 as usize, 1.0);
        assert!((speed(&sim, 2) - top * 1.2).abs() < 0.01);
        assert!((speed(&sim, 1) - top * 0.97).abs() < 0.01);
        assert!((speed(&sim, 0) - top * 0.92).abs() < 0.01);
        assert!(speed(&sim, 2) > top && speed(&sim, 0) < top);
        // Walking pace sits between the minimum and the top.
        assert!(sim.piki_speed(ids[2].0 as usize, 0.5) < speed(&sim, 2));
        // Off again: the disc's stage speeds, the flower the fastest.
        sim.set_squad_pace(0.0, 1.0, 1.0);
        assert!(speed(&sim, 2) > speed(&sim, 0));
        assert!(speed(&sim, 2) < top * 1.2);
    }
}
