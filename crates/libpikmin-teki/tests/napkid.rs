//! A Snitchbug hovers, dives at a Pikmin, carries it off, and drops it.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Tekis, info};

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
fn a_snitchbug_snatches_a_pikmin_and_carries_it() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::SNITCHBUG, 3, Vec3::ZERO, 0.0).expect("no Snitchbug strategy");
    for i in 0..3 {
        sim.spawn_piki(PikiColor::Yellow, Vec3::new(-10.0 + 10.0 * i as f32, 0.0, 150.0)).unwrap();
    }
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut top: f32 = 0.0;
    for _ in 0..(60 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
        let Some(t) = tekis.get(id) else { break };
        states.insert(t.state);
        top = top.max(t.pos.y);
    }
    assert!(top > 20.0, "it flew: {top} {states:?}");
    assert!(states.contains(&4), "it chased: {states:?}");
    assert!(states.contains(&8), "it dived: {states:?}");
    assert!(states.contains(&13) || states.contains(&12), "and carried or missed: {states:?}");
}
