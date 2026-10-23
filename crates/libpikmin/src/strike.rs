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
use crate::piki::{Attack, AttackMode, PikiState};
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
    /// Seconds of forward swarm when there is no target; also enables jump targeting. 0 disables both.
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
    /// squad charging for `charge` seconds. Nearby enemies in the strike direction become
    /// individual jump-and-latch targets; without one, the squad swarms forward as before.
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
        let c = &self.captains[ci];
        if c.strike.is_some() || c.strike_cooldown > 0.0 || c.punch.is_some() || !matches!(c.throw, ThrowState::Idle) {
            return;
        }
        // Toward the locked creature if there is one, else the way he faces.
        let aim = c.lock_target.and_then(|id| self.creatures.get(id)).map(|t| t.pos - c.pos);
        let dir = aim.filter(|d| d.x.hypot(d.z) > 1.0).map_or(crate::math::dir_of(c.face), |d| d.normalize_or_zero());
        let (cpos, locked) = (c.pos, c.lock_target);
        let captain = c.id;
        let c = &mut self.captains[ci];
        c.strike = Some(Strike { frame: 0.0, hit: false });
        c.strike_cooldown = spec.cooldown;
        c.charge = (spec.charge > 0.0).then(|| (crate::Vec2::new(dir.x, dir.z), spec.charge));
        if spec.charge > 0.0 {
            self.strike_targets(ci, cpos, dir, locked, spec.charge);
        }
        self.events.push(Event::StrikeStarted { captain });
    }

    /// Assign only Pikmin actually in the formation. A forward cone keeps enemies beside or
    /// behind Olimar out of the command; distance is bounded by what a charging Pikmin can reach.
    fn strike_targets(&mut self, ci: usize, origin: crate::Vec3, dir: crate::Vec3, locked: Option<crate::CreatureId>, seconds: f32) {
        let reach = (self.params.navi.run_speed * seconds).clamp(200.0, 700.0);
        let forward = crate::Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
        let candidates: Vec<_> = self.creatures.creatures.iter().filter_map(|c| {
            if !c.alive || !self.creatures.parts_of(c).iter().any(|p| p.stickable) {
                return None;
            }
            let (centre, radius) = self.creatures.bounds(c);
            let offset = crate::Vec3::new(centre.x - origin.x, 0.0, centre.z - origin.z);
            let distance = offset.length();
            (distance <= reach + radius && (distance <= radius || offset.dot(forward) >= distance * 0.5)
                && (centre.y - origin.y).abs() <= radius + 300.0).then_some((c.id, centre))
        }).collect();
        if candidates.is_empty() { return; }

        let members = self.captains[ci].plate.members.clone();
        let mut assigned = vec![0usize; candidates.len()];
        for id in members {
            let pi = id.0 as usize;
            if !matches!(self.pikis[pi].state, PikiState::Squad { .. }) { continue; }
            let pos = self.pikis[pi].pos;
            let chosen = candidates.iter().enumerate().min_by(|(ai, a), (bi, b)| {
                let score = |index: usize, target: &(crate::CreatureId, crate::Vec3)| {
                    // The lock-on wins if it is a valid forward target. Otherwise spread the
                    // squad among nearby enemies without sending anyone far across the arena.
                    if Some(target.0) == locked { -10000.0 }
                    else { dist_xz(pos, target.1) + assigned[index] as f32 * 35.0 }
                };
                score(*ai, a).total_cmp(&score(*bi, b))
            }).map(|(index, _)| index).unwrap();
            assigned[chosen] += 1;
            self.leave_current(id);
            self.pikis[pi].state = PikiState::Attack(Attack { target: candidates[chosen].0, mode: AttackMode::StrikeRush });
        }
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
