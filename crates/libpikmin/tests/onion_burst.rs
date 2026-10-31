use libpikmin::{Happa, Params, PikiColor, PikiState, Sim, Vec3};

// Onion Ride's sneeze: the stored Pikmin leave in one step, a third down each leg.
#[test]
fn burst_out_releases_a_third_down_each_leg_at_once() {
    let mut sim = Sim::new(Params::default(), 9);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &PikiColor::EVERY);
    sim.store_in_onion(onion, PikiColor::Red, Happa::Flower, 6);
    assert_eq!(sim.onion_burst_out(captain, onion, PikiColor::Red, 6), 6);
    assert_eq!(sim.onions()[0].stored_total(), 0);
    let mut legs = [0; 3];
    for p in sim.pikis().iter().filter(|p| p.is_alive()) {
        if let PikiState::Exit { leg, .. } = p.state { legs[leg as usize] += 1; }
    }
    assert_eq!(legs, [2, 2, 2]);
    // Asking for more than it holds takes only what is there.
    assert_eq!(sim.onion_burst_out(captain, onion, PikiColor::Red, 3), 0);
}

#[test]
fn cancelled_entries_rejoin_the_squad_and_store_nothing() {
    let mut sim = Sim::new(Params::default(), 9);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &PikiColor::EVERY);
    let id = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::ZERO, captain).unwrap();
    sim.onion_put_away(captain, onion, PikiColor::Red, 1);
    assert!(matches!(sim.piki(id).state, PikiState::Enter { .. }));
    assert_eq!(sim.onion_cancel_entries(captain, onion), 1);
    for _ in 0..300 { sim.step(&mut libpikmin::host::FlatHost); }
    assert_eq!(sim.onions()[0].stored_total(), 0);
    assert!(sim.piki(id).is_alive());
}
