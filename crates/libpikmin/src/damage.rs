//! Pikmin dying and captains taking damage.
//!
//! Enemies are the host's own AI, so the host decides *when* something bites, stomps or
//! swipes and calls these. They mirror the decomp's interactions (interactBattle.cpp,
//! navi.cpp): `InteractAttack`, `InteractPress`, `InteractFlick`, `InteractSwallow`,
//! `InteractKill`.

use glam::Vec3;

use crate::event::Event;
use crate::host::CreatureId;
use crate::math::rotate_y;
use crate::onion::SproutState;
use crate::piki::{Happa, PikiState};
use crate::sim::Sim;
use crate::{CaptainId, PikiId};

/// Why a Pikmin died, for the host's effects and sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathCause {
    /// Health ran out from attacks or flicks.
    Attacked,
    /// Flattened (`PikiPressedState`).
    Crushed,
    /// Chewed while in an enemy's mouth.
    Eaten,
    /// `InteractKill` / host-decided (fell into a pit, hazard...).
    Killed,
}

impl Sim {
    fn hittable(&self, pi: usize) -> bool {
        !matches!(
            self.pikis[pi].state,
            PikiState::Dying { .. }
                | PikiState::Fading { .. }
                | PikiState::Dead
                | PikiState::Swallowed { .. }
                // `InteractAttack` leaves Pikmin going into or out of an Onion alone
                // (interactBattle.cpp).
                | PikiState::Enter { .. }
                | PikiState::Exit { .. }
                // Host extension: an Onion's plucker is on an errand and can't be hurt.
                | PikiState::Plucker(_)
        )
    }

    /// `InteractAttack::actPiki`: lose `damage` health, start dying at 0. Knocked-down
    /// (flicked) Pikmin can't be hit. Returns whether the hit landed.
    pub fn attack_piki(&mut self, id: PikiId, damage: f32) -> bool {
        let pi = id.0 as usize;
        if !self.hittable(pi)
            || matches!(
                self.pikis[pi].state,
                PikiState::Knocked { .. } | PikiState::Pressed { .. }
            )
        {
            return false;
        }
        let p = &mut self.pikis[pi];
        p.health -= damage;
        p.target_vel = Vec3::ZERO;
        self.events.push(Event::PikiHurt { piki: id, damage });
        if self.pikis[pi].health <= 0.0 {
            self.start_dying(pi, DeathCause::Attacked);
        }
        true
    }

    /// Attack every Pikmin within `radius` of `centre` (a swipe or a stomp's shockwave).
    /// Returns how many were hit.
    pub fn attack_pikis_in_sphere(&mut self, centre: Vec3, radius: f32, damage: f32) -> u32 {
        self.ids_in_sphere(centre, radius)
            .into_iter()
            .filter(|&id| self.attack_piki(id, damage))
            .count() as u32
    }

    /// `InteractPress::actPiki`: flattened. If that brings health to 0 the Pikmin dies at once;
    /// otherwise it stays flat for the 1.5 s stun plus `mPressDeathFactor`, then gets up
    /// (`PikiPressedState`).
    pub fn crush_pikis_in_sphere(&mut self, centre: Vec3, radius: f32, damage: f32) -> u32 {
        let mut n = 0;
        for id in self.ids_in_sphere(centre, radius) {
            let pi = id.0 as usize;
            if !self.hittable(pi) || matches!(self.pikis[pi].state, PikiState::Pressed { .. }) {
                continue;
            }
            self.leave_current(id);
            let death_timer = self.params.piki.press_death_factor;
            let p = &mut self.pikis[pi];
            p.health -= damage;
            p.vel = Vec3::ZERO;
            p.target_vel = Vec3::ZERO;
            if p.health <= 0.0 {
                // The press check fires on the first tick: death timer > factor − normal factor.
                self.piki_deaths += 1;
                self.events.push(Event::PikiDied {
                    piki: id,
                    cause: DeathCause::Crushed,
                });
                self.pikis[pi].state = PikiState::Fading { timer: 1.5 };
            } else {
                p.state = PikiState::Pressed {
                    stun: 1.5,
                    death_timer,
                };
            }
            n += 1;
        }
        n
    }

    /// `InteractFlick::actPiki`: knocked away from `from` (also latched ones), losing `damage`.
    pub fn flick_pikis_in_sphere(
        &mut self,
        centre: Vec3,
        radius: f32,
        damage: f32,
        from: Vec3,
    ) -> u32 {
        let mut n = 0;
        for id in self.ids_in_sphere(centre, radius) {
            let pi = id.0 as usize;
            if !self.hittable(pi) || matches!(self.pikis[pi].state, PikiState::Knocked { .. }) {
                continue;
            }
            self.leave_current(id);
            self.pikis[pi].health -= damage;
            self.knock_from(pi, from, None);
            n += 1;
        }
        n
    }

    /// Host extension, not Pikmin 1 (it has no ice): every Pikmin within `radius` of `centre`
    /// is frozen for `seconds` (a host's freezing blast). It lets go of whatever it was
    /// carrying, fighting or walking to, stands where it is and does nothing until it thaws;
    /// squad members stay in the squad but can't be thrown. Pikmin in a captain's hand,
    /// knocked or flattened, or out of reach of attacks are left alone. Returns how many froze.
    pub fn freeze_pikis_in_sphere(&mut self, centre: Vec3, radius: f32, seconds: f32) -> u32 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0;
        }
        let seconds = seconds.min(30.0);
        let mut n = 0;
        for id in self.ids_in_sphere(centre, radius) {
            let pi = id.0 as usize;
            if !self.hittable(pi)
                || matches!(
                    self.pikis[pi].state,
                    PikiState::Held { .. } | PikiState::Knocked { .. } | PikiState::Pressed { .. }
                )
            {
                continue;
            }
            match self.pikis[pi].state {
                PikiState::Squad { .. } | PikiState::Idle => {}
                // On its way to his hand: back to the squad, the throw called off.
                PikiState::GoHang { captain } => {
                    self.leave_current(id);
                    self.join_squad(id, captain);
                }
                _ => self.leave_current(id),
            }
            let p = &mut self.pikis[pi];
            p.frozen = p.frozen.max(seconds);
            p.jump = None;
            p.jump_queue.clear();
            p.target_vel = Vec3::ZERO;
            p.vel.x = 0.0;
            p.vel.z = 0.0;
            n += 1;
        }
        n
    }

    /// `InteractSwallow::actPiki`: up to `max` Pikmin within `radius` of the mouth end up stuck in
    /// it (they ride along with the creature's part `mouth_part`). Follow with
    /// [`Sim::chew`] to kill them or [`Sim::spit_out`] to let them go. If the creature dies
    /// first they're freed (`PikiSwallowedState::exec`).
    pub fn swallow_pikis(
        &mut self,
        creature: CreatureId,
        mouth_part: u32,
        centre: Vec3,
        radius: f32,
        max: u32,
    ) -> u32 {
        let Some(c) = self.creatures.get(creature).copied() else {
            return 0;
        };
        let Some(part) = self
            .creatures
            .parts_of(&c)
            .get(mouth_part as usize)
            .copied()
        else {
            return 0;
        };
        let mut ids = self.ids_in_sphere(centre, radius);
        ids.sort_by(|a, b| {
            let da = (self.pikis[a.0 as usize].pos - centre).length();
            let db = (self.pikis[b.0 as usize].pos - centre).length();
            da.total_cmp(&db)
        });
        let mut n = 0;
        for id in ids {
            if n >= max {
                break;
            }
            let pi = id.0 as usize;
            if !self.hittable(pi) {
                continue;
            }
            self.leave_current(id);
            let p = &mut self.pikis[pi];
            let offset = rotate_y(
                (p.pos - part.centre).clamp_length_max(part.radius * 0.5),
                -c.yaw,
            );
            p.state = PikiState::Swallowed {
                creature,
                part: mouth_part,
                offset,
            };
            p.vel = Vec3::ZERO;
            self.events.push(Event::Swallowed { piki: id, creature });
            n += 1;
        }
        n
    }

    /// The creature finishes eating: everything in its mouth dies (no seed is left).
    pub fn chew(&mut self, creature: CreatureId) -> u32 {
        let mut n = 0;
        for pi in 0..self.pikis.len() {
            if matches!(self.pikis[pi].state, PikiState::Swallowed { creature: c, .. } if c == creature)
            {
                let id = self.pikis[pi].id;
                self.pikis[pi].state = PikiState::Fading { timer: 1.5 };
                self.piki_deaths += 1;
                self.events.push(Event::PikiDied {
                    piki: id,
                    cause: DeathCause::Eaten,
                });
                n += 1;
            }
        }
        n
    }

    /// The creature lets go of what's in its mouth (it was hit, it's shaking, ...).
    pub fn spit_out(&mut self, creature: CreatureId) -> u32 {
        let mut n = 0;
        for pi in 0..self.pikis.len() {
            if matches!(self.pikis[pi].state, PikiState::Swallowed { creature: c, .. } if c == creature)
            {
                self.free_from_mouth(pi);
                n += 1;
            }
        }
        n
    }

    /// `InteractBury::actPiki` / `PikiBuryState::exec`: every Pikmin within `radius` of `centre` is pushed into the ground
    /// and comes back up as a sprout, thrown a little way (220 sideways, 540 up) and landing to take root; with
    /// `make_flower` a leaf or bud is a flower. Returns where each was buried.
    pub fn bury_pikis_in_sphere(&mut self, centre: Vec3, radius: f32, make_flower: bool) -> Vec<Vec3> {
        let mut at = Vec::new();
        for id in self.ids_in_sphere(centre, radius) {
            let pi = id.0 as usize;
            if !self.hittable(pi) || matches!(self.pikis[pi].state, PikiState::Pressed { .. }) {
                continue;
            }
            self.leave_current(id);
            let (color, happa, pos) = {
                let p = &self.pikis[pi];
                (p.color, p.happa, p.pos)
            };
            self.pikis[pi].state = PikiState::Dead;
            let angle = self.rng.f32() * std::f32::consts::TAU;
            let vel = Vec3::new(220.0 * angle.sin(), 540.0, 220.0 * angle.cos());
            let sprout = self.launch_sprout(color, if make_flower { crate::Happa::Flower } else { happa }, pos, vel);
            let _ = sprout;
            at.push(pos);
        }
        at
    }

    /// `InteractKill`: dies on the spot (no death animation).
    pub fn kill_piki(&mut self, id: PikiId) {
        let pi = id.0 as usize;
        if matches!(
            self.pikis[pi].state,
            PikiState::Dying { .. } | PikiState::Fading { .. } | PikiState::Dead | PikiState::Plucker(_)
        ) {
            return;
        }
        self.leave_current(id);
        self.pikis[pi].state = PikiState::Fading { timer: 1.5 };
        self.piki_deaths += 1;
        self.events.push(Event::PikiDied {
            piki: id,
            cause: DeathCause::Killed,
        });
    }

    fn ids_in_sphere(&self, centre: Vec3, radius: f32) -> Vec<PikiId> {
        self.pikis
            .iter()
            .filter(|p| (p.pos - centre).length() <= radius + p.size)
            .map(|p| p.id)
            .collect()
    }

    /// `PikiDyingState::init`: a random death animation, then `PikiDeadState`.
    pub(crate) fn start_dying(&mut self, pi: usize, cause: DeathCause) {
        let id = self.pikis[pi].id;
        self.leave_current(id);
        let ends = self.params.anim.dead;
        let end = ends[((self.rng.f32() * 3.0) as usize).min(2)];
        let p = &mut self.pikis[pi];
        p.vel = Vec3::ZERO;
        p.target_vel = Vec3::ZERO;
        p.state = PikiState::Dying { frame: 0.0, end };
        self.piki_deaths += 1;
        self.events.push(Event::PikiDied { piki: id, cause });
    }

    pub(crate) fn update_dying(&mut self, pi: usize, host: &mut dyn crate::Host, dt: f32) {
        match self.pikis[pi].state {
            PikiState::Dying { frame, end } => {
                let f = frame + self.params.anim.fps * dt;
                self.pikis[pi].target_vel = Vec3::ZERO;
                self.physics(pi, host, dt);
                self.pikis[pi].state = if f >= end {
                    PikiState::Fading { timer: 1.5 }
                } else {
                    PikiState::Dying { frame: f, end }
                };
            }
            // `PikiDeadState`: shrink out over 1.5 s, then gone (`Piki::doKill`).
            PikiState::Fading { timer } => {
                let t = timer - dt;
                if t >= 0.0 {
                    self.pikis[pi].state = PikiState::Fading { timer: t };
                    return;
                }
                self.pikis[pi].state = PikiState::Dead;
                let p = &self.pikis[pi];
                // `Piki::doKill`: the ghost and `SE_PIKI_DEAD` at the shadow (the ground below).
                let ground = host.ground_height(p.pos + Vec3::Y * 20.0).unwrap_or(p.pos.y);
                self.events.push(Event::PikiGone {
                    piki: p.id,
                    pos: p.pos.with_y(ground.min(p.pos.y)),
                });
                let p = &self.pikis[pi];
                // A dead flower may leave a seed: rand >= mPikiLeaveSeedChance (so 20% at 0.8).
                // The original saves it for the next day; we bury it right away.
                if p.happa == Happa::Flower
                    && self.rng.f32() >= self.params.piki.piki_leave_seed_chance
                {
                    let (color, pos) = (p.color, p.pos);
                    let up = self.params.piki.seed_up_time;
                    let s = self.plant_sprout(color, pos);
                    if let Some(Some(sp)) = self.sprouts.get_mut(s.0 as usize) {
                        sp.state = SproutState::Buried { timer: up };
                    }
                }
            }
            // `PikiPressedState::exec`
            PikiState::Pressed {
                stun,
                mut death_timer,
            } => {
                let pp = &self.params.piki;
                let invincible = stun > 0.0;
                if !invincible {
                    death_timer -= dt;
                }
                let p = &mut self.pikis[pi];
                p.vel = Vec3::ZERO;
                p.target_vel = Vec3::ZERO;
                if death_timer > pp.press_death_factor - pp.normal_piki_press_factor
                    && p.health <= 0.0
                {
                    let id = p.id;
                    p.state = PikiState::Fading { timer: 1.5 };
                    self.piki_deaths += 1;
                    self.events.push(Event::PikiDied {
                        piki: id,
                        cause: DeathCause::Crushed,
                    });
                } else if death_timer < 0.0 {
                    p.state = PikiState::Idle;
                } else {
                    p.state = PikiState::Pressed {
                        stun: stun - dt,
                        death_timer,
                    };
                }
            }
            // `PikiSwallowedState`: ride in the mouth; freed if the creature is gone.
            PikiState::Swallowed {
                creature,
                part,
                offset,
            } => {
                let mouth = self
                    .creatures
                    .get(creature)
                    .filter(|c| c.alive)
                    .copied()
                    .and_then(|c| {
                        self.creatures
                            .parts_of(&c)
                            .get(part as usize)
                            .map(|pt| pt.centre + rotate_y(offset, c.yaw))
                    });
                match mouth {
                    Some(pos) => {
                        let p = &mut self.pikis[pi];
                        p.pos = pos;
                        p.vel = Vec3::ZERO;
                    }
                    None => self.free_from_mouth(pi),
                }
            }
            _ => {}
        }
    }

    fn free_from_mouth(&mut self, pi: usize) {
        let p = &mut self.pikis[pi];
        p.state = PikiState::Idle;
        p.vel = Vec3::new(0.0, 60.0, 0.0);
        p.on_ground = false;
        self.events.push(Event::SpitOut { piki: p.id });
    }

    // ---- captains ----------------------------------------------------------------------

    /// `InteractAttack::actNavi`: lose `damage` health. After a hit the captain is invulnerable
    /// for the "DAMAGE" animation (19 frames); if health is ≤ 1 when it ends he goes down
    /// (`Navi::finishDamage` → `NaviDeadState`). Returns whether the hit landed.
    pub fn damage_captain(&mut self, id: CaptainId, damage: f32) -> bool {
        let invuln = self.params.anim.damage.end;
        let c = &mut self.captains[id.0 as usize];
        if c.down || c.damage_frames > 0.0 {
            return false;
        }
        c.health -= damage;
        c.damage_frames = invuln;
        self.events.push(Event::CaptainHurt {
            captain: id,
            damage,
            health: c.health,
        });
        true
    }

    /// Full heal, like stepping into the ship (`NaviUfoState::cleanup`), or by `amount`.
    pub fn heal_captain(&mut self, id: CaptainId, amount: Option<f32>) {
        let max = self.params.navi.health;
        let c = &mut self.captains[id.0 as usize];
        c.health = amount.map_or(max, |a| (c.health + a).min(max));
    }

    /// Puts a captain down right away (e.g. the host's player character died).
    pub fn down_captain(&mut self, id: CaptainId) {
        let ci = id.0 as usize;
        if self.captains[ci].down {
            return;
        }
        self.captains[ci].down = true;
        self.captains[ci].health = self.captains[ci].health.min(0.0);
        // `NaviDeadState::init` → `Navi::releasePikis`: the squad is left behind.
        self.dismiss(ci);
        self.events.push(Event::CaptainDown { captain: id });
    }

    /// Back on his feet at `pos` with full health.
    pub fn revive_captain(&mut self, id: CaptainId, pos: Vec3) {
        let max = self.params.navi.health;
        let c = &mut self.captains[id.0 as usize];
        c.down = false;
        c.health = max;
        c.damage_frames = 0.0;
        c.pos = pos;
        c.vel = Vec3::ZERO;
        // Going down left his squad behind as if dismissed; it may find him again.
        for p in &mut self.pikis {
            if p.captain == Some(id) {
                p.dismissed = false;
            }
        }
    }

    /// Ticks the damage animation; at its end a captain at ≤ 1 health goes down.
    pub(crate) fn update_captain_damage(&mut self, ci: usize, dt: f32) {
        let fps = self.params.anim.fps;
        let c = &mut self.captains[ci];
        if c.damage_frames > 0.0 {
            c.damage_frames -= fps * dt;
            if c.damage_frames <= 0.0 {
                c.damage_frames = 0.0;
                if c.health <= 1.0 {
                    let id = c.id;
                    self.down_captain(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod freeze_tests {
    use crate::captain::ThrowState;
    use crate::host::FlatHost;
    use crate::piki::PikiState;
    use crate::{CaptainInput, Params, PikiColor, Sim, Vec3};

    #[test]
    fn frozen_squad_pikmin_stand_still_cant_be_thrown_and_thaw() {
        let mut sim = Sim::new(Params::default(), 7);
        let c = sim.add_captain(Vec3::ZERO);
        let id = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-20.0, 0.0, 0.0), c).unwrap();
        for _ in 0..10 {
            sim.step(&mut FlatHost);
        }
        assert_eq!(sim.freeze_pikis_in_sphere(sim.piki(id).pos, 5.0, 1.0), 1);
        // He walks off: the frozen one stays put and isn't picked for a throw.
        let at = sim.piki(id).pos;
        for _ in 0..15 {
            sim.set_input(c, CaptainInput { stick: glam::Vec2::new(1.0, 0.0), throw: true, ..Default::default() });
            sim.step(&mut FlatHost);
        }
        assert!(sim.piki(id).pos.distance(at) < 0.5, "moved while frozen");
        assert!(matches!(sim.piki(id).state, PikiState::Squad { .. }));
        assert!(matches!(sim.captain(c).throw, ThrowState::Idle));
        // Thawed, it follows him again.
        for _ in 0..30 {
            sim.set_input(c, CaptainInput::default());
            sim.step(&mut FlatHost);
        }
        assert_eq!(sim.piki(id).frozen, 0.0);
        assert!(sim.piki(id).pos.distance(at) > 5.0, "didn't follow after thawing");
    }
}
