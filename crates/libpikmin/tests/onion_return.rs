use libpikmin::{Happa, Params, PikiColor, PikiState, Sim, Snapshot, Vec3};
use libpikmin::host::FlatHost;

#[test]
fn hovering_fill_jumps_to_half_a_leg_then_climbs_and_keeps_all_five_colours_flowered() {
    let mut sim = Sim::new(Params::default(), 9);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::Y * 100.0, &PikiColor::EVERY);
    sim.set_onion_pose(onion, Vec3::Y * 100.0, true);
    for colour in PikiColor::EVERY {
        let id = sim.spawn_piki_in_squad(colour, Vec3::ZERO, captain).unwrap();
        sim.piki_mut(id).happa = Happa::Flower;
        assert_eq!(sim.onion_put_away(captain, onion, colour, 1), 1);
    }
    let (mut jumping, mut climbing) = (false, false);
    for _ in 0..600 {
        sim.step(&mut FlatHost);
        for p in sim.pikis() {
            if let PikiState::Enter { climbing: false, ratio, .. } = p.state {
                if ratio < 0.0 {
                    jumping = true;
                    assert!(!p.on_ground);
                    let snapshot = Snapshot::of(&sim, &[]);
                    assert!(Snapshot::decode(&snapshot.encode()).is_some());
                }
            }
            if let PikiState::Enter { climbing: true, ratio, .. } = p.state {
                climbing = true;
                assert!(ratio >= 0.5, "flying returns only climb the upper half");
            }
        }
    }
    assert!(jumping && climbing);
    assert_eq!(sim.onions()[0].stored_total(), 5);
    for colour in PikiColor::EVERY { assert_eq!(sim.onions()[0].stored[colour.index()][Happa::Flower as usize], 1); }
    assert_eq!(sim.piki_deaths(), 0, "putting away is not a casualty");
}

#[test]
fn death_recall_cancels_jobs_and_timeout_preserves_unreachable_living_pikmin() {
    let mut sim = Sim::new(Params::default(), 9);
    let captain = sim.add_captain(Vec3::ZERO);
    let other = sim.add_captain(Vec3::X * 500.0);
    let onion = sim.add_onion(Vec3::ZERO, &PikiColor::EVERY);
    for colour in PikiColor::EVERY {
        let id = sim.spawn_piki_in_squad(colour, Vec3::X * 1000.0, captain).unwrap();
        sim.piki_mut(id).happa = Happa::Flower;
        sim.piki_mut(id).state = PikiState::Flying { ground_frames: 0 };
    }
    let guest = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::ZERO, other).unwrap();
    sim.down_captain(captain);
    assert_eq!(sim.send_orphans_home(captain, onion), 5, "busy Pikmin also return");
    for _ in 0..90 { sim.step(&mut FlatHost); }
    let before = format!("{:?}", sim.pikis()[0].state);
    assert_eq!(sim.send_orphans_home(captain, onion), 0, "repeated recall must not restart a climb");
    assert_eq!(before, format!("{:?}", sim.pikis()[0].state));
    sim.finish_orphan_recall(captain, onion);
    assert_eq!(sim.onions()[0].stored_total(), 5);
    assert!(sim.piki(guest).is_alive());
    sim.make_onion_dormant(onion, 99999.0, Vec3::ZERO);
    assert!(sim.onions()[0].dormancy.is_some());
    assert_eq!(sim.piki_deaths(), 0);
}
