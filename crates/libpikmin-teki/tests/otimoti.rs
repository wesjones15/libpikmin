//! The Wollywog jumps and lands on what it went for.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost, Host};
use libpikmin::{Params, PikiColor, PikiState, Sim};
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

fn setup() -> Option<(Sim, Tekis)> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some((Sim::new(Params::default(), 1), Tekis::load(&assets)))
}

#[test]
fn a_wollywog_jumps_and_presses_a_player_standing_close() {
    for kind in [libpikmin_teki::info::WOLLYWOG, libpikmin_teki::info::WOLLYWOG_YELLOW] {
        let Some((mut sim, mut tekis)) = setup() else { return };
        let id = tekis.spawn(kind, 3, Vec3::ZERO, 0.0).expect("no Wollywog strategy");
        let player = Navi { id: 7, pos: Vec3::new(0.0, 0.0, 45.0), size: 8.0, alive: true };
        let mut frame = CreatureFrame::default();
        let (mut states, mut hit, mut peak) = (std::collections::BTreeSet::new(), false, 0.0f32);
        for _ in 0..(40 * 30) {
            tekis.frame(&mut frame);
            sim.step(&mut Frame(&frame));
            let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
            hit |= out.iter().any(|o| matches!(o, Out::HitNavi { navi: 7, .. }));
            if let Some(t) = tekis.get(id) {
                states.insert(t.state);
                peak = peak.max(t.pos.y);
            }
        }
        assert!(states.contains(&9) && states.contains(&11) && states.contains(&12), "it went through the jump: {states:?}");
        assert!(peak > 20.0, "it left the ground: {peak}");
        assert!(hit, "and came down on the player");
    }
}

#[test]
fn a_wollywog_squashes_pikmin_it_lands_on() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    tekis.spawn(libpikmin_teki::info::WOLLYWOG, 3, Vec3::ZERO, 0.0).unwrap();
    for i in 0..4 {
        sim.spawn_piki(PikiColor::Red, Vec3::new(-6.0 + i as f32 * 4.0, 0.0, 40.0)).unwrap();
    }
    let mut frame = CreatureFrame::default();
    let mut flat = false;
    for _ in 0..(40 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
        flat |= sim.pikis().iter().any(|p| matches!(p.state, PikiState::Pressed { .. } | PikiState::Fading { .. } | PikiState::Dying { .. }));
    }
    assert!(flat, "a Pikmin was pressed");
}
