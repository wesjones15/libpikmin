//! Pikmin 2's Burrowing Snagret: the disc's numbers and corpse load, it lies underground until someone is in its territory, comes up beside them, strikes with
//! its head, swallows a Pikmin and eats it, and dives when it has nobody in reach.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::snagret::{APPEAR1, APPEAR2, ATTACK, DISAPPEAR, EAT, STAY, WAIT};
use libpikmin_teki::info::slot;
use libpikmin_teki::p2::{self, BURROWING_SNAGRET};
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
    p2::is_available(&a.root.join("p2"), BURROWING_SNAGRET).then_some(a)
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
        tekis.seed(18);
        let id = tekis.spawn(BURROWING_SNAGRET, 3, Vec3::ZERO, 0.0).expect("spawn");
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
    let info = tekis.info(BURROWING_SNAGRET).expect("loaded");
    assert_eq!((info.life, info.visible_range), (1500.0, 150.0));
    assert_eq!(info.clips.len(), 13);
    assert_eq!((info.corpse_min_carriers, info.corpse_max_carriers), (5, 10));
    assert_eq!((info.floats[101], info.floats[111], info.floats[112]), (0.6, 2.5, 2.5));
}

#[test]
fn it_comes_up_beside_someone_in_its_territory_and_strikes() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    for _ in 0..30 {
        rig.tick();
    }
    let t = rig.tekis.get(rig.id).unwrap();
    assert_eq!(t.state, STAY);
    assert!(t.option(opt::INVINCIBLE) && !t.option(opt::VISIBLE));
    rig.player.pos = Vec3::new(40.0, 0.0, 30.0);
    assert!(rig.run_until(8.0, |r| r.state() == Some(APPEAR1) || r.state() == Some(APPEAR2)), "{:?}", rig.seen);
    let t = rig.tekis.get(rig.id).unwrap();
    assert!((t.pos - rig.player.pos).length() > 80.0 && (t.pos - rig.player.pos).length() < 160.0, "it comes up near him, at {:?}", t.pos);
    assert!(rig.run_until(8.0, |r| r.seen.contains(&WAIT) || r.seen.contains(&ATTACK)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&ATTACK)), "it strikes: {:?}", rig.seen);
}

#[test]
fn it_dives_when_nobody_is_in_reach() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(40.0, 0.0, 30.0));
    assert!(rig.run_until(10.0, |r| r.state() == Some(ATTACK)), "{:?}", rig.seen);
    rig.player.pos = Vec3::new(0.0, 0.0, 4000.0);
    assert!(rig.run_until(15.0, |r| r.seen.contains(&DISAPPEAR)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.state() == Some(STAY)), "{:?}", rig.seen);
}

#[test]
fn it_swallows_a_pikmin_in_reach_and_eats_it() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    // A player brings it up; a Pikmin then stands in front of it, in its second zone.
    let mut rig = Rig::new(&a, Vec3::new(40.0, 0.0, 30.0));
    assert!(rig.run_until(10.0, |r| r.state() == Some(APPEAR1) || r.state() == Some(APPEAR2)), "{:?}", rig.seen);
    let t = rig.tekis.get(rig.id).unwrap();
    let at = t.pos + t.forward() * 100.0;
    rig.player.pos = Vec3::new(0.0, 0.0, 4000.0);
    let piki = rig.sim.spawn_piki(libpikmin::PikiColor::Red, at).unwrap();
    assert!(rig.run_until(40.0, |r| r.seen.contains(&EAT)), "{:?} {:?} {:?}", rig.seen, rig.sim.piki(piki).pos, rig.sim.piki(piki).state);
    assert!(rig.run_until(10.0, |r| !r.sim.piki(piki).is_alive()), "it is eaten");
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 3000.0));
    rig.run_until(2.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(15.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
