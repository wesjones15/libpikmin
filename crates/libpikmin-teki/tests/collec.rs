//! The Breadbug takes a small pellet home and eats it, and leaves ship parts alone.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PelletDesc, PelletState, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::Tekis;

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
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

fn setup() -> Option<(Sim, Tekis)> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some((Sim::new(Params::default(), 1), Tekis::load(&assets)))
}

fn run(sim: &mut Sim, tekis: &mut Tekis, seconds: f32) {
    let mut frame = CreatureFrame::default();
    for _ in 0..(seconds * 30.0) as usize {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(sim, &FlatHost, &[], 1.0 / 30.0);
    }
}

#[test]
fn a_breadbug_carries_a_small_pellet_to_its_nest_and_eats_it() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    let id = tekis.spawn(libpikmin_teki::info::BREADBUG, 9, Vec3::ZERO, 0.0).expect("no Breadbug strategy");
    let p = sim.spawn_pellet(pellet(Vec3::new(70.0, 0.0, 0.0), None, 1));
    let part = sim.spawn_pellet(pellet(Vec3::new(-70.0, 0.0, 0.0), Some(3), 1));
    let mut held = false;
    for _ in 0..90 {
        run(&mut sim, &mut tekis, 1.0);
        held |= sim.pellet(p).state == PelletState::Held;
        if sim.pellet(p).state == PelletState::Gone {
            break;
        }
    }
    assert!(held, "it took hold of the pellet");
    assert_eq!(sim.pellet(p).state, PelletState::Gone, "and ate it at the nest");
    assert_eq!(sim.pellet(part).state, PelletState::Idle, "the ship part was left");
    assert!(tekis.get(id).is_some());
}

#[test]
fn a_breadbug_that_is_shaken_lets_go_of_the_pellet() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    let id = tekis.spawn(libpikmin_teki::info::BREADBUG, 9, Vec3::ZERO, 0.0).unwrap();
    let p = sim.spawn_pellet(pellet(Vec3::new(40.0, 0.0, 0.0), None, 1));
    for _ in 0..40 {
        run(&mut sim, &mut tekis, 0.5);
        if sim.pellet(p).state == PelletState::Held {
            break;
        }
    }
    assert_eq!(sim.pellet(p).state, PelletState::Held);
    let life = tekis.get(id).unwrap().info.life;
    tekis.damage(id, life + 1.0, None);
    run(&mut sim, &mut tekis, 1.0);
    assert_eq!(sim.pellet(p).state, PelletState::Idle, "dropped where it died");
}
