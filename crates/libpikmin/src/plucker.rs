//! Host extension, not Pikmin 1: the Onion plucks its own sprouts. When a sprout has stood
//! pluckable for [`Sim::set_auto_pluck`]'s time with nobody pulling it, and the landed Onion
//! holds a Pikmin, one slides down a leg (`ActExit`, aiExit.cpp), walks to each pluckable
//! sprout around the Onion in turn and pulls it with the captain's pluck (`NaviNukuState`,
//! naviState.cpp: "NUKU", the Pikmin popping out at its `KEY_Action0`). Pikmin and Olimar share
//! one skeleton and motion set, so the Pikmin plays Olimar's own motion. Each plucked Pikmin
//! joins the Onion's owner (Pikmin 1's plucked Pikmin join whoever pulled them). When nothing
//! is left to pull, the plucker walks back and climbs in again (`ActEnter`, aiEnter.cpp).
//!
//! It always gets there: a plucker that stops making progress toward a sprout (a slope too
//! steep, a wall, a ledge) is put down next to it and pulls it anyway, whatever the terrain
//! would let Olimar do. It cannot be hurt, crushed, swallowed or killed on its errand.

use glam::Vec3;

use crate::event::Event;
use crate::math::{ang_dist, dist_xz, round_ang, yaw_of};
use crate::onion::{OnionId, SproutId};
use crate::onion::OnionAnim;
use crate::onion_menu::{SLIDE_SPEED, flying_pose, leg_length, leg_point};
use crate::piki::{Happa, PikiState};
use crate::{Host, Sim};

/// How far from its Onion a plucker goes for sprouts (sim units, about 29 m in Risk of Rain 2;
/// seeds land within about 200).
pub const PLUCKER_RANGE: f32 = 600.0;
/// `NaviNukuAdjustState`: it pulls from this far short of the sprout.
const PULL_DISTANCE: f32 = 6.0;
/// A plucker that hasn't got closer to its sprout in this long (a wall, a cliff, a slope) is
/// put down beside it.
const STUCK_TIME: f32 = 3.0;
/// A plucker this far (sideways) from its Onion has lost it (the Onion flies with Olimar): it is
/// put down near its captain and comes back for the Onion from there.
const TELEPORT_DISTANCE: f32 = 900.0;
/// Under the hovering Onion this closely, the plucker jumps for a leg (`ActEnter`).
const RETURN_RADIUS: f32 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluckerJob {
    /// Sliding down leg `leg` (`ActExit`); `ratio` 1 at the top.
    Descend { leg: u8, ratio: f32 },
    /// Walking to `sprout`. `best` is the closest it has been, `stuck` how long since.
    Walk { sprout: SproutId, best: f32, stuck: f32 },
    /// Pulling `sprout` ("NUKU" at `frame`); `popped` once the Pikmin is out.
    Pull { sprout: SproutId, frame: f32, popped: bool },
    /// Dropped from the middle of a hovering Onion's leg, falling to the ground.
    Fall,
    /// Done (or the Onion flew off): heading for the spot under a hovering Onion to climb in.
    Return,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plucker {
    pub onion: OnionId,
    pub job: PluckerJob,
}

impl Sim {
    /// Turns on the Onion's plucker: a sprout left pluckable this many seconds with nobody
    /// pulling it sends a Pikmin out of its landed Onion to pluck every ready sprout around.
    /// `None` (the default) is off. Hosts shorten it for cooldown items as they go.
    pub fn set_auto_pluck(&mut self, seconds: Option<f32>) {
        self.auto_pluck_after = seconds.filter(|s| s.is_finite()).map(|s| s.max(0.0));
    }

    /// Sends a plucker out of each landed Onion that has a long-forgotten sprout, a Pikmin
    /// inside, and no plucker out already.
    pub(crate) fn update_auto_pluck(&mut self, _dt: f32) {
        let Some(after) = self.auto_pluck_after else { return };
        for oi in 0..self.onions.len() {
            let o = &self.onions[oi];
            if o.ship.is_some() || !o.grounded() || o.stored_total() == 0 {
                continue;
            }
            let (id, opos) = (o.id, o.pos);
            let busy = self
                .pikis
                .iter()
                .any(|p| matches!(p.state, PikiState::Plucker(pl) if pl.onion == id));
            if busy {
                continue;
            }
            let due = self.sprouts.iter().flatten().any(|s| {
                s.pluckable()
                    && s.claimed_by.is_none()
                    && s.unplucked >= after
                    && in_reach(opos, s.pos)
            });
            if due {
                self.send_plucker(oi);
            }
        }
    }

    /// Host button: ask this landed Onion's plucker to collect nearby ready sprouts
    /// now, without changing the automatic pluck delay for the other Onions.
    pub fn trigger_plucker(&mut self, onion: OnionId) -> bool {
        // A hovering Onion works too: the plucker drops from its leg and climbs back up.
        let Some(oi) = self.onions.iter().position(|o| o.id == onion && o.ship.is_none()
            && o.takes_deliveries() && o.stored_total() > 0) else { return false; };
        let at = self.onions[oi].pos;
        if self.pikis.iter().any(|p| matches!(p.state, PikiState::Plucker(pl) if pl.onion == onion))
            || !self.sprouts.iter().flatten().any(|s| s.pluckable() && s.claimed_by.is_none()
                && in_reach(at, s.pos)) { return false; }
        self.send_plucker(oi);
        true
    }

    fn send_plucker(&mut self, oi: usize) {
        let o = &self.onions[oi];
        let mirror = self.mirror_onion_legs;
        // The colour it holds most of, its best leaf stage.
        let Some(color) = crate::PikiColor::ALL
            .into_iter()
            .filter(|&c| o.stored_of(c) > 0)
            .max_by_key(|&c| o.stored_of(c))
        else {
            return;
        };
        let Some(happa) = [Happa::Flower, Happa::Bud, Happa::Leaf]
            .into_iter()
            .find(|&h| o.stored[color.index()][h as usize] > 0)
        else {
            return;
        };
        let (onion, opos, owner) = (o.id, o.pos, o.owner);
        let hover = flying_pose(o).then_some(o.hover_frame);
        let leg = (self.rng.f32() * 3.0) as u8 % 3;
        let Some(id) = self.spawn_piki(color, leg_point(opos, leg, 1.0, hover, mirror)) else {
            return;
        };
        self.onions[oi].stored[color.index()][happa as usize] -= 1;
        let p = &mut self.pikis[id.0 as usize];
        p.happa = happa;
        p.captain = owner;
        p.state = PikiState::Plucker(Plucker {
            onion,
            job: PluckerJob::Descend { leg, ratio: 1.0 },
        });
        self.events.push(Event::PluckerOut { piki: id, onion });
    }

    /// The nearest pluckable sprout around the plucker's Onion it hasn't given up on.
    fn plucker_target(&self, pi: usize, pl: &Plucker) -> Option<SproutId> {
        let opos = self.onions[pl.onion.0 as usize].pos;
        let pos = self.pikis[pi].pos;
        self.sprouts
            .iter()
            .flatten()
            .filter(|s| s.pluckable() && s.claimed_by.is_none() && in_reach(opos, s.pos))
            .min_by(|a, b| dist_xz(a.pos, pos).total_cmp(&dist_xz(b.pos, pos)))
            .map(|s| s.id)
    }

    /// Picks its next job: another sprout, or home.
    fn plucker_next(&mut self, pi: usize, pl: Plucker) {
        match self.plucker_target(pi, &pl) {
            Some(sprout) => {
                self.pikis[pi].state = PikiState::Plucker(Plucker {
                    job: PluckerJob::Walk { sprout, best: f32::MAX, stuck: 0.0 },
                    ..pl
                });
            }
            None => self.plucker_home(pi, pl.onion),
        }
    }

    /// Back into its Onion up the nearest leg; for a hovering Onion, first to the spot under it.
    /// If the Onion has left, off to its captain.
    fn plucker_home(&mut self, pi: usize, onion: OnionId) {
        let mirror = self.mirror_onion_legs;
        let o = &self.onions[onion.0 as usize];
        let p = &mut self.pikis[pi];
        p.target_vel = Vec3::ZERO;
        if o.grounded() {
            let leg = nearest_leg(o.pos, p.pos, None, mirror);
            p.state = PikiState::Enter { onion, leg, climbing: false, ratio: 0.0 };
            return;
        }
        if !onion_gone(o) {
            p.state = PikiState::Plucker(Plucker { onion, job: PluckerJob::Return });
            return;
        }
        let id = p.id;
        match p.captain.filter(|c| !self.captains[c.0 as usize].down) {
            Some(c) => self.join_squad(id, c),
            None => p.state = PikiState::Idle,
        }
    }

    /// Put down beside its captain (or the Onion if nobody is up), on the ground, to come back
    /// for a hovering Onion that flew far off.
    fn plucker_teleport(&mut self, pi: usize, onion: OnionId, host: &mut dyn Host) {
        let o = &self.onions[onion.0 as usize];
        let base = self.pikis[pi]
            .captain
            .filter(|c| !self.captains[c.0 as usize].down)
            .map(|c| self.captains[c.0 as usize].pos)
            .unwrap_or(o.pos);
        let angle = self.rng.f32() * std::f32::consts::TAU;
        let mut at = base + Vec3::new(angle.sin(), 0.0, angle.cos()) * 60.0;
        at.y = host.ground_height(at + Vec3::Y * 400.0).unwrap_or(base.y);
        let p = &mut self.pikis[pi];
        p.pos = at;
        p.vel = Vec3::ZERO;
        p.target_vel = Vec3::ZERO;
        p.on_ground = true;
    }

    pub(crate) fn piki_plucker(&mut self, pi: usize, pl: Plucker, host: &mut dyn Host, dt: f32) {
        let mirror = self.mirror_onion_legs;
        // The Onion left the stage (or went dormant) under it: it gives up and goes to its captain.
        // A hovering Onion is fine: the plucker works from the ground and climbs back up.
        if onion_gone(&self.onions[pl.onion.0 as usize]) && !matches!(pl.job, PluckerJob::Descend { .. }) {
            return self.plucker_home(pi, pl.onion);
        }
        // Lost the Onion (it flew off with Olimar): near the captain, head back for it.
        if matches!(pl.job, PluckerJob::Walk { .. } | PluckerJob::Return)
            && dist_xz(self.pikis[pi].pos, self.onions[pl.onion.0 as usize].pos) > TELEPORT_DISTANCE
        {
            self.plucker_teleport(pi, pl.onion, host);
            self.pikis[pi].state = PikiState::Plucker(Plucker { job: PluckerJob::Return, ..pl });
            return;
        }
        match pl.job {
            PluckerJob::Descend { leg, ratio } => {
                let o = &self.onions[pl.onion.0 as usize];
                let opos = o.pos;
                let hover = flying_pose(o).then_some(o.hover_frame);
                let r = ratio - SLIDE_SPEED * dt / leg_length(leg, hover);
                let p = &mut self.pikis[pi];
                if hover.is_some() && r <= 0.5 {
                    // As a Pikmin called out of a hovering Onion: half the slide, then it drops.
                    p.pos = leg_point(opos, leg, 0.5, hover, mirror);
                    p.vel = Vec3::NEG_Y * SLIDE_SPEED;
                    p.on_ground = false;
                    p.state = PikiState::Plucker(Plucker { job: PluckerJob::Fall, ..pl });
                    return;
                }
                p.pos = leg_point(opos, leg, r, hover, mirror);
                p.vel = Vec3::ZERO;
                let d = p.pos - leg_point(opos, leg, 1.0, hover, mirror);
                p.face = d.x.atan2(d.z);
                if r > 0.0 {
                    p.state = PikiState::Plucker(Plucker { job: PluckerJob::Descend { leg, ratio: r }, ..pl });
                    return;
                }
                p.on_ground = true;
                self.plucker_next(pi, pl);
            }
            PluckerJob::Fall => {
                let p = &mut self.pikis[pi];
                p.target_vel = p.vel;
                self.physics(pi, host, dt);
                if self.pikis[pi].on_ground {
                    self.pikis[pi].vel = Vec3::ZERO;
                    self.plucker_next(pi, pl);
                }
            }
            PluckerJob::Return => {
                let o = &self.onions[pl.onion.0 as usize];
                if o.grounded() {
                    return self.plucker_home(pi, pl.onion);
                }
                let (opos, hover) = (o.pos, flying_pose(o).then_some(o.hover_frame));
                let ready = o.takes_deliveries();
                let to = (opos - self.pikis[pi].pos).with_y(0.0);
                if ready && to.length() <= RETURN_RADIUS {
                    // `ActEnter` takes over: walk to the leg's foot, jump, climb.
                    let leg = nearest_leg(opos, self.pikis[pi].pos, hover, mirror);
                    let p = &mut self.pikis[pi];
                    p.target_vel = Vec3::ZERO;
                    p.state = PikiState::Enter { onion: pl.onion, leg, climbing: false, ratio: 0.0 };
                    return;
                }
                // Landing or taking off: wait where it stands.
                let speed = self.piki_speed(pi, 1.0);
                let dir = to.normalize_or_zero();
                let p = &mut self.pikis[pi];
                p.target_vel = if ready { dir * speed } else { Vec3::ZERO };
                if ready {
                    p.face = round_ang(p.face + 0.3 * ang_dist(yaw_of(dir), p.face));
                }
                self.physics(pi, host, dt);
            }
            PluckerJob::Walk { sprout, best, stuck } => {
                let Some(s) = self.sprouts.get(sprout.0 as usize).and_then(Option::as_ref)
                    .filter(|s| s.pluckable() && s.claimed_by.is_none())
                else {
                    // Somebody else pulled it.
                    return self.plucker_next(pi, pl);
                };
                let spos = s.pos;
                let speed = self.piki_speed(pi, 1.0);
                let p = &mut self.pikis[pi];
                let to = (spos - p.pos).with_y(0.0);
                let dist = to.length();
                let dir = to.normalize_or_zero();
                if dist <= PULL_DISTANCE + 2.0 {
                    p.target_vel = Vec3::ZERO;
                    p.vel = Vec3::ZERO;
                    p.face = yaw_of(dir);
                    p.state = PikiState::Plucker(Plucker {
                        job: PluckerJob::Pull { sprout, frame: 0.0, popped: false },
                        ..pl
                    });
                    self.begin_plucked(sprout, self.pikis[pi].captain, self.pikis[pi].face, false);
                    return;
                }
                // Slow into the pulling spot instead of overshooting it.
                let room = dist - PULL_DISTANCE;
                p.target_vel = dir * speed.min(room / dt.max(1e-3));
                p.face = round_ang(p.face + 0.3 * ang_dist(yaw_of(dir), p.face));
                let (best, stuck) = if dist < best - 2.0 { (dist, 0.0) } else { (best, stuck + dt) };
                if stuck >= STUCK_TIME {
                    // Can't walk there: put it down beside the sprout, on the Onion's side,
                    // and let it pull.
                    let onion = self.onions[pl.onion.0 as usize].pos;
                    let back = (onion - spos).with_y(0.0).normalize_or(Vec3::X);
                    let mut at = spos + back * PULL_DISTANCE;
                    at.y = host.ground_height(at + Vec3::Y * 40.0).unwrap_or(spos.y);
                    let p = &mut self.pikis[pi];
                    p.pos = at;
                    p.vel = Vec3::ZERO;
                    p.target_vel = Vec3::ZERO;
                    p.on_ground = true;
                    p.face = yaw_of(-back);
                    p.state = PikiState::Plucker(Plucker {
                        job: PluckerJob::Pull { sprout, frame: 0.0, popped: false },
                        ..pl
                    });
                    self.begin_plucked(sprout, self.pikis[pi].captain, self.pikis[pi].face, false);
                    return;
                }
                p.state = PikiState::Plucker(Plucker { job: PluckerJob::Walk { sprout, best, stuck }, ..pl });
                self.physics(pi, host, dt);
            }
            PluckerJob::Pull { sprout, frame, popped } => {
                let anim = self.params.anim.nuku;
                let f = frame + self.params.anim.fps * dt * self.attack_rate_of(pi);
                let mut popped = popped;
                self.pikis[pi].target_vel = Vec3::ZERO;
                self.pikis[pi].vel = Vec3::ZERO;
                if !popped && f >= anim.action0 {
                    popped = true;
                    let plucker = self.pikis[pi].id;
                    let captain = self.pikis[pi].captain.filter(|c| !self.captains[c.0 as usize].down);
                    if let Some(plucked) = self.plucked_from(sprout) {
                        self.events.push(Event::PluckerPulled { plucker, plucked, captain });
                    }
                }
                if f >= anim.end {
                    return self.plucker_next(pi, pl);
                }
                self.pikis[pi].state = PikiState::Plucker(Plucker {
                    job: PluckerJob::Pull { sprout, frame: f, popped },
                    ..pl
                });
            }
        }
    }
}

/// The Onion is off the stage or dormant: a plucker out for it has nowhere to climb back to.
fn onion_gone(o: &crate::onion::Onion) -> bool {
    o.ship.is_some() || o.dormancy.is_some() || o.anim == OnionAnim::Away
}

/// The leg whose foot (or, hovering, grab point) is nearest `from`.
fn nearest_leg(onion: Vec3, from: Vec3, hover: Option<f32>, mirror: bool) -> u8 {
    let ratio = if hover.is_some() { 0.5 } else { 0.0 };
    (0..3u8)
        .min_by(|&a, &b| {
            dist_xz(leg_point(onion, a, ratio, hover, mirror), from)
                .total_cmp(&dist_xz(leg_point(onion, b, ratio, hover, mirror), from))
        })
        .unwrap_or(0)
}

/// Near enough, at any height: the plucker is put down beside a sprout it can't walk to.
fn in_reach(onion: Vec3, at: Vec3) -> bool {
    dist_xz(onion, at) <= PLUCKER_RANGE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FlatHost;
    use crate::{Params, PikiColor, Sim};

    fn run(sim: &mut Sim, host: &mut FlatHost, seconds: f32) {
        for _ in 0..(seconds * crate::TICK_HZ as f32) as u32 {
            sim.step(host);
        }
    }

    #[test]
    fn button_sends_one_plucker_without_waiting_for_auto_delay() {
        let mut sim = Sim::new(Params::default(), 7);
        let mut host = FlatHost;
        let captain = sim.add_captain(Vec3::new(2000.0, 0.0, 0.0));
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_onion_owner(onion, Some(captain));
        sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 2);
        sim.plant_sprout(PikiColor::Blue, Vec3::new(150.0, 0.0, 0.0));
        run(&mut sim, &mut host, 3.0);
        assert!(sim.trigger_plucker(onion));
        assert!(!sim.trigger_plucker(onion), "one plucker per Onion");
        assert_eq!(sim.onions()[0].stored_total(), 1);
    }

    fn hovering_sim(captain_at: Vec3) -> (Sim, OnionId) {
        let mut sim = Sim::new(Params::default(), 7);
        let captain = sim.add_captain(captain_at);
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_onion_owner(onion, Some(captain));
        sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 1);
        sim.plant_sprout(PikiColor::Blue, Vec3::new(150.0, 0.0, 0.0));
        sim.plant_sprout(PikiColor::Yellow, Vec3::new(-150.0, 0.0, 60.0));
        sim.set_onion_pose(onion, Vec3::new(0.0, 300.0, 0.0), true);
        (sim, onion)
    }

    #[test]
    fn a_hovering_onions_plucker_drops_pulls_everything_and_climbs_back_up() {
        let (mut sim, onion) = hovering_sim(Vec3::new(200.0, 0.0, 0.0));
        let mut host = FlatHost;
        run(&mut sim, &mut host, 1.0);
        assert!(sim.trigger_plucker(onion), "a flying Onion can send its plucker");
        let mut pulled = 0;
        for _ in 0..(60 * crate::TICK_HZ) {
            sim.step(&mut host);
            pulled += sim.drain_events().filter(|e| matches!(e, Event::PluckerPulled { .. })).count();
        }
        assert_eq!(pulled, 2);
        assert_eq!(sim.sprouts().count(), 0);
        assert_eq!(sim.onions()[0].stored_total(), 1, "the plucker climbed back up into the hovering Onion");
        assert!(!sim.pikis().iter().any(|p| matches!(p.state, PikiState::Plucker(_))));
    }

    #[test]
    fn a_plucker_left_far_behind_is_put_down_by_its_captain_and_still_gets_home() {
        let far = Vec3::new(20000.0, 0.0, 0.0); // too far to walk back in the test time
        let (mut sim, onion) = hovering_sim(far);
        let mut host = FlatHost;
        run(&mut sim, &mut host, 1.0);
        assert!(sim.trigger_plucker(onion));
        run(&mut sim, &mut host, 2.0);
        // The Onion flies off with Olimar while the plucker is out.
        sim.set_onion_pose(onion, far + Vec3::Y * 300.0, true);
        run(&mut sim, &mut host, 90.0);
        assert_eq!(sim.onions()[0].stored_total(), 1, "back inside after being put down near the captain");
        assert!(!sim.pikis().iter().any(|p| matches!(p.state, PikiState::Plucker(_))));
    }

    #[test]
    fn a_forgotten_sprout_is_plucked_by_the_onion_and_the_plucker_goes_home() {
        let mut sim = Sim::new(Params::default(), 7);
        let mut host = FlatHost;
        let captain = sim.add_captain(Vec3::new(2000.0, 0.0, 0.0));
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_onion_owner(onion, Some(captain));
        // Far off, they lose the squad and come back to it (as in Risk of Rain 2).
        sim.set_auto_rejoin(true);
        sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 1);
        sim.set_auto_pluck(Some(2.0));
        sim.plant_sprout(PikiColor::Blue, Vec3::new(150.0, 0.0, 0.0));
        sim.plant_sprout(PikiColor::Yellow, Vec3::new(-150.0, 0.0, 60.0));

        run(&mut sim, &mut host, 1.5);
        assert!(!sim.pikis().iter().any(|p| matches!(p.state, PikiState::Plucker(_))), "too early");
        run(&mut sim, &mut host, 1.0);
        assert_eq!(sim.onions()[0].stored_total(), 0, "the plucker left the Onion");

        let mut pulled = 0;
        for _ in 0..(40 * crate::TICK_HZ) {
            sim.step(&mut host);
            pulled += sim
                .drain_events()
                .filter(|e| matches!(e, Event::PluckerPulled { captain: Some(c), .. } if *c == captain))
                .count();
        }
        assert_eq!(pulled, 2);
        assert_eq!(sim.sprouts().count(), 0);
        assert_eq!(sim.onions()[0].stored_total(), 1, "the plucker climbed back in");
        let squad = sim.pikis().iter().filter(|p| p.is_alive() && p.state.in_squad()).count();
        assert_eq!(squad, 2, "plucked Pikmin joined the owner");
    }

    /// A wall across the way: nothing walks from x < 80 to x > 80.
    struct Wall;

    impl Host for Wall {
        fn ground_height(&self, _pos: Vec3) -> Option<f32> {
            Some(0.0)
        }

        fn walls(&self, centre: Vec3, radius: f32) -> Vec3 {
            let mut c = centre;
            if c.x + radius > 80.0 && c.x < 90.0 {
                c.x = 80.0 - radius;
            }
            c
        }
    }

    #[test]
    fn a_plucker_blocked_by_the_terrain_is_put_down_at_the_sprout_and_still_plucks() {
        let mut sim = Sim::new(Params::default(), 7);
        let mut host = Wall;
        let captain = sim.add_captain(Vec3::new(-3000.0, 0.0, 0.0));
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_onion_owner(onion, Some(captain));
        sim.set_auto_rejoin(true);
        sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 1);
        sim.set_auto_pluck(Some(1.0));
        sim.plant_sprout(PikiColor::Blue, Vec3::new(250.0, 0.0, 0.0));
        let mut pulled = 0;
        for _ in 0..(40 * crate::TICK_HZ) {
            sim.step(&mut host);
            pulled += sim.drain_events().filter(|e| matches!(e, Event::PluckerPulled { .. })).count();
        }
        assert_eq!(pulled, 1, "the wall must not stop it");
        assert_eq!(sim.sprouts().count(), 0);
    }

    #[test]
    fn a_plucker_cannot_be_hurt_on_its_errand() {
        let mut sim = Sim::new(Params::default(), 7);
        let mut host = FlatHost;
        let captain = sim.add_captain(Vec3::new(3000.0, 0.0, 0.0));
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_onion_owner(onion, Some(captain));
        sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 1);
        sim.set_auto_pluck(Some(0.5));
        sim.plant_sprout(PikiColor::Blue, Vec3::new(200.0, 0.0, 0.0));
        run(&mut sim, &mut host, 4.0);
        let plucker = sim
            .pikis()
            .iter()
            .find(|p| matches!(p.state, PikiState::Plucker(_)))
            .expect("a plucker is out")
            .id;
        let health = sim.piki(plucker).health;
        let at = sim.piki(plucker).pos;
        assert!(!sim.attack_piki(plucker, 1000.0));
        assert_eq!(sim.attack_pikis_in_sphere(at, 500.0, 1000.0), 0);
        assert_eq!(sim.crush_pikis_in_sphere(at, 500.0, 1000.0), 0);
        assert_eq!(sim.flick_pikis_in_sphere(at, 500.0, 1000.0, at + Vec3::X), 0);
        sim.kill_piki(plucker);
        assert!(matches!(sim.piki(plucker).state, PikiState::Plucker(_)));
        assert_eq!(sim.piki(plucker).health, health);
    }

    #[test]
    fn no_plucker_without_pikmin_inside() {
        let mut sim = Sim::new(Params::default(), 7);
        let mut host = FlatHost;
        let captain = sim.add_captain(Vec3::new(2000.0, 0.0, 0.0));
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_onion_owner(onion, Some(captain));
        sim.set_auto_pluck(Some(0.5));
        sim.plant_sprout(PikiColor::Blue, Vec3::new(150.0, 0.0, 0.0));
        run(&mut sim, &mut host, 5.0);
        assert_eq!(sim.sprouts().count(), 1);
        assert!(sim.pikis().is_empty());
    }
}

