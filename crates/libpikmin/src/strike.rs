//! Host extension, not Pikmin 1: the dismiss button as a melee strike.
//!
//! Pikmin 1's dismiss (`NaviReleaseState`, naviState.cpp) sends the squad off to stand about,
//! which only matters where a level has captain-specific puzzles. A host without those can
//! turn it into an attack instead ([`Sim::set_dismiss_strike`]): the squad stays put, and
//! Olimar plays the dismissal's own motion ("FUE", with `SE_BREAKUP`) and hits every creature
//! around him for more than a punch does. It has a cooldown.

use crate::captain::ThrowState;
use crate::event::Event;
use crate::math::dist_xz;
use crate::{Host, Sim};

/// Frames (30 per second) into the motion the blow lands, and when it ends.
pub const STRIKE_ACTION: f32 = 8.0;
pub const STRIKE_END: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrikeSpec {
    /// Times `punch_damage`.
    pub damage_scale: f32,
    /// Sim units beyond a creature's own size that the blow reaches, all around.
    pub reach: f32,
    /// Seconds before the next strike.
    pub cooldown: f32,
    /// Seconds the squad charges out the way he struck (a swarm), 0 for none.
    pub charge: f32,
}

/// A strike in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    pub frame: f32,
    pub hit: bool,
}

impl Sim {
    /// Makes the dismiss button strike instead of dismissing: `damage_scale` times a punch's
    /// damage to every creature within `reach` units of its edge, then `cooldown` seconds
    /// before the next. A scale of 0 or less puts the dismissal back. The strike also sends the
    /// squad charging for `charge` seconds, as a swarm toward what he struck at (the creature he
    /// is locked onto, else the way he faces).
    pub fn set_dismiss_strike(&mut self, damage_scale: f32, reach: f32, cooldown: f32, charge: f32) {
        self.dismiss_strike = (damage_scale.is_finite() && damage_scale > 0.0 && reach.is_finite() && cooldown.is_finite())
            .then(|| StrikeSpec {
                damage_scale,
                reach: reach.max(0.0),
                cooldown: cooldown.max(0.0),
                charge: if charge.is_finite() { charge.max(0.0) } else { 0.0 },
            });
    }

    /// Host extension: every captain's punch (`NaviAttackState`) does `damage` instead of the disc's
    /// `punch_damage`. A strike is a multiple of the punch, so it follows. 0 or less (or not a
    /// number) leaves the disc's value.
    pub fn set_punch_damage(&mut self, damage: f32) {
        if damage.is_finite() && damage > 0.0 {
            self.params.navi.punch_damage = damage;
        }
    }

    /// The dismiss button was pressed with strikes on.
    pub(crate) fn start_strike(&mut self, ci: usize) {
        let Some(spec) = self.dismiss_strike else { return };
        let c = &mut self.captains[ci];
        if c.strike.is_some() || c.strike_cooldown > 0.0 || c.punch.is_some() || !matches!(c.throw, ThrowState::Idle) {
            return;
        }
        c.strike = Some(Strike { frame: 0.0, hit: false });
        c.strike_cooldown = spec.cooldown;
        // Toward the locked creature if there is one, else the way he faces.
        let aim = c.lock_target.and_then(|id| self.creatures.get(id)).map(|t| t.pos - c.pos);
        let dir = aim.filter(|d| d.x.hypot(d.z) > 1.0).map_or(crate::math::dir_of(c.face), |d| d.normalize_or_zero());
        c.charge = (spec.charge > 0.0).then(|| (crate::Vec2::new(dir.x, dir.z), spec.charge));
        let captain = c.id;
        self.events.push(Event::StrikeStarted { captain });
    }

    /// Runs a strike in progress; true while it holds the captain's hands.
    pub(crate) fn captain_strike(&mut self, ci: usize, host: &mut dyn Host, dt: f32) -> bool {
        let (Some(mut strike), Some(spec)) = (self.captains[ci].strike, self.dismiss_strike) else {
            self.captains[ci].strike = None;
            return false;
        };
        strike.frame += self.params.anim.fps * dt * self.captains[ci].attack_rate;
        if !strike.hit && strike.frame >= STRIKE_ACTION {
            strike.hit = true;
            let (cpos, cid) = (self.captains[ci].pos, self.captains[ci].id);
            let damage = self.params.navi.punch_damage * spec.damage_scale;
            let targets: Vec<_> = self
                .creatures
                .creatures
                .iter()
                .filter(|c| c.alive && !c.flying && c.parts_len > 0)
                .filter(|c| {
                    let size = self.creatures.bounds(c).1;
                    dist_xz(c.pos, cpos) <= size + spec.reach && (c.pos.y - cpos.y).abs() <= size + spec.reach
                })
                .map(|c| c.id)
                .collect();
            for creature in targets {
                if host.damage(creature, damage, None, crate::host::DamageSource::Strike(cid)) {
                    self.events.push(Event::Punched { captain: cid, creature, damage });
                }
            }
        }
        self.captains[ci].strike = (strike.frame < STRIKE_END).then_some(strike);
        true
    }
}

#[cfg(test)]
mod punch_tests {
    use crate::{Params, Sim};

    #[test]
    fn the_punch_damage_can_be_set_and_junk_is_ignored() {
        let mut sim = Sim::new(Params::default(), 1);
        sim.set_punch_damage(22.5);
        assert_eq!(sim.params.navi.punch_damage, 22.5);
        for bad in [0.0, -3.0, f32::NAN, f32::INFINITY] {
            sim.set_punch_damage(bad);
            assert_eq!(sim.params.navi.punch_damage, 22.5);
        }
    }
}
