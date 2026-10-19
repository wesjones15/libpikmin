use libpikmin::{Event, PikiColor, PikiState, Sim, Snapshot, Vec3};
use libpikmin::host::FlatHost;

#[test]
fn repeated_deaths_reuse_slots_without_losing_the_death_count() {
    let mut sim = Sim::default();
    for generation in 0..5000 {
        let id = sim.spawn_piki(PikiColor::Red, Vec3::ZERO).unwrap();
        assert_eq!(sim.piki(id).generation, generation);
        sim.kill_piki(id);
        // The death animation/ghost still gets its full lifetime.
        sim.reclaim_dead_pikis(std::iter::empty());
        for _ in 0..50 { sim.step(&mut FlatHost); }
        assert!(matches!(sim.piki(id).state, PikiState::Dead));
        let events: Vec<_> = sim.drain_events().collect();
        assert!(events.iter().any(|e| matches!(e, Event::PikiGone { .. })));
        sim.reclaim_dead_pikis(std::iter::empty());
    }
    assert_eq!(sim.pikis().len(), 1);
    assert_eq!(sim.piki_deaths(), 5000);
    let snapshot = Snapshot::decode(&Snapshot::of(&sim, &[]).encode()).unwrap();
    assert_eq!(snapshot.piki_deaths, 5000);
    assert_eq!(snapshot.pikis.len(), 1);
}

#[test]
fn pending_events_protect_dead_slots_and_storage_is_not_a_death() {
    let mut sim = Sim::default();
    let old = sim.spawn_piki(PikiColor::Red, Vec3::ZERO).unwrap();
    // Terminal Onion storage, deliberately isolated from travel animation here.
    sim.piki_mut(old).state = PikiState::Dead;
    sim.reclaim_dead_pikis(std::iter::once(old));
    let other = sim.spawn_piki(PikiColor::Blue, Vec3::ZERO).unwrap();
    assert_ne!(old, other);
    sim.reclaim_dead_pikis(std::iter::empty());
    let reused = sim.spawn_piki(PikiColor::Yellow, Vec3::X * 20.0).unwrap();
    assert_eq!(old, reused);
    assert_eq!(sim.piki(reused).generation, 1);
    assert_eq!(sim.piki_deaths(), 0);
}

#[test]
fn interpolation_is_absolute_and_never_blends_different_slot_occupants() {
    let mut sim = Sim::default();
    let id = sim.spawn_piki(PikiColor::Red, Vec3::ZERO).unwrap();
    let from = Snapshot::of(&sim, &[]);
    sim.piki_mut(id).pos.x = 40.0;
    let to = Snapshot::of(&sim, &[]);
    let mut replica = Sim::default();
    replica.apply_snapshot(&to);
    for t in [0.25, 0.5, 1.0, 1.0] {
        replica.blend_snapshot_motion(&from, &to, t);
        assert_eq!(replica.piki(id).pos.x, 40.0 * t);
    }
    sim.piki_mut(id).state = PikiState::Dead;
    sim.reclaim_dead_pikis(std::iter::empty());
    assert_eq!(sim.spawn_piki(PikiColor::Blue, Vec3::X * 20.0), Some(id));
    let replacement = Snapshot::of(&sim, &[]);
    replica.apply_snapshot_blend(&from, &replacement, 0.0);
    assert_eq!(replica.piki(id).pos.x, 20.0);
}
