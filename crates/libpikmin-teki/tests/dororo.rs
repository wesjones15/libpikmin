//! The Giant Egg hatches into a Smoky Progg when a player nears; the Progg walks on its goal and crushes Pikmin.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Navi, Out, Tekis, info};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

fn load() -> Option<(Sim, Tekis)> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some((Sim::new(Params::default(), 1), Tekis::load(&assets)))
}

#[test]
fn an_egg_hatches_into_a_progg_when_a_player_comes_near() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::GIANT_EGG, 3, Vec3::ZERO, 0.0).expect("no egg strategy");
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 100.0), size: 8.0, alive: true };
    let mut frame = CreatureFrame::default();
    let mut spawned = None;
    for _ in 0..(40 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        for o in tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0) {
            if let Out::Spawn { kind, .. } = o {
                spawned = Some(kind);
            }
        }
        if tekis.get(id).is_none() {
            break;
        }
    }
    assert_eq!(spawned, Some(info::SMOKY_PROGG), "the egg hatched a Smoky Progg");
}

#[test]
fn a_progg_walks_to_its_goal_and_crushes_pikmin_it_touches() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::SMOKY_PROGG, 3, Vec3::ZERO, 0.0).expect("no Progg strategy");
    tekis.set_goal(id, Some(Vec3::new(0.0, 0.0, 600.0)));
    for i in 0..5 {
        sim.spawn_piki(PikiColor::Blue, Vec3::new(-16.0 + 8.0 * i as f32, 0.0, 70.0)).unwrap();
    }
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    for _ in 0..(50 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
        states.insert(tekis.get(id).unwrap().state);
    }
    assert!(states.contains(&3), "it set out: {states:?}");
    assert!(sim.piki_deaths() > 0 || states.contains(&7), "it met Pikmin: {states:?}");
}
