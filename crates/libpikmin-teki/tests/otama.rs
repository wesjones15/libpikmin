//! A Wogpole dawdles, and hops and flees from a player who comes near.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Navi, Tekis, info};

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
fn a_wogpole_ambles_then_runs_from_a_player() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::WOGPOLE, 3, Vec3::ZERO, 0.0).expect("no Wogpole strategy");
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut far_player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 2000.0), size: 8.0, alive: true };
    let mut start = Vec3::ZERO;
    for tick in 0..(40 * 30) {
        if tick == 25 * 30 {
            let p = tekis.get(id).unwrap().pos;
            start = p;
            far_player.pos = p + Vec3::new(0.0, 0.0, 40.0);
        }
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(&mut sim, &FlatHost, &[far_player], 1.0 / 30.0);
        let t = tekis.get(id).unwrap();
        if tick >= 25 * 30 {
            far_player.pos = far_player.pos.lerp(t.pos + Vec3::new(0.0, 0.0, 40.0), 0.5);
        }
        states.insert(t.state);
    }
    assert!(states.contains(&3), "it ambled: {states:?}");
    assert!(states.contains(&5) && states.contains(&6), "and ran from the player: {states:?}");
    assert!(tekis.get(id).unwrap().pos.distance(start) > 5.0, "moving away");
}
