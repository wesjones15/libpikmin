//! The Fiery Blowhog faces what it sees and burns it.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Navi, Out, Tekis};

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
fn a_blowhog_breathes_fire_on_a_player_in_front_of_it() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(libpikmin_teki::info::FIERY_BLOWHOG, 3, Vec3::ZERO, 0.0).expect("no Blowhog strategy");
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 120.0), size: 8.0, alive: true };
    for i in 0..3 {
        sim.spawn_piki(PikiColor::Blue, Vec3::new(-8.0 + 8.0 * i as f32, 0.0, 110.0)).unwrap();
    }
    let mut frame = CreatureFrame::default();
    let (mut states, mut burned, mut fire_effect) = (std::collections::BTreeSet::new(), false, false);
    for _ in 0..(30 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        burned |= out.iter().any(|o| matches!(o, Out::HitNavi { navi: 5, knock, .. } if *knock < 0.0));
        fire_effect |= out.iter().any(|o| matches!(o, Out::EffectDir { id: 100, .. }));
        states.insert(tekis.get(id).unwrap().state);
    }
    assert!(states.contains(&7), "it breathed: {states:?}");
    assert!(fire_effect, "with the flame effect");
    assert!(burned || sim.piki_deaths() > 0, "and burned something");
}
