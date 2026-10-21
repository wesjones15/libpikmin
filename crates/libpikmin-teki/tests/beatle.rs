//! The Cannon Beetle shoots a boulder, and the boulder rolls over what is in its way.

use glam::Vec3;
use libpikmin::host::{CreatureFrame, FlatHost};
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_teki::{Navi, Out, Tekis};

fn setup() -> Option<(Sim, Tekis)> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/.all").exists() {
        eprintln!("SKIP: the enemy files are not extracted");
        return None;
    }
    Some((Sim::new(Params::default(), 1), Tekis::load(&assets)))
}

#[test]
fn a_cannon_beetle_shoots_a_boulder_at_a_player_in_range() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    let id = tekis.spawn(libpikmin_teki::info::CANNON_BEETLE, 3, Vec3::ZERO, 0.0).expect("no Beetle strategy");
    let player = Navi { id: 5, pos: Vec3::new(0.0, 0.0, 150.0), size: 8.0, alive: true };
    let (mut states, mut shot) = (std::collections::BTreeSet::new(), None);
    for _ in 0..(60 * 30) {
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        for o in out {
            if let Out::Spawn { kind, pos, velocity, .. } = o {
                if kind == libpikmin_teki::info::IWAGON {
                    shot.get_or_insert((pos, velocity));
                }
            }
        }
        states.insert(tekis.get(id).unwrap().state);
        sim.step(&mut FlatHost);
        let _ = CreatureFrame::default();
    }
    assert!(states.contains(&8), "it attacked: {states:?}");
    let (_, v) = shot.expect("it shot a boulder");
    assert!(v.length() > 50.0, "{v:?}");
}

#[test]
fn a_boulder_rolls_and_hits_a_player_in_its_path() {
    let Some((mut sim, mut tekis)) = setup() else { return };
    let id = tekis.spawn(libpikmin_teki::info::IWAGON, 4, Vec3::new(0.0, 0.0, 0.0), 0.0).expect("no Iwagon strategy");
    tekis.get_mut(id).unwrap().spawn_vel = Vec3::new(0.0, 0.0, 200.0);
    let player = Navi { id: 9, pos: Vec3::new(0.0, 0.0, 120.0), size: 8.0, alive: true };
    let mut hit = false;
    for _ in 0..(10 * 30) {
        let out = tekis.update(&mut sim, &FlatHost, &[player], 1.0 / 30.0);
        hit |= out.iter().any(|o| matches!(o, Out::HitNavi { navi: 9, .. }));
        sim.step(&mut FlatHost);
    }
    assert!(hit, "it rolled into the player");
}
