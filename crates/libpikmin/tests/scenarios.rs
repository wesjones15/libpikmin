//! Headless scenario tests: whole-sim behaviour against the decomp's rules.

use libpikmin::captain::ThrowState;
use libpikmin::host::{CreatureFrame, CreatureId, CreaturePart};
use libpikmin::piki::AttackMode;
use libpikmin::{
    CaptainId, CaptainInput, Event, Host, Params, PikiColor, PikiId, PikiState, Sim, TICK_DT, Vec2,
    Vec3,
};

struct Dummy {
    id: CreatureId,
    pos: Vec3,
    radius: f32,
    height: f32,
    hp: f32,
}

#[derive(Default)]
struct TestHost {
    creatures: Vec<Dummy>,
    hits: Vec<(CreatureId, f32)>,
}

impl Host for TestHost {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        for c in &self.creatures {
            let part = CreaturePart {
                centre: c.pos + Vec3::new(0.0, c.height, 0.0),
                radius: c.radius,
                stickable: true,
            };
            out.push(c.id, c.pos, 0.0, c.hp > 0.0, false, &[part]);
        }
    }

    fn damage(
        &mut self,
        creature: CreatureId,
        amount: f32,
        _part: Option<u32>,
        _from: PikiId,
    ) -> bool {
        let Some(c) = self.creatures.iter_mut().find(|c| c.id == creature) else {
            return false;
        };
        if c.hp <= 0.0 {
            return false;
        }
        c.hp -= amount;
        self.hits.push((creature, amount));
        true
    }
}

fn run(sim: &mut Sim, host: &mut TestHost, secs: f32, events: &mut Vec<Event>) {
    let ticks = (secs / TICK_DT).round() as u32;
    for _ in 0..ticks {
        sim.step(host);
        events.extend(sim.drain_events());
    }
}

/// Retail-like settings for the bits these tests care about (values from the user's disc;
/// see docs/behavior-spec.md), so the tests exercise the shipped behaviour.
fn retail_params() -> Params {
    let mut p = Params::default();
    p.navi.whistle_max_radius = 90.0;
    p.navi.whistle_expand_time = 0.45;
    p.navi.whistle_hold_time = 1.0;
    p.navi.run_speed = 160.0;
    p.navi.throw_min_height = 75.0;
    p.navi.throw_max_height = 75.0;
    p.navi.cursor_max_radius = 100.0;
    p.navi.cursor_move_speed = 300.0;
    p.navi.cursor_move_stick_threshold = 0.65;
    p.navi.pluck_grab_range = 15.0;
    p.navi.body_collision_radius = 8.0;
    p.navi_creature.acceleration = 0.1;
    p.piki.max_leaf_move_speed = 120.0;
    p.piki.min_move_speed = 10.0;
    p.piki.idle_attack_search_range = 70.0;
    p.piki.min_piki_size = 3.0;
    p.piki.max_piki_size = 5.0;
    p.piki_creature.acceleration = 0.14;
    p.plate.start_offset = 17.5;
    p.plate.length_limit = 90.0;
    p.plate.max_pos_size = 7.0;
    p.ai.gravity = 560.0;
    p.ai.auto_attack = 0;
    p.ai.return_after_attack = 0;
    p.ai.attack_once = 0;
    p
}

fn squad(sim: &mut Sim, captain: CaptainId, n: u32, around: Vec3) -> Vec<PikiId> {
    (0..n)
        .map(|i| {
            let a = i as f32 * 0.7;
            let pos = around + Vec3::new(a.sin() * 20.0, 0.0, a.cos() * 20.0 - 30.0);
            sim.spawn_piki_in_squad(PikiColor::Red, pos, captain)
                .unwrap()
        })
        .collect()
}

/// Light tilt (below the walk threshold) moves only the cursor, then release the stick.
fn aim_cursor(sim: &mut Sim, host: &mut TestHost, c: CaptainId, dir: Vec2, secs: f32) {
    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            stick: dir.normalize() * 0.5,
            ..Default::default()
        },
    );
    run(sim, host, secs, &mut ev);
    sim.set_input(c, CaptainInput::default());
}

#[test]
fn squad_follows_walking_captain() {
    let mut sim = Sim::new(retail_params(), 7);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 20, Vec3::ZERO);

    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            stick: Vec2::new(0.0, 1.0),
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 4.0, &mut ev);
    sim.set_input(c, CaptainInput::default());
    run(&mut sim, &mut host, 3.0, &mut ev);

    let cap = sim.captain(c);
    assert!(
        cap.pos.z > 500.0,
        "captain should have walked, at {:?}",
        cap.pos
    );
    for p in sim.pikis() {
        assert!(matches!(p.state, PikiState::Squad { .. }), "{:?}", p.state);
        let d = (p.pos - cap.pos).length();
        assert!(d < 130.0, "piki {:?} straggling {d} away", p.id);
        assert!(
            p.pos.z < cap.pos.z + 5.0,
            "piki {:?} should be behind the captain",
            p.id
        );
    }
}

#[test]
fn whistle_only_calls_after_full_expansion_and_within_radius() {
    let mut sim = Sim::new(retail_params(), 1);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    let near = sim
        .spawn_piki(PikiColor::Blue, Vec3::new(60.0, 0.0, 0.0))
        .unwrap();
    let far = sim
        .spawn_piki(PikiColor::Blue, Vec3::new(140.0, 0.0, 0.0))
        .unwrap();

    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            whistle: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 0.3, &mut ev);
    assert!(
        matches!(sim.piki(near).state, PikiState::Idle),
        "nothing is called while the circle grows"
    );

    run(&mut sim, &mut host, 0.5, &mut ev);
    assert!(sim.piki(near).state.in_squad(), "within 90 of the cursor");
    assert!(
        matches!(sim.piki(far).state, PikiState::Idle),
        "outside the circle"
    );
}

#[test]
fn whistle_steals_from_another_captain() {
    let mut sim = Sim::new(retail_params(), 3);
    let mut host = TestHost::default();
    let a = sim.add_captain(Vec3::ZERO);
    let b = sim.add_captain(Vec3::new(0.0, 0.0, -40.0));
    let mine = squad(&mut sim, a, 5, Vec3::ZERO);

    let mut ev = Vec::new();
    run(&mut sim, &mut host, 1.0, &mut ev);
    sim.set_input(
        b,
        CaptainInput {
            whistle: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 0.6, &mut ev);

    let stolen: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Recruited {
                piki,
                captain,
                stolen_from: Some(from),
            } if *captain == b && *from == a => Some(*piki),
            _ => None,
        })
        .collect();
    assert!(
        !stolen.is_empty(),
        "b's whistle should take Pikmin from a's squad"
    );
    for id in stolen {
        assert!(mine.contains(&id));
        assert_eq!(sim.piki(id).captain, Some(b));
        assert!(!sim.captain(a).plate.members.contains(&id));
    }
}

#[test]
fn thrown_piki_lands_at_the_cursor() {
    let mut sim = Sim::new(retail_params(), 5);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    let ids = squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);

    aim_cursor(&mut sim, &mut host, c, Vec2::new(1.0, 0.0), 0.5);
    let cursor = sim.captain(c).cursor_world;
    assert!(
        (cursor - sim.captain(c).pos).length() > 80.0,
        "cursor should be out near max radius"
    );

    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 1.0, &mut ev); // a squad member walks to the hand
    assert!(matches!(sim.captain(c).throw, ThrowState::Wait { .. }));
    sim.set_input(c, CaptainInput::default());
    ev.clear();
    run(&mut sim, &mut host, 2.0, &mut ev);

    let thrown = ev.iter().find_map(|e| match e {
        Event::Thrown { piki, .. } => Some(*piki),
        _ => None,
    });
    let thrown = thrown.expect("a Pikmin should have been thrown");
    assert!(ids.contains(&thrown));
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Landed { piki } if *piki == thrown))
    );
    let landed_at = sim.piki(thrown).pos;
    // throwPiki aims a 1 s parabola at the cursor; the 10-unit release height adds a little range.
    assert!(
        (landed_at - cursor).length() < 25.0,
        "landed at {landed_at:?}, cursor was {cursor:?}"
    );
}

fn creature_at_cursor(sim: &mut Sim, host: &mut TestHost, c: CaptainId, hp: f32) -> CreatureId {
    aim_cursor(sim, host, c, Vec2::new(1.0, 0.0), 0.5);
    let at = sim.captain(c).cursor_world;
    host.creatures.push(Dummy {
        id: 42,
        pos: Vec3::new(at.x, 0.0, at.z),
        radius: 18.0,
        height: 18.0,
        hp,
    });
    42
}

fn throw_once(sim: &mut Sim, host: &mut TestHost, c: CaptainId, ev: &mut Vec<Event>) {
    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    run(sim, host, 1.0, ev);
    sim.set_input(c, CaptainInput::default());
    run(sim, host, 0.1, ev);
}

#[test]
fn thrown_piki_latches_on_and_keeps_hitting() {
    let mut sim = Sim::new(retail_params(), 9);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    let target = creature_at_cursor(&mut sim, &mut host, c, 1000.0);

    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 4.0, &mut ev);

    let latched: Vec<_> = sim
        .pikis()
        .iter()
        .filter(|p| p.state.latched_to() == Some(target))
        .collect();
    assert_eq!(
        latched.len(),
        1,
        "the thrown Pikmin should be stuck on the creature"
    );
    let hits = host.hits.iter().filter(|(id, _)| *id == target).count();
    assert!(
        hits >= 4,
        "retail keeps attacking until death (aiconst p10 = 0), got {hits} hits"
    );
    assert!(
        host.hits.iter().all(|&(_, dmg)| dmg == 15.0),
        "red attack power"
    );
}

#[test]
fn latched_hits_follow_the_cling_animation() {
    let mut sim = Sim::new(retail_params(), 9);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    creature_at_cursor(&mut sim, &mut host, c, 1000.0);

    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    let mut latched_at = None;
    let mut hits_at = Vec::new();
    for i in 0..200 {
        if i == 30 {
            sim.set_input(c, CaptainInput::default());
        }
        sim.step(&mut host);
        let tick = sim.tick();
        for e in sim.drain_events() {
            match e {
                Event::Latched { .. } => latched_at = Some(tick),
                Event::Hit { latched: true, .. } => hits_at.push(tick),
                _ => {}
            }
        }
    }
    let latched_at = latched_at.expect("should latch");
    assert!(hits_at.len() >= 4, "{hits_at:?}");
    // kuttuku.dca: KEY_Action0 at frame 11, loop 11→30, 30 frames/s = 1 frame per tick.
    assert_eq!(hits_at[0] - latched_at, 11, "first hit");
    for w in hits_at.windows(2) {
        assert_eq!(w[1] - w[0], 19, "loop period");
    }
}

#[test]
fn attack_once_never_releases_latched_pikmin() {
    // The cling animation loops, so KEY_Finished never fires and p10 has no effect on it.
    let mut params = retail_params();
    params.ai.attack_once = 1; // code default
    let mut sim = Sim::new(params, 9);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    let target = creature_at_cursor(&mut sim, &mut host, c, 1000.0);
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 4.0, &mut ev);
    assert!(host.hits.iter().filter(|(id, _)| *id == target).count() >= 4);
    assert!(
        sim.pikis()
            .iter()
            .any(|p| p.state.latched_to() == Some(target))
    );
}

#[test]
fn attack_once_stops_after_one_swing() {
    let mut params = retail_params();
    params.ai.attack_once = 1;
    let mut sim = Sim::new(params, 13);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    aim_cursor(&mut sim, &mut host, c, Vec2::new(1.0, 0.0), 0.5);
    let at = sim.captain(c).cursor_world;
    // Low creature so a landed Pikmin swings at it from the ground.
    host.creatures.push(Dummy {
        id: 7,
        pos: Vec3::new(at.x, 0.0, at.z + 50.0),
        radius: 12.0,
        height: 4.0,
        hp: 1000.0,
    });
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 4.0, &mut ev);

    let swings = ev
        .iter()
        .filter(|e| matches!(e, Event::Hit { latched: false, .. }))
        .count();
    let latched = ev.iter().any(|e| matches!(e, Event::Latched { .. }));
    assert!(!latched, "this setup should produce a ground swing");
    assert_eq!(swings, 1, "one swing, then the attack is over");
}

#[test]
fn shake_off_knocks_pikmin_down_then_they_get_up() {
    let mut sim = Sim::new(retail_params(), 11);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    let target = creature_at_cursor(&mut sim, &mut host, c, 1000.0);
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 1.5, &mut ev);
    let stuck = sim
        .pikis()
        .iter()
        .find(|p| p.state.latched_to() == Some(target))
        .map(|p| p.id)
        .expect("latched");

    sim.shake_off(target);
    run(&mut sim, &mut host, 0.1, &mut ev);
    assert!(matches!(sim.piki(stuck).state, PikiState::Knocked { .. }));
    run(&mut sim, &mut host, 1.5, &mut ev); // knockdown is 0.7 s with code defaults
    assert!(!matches!(sim.piki(stuck).state, PikiState::Knocked { .. }));
}

#[test]
fn landing_near_a_creature_starts_an_attack() {
    let mut sim = Sim::new(retail_params(), 13);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    aim_cursor(&mut sim, &mut host, c, Vec2::new(1.0, 0.0), 0.5);
    let at = sim.captain(c).cursor_world;
    // Off to the side of the landing spot: within idle_attack_search_range (70) + size, not under the arc.
    host.creatures.push(Dummy {
        id: 7,
        pos: Vec3::new(at.x, 0.0, at.z + 50.0),
        radius: 12.0,
        height: 12.0,
        hp: 1000.0,
    });

    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 4.0, &mut ev);
    assert!(
        host.hits.iter().any(|(id, _)| *id == 7),
        "the landed Pikmin should run over and hit the creature"
    );
}

#[test]
fn dead_target_releases_latched_pikmin() {
    let mut sim = Sim::new(retail_params(), 17);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    let target = creature_at_cursor(&mut sim, &mut host, c, 40.0);
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 4.0, &mut ev);
    assert!(
        host.creatures[0].hp <= 0.0,
        "3 hits of 15 should kill a 40 HP creature"
    );
    assert!(
        sim.pikis()
            .iter()
            .all(|p| p.state.latched_to() != Some(target))
    );
    assert!(
        sim.pikis().iter().all(|p| !matches!(p.state, PikiState::Attack(a) if a.mode != AttackMode::Approach && a.target == target)),
    );
}

#[test]
fn same_seed_same_result() {
    fn play(seed: u64) -> Vec<Vec3> {
        let mut sim = Sim::new(retail_params(), seed);
        let mut host = TestHost::default();
        let c = sim.add_captain(Vec3::ZERO);
        squad(&mut sim, c, 30, Vec3::ZERO);
        let mut ev = Vec::new();
        for i in 0..200 {
            let a = i as f32 * 0.05;
            sim.set_input(
                c,
                CaptainInput {
                    stick: Vec2::new(a.sin(), a.cos()),
                    throw: i % 40 < 3,
                    ..Default::default()
                },
            );
            run(&mut sim, &mut host, TICK_DT, &mut ev);
        }
        sim.pikis().iter().map(|p| p.pos).collect()
    }
    assert_eq!(play(99), play(99));
}

#[test]
fn hundred_pikmin_tick_budget() {
    let mut sim = Sim::new(retail_params(), 21);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 100, Vec3::ZERO);
    host.creatures.push(Dummy {
        id: 1,
        pos: Vec3::new(200.0, 0.0, 200.0),
        radius: 30.0,
        height: 20.0,
        hp: 1e9,
    });
    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            stick: Vec2::new(0.7, 0.7),
            ..Default::default()
        },
    );
    let start = std::time::Instant::now();
    run(&mut sim, &mut host, 1000.0 * TICK_DT, &mut ev);
    let per_tick = start.elapsed().as_secs_f64() * 1000.0 / 1000.0;
    eprintln!("100 Pikmin: {per_tick:.3} ms/tick");
    assert_eq!(sim.pikis().len(), 100);
}

// ---------------------------------------------------------------------------------------------
// Carrying, Onions, sprouts

fn corpse(pos: Vec3, min: u32, max: u32, seeds: i32) -> libpikmin::PelletDesc {
    libpikmin::PelletDesc {
        pos,
        yaw: 0.0,
        radius: 12.0,
        height: 10.0,
        min_carriers: min,
        max_carriers: max,
        seeds_matching: seeds,
        seeds_other: seeds,
        color: None,
        creature: None,
    }
}

/// Captain with a squad of `color`, cursor aimed 100 to +X, a corpse under the cursor.
fn carry_setup(
    color: PikiColor,
    n: u32,
    min: u32,
    max: u32,
    seeds: i32,
) -> (Sim, TestHost, CaptainId, libpikmin::PelletId) {
    let mut sim = Sim::new(retail_params(), 31);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    for i in 0..n {
        let a = i as f32 * 0.7;
        sim.spawn_piki_in_squad(
            color,
            Vec3::new(a.sin() * 20.0, 0.0, a.cos() * 20.0 - 30.0),
            c,
        );
    }
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    aim_cursor(&mut sim, &mut host, c, Vec2::new(1.0, 0.0), 0.5);
    let at = sim.captain(c).cursor_world;
    let pel = sim.spawn_pellet(corpse(Vec3::new(at.x, 0.0, at.z), min, max, seeds));
    (sim, host, c, pel)
}

#[test]
fn thrown_piki_grabs_a_corpse_and_carries_at_the_decomp_speed() {
    let (mut sim, mut host, c, pel) = carry_setup(PikiColor::Red, 3, 1, 2, 3);
    sim.add_onion(Vec3::new(2000.0, 0.0, 0.0), &[PikiColor::Red]);
    let mut ev = Vec::new();
    throw_once(&mut sim, &mut host, c, &mut ev);

    let mut lifted_at = None;
    for _ in 0..120 {
        sim.step(&mut host);
        if sim
            .drain_events()
            .any(|e| matches!(e, Event::PelletLifted { .. }))
        {
            lifted_at = Some(sim.pellet(pel).pos);
            break;
        }
    }
    let start = lifted_at.expect("one carrier meets min 1 and lifts");
    run(&mut sim, &mut host, 1.0, &mut ev);
    let moved = (sim.pellet(pel).pos - start).with_y(0.0).length();
    // aiTransport.cpp: max = 0.7 * 120 = 84, min = 42, speed = 42 + (1 + 1 - 1)/2 * 42 = 63.
    assert!((moved - 63.0).abs() < 1.5, "moved {moved} in 1 s");
}

#[test]
fn corpse_is_delivered_seeds_fly_out_and_sprouts_can_be_plucked() {
    let (mut sim, mut host, c, pel) = carry_setup(PikiColor::Red, 3, 1, 2, 3);
    let onion = sim.add_onion(Vec3::new(250.0, 0.0, 0.0), &[PikiColor::Red]);
    let mut ev = Vec::new();
    throw_once(&mut sim, &mut host, c, &mut ev);
    // Lift + carry + 1.5 s settle + suck + suction + 3 seeds, then ~3.5 s of drifting down
    // (air drag) and up to 4 s of sprout timers.
    run(&mut sim, &mut host, 25.0, &mut ev);

    assert!(
        ev.iter().any(|e| matches!(e, Event::PelletDelivered { pellet, onion: o, seeds: 3, .. } if *pellet == pel && *o == onion)),
        "delivered with 3 seeds"
    );
    assert!(matches!(
        sim.pellet(pel).state,
        libpikmin::PelletState::Gone
    ));
    let launched = ev
        .iter()
        .filter(|e| matches!(e, Event::SeedLaunched { .. }))
        .count();
    assert_eq!(launched, 3);
    let ready: Vec<_> = sim
        .sprouts()
        .filter(|s| s.pluckable())
        .map(|s| (s.id, s.pos))
        .collect();
    assert_eq!(
        ready.len(),
        3,
        "code-default sprout timers are short: all ready within 25 s"
    );

    // Put the captain next to a sprout (host-driven pose) and press the action button.
    let (_, at) = ready[0];
    let squad_before = sim.captain(c).plate.len();
    let pose = libpikmin::CaptainPose {
        pos: at + Vec3::new(-10.0, 0.0, 0.0),
        vel: Vec3::ZERO,
        face: 0.0,
    };
    sim.set_input(
        c,
        CaptainInput {
            pose: Some(pose),
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 0.2, &mut ev);
    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 0.1, &mut ev);
    sim.set_input(c, CaptainInput::default());
    ev.clear();
    run(&mut sim, &mut host, 2.0, &mut ev);
    let plucked = ev.iter().find_map(|e| match e {
        Event::Plucked { captain, piki, .. } if *captain == c => Some(*piki),
        _ => None,
    });
    let plucked = plucked.expect("the sprout should be plucked");
    assert_eq!(sim.piki(plucked).color, PikiColor::Red);
    assert_eq!(sim.captain(c).plate.len(), squad_before + 1);
}

#[test]
fn too_few_carriers_struggle_then_drop_when_whistled_away() {
    let (mut sim, mut host, c, pel) = carry_setup(PikiColor::Blue, 4, 2, 4, 2);
    sim.add_onion(Vec3::new(2000.0, 0.0, 0.0), &[PikiColor::Blue]);
    let mut ev = Vec::new();
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 2.0, &mut ev);
    assert!(
        matches!(sim.pellet(pel).state, libpikmin::PelletState::Idle),
        "1 of min 2: can not lift"
    );
    assert_eq!(sim.pellet(pel).carriers.len(), 1);

    throw_once(&mut sim, &mut host, c, &mut ev);
    for _ in 0..90 {
        if matches!(
            sim.pellet(pel).state,
            libpikmin::PelletState::Carried { .. }
        ) {
            break;
        }
        run(&mut sim, &mut host, TICK_DT, &mut ev);
    }
    assert!(
        matches!(
            sim.pellet(pel).state,
            libpikmin::PelletState::Carried { .. }
        ),
        "2 of min 2: lifted"
    );

    // Whistle at the pellet (the cursor is still on it): the carriers come back, it's put down.
    ev.clear();
    sim.set_input(
        c,
        CaptainInput {
            whistle: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 0.6, &mut ev);
    assert!(ev.iter().any(|e| matches!(e, Event::PelletDropped { .. })));
    assert!(sim.pellet(pel).carriers.is_empty());
}

#[test]
fn carriers_colour_picks_the_onion_not_distance() {
    let (mut sim, mut host, c, pel) = carry_setup(PikiColor::Blue, 3, 1, 2, 1);
    let red = sim.add_onion(Vec3::new(300.0, 0.0, 0.0), &[PikiColor::Red]);
    let blue = sim.add_onion(Vec3::new(-400.0, 0.0, 0.0), &[PikiColor::Blue]);
    let mut ev = Vec::new();
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 2.0, &mut ev);
    let lifted = ev.iter().find_map(|e| match e {
        Event::PelletLifted { pellet, onion } if *pellet == pel => Some(*onion),
        _ => None,
    });
    assert_eq!(lifted, Some(blue));
    assert_ne!(lifted, Some(red));
}

#[test]
fn a_shared_onion_takes_any_colour() {
    let (mut sim, mut host, c, pel) = carry_setup(PikiColor::Yellow, 3, 1, 2, 1);
    let shared = sim.add_onion(Vec3::new(250.0, 0.0, 0.0), &PikiColor::ALL);
    let mut ev = Vec::new();
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 12.0, &mut ev);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::PelletDelivered { pellet, onion, color: PikiColor::Yellow, .. } if *pellet == pel && *onion == shared
    )));
    assert!(sim.sprouts().all(|s| s.color == PikiColor::Yellow));
}

#[test]
fn full_field_keeps_seeds_in_the_onion() {
    let mut params = retail_params();
    params.ai.max_pikis_on_field = 3;
    let mut sim = Sim::new(params, 5);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, 3, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    aim_cursor(&mut sim, &mut host, c, Vec2::new(1.0, 0.0), 0.5);
    let at = sim.captain(c).cursor_world;
    sim.spawn_pellet(corpse(Vec3::new(at.x, 0.0, at.z), 1, 2, 2));
    let onion = sim.add_onion(Vec3::new(250.0, 0.0, 0.0), &[PikiColor::Red]);
    throw_once(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 12.0, &mut ev);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, Event::SeedStored { .. }))
            .count(),
        2
    );
    assert_eq!(
        sim.onions()[onion.0 as usize].stored[PikiColor::Red.index()],
        2
    );
}

#[test]
fn sprouts_grow_to_flower_then_wilt() {
    let mut sim = Sim::new(Params::default(), 3);
    let mut host = TestHost::default();
    let s = sim.plant_sprout(PikiColor::Blue, Vec3::new(50.0, 0.0, 0.0));
    let mut ev = Vec::new();
    // Code defaults: grow 4 s (+0..2 random) per stage, wilt 4 s.
    run(&mut sim, &mut host, 20.0, &mut ev);
    let grew: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::SproutGrew { sprout, happa } if *sprout == s => Some(*happa),
            _ => None,
        })
        .collect();
    assert_eq!(grew[..2], [libpikmin::Happa::Bud, libpikmin::Happa::Flower]);
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::SproutWilted { sprout } if *sprout == s))
    );
}
