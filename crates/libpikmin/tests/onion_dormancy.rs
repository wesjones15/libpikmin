use libpikmin::{Params, Sim, Vec3, PikiColor, PelletDesc, PelletState};
use libpikmin::host::FlatHost;

#[test]
fn loose_pickup_requires_descent_and_works_without_any_pikmin() {
    let mut s = Sim::new(Params::default(), 1);
    let o = s.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    let p = s.spawn_pellet(PelletDesc::numbered(s.params(), 1, PikiColor::Red, Vec3::ZERO, 0.0));
    s.set_onion_pose(o, Vec3::Y * 125.0, true);
    assert!(!s.collect_ground_pellet(o, p));
    s.set_onion_pose(o, Vec3::Y * 40.0, true);
    assert!(s.collect_ground_pellet(o, p));
    assert!(!s.collect_ground_pellet(o, p), "no duplicate pickup");
    for _ in 0..150 { s.step(&mut FlatHost); }
    assert_eq!(s.pellet(p).state, PelletState::Gone);
    assert_eq!(s.onions()[0].stored_total(), 2);
}

#[test]
fn dormancy_locks_stock_ignores_pellets_and_survives_snapshots() {
    let mut s = Sim::new(Params::default(), 2);
    let c = s.add_captain(Vec3::ZERO);
    let o = s.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    s.store_in_onion(o, PikiColor::Red, libpikmin::Happa::Leaf, 5);
    let p = s.spawn_pellet(PelletDesc::numbered(s.params(), 1, PikiColor::Red, Vec3::ZERO, 0.0));
    s.spawn_squad(c, Vec3::ZERO, 3);
    s.make_onion_dormant(o, 2.0, Vec3::ZERO);
    assert!(!s.onions()[0].grounded());
    assert!(!s.onions()[0].takes_deliveries());
    assert_eq!(s.onion_call_out(c, o, PikiColor::Red, 5), 0);
    assert_eq!(s.recruit_to_pellet(p, None, 1000.0), 0);
    assert!(!s.collect_ground_pellet(o, p));
    s.set_onion_pose(o, Vec3::Y * 1000.0, true);
    assert_eq!(s.onions()[0].pos, Vec3::ZERO, "flight cannot reactivate a dormant Onion");
    for _ in 0..45 { s.step(&mut FlatHost); }
    let d = s.onions()[0].dormancy.unwrap();
    assert!(d.progress < 0.001 && !d.reviving);
    assert!(d.remaining > 0.0);
    // Lifecycle clocks are authoritative replica state, not local host timers.
    let snap = libpikmin::Snapshot::of(&s, &[]);
    let decoded = libpikmin::Snapshot::decode(&snap.encode()).unwrap();
    assert_eq!(format!("{:?}", snap), format!("{:?}", decoded));
    for _ in 0..120 { s.step(&mut FlatHost); }
    assert!(s.onions()[0].dormancy.is_none(), "revived after timer plus startup");
    assert!(s.onions()[0].grounded());
    assert_eq!(s.onions()[0].stored_total(), 5);
}

#[test]
fn disabling_the_destination_releases_an_existing_carry_team() {
    let mut s = Sim::new(Params::default(), 6);
    let c = s.add_captain(Vec3::ZERO);
    let o = s.add_onion(Vec3::X * 3000.0, &[PikiColor::Red]);
    let p = s.spawn_pellet(PelletDesc::numbered(s.params(), 1, PikiColor::Red, Vec3::ZERO, 0.0));
    s.spawn_squad(c, Vec3::ZERO, 3);
    // Start from an authoritative in-flight delivery, without testing approach AI here.
    let mut snap = libpikmin::Snapshot::of(&s, &[]);
    let worker = snap.pikis[0].id;
    snap.pellets[0].carriers.push(worker);
    snap.pellets[0].slots[0] = Some(worker);
    snap.pellets[0].state = PelletState::Carried {
        onion: o, color: PikiColor::Red, path: vec![Vec3::X * 3000.0], next: 0,
    };
    snap.pikis[0].state = libpikmin::PikiState::Carry(libpikmin::carry::Carry {
        pellet: p, slot: 0, phase: libpikmin::carry::CarryPhase::Move,
    });
    s.apply_snapshot(&snap);
    s.make_onion_dormant(o, 30.0, Vec3::X * 3000.0);
    for _ in 0..15 { s.step(&mut FlatHost); }
    assert_eq!(s.pellet(p).state, PelletState::Idle);
    assert!(s.pellet(p).carriers.is_empty());
    assert!(!s.pikis().iter().any(|p| matches!(p.state, libpikmin::PikiState::Carry(_))));
}

#[test]
fn interrupted_suction_preserves_the_pellet_and_has_no_seed_reward() {
    let mut s = Sim::new(Params::default(), 3);
    let o = s.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    let p = s.spawn_pellet(PelletDesc::numbered(s.params(), 1, PikiColor::Red, Vec3::ZERO, 0.0));
    s.set_onion_pose(o, Vec3::Y * 40.0, true);
    assert!(s.collect_ground_pellet(o, p));
    for _ in 0..50 { s.step(&mut FlatHost); }
    s.make_onion_dormant(o, 30.0, Vec3::ZERO);
    for _ in 0..100 { s.step(&mut FlatHost); }
    assert_eq!(s.pellet(p).state, PelletState::Idle);
    assert_eq!(s.pellet(p).scale, 1.0);
    assert_eq!(s.onions()[0].stored_total(), 0);
}

#[test]
fn a_moving_onion_carries_the_pellet_in_its_beam() {
    let mut s = Sim::new(Params::default(), 3);
    let o = s.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    let p = s.spawn_pellet(PelletDesc::numbered(s.params(), 1, PikiColor::Red, Vec3::ZERO, 0.0));
    s.set_onion_pose(o, Vec3::Y * 40.0, true);
    assert!(s.collect_ground_pellet(o, p));
    s.step(&mut FlatHost);
    // During the 1.5 s settle the pellet stays in the beam, so the Onion can pull it along.
    let before = s.pellet(p).pos;
    s.set_onion_pose(o, Vec3::new(100.0, 40.0, 0.0), true);
    s.step(&mut FlatHost);
    assert!((s.pellet(p).pos.x - before.x - 100.0).abs() < 1.0, "pellet stayed behind at {}", s.pellet(p).pos.x);
    // And while rising, the whole path moves with it instead of cutting across diagonally.
    for _ in 0..30 { s.step(&mut FlatHost); }
    let before = s.pellet(p).pos;
    s.set_onion_pose(o, Vec3::new(100.0, 40.0, 80.0), true);
    s.step(&mut FlatHost);
    assert!((s.pellet(p).pos.z - before.z - 80.0).abs() < 1.0, "rising pellet stayed behind at {}", s.pellet(p).pos.z);
}

#[test]
fn queued_seeds_resume_after_revival() {
    let mut s = Sim::new(Params::default(), 8);
    let o = s.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    s.give_onion_seeds(o, PikiColor::Red, 2);
    s.make_onion_dormant(o, 0.0, Vec3::ZERO);
    for _ in 0..90 { s.step(&mut FlatHost); }
    assert_eq!(s.sprouts().count(), 0, "no emission while disabled");
    for _ in 0..240 { s.step(&mut FlatHost); }
    assert!(s.onions()[0].queue.is_empty());
    assert_eq!(s.sprouts().count(), 2, "pending seed reward was not stranded");
}
