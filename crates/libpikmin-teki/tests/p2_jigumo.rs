//! Pikmin 2's Hermit Crawmad: the disc's numbers and corpse load, it lies in its hole and cannot be hurt until someone is near, comes up, turns
//! to face them and lunges; what it catches it carries home and eats (open to blows then), a miss leaves it walking home open to blows too.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::jigumo::{APPEAR, ATTACK, CARRY, EAT, HIDE, MISS, RETURN, SEARCH, WAIT};
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, HERMIT_CRAWMAD};
use libpikmin_teki::{Navi, Out, TekiId, Tekis, opt};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

fn assets() -> Option<Assets> {
    let a = Assets { root: Assets::default_dir() };
    p2::is_available(&a.root.join("p2"), HERMIT_CRAWMAD).then_some(a)
}

struct Rig {
    sim: Sim,
    tekis: Tekis,
    id: TekiId,
    player: Navi,
    outs: Vec<Out>,
    seen: std::collections::BTreeSet<i32>,
}

impl Rig {
    fn new(a: &Assets, player_at: Vec3) -> Rig {
        let sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(a);
        tekis.seed(14);
        let id = tekis.spawn(HERMIT_CRAWMAD, 3, Vec3::ZERO, 0.0).expect("spawn");
        Rig { sim, tekis, id, player: Navi { id: 5, pos: player_at, size: 8.0, alive: true }, outs: Vec::new(), seen: Default::default() }
    }
    fn tick(&mut self) {
        let mut frame = CreatureFrame::default();
        self.tekis.set_ground(self.id, 0.0, false);
        self.tekis.frame(&mut frame);
        self.sim.step(&mut Frame(&frame));
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[self.player], 1.0 / 30.0);
        self.outs.extend(out);
        if let Some(t) = self.tekis.get(self.id) {
            self.seen.insert(t.state);
        }
    }
    fn state(&self) -> Option<i32> {
        self.tekis.get(self.id).map(|t| t.state)
    }
    fn run_until(&mut self, seconds: f32, mut f: impl FnMut(&Rig) -> bool) -> bool {
        for _ in 0..(seconds * 30.0) as u32 {
            if f(self) {
                return true;
            }
            self.tick();
        }
        f(self)
    }
}

#[test]
fn it_loads_its_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(HERMIT_CRAWMAD).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (500.0, 300.0, 400.0));
    assert_eq!(info.clips.len(), 17);
    assert!(info.clips[0].starts_with("appear") && info.clips[16].contains("runaway"), "{:?}", info.clips);
    assert_eq!((info.corpse_min_carriers, info.corpse_max_carriers), (5, 8));
    assert!(info.motions[slot::DEAD].end > 1.0);
    // Its own block: carry and return speeds, the two ends of its size range.
    assert_eq!((info.floats[101], info.floats[102], info.floats[103], info.floats[104]), (75.0, 30.0, 1.0, 2.0));
    assert_eq!(info.ints[101], 30);
}

#[test]
fn it_hides_unhurt_until_someone_is_near_then_comes_up_and_lunges() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 2000.0));
    for _ in 0..90 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, APPEAR);
    assert!(t.option(opt::INVINCIBLE) && !t.option(opt::VISIBLE) && !t.option(opt::ATARI));
    let size = t.fade;
    assert!((1.0..=2.0).contains(&size), "one of its five sizes: {size}");
    rig.tekis.damage(rig.id, 50.0, None);
    rig.tick();
    assert_eq!(rig.tekis.get(rig.id).unwrap().hp, 500.0, "a blow on it in its hole does nothing");
    rig.player.pos = Vec3::new(0.0, 0.0, 150.0);
    assert!(rig.run_until(4.0, |r| r.state() == Some(WAIT)), "{:?}", rig.seen);
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::VISIBLE));
    assert!(rig.run_until(6.0, |r| r.seen.contains(&SEARCH)), "{:?}", rig.seen);
    assert!(rig.run_until(6.0, |r| r.state() == Some(ATTACK)), "{:?}", rig.seen);
    assert!(rig.run_until(4.0, |r| r.outs.iter().any(|o| matches!(o, Out::HitNavi { .. }))), "the lunge hurts a player it reaches");
}

#[test]
fn it_carries_what_it_caught_home_open_to_blows_and_eats_it() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 4000.0));
    let piki = rig.sim.spawn_piki(PikiColor::Red, Vec3::new(0.0, 0.0, 120.0)).unwrap();
    assert!(rig.run_until(20.0, |r| r.state() == Some(CARRY)), "{:?}", rig.seen);
    // The Pikmin came to meet it at the hole; carry it from further off so the walk home is long enough to look at.
    rig.tekis.get_mut(rig.id).unwrap().pos = Vec3::new(0.0, 0.0, 300.0);
    rig.tick();
    rig.tick();
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, CARRY);
    assert!(!t.option(opt::INVINCIBLE), "carrying, it is open to blows");
    assert!(t.pos.z < 300.0 && t.target_vel.length() > 0.0, "backing towards the hole: {:?}", t.pos);
    rig.tekis.damage(rig.id, 25.0, None);
    rig.tick();
    assert!(rig.tekis.get(rig.id).unwrap().hp < 500.0);
    assert!(rig.run_until(40.0, |r| r.seen.contains(&EAT)), "{:?}", rig.seen);
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::INVINCIBLE), "eating, it is not");
    assert!(rig.run_until(10.0, |r| r.seen.contains(&HIDE)), "{:?}", rig.seen);
    assert!(!rig.sim.piki(piki).is_alive(), "what it ate is gone");
}

#[test]
fn a_miss_leaves_it_walking_home_open_to_blows() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 150.0));
    assert!(rig.run_until(15.0, |r| r.state() == Some(ATTACK)), "{:?}", rig.seen);
    // He steps away before the lunge lands.
    rig.player.pos = Vec3::new(0.0, 0.0, 4000.0);
    assert!(rig.run_until(10.0, |r| r.state() == Some(RETURN)), "{:?}", rig.seen);
    assert!(rig.seen.contains(&MISS));
    rig.tick();
    assert!(!rig.tekis.get(rig.id).unwrap().option(opt::INVINCIBLE));
    rig.tekis.damage(rig.id, 25.0, None);
    rig.tick();
    assert!(rig.tekis.get(rig.id).unwrap().hp < 500.0);
    assert!(rig.run_until(40.0, |r| r.seen.contains(&HIDE)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.state() == Some(APPEAR)), "it goes back into its hole: {:?}", rig.seen);
    assert!(rig.tekis.get(rig.id).unwrap().option(opt::INVINCIBLE));
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 2000.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(10.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
