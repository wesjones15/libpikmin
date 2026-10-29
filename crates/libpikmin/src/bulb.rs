//! Candypop Buds: Pikmin 2's `Game::Pom` (Pom.cpp, PomState.cpp, the `Pom` parms in Pom.h).
//!
//! A bud stands open. A Pikmin that comes down on its mouth while it is open is swallowed (`Obj::pressCallBack`,
//! `InteractSwallow`) and takes one of its slots (`mNormalMaxSlots`, 5; the rainbow Candypop Pop has 1). Once a Pikmin
//! has gone in the bud waits `mRemainOpenTime` (30 s) after the last one, or shuts the moment its slots are full
//! (`StateSwing::exec`), then spits one sprout of its colour for every Pikmin it swallowed (`Obj::shotPikmin`: a leaf
//! seed thrown up at (110, 750, 110) and taking root), times `mQueenShotMultiplier` (5) for the Pop. A Pikmin of the bud's
//! own colour gives its slot back (the decomp only counts the others against it), so a bud that was only fed its own
//! colour opens again; one whose slots are all gone withers (`POM_Dead`). The Pop turns Blue, Red, Yellow every
//! `mColorChangeTime` (1.25 s) while it waits (`Obj::changePomColor`).
//!
//! Host adaptations: Pikmin 2's bud swallows a Pikmin that presses its `slot` part; here a thrown Pikmin that comes down
//! within [`RADIUS`] of the bud's foot and under [`HEIGHT`] is swallowed. The phases last as long as the disc's clips
//! ([`OPEN_TIME`], [`CLOSE_TIME`], [`SHOT_TIME`], [`WITHER_TIME`]); the seeds fly [`SHOT_AT`] seconds into the shot.
//! Hosts decide where buds stand ([`Sim::add_bulb`]) and may give them many more slots than Pikmin 2's
//! ([`Sim::set_bulb_slots`]), so a bud spits each batch of [`SLOTS`] (the Pop: [`QUEEN_SLOTS`]) as soon as it has
//! swallowed it, and keeps doing so until its slots are gone. What it spits are live Pikmin, thrown up as the seeds
//! are, that land beside it in the squad of the captain who threw the last one in; the decomp's seeds take root
//! instead. Only when the field already holds the most Pikmin it can does a bud plant seeds.

use glam::Vec3;

use crate::CaptainId;
use crate::event::Event;
use crate::piki::{Happa, PikiColor, PikiState};
use crate::sim::Sim;

/// `mNormalMaxSlots` (ip01).
pub const SLOTS: u32 = 5;
/// `mQueenMaxSlots` (ip11) and `mQueenShotMultiplier` (ip13).
pub const QUEEN_SLOTS: u32 = 1;
pub const QUEEN_MULTIPLIER: u32 = 5;
/// The most slots a host may give a bud.
pub const MAX_SLOTS: u32 = 200;
/// A lock-on "thing" id at or above this is a bud ([`BulbId`] plus this), below it a pellet: one id space for hosts that
/// pass what a captain is locked onto as a single number ([`crate::Sim::set_lock_bulb`]).
pub const LOCK_BASE: u32 = 0x4000_0000;
/// `mRemainOpenTime` (fp01): seconds it stays open after a Pikmin went in.
pub const REMAIN_OPEN: f32 = 30.0;
/// `mColorChangeTime` (fp02).
pub const COLOUR_CHANGE: f32 = 1.25;
/// How far from the bud's foot (across the ground) a thrown Pikmin must come down to go in, and how high up the mouth is.
pub const RADIUS: f32 = 18.0;
pub const HEIGHT: f32 = 46.0;
/// Seconds of the open, close, shot and wither animations, and when in the shot the seeds fly: the disc's clips
/// (`enemy/data/Pom/anim`, 30 frames a second, `enemyAnimMgr.txt`): `type1` opens and its key 2 at frame 25 lets it swallow,
/// `type2` closes (30 frames), `type3` spits (40 frames, key 2 at frame 20 throws the seeds), `dead` withers (40 frames).
pub const OPEN_TIME: f32 = 25.0 / 30.0;
pub const CLOSE_TIME: f32 = 1.0;
pub const SHOT_TIME: f32 = 40.0 / 30.0;
pub const SHOT_AT: f32 = 20.0 / 30.0;
pub const WITHER_TIME: f32 = 40.0 / 30.0;
/// `Obj::shotPikmin`: where the seeds leave (above the bud) and how they are thrown (110 sideways, 750 up).
const SHOT_HEIGHT: f32 = 50.0;
const SHOT_SIDE: f32 = 110.0;
const SHOT_UP: f32 = 750.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BulbId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BulbState {
    /// `POM_Open`: the petals unfold, then it takes whatever comes down on it.
    Open { t: f32 },
    /// `POM_Swing`: a Pikmin went in; `t` seconds since the last.
    Swing { t: f32 },
    /// `POM_Close`.
    Close { t: f32 },
    /// `POM_Shot`: `fired` once the seeds are out.
    Shot { t: f32, fired: bool },
    /// `POM_Dead`: withering; gone when it ends.
    Dead { t: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bulb {
    pub id: BulbId,
    pub pos: Vec3,
    /// The colour of the seeds it spits now (the Pop's changes).
    pub colour: PikiColor,
    /// The rainbow Candypop Pop (`EnemyID_RandPom`): Blue, Red and Yellow in turn, one slot, five seeds.
    pub queen: bool,
    /// How many Pikmin it can take before it withers (`mTotalSlotCount`; see [`Sim::set_bulb_slots`]).
    pub slots: u32,
    /// Slots taken (`mUsedSlotCount`) and Pikmin in it now to be spat (`mStuckPikminCount`).
    pub used: u32,
    pub swallowed: u32,
    /// Of those, how many were its own colour (their slots come back when it spits).
    pub same: u32,
    /// The Pop's colour change timer.
    pub change: f32,
    pub state: BulbState,
    /// The captain whose Pikmin went in last: the Pikmin it spits are his.
    pub owner: Option<CaptainId>,
}

impl Bulb {
    pub fn slots(&self) -> u32 {
        self.slots
    }

    /// How many swallowed Pikmin make it close and spit: Pikmin 2's own slot count ([`SLOTS`], the Pop's [`QUEEN_SLOTS`]).
    pub fn batch(&self) -> u32 {
        if self.queen { QUEEN_SLOTS } else { SLOTS }
    }

    /// `mCanSwallowPiki`: open and unfolded, with a slot free and its batch not yet full.
    pub fn can_swallow(&self) -> bool {
        self.used < self.slots()
            && self.swallowed < self.batch()
            && match self.state {
                BulbState::Open { t } => t >= OPEN_TIME,
                BulbState::Swing { .. } => true,
                _ => false,
            }
    }

    /// Whether it still stands (a withering bud does not).
    pub fn alive(&self) -> bool {
        !matches!(self.state, BulbState::Dead { .. })
    }
}

impl Sim {
    /// Plants a Candypop Bud of `colour` at `pos` (its foot). `queen` makes it the rainbow Pop, which starts on `colour`
    /// if that is one of Blue, Red and Yellow.
    pub fn add_bulb(&mut self, colour: PikiColor, queen: bool, pos: Vec3) -> BulbId {
        self.next_bulb += 1;
        let id = BulbId(self.next_bulb);
        let colour = if queen && !PikiColor::ALL.contains(&colour) { PikiColor::Red } else { colour };
        self.bulbs.push(Bulb {
            id,
            pos,
            colour,
            queen,
            slots: if queen { self.bulb_slots.1 } else { self.bulb_slots.0 },
            used: 0,
            swallowed: 0,
            same: 0,
            change: 0.0,
            state: BulbState::Open { t: 0.0 },
            owner: None,
        });
        id
    }

    /// How many Pikmin a plain bud and the rainbow Pop take before they wither (Pikmin 2: [`SLOTS`] 5 and [`QUEEN_SLOTS`] 1),
    /// for the buds there are and the ones added later. A host with bigger squads than Pikmin 2's gives them more room;
    /// a bud still spits after each [`Bulb::batch`], one Pikmin for each that went in (five from the Pop).
    /// Clamped to 1..=[`MAX_SLOTS`].
    pub fn set_bulb_slots(&mut self, normal: u32, queen: u32) {
        self.bulb_slots = (normal.clamp(1, MAX_SLOTS), queen.clamp(1, MAX_SLOTS));
        for b in &mut self.bulbs {
            b.slots = if b.queen { self.bulb_slots.1 } else { self.bulb_slots.0 };
        }
    }

    pub fn bulbs(&self) -> &[Bulb] {
        &self.bulbs
    }

    /// Takes a bud away.
    pub fn remove_bulb(&mut self, id: BulbId) {
        self.bulbs.retain(|b| b.id != id);
    }

    /// A thrown Pikmin that has just come down: the bud it went into, if it did. `Obj::pressCallBack`.
    pub(crate) fn bulb_swallow(&mut self, pi: usize) -> bool {
        let (pos, color, id, captain) = (self.pikis[pi].pos, self.pikis[pi].color, self.pikis[pi].id, self.pikis[pi].captain);
        let Some(bi) = self.bulbs.iter().position(|b| {
            b.can_swallow()
                && (b.pos - pos).truncate_y_len() <= RADIUS + self.pikis[pi].size
                && pos.y >= b.pos.y - 6.0
                && pos.y <= b.pos.y + HEIGHT
        }) else {
            return false;
        };
        self.leave_current(id);
        self.pikis[pi].state = PikiState::Dead;
        let b = &mut self.bulbs[bi];
        b.used += 1;
        b.swallowed += 1;
        if color == b.colour {
            b.same += 1;
        }
        if captain.is_some() {
            b.owner = captain;
        }
        b.state = BulbState::Swing { t: 0.0 };
        let (bulb, at) = (b.id, b.pos);
        self.events.push(Event::BulbSwallowed { piki: id, bulb, colour: color, pos: at });
        true
    }

    pub(crate) fn update_bulbs(&mut self, dt: f32) {
        let mut spat: Vec<(Vec3, PikiColor, u32, BulbId, Option<CaptainId>)> = Vec::new();
        for b in &mut self.bulbs {
            // The Pop turns while it waits (`changePomColor` in Wait, Open and Swing).
            if b.queen && matches!(b.state, BulbState::Open { .. } | BulbState::Swing { .. }) {
                b.change += dt;
                if b.change > COLOUR_CHANGE {
                    b.change = 0.0;
                    b.colour = match b.colour {
                        PikiColor::Blue => PikiColor::Red,
                        PikiColor::Red => PikiColor::Yellow,
                        _ => PikiColor::Blue,
                    };
                }
            }
            b.state = match b.state {
                BulbState::Open { t } => BulbState::Open { t: t + dt },
                BulbState::Swing { t } => {
                    if t + dt > REMAIN_OPEN || b.used >= b.slots() || b.swallowed >= b.batch() {
                        BulbState::Close { t: 0.0 }
                    } else {
                        BulbState::Swing { t: t + dt }
                    }
                }
                BulbState::Close { t } => {
                    if t + dt >= CLOSE_TIME {
                        if b.swallowed > 0 { BulbState::Shot { t: 0.0, fired: false } } else { BulbState::Open { t: 0.0 } }
                    } else {
                        BulbState::Close { t: t + dt }
                    }
                }
                BulbState::Shot { t, fired } => {
                    let t = t + dt;
                    let mut fired = fired;
                    if !fired && t >= SHOT_AT {
                        fired = true;
                        let count = b.swallowed * if b.queen { QUEEN_MULTIPLIER } else { 1 };
                        spat.push((b.pos + Vec3::Y * SHOT_HEIGHT, b.colour, count, b.id, b.owner));
                        // Its own colour gives the slot back; the Pop never does.
                        if !b.queen {
                            b.used = b.used.saturating_sub(b.same);
                        }
                        b.swallowed = 0;
                        b.same = 0;
                    }
                    if t >= SHOT_TIME {
                        if b.used < b.slots() { BulbState::Open { t: 0.0 } } else { BulbState::Dead { t: 0.0 } }
                    } else {
                        BulbState::Shot { t, fired }
                    }
                }
                BulbState::Dead { t } => BulbState::Dead { t: t + dt },
            };
        }
        for (at, colour, count, bulb, owner) in spat {
            // A captain who has left takes no Pikmin; they land as nobody's.
            let owner = owner.filter(|c| (c.0 as usize) < self.captains.len());
            for _ in 0..count {
                let angle = self.rng.f32() * std::f32::consts::TAU;
                let vel = Vec3::new(SHOT_SIDE * angle.cos(), SHOT_UP, SHOT_SIDE * angle.sin());
                // Thrown up as `Obj::shotPikmin` throws its seeds, and tumbling down as a thrown Pikmin does.
                let Some(id) = self.spawn_piki(colour, at) else {
                    self.launch_sprout(colour, Happa::Leaf, at, vel);
                    continue;
                };
                let p = &mut self.pikis[id.0 as usize];
                p.captain = owner;
                p.vel = vel;
                p.target_vel = vel;
                p.on_ground = false;
                p.state = PikiState::Flying { ground_frames: 0 };
            }
            self.events.push(Event::BulbShot { bulb, colour, count, pos: at });
        }
        self.bulbs.retain(|b| !matches!(b.state, BulbState::Dead { t } if t >= WITHER_TIME));
    }
}

/// Length across the ground.
trait FlatLen {
    fn truncate_y_len(self) -> f32;
}

impl FlatLen for Vec3 {
    fn truncate_y_len(self) -> f32 {
        (self.x * self.x + self.z * self.z).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn throw_at(sim: &mut Sim, color: PikiColor, at: Vec3) -> crate::PikiId {
        let id = sim.spawn_piki(color, at).unwrap();
        sim.pikis[id.0 as usize].state = PikiState::Flying { ground_frames: 0 };
        id
    }

    #[test]
    fn a_closed_bud_takes_nothing_and_an_open_one_takes_a_pikmin_into_a_slot() {
        let mut sim = Sim::default();
        let bulb = sim.add_bulb(PikiColor::Purple, false, Vec3::ZERO);
        let a = throw_at(&mut sim, PikiColor::Red, Vec3::new(2.0, 20.0, 0.0));
        assert!(!sim.bulb_swallow(a.0 as usize), "the petals are still unfolding");
        for _ in 0..30 {
            sim.update_bulbs(1.0 / 30.0);
        }
        assert!(sim.bulb_swallow(a.0 as usize));
        let b = &sim.bulbs()[0];
        assert_eq!((b.id, b.used, b.swallowed, b.same), (bulb, 1, 1, 0));
        assert!(matches!(sim.pikis[a.0 as usize].state, PikiState::Dead));
        assert!(matches!(b.state, BulbState::Swing { .. }));
        let far = throw_at(&mut sim, PikiColor::Red, Vec3::new(60.0, 20.0, 0.0));
        assert!(!sim.bulb_swallow(far.0 as usize), "it must come down on the bud");
    }

    fn alive(sim: &Sim, color: PikiColor) -> usize {
        sim.pikis.iter().filter(|p| p.is_alive() && p.color == color).count()
    }

    #[test]
    fn it_spits_one_pikmin_of_its_colour_for_each_after_thirty_seconds() {
        let mut sim = Sim::default();
        sim.add_bulb(PikiColor::Purple, false, Vec3::ZERO);
        for _ in 0..30 {
            sim.update_bulbs(1.0 / 30.0);
        }
        for color in [PikiColor::Red, PikiColor::Blue] {
            let p = throw_at(&mut sim, color, Vec3::new(0.0, 20.0, 0.0));
            assert!(sim.bulb_swallow(p.0 as usize));
        }
        sim.drain_events().for_each(drop);
        for _ in 0..(34 * 30) {
            sim.update_bulbs(1.0 / 30.0);
        }
        let shot: Vec<_> = sim.drain_events().filter_map(|e| if let Event::BulbShot { colour, count, .. } = e { Some((colour, count)) } else { None }).collect();
        assert_eq!(shot, [(PikiColor::Purple, 2)]);
        assert_eq!(alive(&sim, PikiColor::Purple), 2, "live Pikmin");
        assert_eq!(sim.sprouts().count(), 0, "nothing planted");
        assert!(sim.pikis.iter().filter(|p| p.is_alive()).all(|p| matches!(p.state, PikiState::Flying { .. })), "thrown up");
        assert!(matches!(sim.bulbs()[0].state, BulbState::Open { .. }), "two of its five slots are used, so it opens again");
        assert_eq!(sim.bulbs()[0].used, 2);
    }

    #[test]
    fn its_own_colour_gives_the_slot_back_and_a_full_bud_withers() {
        let mut sim = Sim::default();
        sim.add_bulb(PikiColor::Red, false, Vec3::ZERO);
        for _ in 0..30 {
            sim.update_bulbs(1.0 / 30.0);
        }
        for _ in 0..2 {
            let p = throw_at(&mut sim, PikiColor::Red, Vec3::new(0.0, 20.0, 0.0));
            assert!(sim.bulb_swallow(p.0 as usize));
        }
        for _ in 0..(34 * 30) {
            sim.update_bulbs(1.0 / 30.0);
        }
        assert_eq!(sim.bulbs()[0].used, 0, "red into a red bud costs nothing");
        for _ in 0..SLOTS {
            let p = throw_at(&mut sim, PikiColor::Blue, Vec3::new(0.0, 20.0, 0.0));
            assert!(sim.bulb_swallow(p.0 as usize));
        }
        assert!(!sim.bulbs()[0].can_swallow(), "full");
        for _ in 0..(3 * 30) {
            sim.update_bulbs(1.0 / 30.0);
        }
        assert!(matches!(sim.bulbs()[0].state, BulbState::Dead { .. }));
        for _ in 0..(3 * 30) {
            sim.update_bulbs(1.0 / 30.0);
        }
        assert!(sim.bulbs().is_empty());
        assert_eq!(alive(&sim, PikiColor::Red), 2 + SLOTS as usize, "a Pikmin for each that went in");
    }

    #[test]
    fn a_big_bud_spits_every_five_until_its_slots_run_out() {
        let mut sim = Sim::default();
        sim.set_bulb_slots(25, 5);
        let olimar = sim.add_captain(Vec3::new(300.0, 0.0, 0.0));
        sim.add_bulb(PikiColor::White, false, Vec3::ZERO);
        let step = |sim: &mut Sim, secs: f32| {
            for _ in 0..(secs * 30.0) as usize {
                sim.update_bulbs(1.0 / 30.0);
            }
        };
        step(&mut sim, 1.0);
        for round in 1..=5 {
            for i in 0..SLOTS {
                let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(300.0, 0.0, 0.0), olimar).unwrap();
                let pi = p.0 as usize;
                sim.pikis[pi].pos = Vec3::new(0.0, 20.0, 0.0);
                sim.pikis[pi].state = PikiState::Flying { ground_frames: 0 };
                assert!(sim.bulb_swallow(pi), "round {round}, Pikmin {i}");
            }
            assert!(!sim.bulbs()[0].can_swallow(), "five in: it shuts at once");
            // Close and shot (about 2.3 s), well short of the 30 s it waits after fewer than five.
            step(&mut sim, 2.5);
            assert_eq!(alive(&sim, PikiColor::White), (round * SLOTS) as usize, "round {round}");
            if round < 5 {
                step(&mut sim, 1.0);
                assert!(sim.bulbs()[0].can_swallow(), "round {round}: open again");
            }
        }
        assert!(sim.pikis.iter().filter(|p| p.is_alive()).all(|p| p.captain == Some(olimar)), "the thrower's");
        assert_eq!(sim.sprouts().count(), 0);
        step(&mut sim, 2.0);
        assert!(sim.bulbs().is_empty(), "25 used: withered");
    }

    #[test]
    fn the_pop_changes_colour_and_spits_five_for_one() {
        let mut sim = Sim::default();
        sim.add_bulb(PikiColor::Blue, true, Vec3::ZERO);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..(6 * 30) {
            sim.update_bulbs(1.0 / 30.0);
            seen.insert(sim.bulbs()[0].colour);
        }
        assert_eq!(seen.len(), 3, "Blue, Red and Yellow in turn");
        let want = sim.bulbs()[0].colour;
        let p = throw_at(&mut sim, PikiColor::Purple, Vec3::new(0.0, 20.0, 0.0));
        assert!(sim.bulb_swallow(p.0 as usize));
        assert!(!sim.bulbs()[0].can_swallow(), "one slot");
        for _ in 0..(5 * 30) {
            sim.update_bulbs(1.0 / 30.0);
        }
        let n = sim.pikis.iter().filter(|p| p.is_alive() && p.color != PikiColor::Purple).count();
        assert_eq!(n, 5);
        let _ = want;
        assert!(sim.bulbs().iter().all(|b| !b.alive()) || sim.bulbs().is_empty(), "its one slot is gone, so it withers");
    }
}
