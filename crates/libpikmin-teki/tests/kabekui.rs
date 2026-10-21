//! The Sheargrubs rise when a player comes near, bite what they meet, and the Shearwig flies when hurt.

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
fn a_male_sheargrub_rises_walks_and_bites() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::SHEARGRUB_M, 3, Vec3::ZERO, 0.0).expect("no Sheargrub strategy");
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 120.0), size: 8.0, alive: true };
    for i in 0..4 {
        sim.spawn_piki(PikiColor::Red, Vec3::new(-12.0 + 8.0 * i as f32, 0.0, 60.0)).unwrap();
    }
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let (mut dirt, mut hurt) = (false, false);
    for _ in 0..(40 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        dirt |= out.iter().any(|o| matches!(o, Out::Effect { id: 50, .. }));
        hurt |= out.iter().any(|o| matches!(o, Out::HitNavi { navi: 5, .. }));
        let Some(t) = tekis.get(id) else { break };
        states.insert(t.state);
    }
    assert!(states.contains(&2), "it rose: {states:?}");
    assert!(dirt, "with dirt");
    assert!(states.contains(&4) || states.contains(&5), "and walked: {states:?}");
    assert!(states.contains(&6) || hurt || sim.piki_deaths() > 0 || states.contains(&12), "and bit or dug in: {states:?}");
}

#[test]
fn a_shearwig_flies_when_it_is_hurt() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::SHEARWIG, 3, Vec3::ZERO, 0.0).expect("no Shearwig strategy");
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 120.0), size: 8.0, alive: true };
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut top: f32 = 0.0;
    for tick in 0..(40 * 30) {
        if tick == 6 * 30 {
            let life = tekis.get(id).unwrap().hp;
            tekis.get_mut(id).unwrap().hp = life * 0.3;
        }
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        let Some(t) = tekis.get(id) else { break };
        states.insert(t.state);
        top = top.max(t.pos.y);
    }
    assert!(states.contains(&8) && states.contains(&9), "it took off: {states:?}");
    assert!(top > 20.0, "and rose: {top}");
}
