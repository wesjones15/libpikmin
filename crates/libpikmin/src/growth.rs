//! Host extension, not Pikmin 1: ways to grow besides a sprout's own pace and nectar, for
//! Risk of Rain 2's runs (backlog O-04, O-05).
//!
//! - **Planted Pikmin** ([`Sim::set_sprout_stages`]): a sprout becomes a bud, then a flower,
//!   after a fixed number of seconds per stage, and its flower lasts a fixed time before
//!   wilting. The timer starts when the sprout is pluckable and starts over at each stage.
//!   The disc's `grow_up_time` and `wilt_time` (itemAI.cpp `PikiHeadAI`) are the defaults.
//! - **Kills** ([`Sim::set_kill_growth`], [`Sim::credit_kill`]): when the host reports a
//!   creature dead, every living Pikmin that attacked it earns one credit; every N credits
//!   raise it one stage, to a flower at most. Only the creature's attackers count, so
//!   sprouts (which never fight) and Pikmin that stood by don't grow.

use crate::Sim;
use crate::event::Event;
use crate::host::CreatureId;
use crate::piki::{Happa, PikiState};

/// Creatures a Pikmin remembers having attacked.
pub(crate) const HIT_LOG: usize = 8;

impl Sim {
    /// Seconds a planted sprout spends as a leaf, then as a bud, and how long its flower
    /// stands before wilting. 0 or less for either keeps the disc's time. Unlike the disc's
    /// times these have no random extra, and cooldown items ([`Sim::set_growth_rate`]) still
    /// speed up growth but not wilting.
    pub fn set_sprout_stages(&mut self, stage: f32, flower: f32) {
        let ok = |s: f32| s.is_finite() && s > 0.0;
        self.sprout_stage = ok(stage).then_some(stage);
        self.sprout_flower = ok(flower).then_some(flower);
    }

    /// Credited kills it takes to raise a Pikmin one stage; 0 (the default) is off.
    pub fn set_kill_growth(&mut self, kills: u32) {
        self.kill_growth = kills.min(1000);
    }

    /// Remembers that Pikmin `pi` attacked `creature`, for [`Sim::credit_kill`].
    pub(crate) fn note_attack(&mut self, pi: usize, creature: CreatureId) {
        let p = &mut self.pikis[pi];
        if p.hit_log.contains(&Some(creature)) {
            return;
        }
        p.hit_log[p.hit_next as usize % HIT_LOG] = Some(creature);
        p.hit_next = p.hit_next.wrapping_add(1);
    }

    /// How many living Pikmin of each colour ([`PikiColor`] order) attacked `creature` and have not been
    /// credited for it yet: the ones that fought it, for the host to colour its corpse by. Ask before
    /// [`Sim::credit_kill`], which forgets them.
    pub fn attackers_by_colour(&self, creature: CreatureId) -> [u32; 3] {
        let mut by_colour = [0; 3];
        for p in &self.pikis {
            if p.is_alive() && p.hit_log.contains(&Some(creature)) {
                by_colour[p.color as usize] += 1;
            }
        }
        by_colour
    }

    /// `creature` died, to anything. Each living Pikmin that attacked it gets a credit (once
    /// per creature, whatever its hits), and grows a stage on every
    /// [`Sim::set_kill_growth`] credits. Returns how many Pikmin grew.
    pub fn credit_kill(&mut self, creature: CreatureId) -> u32 {
        let need = self.kill_growth;
        let mut grew = 0;
        for pi in 0..self.pikis.len() {
            let p = &mut self.pikis[pi];
            let Some(slot) = p.hit_log.iter().position(|c| *c == Some(creature)) else { continue };
            p.hit_log[slot] = None;
            if need == 0 || !p.is_alive() || matches!(p.state, PikiState::Swallowed { .. }) {
                continue;
            }
            if p.happa == Happa::Flower {
                p.kill_credit = 0;
                continue;
            }
            p.kill_credit = p.kill_credit.saturating_add(1);
            if u32::from(p.kill_credit) < need {
                continue;
            }
            p.kill_credit = 0;
            p.happa = if p.happa == Happa::Leaf { Happa::Bud } else { Happa::Flower };
            let (piki, happa) = (p.id, p.happa);
            self.events.push(Event::PikiGrew { piki, happa });
            grew += 1;
        }
        grew
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FlatHost;
    use crate::{Params, PikiColor, Vec3};

    fn sim_with(n: usize) -> Sim {
        let mut sim = Sim::new(Params::default(), 3);
        let c = sim.add_captain(Vec3::ZERO);
        for k in 0..n {
            sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(k as f32 * 10.0, 0.0, 0.0), c);
        }
        sim.step(&mut FlatHost);
        sim
    }

    #[test]
    fn only_attackers_earn_credit_and_two_kills_raise_a_stage() {
        let mut sim = sim_with(3);
        sim.set_kill_growth(2);
        sim.note_attack(0, 40);
        sim.note_attack(0, 40); // the same creature again is still one credit
        sim.note_attack(1, 40);
        assert_eq!(sim.credit_kill(40), 0);
        assert_eq!(sim.credit_kill(40), 0, "a second report of the same death earns nothing");
        sim.note_attack(0, 41);
        sim.note_attack(2, 41);
        assert_eq!(sim.credit_kill(41), 1);
        let happa: Vec<_> = sim.pikis().iter().map(|p| p.happa).collect();
        assert_eq!(happa, [Happa::Bud, Happa::Leaf, Happa::Leaf]);
        let grew = sim.drain_events().filter(|e| matches!(e, Event::PikiGrew { .. })).count();
        assert_eq!(grew, 1);
    }

    #[test]
    fn the_attackers_are_counted_by_colour_until_the_kill_is_credited() {
        let mut sim = sim_with(1);
        let c = sim.captains()[0].id;
        sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(40.0, 0.0, 0.0), c);
        sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(50.0, 0.0, 0.0), c);
        sim.step(&mut FlatHost);
        assert_eq!(sim.attackers_by_colour(7), [0, 0, 0]);
        sim.note_attack(0, 7);
        sim.note_attack(1, 7);
        sim.note_attack(2, 7);
        sim.note_attack(1, 8);
        assert_eq!(sim.attackers_by_colour(7), [2, 1, 0], "blue, red, yellow");
        assert_eq!(sim.attackers_by_colour(8), [1, 0, 0]);
        sim.credit_kill(7);
        assert_eq!(sim.attackers_by_colour(7), [0, 0, 0], "credited: forgotten");
    }

    #[test]
    fn growth_stops_at_a_flower_and_off_means_off() {
        let mut sim = sim_with(1);
        sim.set_kill_growth(1);
        for k in 0..4 {
            sim.note_attack(0, k);
            sim.credit_kill(k);
        }
        assert_eq!(sim.pikis()[0].happa, Happa::Flower);
        assert_eq!(sim.pikis()[0].kill_credit, 0);

        let mut sim = sim_with(1);
        sim.note_attack(0, 5);
        assert_eq!(sim.credit_kill(5), 0);
        assert_eq!(sim.pikis()[0].happa, Happa::Leaf);
    }

    #[test]
    fn a_dead_pikmin_earns_nothing() {
        let mut sim = sim_with(1);
        sim.set_kill_growth(1);
        sim.note_attack(0, 9);
        sim.pikis[0].state = PikiState::Dead;
        assert_eq!(sim.credit_kill(9), 0);
    }

    #[test]
    fn it_remembers_the_most_recent_creatures() {
        let mut sim = sim_with(1);
        sim.set_kill_growth(1);
        for k in 0..(HIT_LOG as u64 + 2) {
            sim.note_attack(0, k);
        }
        assert_eq!(sim.credit_kill(0), 0, "pushed out of the log");
        assert_eq!(sim.credit_kill(HIT_LOG as u64 + 1), 1);
    }
}
