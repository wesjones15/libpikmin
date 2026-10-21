//! Every enemy kind runs for a while against a player and some Pikmin, hurt halfway, without panicking, and every
//! kind the roster names has a strategy.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::info::KINDS;
use libpikmin_teki::{Navi, Target, Tekis};

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
fn every_kind_runs_without_panicking_and_dies_when_killed() {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return;
    }
    let mut missing = Vec::new();
    for kind in 0..KINDS as u8 {
        let mut sim = Sim::new(Params::default(), 7);
        let mut tekis = Tekis::load(&assets);
        if tekis.info(kind).is_none() {
            continue;
        }
        let Some(id) = tekis.spawn(kind, 3, Vec3::new(0.0, 0.0, 0.0), 0.3) else {
            missing.push(kind);
            continue;
        };
        for i in 0..5 {
            sim.spawn_piki(PikiColor::Red, Vec3::new(-30.0 + 15.0 * i as f32, 0.0, 90.0)).unwrap();
        }
        let mut player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 140.0), size: 8.0, alive: true };
        let mut frame = CreatureFrame::default();
        let mut gone = false;
        for tick in 0..(45 * 30) {
            if tick == 20 * 30 {
                tekis.damage(id, 1.0e6, Some(Target::Navi(5)));
            }
            tekis.set_ground(id, 0.0, tick % 400 > 300);
            tekis.frame(&mut frame);
            sim.step(&mut Frame(&frame));
            tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
            if tekis.get(id).is_none() {
                gone = true;
                break;
            }
            player.pos.x = (tick as f32 * 0.05).sin() * 40.0;
        }
        // Fixtures that cannot be killed (geysers, the shell) may stay; everything else is gone after being killed.
        let stays = [libpikmin_teki::info::FIRE_GEYSER, libpikmin_teki::info::CLAMCLAMP, libpikmin_teki::info::HONEYWISP];
        let t = tekis.get(id);
        assert!(gone || stays.contains(&kind), "kind {kind} did not die when killed: state {:?} hp {:?} opts {:?}", t.map(|t| t.state), t.map(|t| t.hp), t.map(|t| t.options));
    }
    assert!(missing.is_empty(), "kinds with files and no strategy: {missing:?}");
}
