//! A captain who goes down leaves his Pikmin behind; the host can hand them to another captain or send them
//! home to their Onion.

use libpikmin::{Host, Params, PikiColor, Sim, Vec3};

struct Flat;

impl Host for Flat {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }
}

fn scene() -> (Sim, libpikmin::CaptainId, libpikmin::CaptainId, libpikmin::OnionId) {
    let mut sim = Sim::new(Params::default(), 3);
    let a = sim.add_captain(Vec3::ZERO);
    let b = sim.add_captain(Vec3::new(200.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::new(-300.0, 0.0, 0.0), &PikiColor::ALL);
    for i in 0..3 {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-20.0 - 10.0 * i as f32, 0.0, 0.0), a).unwrap();
    }
    for _ in 0..10 {
        sim.step(&mut Flat);
    }
    sim.down_captain(a);
    sim.step(&mut Flat);
    (sim, a, b, onion)
}

#[test]
fn orphans_join_another_captains_squad() {
    let (mut sim, a, b, _) = scene();
    assert!(sim.pikis().iter().all(|p| !p.state.in_squad()), "his squad is left behind when he goes down");
    assert_eq!(sim.adopt_orphans(a, b), 3);
    assert!(sim.pikis().iter().all(|p| p.captain == Some(b) && p.state.in_squad()));
}

#[test]
fn a_downed_captain_cannot_adopt() {
    let (mut sim, a, b, _) = scene();
    sim.down_captain(b);
    assert_eq!(sim.adopt_orphans(a, b), 0);
}

#[test]
fn orphans_walk_into_their_onion() {
    let (mut sim, a, _, onion) = scene();
    assert_eq!(sim.send_orphans_home(a, onion), 3);
    for _ in 0..30 * 40 {
        sim.step(&mut Flat);
    }
    assert!(sim.pikis().iter().all(|p| !p.is_alive() || !matches!(p.state, libpikmin::PikiState::Idle)), "none stand about");
    let stored: u32 = sim.onions()[onion.0 as usize].stored_total();
    assert_eq!(stored, 3, "all three are in the Onion");
}

#[test]
fn a_dormant_onion_wakes_when_told_to() {
    let (mut sim, _, _, onion) = scene();
    sim.make_onion_dormant(onion, 9999.0, Vec3::new(-300.0, 0.0, 0.0));
    for _ in 0..60 {
        sim.step(&mut Flat);
    }
    assert!(sim.onions()[onion.0 as usize].dormancy.is_some_and(|d| !d.reviving));
    sim.set_onion_dormancy_seconds(onion, 0.0);
    for _ in 0..30 * 20 {
        sim.step(&mut Flat);
    }
    assert!(sim.onions()[onion.0 as usize].dormancy.is_none(), "it revived");
}
