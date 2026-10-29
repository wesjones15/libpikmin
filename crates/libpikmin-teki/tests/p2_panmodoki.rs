//! Pikmin 2's Giant Breadbug: the disc's numbers and corpse load, it comes up from its mound, ambles, takes a pellet and backs home with it, eats it
//! underground and comes up again; a blow makes it let go; ship parts are left alone.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PelletDesc, PelletState, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::panmodoki::{APPEAR, BACK, CARRY_END, DAMAGE, HIDE, STICK, WALK};
use libpikmin_teki::p2::{self, GIANT_BREADBUG};
use libpikmin_teki::{Navi, Out, TekiId, Tekis};

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
    p2::is_available(&a.root.join("p2"), GIANT_BREADBUG).then_some(a)
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
        tekis.seed(16);
        let id = tekis.spawn(GIANT_BREADBUG, 3, Vec3::ZERO, 0.0).expect("spawn");
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

fn pellet(pos: Vec3, ship_part: Option<u32>, min: u32) -> PelletDesc {
    PelletDesc {
        pos,
        yaw: 0.0,
        radius: 8.0,
        height: 8.0,
        min_carriers: min,
        max_carriers: min * 2,
        seeds_matching: 1,
        seeds_other: 1,
        color: None,
        creature: None,
        ship_part,
    }
}

#[test]
fn it_loads_its_numbers_and_corpse() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let tekis = Tekis::load(&a);
    let info = tekis.info(GIANT_BREADBUG).expect("loaded");
    assert_eq!((info.life, info.run_speed, info.visible_range), (2000.0, 85.0, 300.0));
    assert_eq!(info.clips.len(), 9);
    assert_eq!((info.corpse_min_carriers, info.corpse_max_carriers), (10, 20));
    assert_eq!((info.floats[103], info.floats[106], info.floats[115]), (45.0, 100.0, 150.0));
}

#[test]
fn it_takes_a_pellet_home_eats_it_goes_under_and_comes_up_again() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 6000.0));
    let loose = rig.sim.spawn_pellet(pellet(Vec3::new(120.0, 0.0, 0.0), None, 2));
    let part = rig.sim.spawn_pellet(pellet(Vec3::new(-120.0, 0.0, 0.0), Some(3), 2));
    assert!(rig.run_until(8.0, |r| r.state() == Some(WALK)), "{:?}", rig.seen);
    assert!(rig.run_until(15.0, |r| r.state() == Some(STICK)), "it goes for the pellet: {:?}", rig.seen);
    assert!(rig.run_until(15.0, |r| r.state() == Some(BACK)), "{:?}", rig.seen);
    assert_eq!(rig.sim.pellet(loose).state, PelletState::Held);
    assert!(rig.run_until(40.0, |r| r.seen.contains(&CARRY_END)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.seen.contains(&HIDE)), "{:?}", rig.seen);
    assert!(rig.run_until(10.0, |r| r.sim.pellet(loose).state == PelletState::Gone), "it eats it");
    assert_eq!(rig.sim.pellet(part).state, PelletState::Idle, "the ship part is left");
    let n = rig.seen.iter().filter(|&&s| s == APPEAR).count();
    assert!(rig.run_until(15.0, |r| r.state() == Some(APPEAR) || r.state() == Some(WALK)), "it comes up again after {n}: {:?}", rig.seen);
}

#[test]
fn a_blow_makes_it_let_go_and_hurts_it() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 6000.0));
    let loose = rig.sim.spawn_pellet(pellet(Vec3::new(100.0, 0.0, 0.0), None, 2));
    assert!(rig.run_until(40.0, |r| r.state() == Some(BACK)), "{:?}", rig.seen);
    rig.tekis.damage(rig.id, 30.0, None);
    assert!(rig.run_until(2.0, |r| r.state() == Some(DAMAGE)), "{:?}", rig.seen);
    assert_ne!(rig.sim.pellet(loose).state, PelletState::Held);
    rig.tick();
    assert_eq!(rig.tekis.get(rig.id).unwrap().hp, 1900.0, "the press damage");
}

#[test]
fn it_dies_with_the_dying_notice() {
    let Some(a) = assets() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
    let mut rig = Rig::new(&a, Vec3::new(0.0, 0.0, 6000.0));
    rig.run_until(4.0, |_| false);
    rig.tekis.damage(rig.id, 1.0e6, None);
    assert!(rig.run_until(15.0, |r| r.tekis.get(r.id).is_none()));
    let dying = rig.outs.iter().position(|o| matches!(o, Out::Dying { .. }));
    let died = rig.outs.iter().position(|o| matches!(o, Out::Died { .. }));
    assert!(dying.is_some() && died.is_some() && dying < died);
}
