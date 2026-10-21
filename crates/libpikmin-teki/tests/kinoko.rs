//! A Puffstool turns and runs from a player, then puffs spores.

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
fn a_puffstool_runs_from_a_player_then_puffs_spores() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::PUFFSTOOL, 3, Vec3::ZERO, 0.0).expect("no Puffstool strategy");
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut effects = std::collections::BTreeSet::new();
    let mut player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 150.0), size: 8.0, alive: true };
    for _ in 0..(40 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        for o in &out {
            if let Out::Effect { id, .. } = o {
                effects.insert(*id);
            }
        }
        let t = tekis.get(id).unwrap();
        // The player keeps after it.
        player.pos = t.pos + Vec3::new(0.0, 0.0, 120.0);
        states.insert(t.state);
    }
    assert!(states.contains(&3) && states.contains(&4), "it turned and ran: {states:?}");
    assert!(states.contains(&12) || states.contains(&5), "then spored or tripped: {states:?}");
    assert!(effects.contains(&150) || effects.contains(&154), "with spores: {effects:?}");
}
