//! The Honeywisp on its path, and what a hit makes it do.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Out, Tekis, Event, opt};

fn setup() -> Option<(Sim, Tekis)> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some((Sim::new(Params::default(), 1), Tekis::load(&assets)))
}

#[test]
fn a_honeywisp_flies_its_path_hides_and_comes_back() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    let id = tekis.spawn(libpikmin_teki::info::HONEYWISP, 5, Vec3::ZERO, 0.0).unwrap();
    {
        let t = tekis.get_mut(id).unwrap();
        t.target_pos = Vec3::new(0.0, 0.0, 300.0);
        t.territory = 300.0;
    }
    let mut seen_hidden = false;
    let mut far = 0.0f32;
    for _ in 0..(40 * 30) {
        tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
        let t = tekis.get(id).unwrap();
        far = far.max(t.pos.z);
        seen_hidden |= !t.option(opt::VISIBLE);
        assert!(t.pos.y > 20.0 || !t.option(opt::VISIBLE), "it flies: {:?}", t.pos);
    }
    assert!(far > 200.0, "it crossed its territory: {far}");
    assert!(seen_hidden, "it hid between trips");
    let _ = CreatureFrame::default();
}

#[test]
fn a_hit_honeywisp_drops_nectar_and_flies_away() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    let id = tekis.spawn(libpikmin_teki::info::HONEYWISP, 5, Vec3::ZERO, 0.0).unwrap();
    tekis.get_mut(id).unwrap().target_pos = Vec3::new(0.0, 0.0, 300.0);
    tekis.get_mut(id).unwrap().territory = 300.0;
    for _ in 0..60 {
        tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
    }
    tekis.damage(id, 1.0, None);
    let mut outs = Vec::new();
    for _ in 0..(20 * 30) {
        outs.extend(tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0));
        sim.step(&mut FlatHost);
    }
    assert!(!sim.nectar_drops().is_empty() || !sim.puddles().is_empty(), "nectar fell");
    assert!(outs.iter().any(|o| matches!(o, Out::Died { .. })), "it flew off and was gone");
    let _ = Event::Ground;
}
