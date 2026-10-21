//! The Bulborbs against the sim's Pikmin on flat ground, with the disc's numbers (skipped without them).

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Out, Tekis};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

struct World {
    sim: Sim,
    tekis: Tekis,
    frame: CreatureFrame,
    outs: Vec<Out>,
}

fn world() -> Option<World> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some(World { sim: Sim::new(Params::default(), 1), tekis: Tekis::load(&assets), frame: CreatureFrame::default(), outs: Vec::new() })
}

impl World {
    fn tick(&mut self) {
        self.tekis.frame(&mut self.frame);
        let mut host = Frame(&self.frame);
        self.sim.step(&mut host);
        let out = self.tekis.update(&mut self.sim, &FlatHost, &[], 1.0 / 30.0);
        self.outs.extend(out);
    }
    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds * 30.0) as usize {
            self.tick();
        }
    }
}

#[test]
fn a_dwarf_bulborb_chases_and_eats_a_pikmin() {
    let Some(mut w) = world() else { return };
    let id = w.tekis.spawn(libpikmin_teki::info::DWARF, 77, Vec3::ZERO, 0.0).expect("no strategy for the Dwarf Bulborb");
    w.sim.spawn_piki(PikiColor::Red, Vec3::new(0.0, 0.0, 70.0)).unwrap();
    assert_eq!(w.tekis.get(id).unwrap().state, 15, "starts waiting");
    w.run(1.0);
    let t = w.tekis.get(id).unwrap();
    assert!(t.state != 15 && t.state != 6, "it saw the Pikmin and moved on: state {}", t.state);
    w.run(20.0);
    assert!(w.sim.piki_deaths() >= 1 || w.sim.pikis().iter().any(|p| matches!(p.state, libpikmin::PikiState::Swallowed { .. })),
        "the Pikmin was eaten");
}

#[test]
fn a_spotty_bulborb_sleeps_until_something_comes_near() {
    let Some(mut w) = world() else { return };
    let id = w.tekis.spawn(libpikmin_teki::info::SPOTTY, 78, Vec3::ZERO, 0.0).unwrap();
    w.run(2.0);
    assert!(matches!(w.tekis.get(id).unwrap().state, 15 | 1), "asleep with nothing about");
    let notice = w.tekis.get(id).unwrap().f(50);
    w.sim.spawn_piki(PikiColor::Blue, Vec3::new(0.0, 0.0, notice * 0.5)).unwrap();
    w.run(3.0);
    assert!(!matches!(w.tekis.get(id).unwrap().state, 15 | 1), "woke: state {}", w.tekis.get(id).unwrap().state);
}

#[test]
fn a_dwarf_cry_wakes_a_sleeping_spotty() {
    let Some(mut w) = world() else { return };
    let spotty = w.tekis.spawn(libpikmin_teki::info::SPOTTY, 80, Vec3::new(150.0, 0.0, 0.0), 0.0).unwrap();
    w.tekis.spawn(libpikmin_teki::info::DWARF, 81, Vec3::ZERO, 0.0).unwrap();
    // Enough Pikmin that the chase outlasts its crying period.
    for i in 0..8 {
        w.sim.spawn_piki(PikiColor::Yellow, Vec3::new(-30.0 + i as f32 * 8.0, 0.0, 80.0)).unwrap();
    }
    w.run(25.0);
    assert!(!matches!(w.tekis.get(spotty).unwrap().state, 15 | 1), "woken by the cry or the noise: state {}", w.tekis.get(spotty).unwrap().state);
}

#[test]
fn a_dead_bulborb_dies_after_its_death_motion() {
    let Some(mut w) = world() else { return };
    let id = w.tekis.spawn(libpikmin_teki::info::DWARF, 79, Vec3::ZERO, 0.0).unwrap();
    w.tick();
    let life = w.tekis.get(id).unwrap().info.life;
    w.tekis.damage(id, life + 1.0, None);
    w.run(6.0);
    assert!(w.outs.iter().any(|o| matches!(o, Out::Dying { .. })), "began dying");
    assert!(w.outs.iter().any(|o| matches!(o, Out::Died { .. })), "finished dying");
    assert!(w.tekis.get(id).is_none());
}
