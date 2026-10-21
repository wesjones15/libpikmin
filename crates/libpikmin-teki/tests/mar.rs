//! A Puffy Blowhog floats, closes on a player and blows wind on Pikmin.

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

#[test]
fn a_puffy_blowhog_floats_closes_in_and_blows() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::PUFFY_BLOWHOG, 3, Vec3::ZERO, 0.0).expect("no Puffy Blowhog strategy");
    for i in 0..4 {
        sim.spawn_piki(PikiColor::Red, Vec3::new(-12.0 + 8.0 * i as f32, 0.0, 120.0)).unwrap();
    }
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 130.0), size: 8.0, alive: true };
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut top: f32 = 0.0;
    let mut wind = false;
    for _ in 0..(60 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        wind |= out.iter().any(|o| matches!(o, Out::EffectDir { id: 176, .. }));
        let Some(t) = tekis.get(id) else { break };
        states.insert(t.state);
        top = top.max(t.pos.y);
    }
    assert!(top > 20.0, "it floated: {top} {states:?}");
    assert!(states.contains(&11), "it approached: {states:?}");
    assert!(states.contains(&12) && wind, "and blew: {states:?}");
}
