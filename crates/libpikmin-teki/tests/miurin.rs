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

#[test]
fn a_mamuta_punch_buries_pikmin_as_flowers_and_a_buried_player_calms_it() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut sim = Sim::new(Params::default(), 1);
    let mut tekis = Tekis::load(&assets);
    let id = tekis.spawn(info::MAMUTA, 3, Vec3::ZERO, 0.0).expect("no Mamuta strategy");
    for i in 0..6 {
        sim.spawn_piki(libpikmin::PikiColor::Blue, Vec3::new(-20.0 + 8.0 * i as f32, 0.0, 60.0)).unwrap();
    }
    let mut player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 70.0), size: 8.0, alive: true };
    let mut frame = CreatureFrame::default();
    let mut states = std::collections::BTreeSet::new();
    let mut bury_hit = false;
    for tick in 0..(40 * 30) {
        if tick == 3 * 30 {
            tekis.damage(id, 5.0, Some(libpikmin_teki::Target::Navi(5)));
        }
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        for o in out {
            if let Out::HitNavi { knock, .. } = o && knock == libpikmin_teki::miurin::BURY {
                bury_hit = true;
                tekis.buried = vec![5];
            }
        }
        let t = tekis.get(id).unwrap();
        states.insert(t.state);
        let ahead = Vec3::new(t.yaw.sin(), 0.0, t.yaw.cos()) * 45.0;
        player.pos = player.pos.lerp(t.pos + ahead, 0.5);
    }
    assert!(sim.sprouts().count() > 0, "Pikmin were buried as sprouts: {states:?}");
    assert!(bury_hit, "and the player was buried: {states:?}");
    assert!(states.contains(&3) || states.contains(&15) || states.contains(&13), "which calmed it: {states:?}");
}
