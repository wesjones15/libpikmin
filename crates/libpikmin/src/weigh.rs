//! Pikmin weighing a flying creature down.
//!
//! Pikmin 2 lets a Purple Pikmin's pound bring a flying enemy to the ground ([`Event::Grounded`], `Piki::pound`);
//! the same idea applied evenly: the Pikmin hanging on a flier together weigh it down. A Purple counts as five of the
//! others ([`PikiColor::ground_weight`]; a Purple also carries as ten, `PikiMgr::getColorTransportScale`, which is a
//! different scale). How much a flier needs is the host's to say ([`Sim::set_ground_need`]); these are the numbers it
//! is meant to use: [`SMALL`] 5 (a Pikmin, or 1 Purple), [`LARGE`] 15 (3 Purple), [`BOSS`] 25 (5 Purple) and a
//! shot that can be shot down [`PROJECTILE`] 1. The weight is checked every [`PERIOD`] seconds; while it is enough,
//! the host gets [`Event::WeighedDown`] each time and keeps the creature down a little longer.
//!
//! This is a host rule, not Pikmin 2's: the decomp has no such weight, only the pound.

use crate::event::Event;
use crate::host::CreatureId;
use crate::piki::PikiColor;
use crate::sim::Sim;

/// What a small flier needs: five Pikmin or one Purple.
pub const SMALL: u32 = 5;
/// A large flier: fifteen Pikmin or three Purple.
pub const LARGE: u32 = 15;
/// A boss that flies: twenty-five Pikmin or five Purple.
pub const BOSS: u32 = 25;
/// A projectile that can be shot at: one Pikmin on it.
pub const PROJECTILE: u32 = 1;
/// Seconds between weighings.
pub const PERIOD: f32 = 0.5;

impl PikiColor {
    /// How many ordinary Pikmin it counts as when weighing a flying creature down: a Purple five, the rest one.
    pub fn ground_weight(self) -> u32 {
        if self == PikiColor::Purple { 5 } else { 1 }
    }
}

impl Sim {
    /// Sets how much weight of Pikmin brings the flying creature `creature` down (0 takes the rule away).
    /// Entries for creatures that have left the host's frame are dropped at the next weighing.
    pub fn set_ground_need(&mut self, creature: CreatureId, need: u32) {
        if need == 0 {
            self.ground_need.remove(&creature);
        } else {
            self.ground_need.insert(creature, need);
        }
    }

    /// The weight of Pikmin latched to `creature` now.
    pub fn latched_weight(&self, creature: CreatureId) -> u32 {
        self.pikis
            .iter()
            .filter(|p| p.is_alive() && p.state.latched_to() == Some(creature))
            .map(|p| p.color.ground_weight())
            .sum()
    }

    pub(crate) fn update_weights(&mut self, dt: f32) {
        if self.ground_need.is_empty() {
            return;
        }
        self.weigh_timer += dt;
        if self.weigh_timer < PERIOD {
            return;
        }
        self.weigh_timer = 0.0;
        let mut gone = Vec::new();
        let mut down = Vec::new();
        for (&creature, &need) in &self.ground_need {
            let Some(c) = self.creatures.get(creature) else {
                gone.push(creature);
                continue;
            };
            if !c.alive || !c.flying {
                continue;
            }
            let weight = self.latched_weight(creature);
            if weight >= need {
                // The heaviest Pikmin on it stands for the rest.
                let piki = self
                    .pikis
                    .iter()
                    .filter(|p| p.is_alive() && p.state.latched_to() == Some(creature))
                    .max_by_key(|p| p.color.ground_weight())
                    .map(|p| p.id);
                if let Some(piki) = piki {
                    down.push((creature, piki, weight));
                }
            }
        }
        for creature in gone {
            self.ground_need.remove(&creature);
        }
        down.sort_by_key(|d| d.0);
        for (creature, piki, weight) in down {
            self.events.push(Event::WeighedDown { creature, piki, weight });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn a_purple_counts_as_five() {
        assert_eq!(PikiColor::Purple.ground_weight(), 5);
        for c in [PikiColor::Red, PikiColor::Blue, PikiColor::Yellow, PikiColor::White] {
            assert_eq!(c.ground_weight(), 1);
        }
        // The numbers asked for: five Pikmin or one Purple, fifteen or three, twenty-five or five.
        assert_eq!([SMALL / 5, LARGE / 5, BOSS / 5], [1, 3, 5]);
    }

    /// A flier at id 7 with Pikmin of `colours` latched to it, and `need` set; the weighings in `seconds`.
    fn weigh(colours: &[PikiColor], need: u32, seconds: f32) -> Vec<(u64, u32)> {
        use crate::piki::{Attack, AttackMode, PikiState};
        let mut sim = Sim::default();
        for &c in colours {
            let id = sim.spawn_piki(c, Vec3::ZERO).unwrap();
            sim.pikis[id.0 as usize].state = PikiState::Attack(Attack {
                target: 7,
                mode: AttackMode::Latched { part: 0, offset: Vec3::ZERO, frame: 0.0, hit: false },
            });
        }
        sim.set_ground_need(7, need);
        let part = [crate::host::CreaturePart { centre: Vec3::Y * 50.0, radius: 10.0, stickable: true }];
        sim.creatures.push(7, Vec3::Y * 40.0, 0.0, true, true, &part);
        for _ in 0..(seconds * 30.0) as u32 {
            sim.update_weights(1.0 / 30.0);
        }
        sim.drain_events()
            .filter_map(|e| if let Event::WeighedDown { creature, weight, .. } = e { Some((creature, weight)) } else { None })
            .collect()
    }

    #[test]
    fn five_pikmin_or_one_purple_bring_down_a_small_flier() {
        let red = [PikiColor::Red; 5];
        assert!(weigh(&red[..4], SMALL, 1.1).is_empty(), "four are not enough");
        assert_eq!(weigh(&red, SMALL, 1.1), [(7, 5), (7, 5)], "five weigh it down at each half second");
        assert_eq!(weigh(&[PikiColor::Purple], SMALL, 0.6), [(7, 5)]);
    }

    #[test]
    fn fifteen_pikmin_or_three_purple_for_a_large_one_and_twenty_five_or_five_for_a_boss() {
        assert!(weigh(&[PikiColor::Blue; 14], LARGE, 0.6).is_empty());
        assert_eq!(weigh(&[PikiColor::Blue; 15], LARGE, 0.6).len(), 1);
        assert!(weigh(&[PikiColor::Purple; 2], LARGE, 0.6).is_empty());
        assert_eq!(weigh(&[PikiColor::Purple; 3], LARGE, 0.6).len(), 1);
        assert!(weigh(&[PikiColor::Yellow; 24], BOSS, 0.6).is_empty());
        assert_eq!(weigh(&[PikiColor::Yellow; 25], BOSS, 0.6).len(), 1);
        assert!(weigh(&[PikiColor::Purple; 4], BOSS, 0.6).is_empty());
        assert_eq!(weigh(&[PikiColor::Purple; 5], BOSS, 0.6).len(), 1);
        // A mixed crew adds up: two Purple (ten) and five others make a large flier's fifteen.
        let mut mixed = vec![PikiColor::Purple; 2];
        mixed.extend([PikiColor::Red; 5]);
        assert_eq!(weigh(&mixed, LARGE, 0.6), [(7, 15)]);
    }

    #[test]
    fn a_projectile_needs_one_pikmin_and_a_creature_that_is_not_flying_is_left_alone() {
        assert_eq!(weigh(&[PikiColor::White], PROJECTILE, 0.6).len(), 1);
        assert!(weigh(&[], PROJECTILE, 0.6).is_empty());
    }
}
