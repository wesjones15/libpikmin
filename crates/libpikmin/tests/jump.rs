use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Host, Params, PikiColor, PikiState, Sim, Vec2, Vec3};

fn setup() -> (Sim, libpikmin::CaptainId, libpikmin::PikiId) {
    let mut params = Params::default();
    params.ai.auto_attack = 0;
    let mut sim = Sim::new(params, 21);
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim
        .spawn_piki_in_squad(PikiColor::Red, Vec3::new(-60.0, 0.0, 0.0), c)
        .unwrap();
    sim.step(&mut FlatHost);
    (sim, c, p)
}

#[test]
fn stationary_jump_is_in_place_and_holding_does_not_bunny_hop() {
    let (mut sim, c, p) = setup();
    let start = sim.piki(p).pos;
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            ..Default::default()
        },
    );
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).pos.y > 0.0 && sim.piki(p).pos.y > 0.0);
    assert_eq!(sim.piki(p).pos.with_y(0.0), start.with_y(0.0));
    let mut peak = sim.captain(c).pos.y;
    for _ in 0..90 {
        sim.step(&mut FlatHost);
        peak = peak.max(sim.captain(c).pos.y);
    }
    assert!((30.0..45.0).contains(&peak), "peak {peak}");
    assert!(sim.captain(c).on_ground && sim.captain(c).jump.is_none());
    assert!(sim.piki(p).on_ground && sim.piki(p).jump.is_none());
    sim.set_input(c, CaptainInput::default());
    sim.step(&mut FlatHost);
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            ..Default::default()
        },
    );
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).jump.is_some());
}

#[test]
fn modifier_blocks_jump_and_a_brief_press_is_latched() {
    let (mut sim, c, p) = setup();
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            jump_blocked: true,
            whistle: true,
            ..Default::default()
        },
    );
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).on_ground && sim.piki(p).on_ground);
    assert!(sim.captain(c).jump.is_none());
    // Letting go of the modifier with B held is not a new jump edge.
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            ..Default::default()
        },
    );
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).on_ground);
    sim.set_input(c, CaptainInput::default());
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            ..Default::default()
        },
    );
    sim.set_input(c, CaptainInput::default());
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).jump.is_some());
}

#[test]
fn host_driven_jump_records_the_last_grounded_position() {
    use libpikmin::CaptainPose;
    let (mut sim, c, p) = setup();
    sim.set_auto_rejoin(true);
    sim.set_input(
        c,
        CaptainInput {
            pose: Some(CaptainPose {
                pos: Vec3::new(5.0, 15.0, 0.0),
                vel: Vec3::new(120.0, 200.0, 0.0),
                face: 0.0,
            }),
            ..Default::default()
        },
    );
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).jump.is_some() && !sim.captain(c).on_ground);
    sim.set_input(
        c,
        CaptainInput {
            pose: Some(CaptainPose {
                pos: Vec3::new(150.0, 0.0, 0.0),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let mut launch = None;
    for _ in 0..90 {
        let before = sim.piki(p).pos;
        sim.step(&mut FlatHost);
        if launch.is_none() && sim.piki(p).jump.is_some() {
            launch = Some(before);
        }
    }
    assert!(
        launch.is_some_and(|at| at.with_y(0.0).length() <= 12.1),
        "takeoff {launch:?}"
    );
    assert!(sim.captain(c).on_ground && sim.captain(c).jump.is_none());
}

#[test]
fn followers_jump_at_each_takeoff_point_after_the_captain() {
    let (mut sim, c, p) = setup();
    sim.set_auto_rejoin(true);
    let takeoff = sim.captain(c).pos;
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            stick: Vec2::X,
            ..Default::default()
        },
    );
    sim.step(&mut FlatHost);
    assert!(
        sim.piki(p).jump.is_none(),
        "distant follower jumped too early"
    );
    sim.set_input(
        c,
        CaptainInput {
            stick: Vec2::X,
            ..Default::default()
        },
    );
    let mut launches = Vec::new();
    let mut second_takeoff = None;
    for tick in 0..150 {
        if tick == 30 {
            second_takeoff = Some(sim.captain(c).pos);
            sim.set_input(
                c,
                CaptainInput {
                    jump: true,
                    stick: Vec2::X,
                    ..Default::default()
                },
            );
        } else if tick == 31 {
            sim.set_input(
                c,
                CaptainInput {
                    stick: Vec2::X,
                    ..Default::default()
                },
            );
        }
        let before = sim.piki(p).pos;
        let jumping = sim.piki(p).jump.is_some();
        sim.step(&mut FlatHost);
        if !jumping && sim.piki(p).jump.is_some() {
            launches.push(before);
        }
    }
    assert_eq!(launches.len(), 2, "launches {launches:?}");
    for (from, at) in launches.iter().zip([takeoff, second_takeoff.unwrap()]) {
        assert!(
            (*from - at).with_y(0.0).length() <= 12.1,
            "from {from}, expected {at}"
        );
    }
    assert!(matches!(sim.piki(p).state, PikiState::Squad { .. }));
}

struct Obstacle {
    height: f32,
    gap: bool,
}
impl Host for Obstacle {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        if self.gap && (30.0..55.0).contains(&pos.x) {
            None
        } else {
            (pos.y >= 0.0).then_some(0.0)
        }
    }
    fn walls(&self, centre: Vec3, radius: f32) -> Vec3 {
        if !self.gap
            && centre.y - radius < self.height
            && centre.x + radius > 30.0
            && centre.x < 40.0
        {
            centre.with_x(30.0 - radius)
        } else {
            centre
        }
    }
}

#[test]
fn navigating_pikmin_clear_low_obstacles_and_short_gaps_but_not_tall_walls() {
    for (height, gap, should_cross) in [
        (15.0, false, true),
        (200.0, false, false),
        (0.0, true, true),
    ] {
        let mut sim = Sim::default();
        let c = sim.add_captain(Vec3::new(150.0, 0.0, 0.0));
        let p = sim
            .spawn_piki_in_squad(PikiColor::Red, Vec3::ZERO, c)
            .unwrap();
        let mut host = Obstacle { height, gap };
        let mut jumped = false;
        for _ in 0..150 {
            sim.step(&mut host);
            jumped |= sim.piki(p).jump.is_some();
        }
        assert_eq!(jumped, should_cross, "height {height}, gap {gap}");
        assert_eq!(
            sim.piki(p).pos.x > 55.0,
            should_cross,
            "at {:?}",
            sim.piki(p).pos
        );
    }
}

#[test]
fn carrying_pikmin_do_not_jump_with_the_squad_or_over_obstacles() {
    use libpikmin::carry::{CarryPhase, PelletDesc};
    let (mut sim, c, p) = setup();
    let pel = sim.spawn_pellet(PelletDesc::numbered(
        sim.params(),
        1,
        PikiColor::Red,
        Vec3::new(-60.0, 0.0, 0.0),
        0.0,
    ));
    sim.add_onion(Vec3::new(150.0, 0.0, 0.0), &[PikiColor::Red]);
    assert!(sim.recruit_to_pellet(pel, None, 100.0) > 0);
    for _ in 0..50 {
        sim.step(&mut FlatHost);
    }
    assert!(
        matches!(sim.piki(p).state, PikiState::Carry(c) if !matches!(c.phase, CarryPhase::Go { .. }))
    );
    sim.set_input(
        c,
        CaptainInput {
            jump: true,
            ..Default::default()
        },
    );
    let mut host = Obstacle {
        height: 15.0,
        gap: false,
    };
    for _ in 0..90 {
        sim.step(&mut host);
        assert!(sim.piki(p).jump.is_none());
    }
}

// Host extension regression: a pad targets a platform far above a voluntary jump's apex.
struct PadPlatform;
impl Host for PadPlatform {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        let floor = if pos.x >= 300.0 { 300.0 } else { 0.0 };
        (pos.y >= floor).then_some(floor)
    }
}

#[test]
fn jump_pad_squad_repeats_full_trajectory_and_reaches_high_platform() {
    let (mut sim, c, p) = setup();
    let other = sim.add_captain(Vec3::new(-100.0, 0.0, 0.0));
    let other_p = sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(-100.0, 0.0, 0.0), other).unwrap();
    let v = Vec3::new(180.0, 400.0, 0.0);
    sim.launch_captain(c, v, 200.0, true);
    let mut captain_arc = Vec::new();
    let mut piki_arc = Vec::new();
    let mut launched = false;
    let mut landed = false;
    for _ in 0..180 {
        sim.step(&mut PadPlatform);
        if sim.captain(c).jump.is_some() { captain_arc.push(sim.captain(c).pos); }
        if sim.piki(p).jump.is_some() {
            launched = true;
            piki_arc.push(sim.piki(p).pos);
        } else if launched {
            landed = true;
            break;
        }
        assert!(sim.piki(other_p).jump.is_none(), "other captain's squad must stay behind");
    }
    assert!(launched && landed, "follower must launch and land");
    assert!(captain_arc.len() > 30 && piki_arc.len() == captain_arc.len());
    for (a, b) in captain_arc.iter().zip(&piki_arc) {
        assert!((*a - *b).length() < 0.02, "captain {a}, follower {b}");
    }
    assert_eq!(sim.captain(c).pos.y, 300.0);
    assert_eq!(sim.piki(p).pos.y, 300.0);
    assert!(sim.piki(p).pos.x >= 300.0);
}

#[test]
fn crowd_control_stops_input_but_not_external_launches() {
    let (mut sim, c, p) = setup();
    sim.set_captain_hold(c, true, true);
    let pos = sim.captain(c).pos;
    sim.set_input(c, CaptainInput { stick: Vec2::X, jump: true, throw: true, dismiss: true,
        ..Default::default() });
    for _ in 0..15 { sim.step(&mut FlatHost); }
    assert_eq!(sim.captain(c).pos, pos);
    assert!(matches!(sim.piki(p).state, PikiState::Squad { .. }));
    sim.launch_captain(c, Vec3::new(100.0, 300.0, 0.0), 200.0, false);
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).pos.x > 3.0 && sim.captain(c).pos.y > 9.0);
    assert!(sim.piki(p).jump.is_none(), "knockback must not launch the squad");
    sim.launch_captain(c, Vec3::splat(f32::NAN), 200.0, true);
    sim.launch_captain(libpikmin::CaptainId(u32::MAX), Vec3::ZERO, 200.0, true);
    assert!(sim.captain(c).vel.is_finite());
    for _ in 0..100 { sim.step(&mut FlatHost); }
    sim.set_captain_hold(c, false, false);
    sim.set_input(c, CaptainInput { stick: Vec2::X, ..Default::default() });
    sim.step(&mut FlatHost);
    assert!(sim.captain(c).vel.x > 0.0, "movement resumes after hold");
}

#[test]
fn root_holds_movement_and_jump_but_allows_throwing() {
    let (mut sim, c, p) = setup();
    sim.set_captain_hold(c, true, false);
    let pos = sim.captain(c).pos;
    sim.set_input(c, CaptainInput { stick: Vec2::X, jump: true, throw: true,
        ..Default::default() });
    for _ in 0..30 { sim.step(&mut FlatHost); }
    assert_eq!(sim.captain(c).pos, pos);
    assert!(!matches!(sim.piki(p).state, PikiState::Squad { .. }), "rooted captain can throw");
}

#[test]
fn vertical_pad_sends_followers_to_the_pad_before_launching() {
    let (mut sim, c, p) = setup();
    sim.launch_captain(c, Vec3::new(0.0, 400.0, 0.0), 200.0, true);
    sim.step(&mut FlatHost);
    assert!(sim.piki(p).jump.is_none(), "distant follower must run to the vertical pad");
    let mut launched = false;
    for _ in 0..60 {
        sim.step(&mut FlatHost);
        if sim.piki(p).jump.is_some() {
            launched = true;
            assert!(sim.piki(p).pos.with_y(0.0).length() < 0.01);
            assert!(sim.piki(p).vel.y > 300.0);
            break;
        }
    }
    assert!(launched, "follower at {:?}, state {:?}, captain {:?}", sim.piki(p).pos, sim.piki(p).state, sim.captain(c).pos);
}

// A host pad can report its throw on several ticks in a row while the captain still overlaps
// its volume. Followers must repeat that one takeoff, then stay with their captain on landing,
// not walk back to the pad for the extra reports.
#[test]
fn repeated_pad_reports_launch_followers_once_and_they_stay_with_the_captain() {
    let (mut sim, c, p) = setup();
    let v = Vec3::new(180.0, 400.0, 0.0);
    for _ in 0..5 {
        sim.launch_captain(c, v, 200.0, true);
        sim.step(&mut FlatHost);
    }
    let mut takeoffs = 0;
    let mut launch_events = 0;
    let mut was_airborne = false;
    let mut landed_at = None;
    let mut farthest_after_landing = 0.0f32;
    for _ in 0..900 {
        sim.step(&mut FlatHost);
        launch_events += sim
            .drain_events()
            .filter(|e| matches!(e, libpikmin::Event::PikiLaunched { piki, captain } if *piki == p && *captain == c))
            .count();
        let airborne = sim.piki(p).jump.is_some();
        if airborne && !was_airborne {
            takeoffs += 1;
        }
        was_airborne = airborne;
        if takeoffs > 0 && !airborne && landed_at.is_none() {
            landed_at = Some(sim.piki(p).pos);
        }
        if landed_at.is_some() {
            farthest_after_landing = farthest_after_landing.max((sim.piki(p).pos - sim.captain(c).pos).with_y(0.0).length());
        }
    }
    let landed = landed_at.expect("follower must land");
    assert_eq!(takeoffs, 1, "one pad throw is one takeoff, not one per report");
    assert_eq!(launch_events, 1, "the follower's pad takeoff is announced once, for its voice");
    assert!(
        farthest_after_landing < 150.0,
        "follower left its captain after landing (landed {landed}, captain {}, farthest {farthest_after_landing})",
        sim.captain(c).pos
    );
}

fn landings(sim: &mut Sim) -> Vec<(f32, bool)> {
    sim.drain_events()
        .filter_map(|e| match e {
            libpikmin::Event::CaptainLanded { drop, stomp, .. } => Some((drop, stomp)),
            _ => None,
        })
        .collect()
}

#[test]
fn landing_reports_the_fall_from_the_peak() {
    let (mut sim, c, _) = setup();
    let _ = landings(&mut sim);
    // A jump falls back as far as it rose.
    sim.set_input(c, CaptainInput { jump: true, ..Default::default() });
    sim.step(&mut FlatHost);
    sim.set_input(c, CaptainInput::default());
    let mut peak = sim.captain(c).pos.y;
    let mut seen = Vec::new();
    for _ in 0..90 {
        sim.step(&mut FlatHost);
        peak = peak.max(sim.captain(c).pos.y);
        seen.extend(landings(&mut sim));
    }
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!((seen[0].0 - peak).abs() < 1.0 && !seen[0].1, "{seen:?} peak {peak}");

    // Thrown up high by a pad, he falls from its apex, not from where he started.
    sim.launch_captain(c, Vec3::new(0.0, 600.0, 0.0), 0.0, false);
    let mut apex = 0.0f32;
    let mut seen = Vec::new();
    for _ in 0..300 {
        sim.step(&mut FlatHost);
        apex = apex.max(sim.captain(c).pos.y);
        seen.extend(landings(&mut sim));
    }
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(apex > 300.0 && (seen[0].0 - apex).abs() < 1.0, "{seen:?} apex {apex}");

    // Standing still reports nothing.
    for _ in 0..30 {
        sim.step(&mut FlatHost);
    }
    assert!(landings(&mut sim).is_empty());
}

#[test]
fn a_host_placed_captain_reports_no_fall() {
    use libpikmin::CaptainPose;
    let (mut sim, c, _) = setup();
    let _ = landings(&mut sim);
    let pose = |y: f32| CaptainInput {
        pose: Some(CaptainPose { pos: Vec3::new(0.0, y, 0.0), ..Default::default() }),
        ..Default::default()
    };
    sim.set_input(c, pose(5000.0));
    sim.step(&mut FlatHost);
    sim.set_input(c, pose(0.0));
    sim.step(&mut FlatHost);
    sim.set_input(c, CaptainInput::default());
    for _ in 0..10 {
        sim.step(&mut FlatHost);
    }
    let seen = landings(&mut sim);
    assert!(seen.iter().all(|(d, _)| *d < 1.0), "{seen:?}");
}
