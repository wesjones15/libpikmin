//! A Clamclamp makes its pearl, opens when something comes near it and snaps shut on the Pikmin inside.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Out, Tekis, info};

struct Frame<'a>(&'a CreatureFrame);

impl Host for Frame<'_> {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
    fn creatures(&mut self, out: &mut CreatureFrame) {
        *out = self.0.clone();
    }
}

#[test]
fn a_clamclamp_closes_on_what_comes_to_its_pearl() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::CLAMCLAMP, 3, Vec3::ZERO, 0.0).expect("no Clamclamp strategy");
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut pearl_at = None;
    let mut spawned_pikis = false;
    for _ in 0..(40 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
        for o in &out {
            if let Out::Spawn { kind, pos, .. } = o {
                assert_eq!(*kind, info::PEARL);
                pearl_at = Some(*pos);
            }
        }
        if let (Some(p), false) = (pearl_at, spawned_pikis) {
            for i in 0..3 {
                sim.spawn_piki(PikiColor::Red, p + Vec3::new(-6.0 + 6.0 * i as f32, 0.0, 0.0)).unwrap();
            }
            spawned_pikis = true;
        }
        states.insert(tekis.get(id).unwrap().state);
    }
    assert!(pearl_at.is_some(), "it made its pearl");
    assert!(states.contains(&1) && states.contains(&2) && states.contains(&3), "it opened, warned, closed: {states:?}");
    assert!(sim.piki_deaths() >= 1, "and ate Pikmin: {}", sim.piki_deaths());
}
