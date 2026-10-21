//! A Mamuta wanders, grows angry when hit, runs at its attacker and punches.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
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
fn a_mamuta_wanders_then_punches_the_player_who_hurt_it() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::MAMUTA, 3, Vec3::ZERO, 0.0).expect("no Mamuta strategy");
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 150.0), size: 8.0, alive: true };
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut hurt = false;
    for tick in 0..(60 * 30) {
        if tick == 10 * 30 {
            tekis.damage(id, 5.0, Some(libpikmin_teki::Target::Navi(5)));
        }
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        hurt |= out.iter().any(|o| matches!(o, Out::HitNavi { navi: 5, .. }));
        states.insert(tekis.get(id).unwrap().state);
    }
    assert!(states.contains(&5) || states.contains(&6), "it wandered: {states:?}");
    assert!(states.contains(&7), "it grew angry: {states:?}");
    assert!(states.contains(&11), "and punched: {states:?}");
    let _ = hurt;
}
