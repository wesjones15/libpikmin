use libpikmin::{Happa, Params, PikiColor, PikiState, Sim, Snapshot, Vec3};
use libpikmin::host::FlatHost;
use libpikmin::onion_menu::hover_leg_point;

#[test]
fn hovering_call_out_slides_half_a_leg_then_falls_independently() {
    for color in [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow] {
        let mut sim = Sim::new(Params::default(), 21);
        sim.set_auto_rejoin(true);
        let captain = sim.add_captain(Vec3::ZERO);
        let onion = sim.add_onion(Vec3::ZERO, &[color]);
        sim.store_in_onion(onion, color, Happa::Flower, 2);
        let air = Vec3::Y * 300.0;
        sim.set_onion_pose(onion, air, true);
        assert_eq!(sim.onion_call_out(captain, onion, color, 1), 1);
        let mut leg = None;
        let mut detached = None;
        for _ in 0..150 {
            // The travel code measures the ropes with the frame the step starts on; the
            // Onion's own clock advances at the end of the step.
            let phase = sim.onions()[0].hover_frame;
            sim.step(&mut FlatHost);
            let Some(p) = sim.pikis().first() else { continue };
            match p.state {
                PikiState::Exit { leg: l, ratio, .. } => {
                    assert!((0.5..=1.0).contains(&ratio));
                    assert!(!p.on_ground);
                    leg = Some(l);
                }
                PikiState::Flying { .. } => {
                    let l = leg.expect("played the leg slide first");
                    let midpoint = air + hover_leg_point(l, 0.5, phase);
                    assert!(p.pos.distance(midpoint) < 0.01, "detached at the exact leg midpoint");
                    assert!(p.vel.y < 0.0 && !p.on_ground);
                    assert_eq!(p.happa, Happa::Flower);
                    assert_eq!(p.captain, Some(captain));
                    let snap = Snapshot::of(&sim, &[]);
                    let decoded = Snapshot::decode(&snap.encode()).unwrap();
                    assert_eq!(format!("{:?}", snap), format!("{:?}", decoded));
                    detached = Some(p.pos);
                    break;
                }
                other => panic!("unexpected state before detaching: {other:?}"),
            }
        }
        let detached = detached.expect("let go halfway down the leg");
        // The Onion moving again must not drag the released Pikmin through the air.
        sim.set_onion_pose(onion, air + Vec3::X * 1000.0, true);
        sim.step(&mut FlatHost);
        let p = &sim.pikis()[0];
        assert!((p.pos.x - detached.x).abs() < 0.001);
        assert!(p.pos.y < detached.y);
        for _ in 0..300 { sim.step(&mut FlatHost); }
        let p = &sim.pikis()[0];
        assert!(p.on_ground && p.state.in_squad(), "landed and rejoined its captain");
        assert_eq!(sim.onions()[0].stored_total(), 1);
    }
}

#[test]
fn airborne_pending_exits_keep_the_field_limit_and_cannot_be_put_away_before_they_exit() {
    let mut params = Params::default();
    params.ai.max_pikis_on_field = 2;
    let mut sim = Sim::new(params, 22);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    sim.store_in_onion(onion, PikiColor::Red, Happa::Leaf, 5);
    sim.set_onion_pose(onion, Vec3::Y * 300.0, true);
    assert_eq!(sim.onion_call_out(captain, onion, PikiColor::Red, 10), 2);
    assert_eq!(sim.onion_call_out(captain, onion, PikiColor::Red, 10), 0);
    assert_eq!(sim.onion_put_away(captain, onion, PikiColor::Red, 10), 0);
    for _ in 0..300 { sim.step(&mut FlatHost); }
    assert_eq!(sim.pikis().len(), 2);
    assert_eq!(sim.onions()[0].stored_total(), 3);
}

#[test]
fn returning_the_squad_also_returns_held_and_approaching_pikmin() {
    for distance in [1.0, 100.0] {
        let mut sim = Sim::new(Params::default(), 23);
        let captain = sim.add_captain(Vec3::ZERO);
        let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
        let piki = sim.spawn_piki_in_squad(PikiColor::Red, -Vec3::Z * distance, captain).unwrap();
        sim.set_input(captain, libpikmin::CaptainInput { throw: true, ..Default::default() });
        sim.step(&mut FlatHost);
        assert!(matches!(sim.piki(piki).state, PikiState::Held { .. } | PikiState::GoHang { .. }));
        assert_eq!(sim.onion_counts(onion, captain, PikiColor::Red).in_squad, 1);
        assert_eq!(sim.onion_put_away(captain, onion, PikiColor::Red, 100), 1);
        assert!(matches!(sim.piki(piki).state, PikiState::Enter { .. }));
        assert!(matches!(sim.captain(captain).throw, libpikmin::captain::ThrowState::Idle));
        assert!(sim.captain(captain).plate.members.is_empty());
    }
}

#[test]
fn entering_pikmin_climb_the_flying_onions_legs() {
    let mut sim = Sim::new(Params::default(), 24);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    let piki = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::Z * 40.0, captain).unwrap();
    let air = Vec3::Y * 200.0;
    sim.set_onion_pose(onion, air, true);
    assert_eq!(sim.onion_put_away(captain, onion, PikiColor::Red, 1), 1);
    let mut climbed = false;
    for _ in 0..240 {
        let phase = sim.onions()[0].hover_frame;
        sim.step(&mut FlatHost);
        let p = sim.piki(piki);
        if let PikiState::Enter { leg, ratio, climbing: true, .. } = p.state {
            let on_leg = air + hover_leg_point(leg, ratio, phase);
            assert!(p.pos.distance(on_leg) < 0.01, "climbs the flying leg, not the landed one");
            assert!((0.5..=1.0).contains(&ratio));
            climbed = true;
            break;
        }
    }
    assert!(climbed, "the Pikmin boarded the flying Onion");
}

/// A host that mirrors its models (`ModelView.MirrorX`, Unity) draws the Onion's legs on the
/// other side. The sim must walk, jump and climb those drawn legs, not the disc's right-handed
/// ones; otherwise the Pikmin walks to a leg nobody can see and the jump crosses the Onion.
#[test]
fn mirrored_onion_legs_climb_the_hosts_drawn_side() {
    let mut sim = Sim::new(Params::default(), 25);
    let captain = sim.add_captain(Vec3::ZERO);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    sim.set_mirror_onion_legs(true);
    let piki = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::Z * 40.0, captain).unwrap();
    let air = Vec3::Y * 200.0;
    sim.set_onion_pose(onion, air, true);
    assert_eq!(sim.onion_put_away(captain, onion, PikiColor::Red, 1), 1);
    let mut climbed = false;
    for _ in 0..240 {
        let phase = sim.onions()[0].hover_frame;
        sim.step(&mut FlatHost);
        let p = sim.piki(piki);
        if let PikiState::Enter { leg, ratio, climbing: true, .. } = p.state {
            let disc = hover_leg_point(leg, ratio, phase);
            assert!(disc.x.abs() > 1.0, "this leg is not on the mirror axis");
            assert!(p.pos.distance(air + Vec3::new(-disc.x, disc.y, disc.z)) < 0.01,
                "climbs the mirrored leg at {:?}; the disc leg is at {:?}", p.pos, air + disc);
            climbed = true;
            break;
        }
    }
    assert!(climbed, "the Pikmin boarded the Onion");
}
