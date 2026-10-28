use libpikmin::{CaptainInput, Happa, Params, PikiColor, Sim, Vec3};
use libpikmin::host::FlatHost;

#[test]
fn healing_prefers_leaves_then_buds_then_flowers_and_never_touches_squad() {
    let mut sim = Sim::new(Params::default(), 44);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Blue, PikiColor::Red, PikiColor::Yellow]);
    sim.spawn_squad(captain, Vec3::ZERO, 3);
    sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 4);
    sim.store_in_onion(onion, PikiColor::Blue, Happa::Bud, 3);
    sim.store_in_onion(onion, PikiColor::Yellow, Happa::Flower, 2);
    let (count, fraction) = sim.sacrifice_onion_stock(onion, 0.52);
    assert_eq!(count, 8);
    assert!((fraction - 0.52).abs() < 1e-6);
    assert_eq!(sim.onions()[0].stored, [[0, 0, 0], [0, 0, 0], [0, 0, 1], [0, 0, 0], [0, 0, 0]]);
    assert_eq!(sim.captain(captain).plate.len(), 3);
    assert_eq!(sim.sacrifice_onion_stock(onion, 1.0), (0, 0.0));
}

#[test]
fn healing_stops_at_full_health_or_one_stored_pikmin_and_preserves_queued_exits() {
    let mut sim = Sim::new(Params::default(), 45);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 10);
    assert_eq!(sim.sacrifice_onion_stock(onion, 0.0), (0, 0.0));
    assert_eq!(sim.sacrifice_onion_stock(onion, f32::NAN), (0, 0.0));
    let (count, fraction) = sim.sacrifice_onion_stock(onion, 0.1);
    assert_eq!(count, 2);
    assert!((fraction - 0.1).abs() < 1e-6);
    let (count, fraction) = sim.sacrifice_onion_stock(onion, 1.0);
    assert_eq!(count, 7);
    assert!((fraction - 0.35).abs() < 1e-6);
    assert_eq!(sim.onions()[0].stored_total(), 1);
    sim.store_in_onion(onion, PikiColor::Red, Happa::Flower, 4);
    sim.set_onion_pose(onion, Vec3::Y * 300.0, true);
    assert_eq!(sim.onion_call_out(captain, onion, PikiColor::Red, 4), 4);
    assert_eq!(sim.sacrifice_onion_stock(onion, 1.0), (0, 0.0), "four queued flowers and the single leaf survive");
    sim.store_in_onion(onion, PikiColor::Red, Happa::Bud, 2);
    let (count, fraction) = sim.sacrifice_onion_stock(onion, 0.06);
    assert_eq!(count, 2);
    assert!((fraction - 0.06).abs() < 1e-6);
    assert_eq!(sim.onions()[0].stored[1], [0, 1, 4], "queued flowers remain reserved");
    sim.make_onion_dormant(onion, 30.0, Vec3::ZERO);
    assert_eq!(sim.sacrifice_onion_stock(onion, 1.0), (0, 0.0));
}

#[test]
fn requested_host_jump_height_is_reached_by_olimar_and_stationary_followers() {
    for target in [80.0, 150.0, 300.0] {
        let mut sim = Sim::new(Params::default(), 46);
        let captain = sim.add_captain(Vec3::ZERO);
        let piki = sim.spawn_piki_in_squad(PikiColor::Red, -Vec3::X * 40.0, captain).unwrap();
        sim.step(&mut FlatHost);
        sim.set_captain_jump_height(captain, target);
        sim.set_input(captain, CaptainInput { jump: true, ..Default::default() });
        let (mut captain_peak, mut piki_peak) = (0.0f32, 0.0f32);
        for _ in 0..180 {
            sim.step(&mut FlatHost);
            captain_peak = captain_peak.max(sim.captain(captain).pos.y);
            piki_peak = piki_peak.max(sim.piki(piki).pos.y);
        }
        assert!(captain_peak >= target, "captain peak {captain_peak}, target {target}");
        assert!(piki_peak >= target, "Pikmin peak {piki_peak}, target {target}");
    }
}
