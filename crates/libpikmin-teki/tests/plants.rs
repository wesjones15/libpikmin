//! The Fire Geyser shoots flame upward now and then; the Pellet Posy grows and drops a pellet when killed.

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

fn load() -> Option<(Sim, Tekis)> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some((Sim::new(Params::default(), 1), Tekis::load(&assets)))
}

#[test]
fn a_fire_geyser_erupts_on_a_timer() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::FIRE_GEYSER, 3, Vec3::new(50.0, 0.0, 50.0), 0.0).expect("no geyser strategy");
    let mut frame = CreatureFrame::default();
    let (mut flame, mut states) = (false, std::collections::BTreeSet::new());
    for _ in 0..(40 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
        flame |= out.iter().any(|o| matches!(o, Out::EffectDir { id: 227, .. }));
        states.insert(tekis.get(id).unwrap().state);
    }
    assert!(states.contains(&2) && flame, "it erupted: {states:?}");
    assert_eq!(tekis.get(id).unwrap().pos, Vec3::new(50.0, 0.0, 50.0), "and stayed put");
}

#[test]
fn a_grown_pellet_posy_drops_a_pellet_when_killed() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::PELLET_POSY, 3, Vec3::ZERO, 0.0).expect("no posy strategy");
    tekis.get_mut(id).unwrap().count[0] = 2;
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 50.0), size: 8.0, alive: true };
    let mut frame = CreatureFrame::default();
    let mut dropped = false;
    let mut fall_started = None;
    for tick in 0..(10 * 30) {
        if tick == 30 {
            let t = tekis.get_mut(id).unwrap();
            t.hp = 0.0;
        }
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        if out.iter().any(|o| matches!(o, Out::Dying { .. })) { fall_started = Some(tick); }
        for event in &out {
            if let Out::SpawnPellets { number, color, count, .. } = event {
                assert_eq!((*number, *count), (0, 1), "the model bears one 1 pellet");
                assert!(*color < 3);
                assert!(tick > fall_started.expect("fall before drop") + 10, "release after the falling animation");
            }
        }
        dropped |= out.iter().any(|o| matches!(o, Out::SpawnPellets { .. }));
    }
    assert!(dropped, "a pellet came out");
}

#[test]
fn immature_posies_never_drop_pellets_and_growing_keys_emit_exact_effects_and_sounds() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let mut frame = CreatureFrame::default();
    let id = tekis.spawn(info::PELLET_POSY, 3, Vec3::ZERO, 0.0).unwrap();
    let mut effects = std::collections::BTreeSet::new();
    let mut sounds = std::collections::BTreeSet::new();
    for tick in 0..180 {
        if tick == 1 || tick == 70 { tekis.get_mut(id).unwrap().timers[0] = 0.0; }
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        for out in tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0) {
            match out {
                Out::Effect { id, .. } | Out::EffectDir { id, .. } => { effects.insert(id); }
                Out::TableSound { index, .. } => { sounds.insert(index); }
                _ => {}
            }
        }
    }
    assert_eq!(effects, [70,71,72,73].into_iter().collect());
    assert!(sounds.is_superset(&[2,3,4].into_iter().collect()));
    for size in [0, 1] {
        let id = tekis.spawn(info::PELLET_POSY, 10 + size, Vec3::X * 200.0, 0.0).unwrap();
        tekis.set_count(id, 0, size as i32);
        tekis.damage(id, 1e6, None);
        for _ in 0..40 {
            let out = tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
            assert!(!out.iter().any(|o| matches!(o, Out::SpawnPellets { .. })));
        }
    }
}

#[test]
fn a_pellet_posy_grows_with_time() {
    let Some((mut sim, mut tekis)) = load() else { return };
    let id = tekis.spawn(info::PELLET_POSY, 3, Vec3::ZERO, 0.0).expect("no posy strategy");
    let mut frame = CreatureFrame::default();
    // Each size takes the disc's 120 seconds.
    for _ in 0..(260 * 30) {
        tekis.frame(&mut frame);
        sim.step(&mut Frame(&frame));
        tekis.update(&mut sim, &FlatHost, &[], 1.0 / 30.0);
    }
    assert_eq!(tekis.get(id).unwrap().count[0], 2, "it reached full size");
}
