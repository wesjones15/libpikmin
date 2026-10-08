//! Headless scenario tests: whole-sim behaviour against the decomp's rules.

use libpikmin::captain::ThrowState;
use libpikmin::host::{CreatureFrame, CreatureId, CreaturePart};
use libpikmin::piki::AttackMode;
use libpikmin::{
    CaptainId, CaptainInput, Event, Happa, Host, Params, PikiColor, PikiId, PikiState, Sim, TICK_DT, Vec2,
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
        _from: libpikmin::DamageSource,
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
    p.piki.piki_max_health = 30.0;
    p.navi.punch_damage = 10.0;
    p.piki.press_death_factor = 2.5;
    p.piki.normal_piki_press_factor = 1.0;
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
fn speed_scale_speeds_up_the_captain() {
    let walk = |scale: f32| {
        let mut sim = Sim::new(retail_params(), 7);
        let mut host = TestHost::default();
        let c = sim.add_captain(Vec3::ZERO);
        sim.set_captain_speed(c, scale);
        sim.set_input(
            c,
            CaptainInput {
                stick: Vec2::new(0.0, 1.0),
                ..Default::default()
            },
        );
        run(&mut sim, &mut host, 2.0, &mut Vec::new());
        sim.captain(c).pos.z
    };
    let (normal, fast) = (walk(1.0), walk(1.5));
    assert!(
        (fast / normal - 1.5).abs() < 0.05,
        "1.5x speed walked {fast} against {normal}"
    );
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
    // kuttuku.dca: KEY_Action0 at frame 11, loop 11â†’30, 30 frames/s = 1 frame per tick.
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
    // JHIT 9 + JKOKE 15 frames, down 0.7 s (code defaults), GETUP 10 frames: about 1.8 s.
    run(&mut sim, &mut host, 1.5, &mut ev);
    assert!(matches!(sim.piki(stuck).state, PikiState::Knocked { .. }));
    run(&mut sim, &mut host, 0.6, &mut ev);
    assert!(!matches!(sim.piki(stuck).state, PikiState::Knocked { .. }));
}

#[test]
fn whistled_while_down_pikmin_get_up_then_join() {
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
    // Flying back and landing take 24 frames; whistle once it's lying there.
    run(&mut sim, &mut host, 0.9, &mut ev);
    assert!(matches!(
        sim.piki(stuck).state,
        PikiState::Knocked { phase: libpikmin::KnockPhase::Down { .. }, .. }
    ));
    // The cursor is still on the creature, next to where it landed.
    host.creatures.clear();
    ev.clear();
    sim.set_input(c, CaptainInput { whistle: true, ..Default::default() });
    let full = sim.params().navi.whistle_expand_time;
    run(&mut sim, &mut host, full + 0.05, &mut ev);
    sim.set_input(c, CaptainInput::default());
    run(&mut sim, &mut host, TICK_DT, &mut ev);
    assert!(
        matches!(sim.piki(stuck).state, PikiState::Knocked { whistled: Some(w), .. } if w == c)
            || ev.iter().any(|e| matches!(e, Event::Recruited { piki, .. } if *piki == stuck)),
        "the whistle is booked: {:?} at {} from the cursor, whistle {:?}",
        sim.piki(stuck).state,
        (sim.piki(stuck).pos - sim.captain(c).cursor_world).length(),
        sim.captain(c).whistle
    );
    run(&mut sim, &mut host, 0.5, &mut ev);
    assert!(ev.iter().any(|e| matches!(e, Event::Recruited { piki, .. } if *piki == stuck)));
    assert!(sim.piki(stuck).state.in_squad());
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
        ship_part: None,
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
        sim.onions()[onion.0 as usize].stored_of(PikiColor::Red),
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

// ---------------------------------------------------------------------------------------------
// Dying and captain damage

fn died(ev: &[Event], piki: PikiId) -> Option<libpikmin::DeathCause> {
    ev.iter().find_map(|e| match e {
        Event::PikiDied { piki: p, cause } if *p == piki => Some(*cause),
        _ => None,
    })
}

#[test]
fn attacks_use_up_health_then_the_death_animation_plays_and_it_fades() {
    let mut sim = Sim::new(retail_params(), 41);
    let mut host = TestHost::default();
    let id = sim.spawn_piki(PikiColor::Red, Vec3::ZERO).unwrap();
    assert_eq!(sim.piki(id).health, 30.0, "retail mPikiMaxHealth");

    assert!(sim.attack_piki(id, 15.0));
    assert!(sim.piki(id).is_alive());
    assert!(sim.attack_piki(id, 15.0));
    assert!(matches!(sim.piki(id).state, PikiState::Dying { .. }));
    assert!(
        !sim.attack_piki(id, 15.0),
        "dying Pikmin can't be hit again"
    );

    let mut ev = Vec::new();
    // Longest death animation is 129 frames, then 1.5 s of fading.
    run(&mut sim, &mut host, 129.0 / 30.0 + 1.6, &mut ev);
    assert!(matches!(sim.piki(id).state, PikiState::Dead));
    assert!(!sim.piki(id).on_field());
}

#[test]
fn a_dying_squad_member_leaves_the_formation() {
    let mut sim = Sim::new(retail_params(), 42);
    let c = sim.add_captain(Vec3::ZERO);
    let ids = squad(&mut sim, c, 3, Vec3::ZERO);
    sim.attack_piki(ids[0], 100.0);
    assert!(!sim.captain(c).plate.members.contains(&ids[0]));
    assert_eq!(sim.captain(c).plate.len(), 2);
}

#[test]
fn crushing_kills_at_once_or_flattens_then_recovers() {
    let mut sim = Sim::new(retail_params(), 43);
    let mut host = TestHost::default();
    let weak = sim
        .spawn_piki(PikiColor::Blue, Vec3::new(0.0, 0.0, 0.0))
        .unwrap();
    let strong = sim
        .spawn_piki(PikiColor::Blue, Vec3::new(10.0, 0.0, 0.0))
        .unwrap();
    sim.attack_piki(weak, 20.0); // 10 left
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 0.1, &mut ev);

    assert_eq!(
        sim.crush_pikis_in_sphere(Vec3::new(5.0, 0.0, 0.0), 20.0, 15.0),
        2
    );
    run(&mut sim, &mut host, 0.1, &mut ev);
    assert_eq!(died(&ev, weak), Some(libpikmin::DeathCause::Crushed));
    assert!(matches!(sim.piki(strong).state, PikiState::Pressed { .. }));

    // Retail: 1.5 s stun + mPressDeathFactor (2.5 s) flat, then back up.
    run(&mut sim, &mut host, 3.5, &mut ev);
    assert!(matches!(sim.piki(strong).state, PikiState::Pressed { .. }));
    run(&mut sim, &mut host, 0.7, &mut ev);
    assert!(matches!(sim.piki(strong).state, PikiState::Idle));
}

#[test]
fn swallowed_pikmin_ride_in_the_mouth_and_die_when_chewed() {
    let mut sim = Sim::new(retail_params(), 44);
    let mut host = TestHost::default();
    host.creatures.push(Dummy {
        id: 9,
        pos: Vec3::new(0.0, 0.0, 0.0),
        radius: 15.0,
        height: 15.0,
        hp: 100.0,
    });
    let a = sim
        .spawn_piki(PikiColor::Red, Vec3::new(20.0, 0.0, 0.0))
        .unwrap();
    let b = sim
        .spawn_piki(PikiColor::Red, Vec3::new(22.0, 0.0, 5.0))
        .unwrap();
    let far = sim
        .spawn_piki(PikiColor::Red, Vec3::new(200.0, 0.0, 0.0))
        .unwrap();
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 0.1, &mut ev);

    assert_eq!(
        sim.swallow_pikis(9, 0, Vec3::new(20.0, 0.0, 0.0), 15.0, 5),
        2
    );
    host.creatures[0].pos.x += 50.0;
    run(&mut sim, &mut host, 0.2, &mut ev);
    assert!(
        (sim.piki(a).pos.x - 50.0).abs() < 20.0,
        "rides along: {:?}",
        sim.piki(a).pos
    );
    assert!(matches!(sim.piki(far).state, PikiState::Idle));

    assert_eq!(sim.chew(9), 2);
    run(&mut sim, &mut host, 0.1, &mut ev);
    assert_eq!(died(&ev, a), Some(libpikmin::DeathCause::Eaten));
    assert_eq!(died(&ev, b), Some(libpikmin::DeathCause::Eaten));
}

#[test]
fn killing_the_creature_frees_whatever_it_swallowed() {
    let mut sim = Sim::new(retail_params(), 45);
    let mut host = TestHost::default();
    host.creatures.push(Dummy {
        id: 9,
        pos: Vec3::ZERO,
        radius: 15.0,
        height: 15.0,
        hp: 100.0,
    });
    let a = sim
        .spawn_piki(PikiColor::Red, Vec3::new(15.0, 0.0, 0.0))
        .unwrap();
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 0.1, &mut ev);
    sim.swallow_pikis(9, 0, Vec3::new(15.0, 0.0, 0.0), 10.0, 1);
    host.creatures[0].hp = 0.0;
    run(&mut sim, &mut host, 0.5, &mut ev);
    assert!(sim.piki(a).is_alive());
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::SpitOut { piki } if *piki == a))
    );
}

#[test]
fn captain_damage_has_an_invulnerable_window_and_runs_out() {
    let mut sim = Sim::new(retail_params(), 46);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    let ids = squad(&mut sim, c, 4, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 1.0, &mut ev);

    assert!(sim.damage_captain(c, 40.0));
    assert!(
        !sim.damage_captain(c, 40.0),
        "still in the 19-frame damage animation"
    );
    run(&mut sim, &mut host, 19.0 / 30.0 + 0.05, &mut ev);
    assert!(sim.damage_captain(c, 40.0));
    assert_eq!(sim.captain(c).health, 20.0);
    run(&mut sim, &mut host, 0.7, &mut ev);
    assert!(sim.damage_captain(c, 25.0));
    assert!(
        !sim.captain(c).down,
        "goes down when the damage animation ends, not on the hit"
    );
    run(&mut sim, &mut host, 0.7, &mut ev);
    assert!(sim.captain(c).down);
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::CaptainDown { captain } if *captain == c))
    );
    assert!(sim.captain(c).plate.is_empty(), "the squad was released");
    for id in &ids {
        assert!(matches!(sim.piki(*id).state, PikiState::Idle));
    }

    // Down: no walking.
    let before = sim.captain(c).pos;
    sim.set_input(
        c,
        CaptainInput {
            stick: Vec2::new(1.0, 0.0),
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 1.0, &mut ev);
    assert_eq!(sim.captain(c).pos, before);

    sim.revive_captain(c, Vec3::new(10.0, 0.0, 0.0));
    assert_eq!(sim.captain(c).health, 100.0);
    run(&mut sim, &mut host, 1.0, &mut ev);
    assert!(sim.captain(c).pos.x > 50.0, "walks again");
}

#[test]
fn a_dead_flower_can_leave_a_seed() {
    let mut params = retail_params();
    params.piki.piki_leave_seed_chance = 0.0; // rand >= 0 always: guaranteed seed
    let mut sim = Sim::new(params, 47);
    let mut host = TestHost::default();
    let flower = sim
        .spawn_piki(PikiColor::Yellow, Vec3::new(40.0, 0.0, 0.0))
        .unwrap();
    let leaf = sim
        .spawn_piki(PikiColor::Yellow, Vec3::new(-40.0, 0.0, 0.0))
        .unwrap();
    sim.piki_mut(flower).happa = libpikmin::Happa::Flower;
    sim.attack_piki(flower, 100.0);
    sim.attack_piki(leaf, 100.0);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 6.5, &mut ev);
    let seeds: Vec<_> = sim.sprouts().collect();
    assert_eq!(seeds.len(), 1, "only the flower leaves one");
    assert_eq!(seeds[0].color, PikiColor::Yellow);
    assert!((seeds[0].pos.x - 40.0).abs() < 5.0);
}

// ---------------------------------------------------------------------------------------------
// Punch, chained plucking, C-stick swarming

fn press_throw(sim: &mut Sim, host: &mut TestHost, c: CaptainId, ev: &mut Vec<Event>) {
    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    run(sim, host, TICK_DT, ev);
    sim.set_input(c, CaptainInput::default());
}

#[test]
fn punch_hits_what_is_in_front_when_nobody_can_be_thrown() {
    let mut sim = Sim::new(retail_params(), 51);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO); // faces +Z
    host.creatures.push(Dummy {
        id: 1,
        pos: Vec3::new(0.0, 0.0, 20.0),
        radius: 10.0,
        height: 10.0,
        hp: 100.0,
    });
    host.creatures.push(Dummy {
        id: 2,
        pos: Vec3::new(0.0, 0.0, -20.0),
        radius: 10.0,
        height: 10.0,
        hp: 100.0,
    });
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 0.1, &mut ev);

    press_throw(&mut sim, &mut host, c, &mut ev);
    run(&mut sim, &mut host, 0.5, &mut ev);
    assert_eq!(
        host.hits,
        vec![(1, 10.0)],
        "one punch, retail mPunchDamage 10, only the one in front"
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::Punched { creature: 1, .. }))
    );
    assert!(sim.captain(c).punch.is_none(), "9-frame animation is over");
}

fn sprout_row(sim: &mut Sim, xs: &[f32]) {
    for &x in xs {
        sim.plant_sprout(PikiColor::Red, Vec3::new(x, 0.0, 0.0));
    }
}

#[test]
fn holding_the_button_chains_plucks_through_the_patch() {
    // libpikmin extra (on by default): hold to keep plucking.
    let mut sim = Sim::new(retail_params(), 52);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    sprout_row(&mut sim, &[10.0, 50.0, 90.0, 130.0]);
    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 6.0, &mut ev);

    let plucked = ev
        .iter()
        .filter(|e| matches!(e, Event::Plucked { .. }))
        .count();
    assert_eq!(
        plucked, 4,
        "chained across 40-unit gaps (mContinuousPluckDistance 200)"
    );
    assert_eq!(sim.captain(c).plate.len(), 4);
}

fn pikmin1_rules() -> Params {
    let mut p = retail_params();
    p.extras.hold_to_chain_pluck = false;
    p
}

#[test]
fn pikmin1_rules_mashing_chains_plucks() {
    let mut sim = Sim::new(pikmin1_rules(), 52);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    sprout_row(&mut sim, &[10.0, 50.0, 90.0, 130.0]);
    let mut ev = Vec::new();
    let mut pull_starts = Vec::new();
    for i in 0..180 {
        let throw = i % 6 == 0; // tap: 1 tick down, 5 up
        sim.set_input(
            c,
            CaptainInput {
                throw,
                ..Default::default()
            },
        );
        run(&mut sim, &mut host, TICK_DT, &mut ev);
        if let Some(libpikmin::onion::Pluck {
            phase: libpikmin::onion::PluckPhase::Pull { frame, fast, .. },
            ..
        }) = sim.captain(c).pluck
            && frame == 0.0
        {
            pull_starts.push(fast);
        }
    }
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, Event::Plucked { .. }))
            .count(),
        4
    );
    assert_eq!(
        pull_starts.first(),
        Some(&false),
        "first pull is the normal NUKU"
    );
    assert!(
        pull_starts[1..].iter().all(|&f| f),
        "chained pulls use NUKU fast: {pull_starts:?}"
    );
}

#[test]
fn pikmin1_rules_holding_from_the_first_press_plucks_just_one() {
    let mut sim = Sim::new(pikmin1_rules(), 53);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    sprout_row(&mut sim, &[10.0, 50.0]);
    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            throw: true,
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 4.0, &mut ev);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, Event::Plucked { .. }))
            .count(),
        1
    );
}

fn formed_squad(seed: u64, n: u32) -> (Sim, TestHost, CaptainId) {
    let mut sim = Sim::new(retail_params(), seed);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::ZERO);
    squad(&mut sim, c, n, Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 3.0, &mut ev);
    (sim, host, c)
}

fn squad_centre(sim: &Sim, c: CaptainId) -> Vec3 {
    let m = &sim.captain(c).plate.members;
    m.iter().map(|id| sim.piki(*id).pos).sum::<Vec3>() / m.len() as f32
}

#[test]
fn the_c_stick_swings_the_squad_out_in_front() {
    let (mut sim, mut host, c) = formed_squad(54, 12);
    assert!(
        squad_centre(&sim, c).z < -10.0,
        "starts behind the captain (he faces +Z)"
    );
    let mut ev = Vec::new();
    sim.set_input(
        c,
        CaptainInput {
            swarm: Vec2::new(1.0, 0.0),
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 3.0, &mut ev);
    assert!(sim.captain(c).swarming);
    let centre = squad_centre(&sim, c);
    assert!(centre.x > 40.0, "squad pushed toward +X: {centre:?}");
}

#[test]
fn swarming_into_a_corpse_starts_carrying_but_walking_past_does_not() {
    let (mut sim, mut host, c) = formed_squad(55, 12);
    sim.add_onion(Vec3::new(-2000.0, 0.0, 0.0), &[PikiColor::Red]);
    let pel = sim.spawn_pellet(corpse(Vec3::new(70.0, 0.0, -40.0), 3, 6, 4));
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 2.0, &mut ev);
    assert!(
        sim.pellet(pel).carriers.is_empty(),
        "C-stick neutral: no pellet pickup on contact"
    );

    sim.set_input(
        c,
        CaptainInput {
            swarm: Vec2::new(1.0, -0.6),
            ..Default::default()
        },
    );
    run(&mut sim, &mut host, 3.0, &mut ev);
    assert!(
        !sim.pellet(pel).carriers.is_empty(),
        "swarmed into it: they grab on"
    );
}

#[test]
fn settled_squad_members_attack_an_enemy_they_bump() {
    for (attack, expect) in [(1, true), (0, false)] {
        let (mut sim, mut host, c) = formed_squad(56, 12);
        sim.params_mut().ai.do_cstick_attack = attack;
        let centre = squad_centre(&sim, c);
        host.creatures.push(Dummy {
            id: 5,
            pos: centre.with_y(0.0),
            radius: 12.0,
            height: 10.0,
            hp: 1e6,
        });
        let mut ev = Vec::new();
        run(&mut sim, &mut host, 0.5, &mut ev);
        let attacking = sim
            .pikis()
            .iter()
            .filter(|p| matches!(p.state, PikiState::Attack(a) if a.target == 5))
            .count();
        assert_eq!(
            attacking > 0,
            expect,
            "aiconst p05 = {attack}: {attacking} attacking"
        );
    }
}

// ---- nectar ----

#[test]
fn nectar_drop_splashes_into_a_puddle() {
    let mut sim = Sim::new(retail_params(), 60);
    let mut host = TestHost::default();
    sim.spawn_nectar(Vec3::new(50.0, 120.0, 0.0));
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 3.0, &mut ev);
    assert!(ev.iter().any(|e| matches!(e, Event::NectarSplash { .. })));
    assert!(sim.nectar_drops().is_empty(), "the drop is gone once its splash ends");
    assert_eq!(sim.puddles().len(), 1);
    assert!((sim.puddles()[0].pos - Vec3::new(50.0, 0.0, 0.0)).length() < 1.0);
}

#[test]
fn leaf_pikmin_drinks_nectar_and_blooms() {
    let mut sim = Sim::new(retail_params(), 61);
    let mut host = TestHost::default();
    sim.add_nectar_puddle(Vec3::ZERO);
    let id = sim.spawn_piki(PikiColor::Blue, Vec3::new(10.0, 0.0, 0.0)).unwrap();
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 0.2, &mut ev);
    assert!(matches!(sim.piki(id).state, PikiState::Absorb { .. }));
    run(&mut sim, &mut host, 6.0, &mut ev);
    assert_eq!(sim.piki(id).happa, Happa::Flower);
    assert_eq!(sim.piki(id).state, PikiState::Idle);
    assert!(ev.iter().any(|e| matches!(e, Event::Bloomed { piki } if *piki == id)));
    assert!(sim.puddles().is_empty(), "drinking drained the puddle");
}

#[test]
fn several_pikmin_share_one_puddle() {
    let mut sim = Sim::new(retail_params(), 62);
    let mut host = TestHost::default();
    sim.add_nectar_puddle(Vec3::ZERO);
    let ids: Vec<_> = (0..3)
        .map(|i| {
            let a = i as f32 * 2.1;
            sim.spawn_piki(PikiColor::Red, Vec3::new(a.sin() * 12.0, 0.0, a.cos() * 12.0)).unwrap()
        })
        .collect();
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 7.0, &mut ev);
    for id in ids {
        assert_eq!(sim.piki(id).happa, Happa::Flower, "{id:?}");
    }
}

#[test]
fn squad_pikmin_leave_the_squad_to_drink_and_ignore_the_whistle() {
    let mut sim = Sim::new(retail_params(), 63);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::new(0.0, 0.0, 200.0));
    let id = sim.spawn_piki_in_squad(PikiColor::Yellow, Vec3::new(5.0, 0.0, 0.0), c).unwrap();
    sim.add_nectar_puddle(Vec3::ZERO);
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 0.2, &mut ev);
    assert!(matches!(sim.piki(id).state, PikiState::Absorb { .. }));
    assert_eq!(sim.captain(c).plate.len(), 0, "drinking takes it out of the formation");
    // Whistle over it while it drinks: Navi::callPikis skips PIKISTATE_Absorb.
    aim_cursor(&mut sim, &mut host, c, Vec2::new(0.0, -1.0), 2.0);
    sim.set_input(c, CaptainInput { whistle: true, ..Default::default() });
    run(&mut sim, &mut host, 0.5, &mut ev);
    assert!(!matches!(sim.piki(id).state, PikiState::Squad { .. }));
}

#[test]
fn flower_pikmin_ignore_nectar() {
    let mut sim = Sim::new(retail_params(), 64);
    let mut host = TestHost::default();
    sim.add_nectar_puddle(Vec3::ZERO);
    let id = sim.spawn_piki(PikiColor::Blue, Vec3::new(10.0, 0.0, 0.0)).unwrap();
    sim.piki_mut(id).happa = Happa::Flower;
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 3.0, &mut ev);
    assert_eq!(sim.piki(id).state, PikiState::Idle);
    assert_eq!(sim.puddles().len(), 1);
}

#[test]
fn a_draining_puddle_takes_no_new_drinkers() {
    let mut sim = Sim::new(retail_params(), 65);
    let mut host = TestHost::default();
    sim.add_nectar_puddle(Vec3::ZERO);
    let first = sim.spawn_piki(PikiColor::Red, Vec3::new(10.0, 0.0, 0.0)).unwrap();
    let mut ev = Vec::new();
    // Past "swing" (42 frames) and the drinker's loop start: the puddle is draining now.
    run(&mut sim, &mut host, 2.0, &mut ev);
    assert!(matches!(sim.puddles()[0].state, libpikmin::PuddleState::Drain { .. }));
    let late = sim.spawn_piki(PikiColor::Red, Vec3::new(-10.0, 0.0, 0.0)).unwrap();
    run(&mut sim, &mut host, 5.0, &mut ev);
    assert_eq!(sim.piki(first).happa, Happa::Flower);
    assert_eq!(sim.piki(late).happa, Happa::Leaf);
}

// ---- Onion menu ----

#[test]
fn captain_under_the_onion_can_open_its_menu() {
    let mut sim = Sim::new(retail_params(), 70);
    let c = sim.add_captain(Vec3::new(30.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    assert_eq!(sim.onion_in_reach(c), Some(onion));
    let far = sim.add_captain(Vec3::new(400.0, 0.0, 0.0));
    assert_eq!(sim.onion_in_reach(far), None);
}

#[test]
fn putting_pikmin_away_stores_them_in_the_onion() {
    let mut sim = Sim::new(retail_params(), 71);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::new(30.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    squad(&mut sim, c, 5, Vec3::new(30.0, 0.0, 0.0));
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 1.0, &mut ev);
    let before = sim.onion_counts(onion, c, PikiColor::Red);
    assert_eq!(before.in_squad, 5);
    assert_eq!(sim.onion_put_away(c, onion, PikiColor::Red, 3), 3);
    assert_eq!(sim.captain(c).plate.len(), 2);
    run(&mut sim, &mut host, 15.0, &mut ev);
    assert_eq!(sim.onions()[0].stored_of(PikiColor::Red), 3);
    assert_eq!(ev.iter().filter(|e| matches!(e, Event::PikiEnteredOnion { .. })).count(), 3);
    assert_eq!(sim.pikis().iter().filter(|p| p.on_field()).count(), 2);
}

#[test]
fn calling_out_brings_stored_pikmin_to_the_squad_flowers_first() {
    let mut sim = Sim::new(retail_params(), 72);
    let mut host = TestHost::default();
    let c = sim.add_captain(Vec3::new(30.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Blue]);
    sim.store_in_onion(onion, PikiColor::Blue, Happa::Leaf, 4);
    sim.store_in_onion(onion, PikiColor::Blue, Happa::Flower, 1);
    assert_eq!(sim.onion_call_out(c, onion, PikiColor::Blue, 3), 3);
    assert_eq!(sim.onion_counts(onion, c, PikiColor::Blue).in_onion, 2, "pending exits are spoken for");
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 4.0, &mut ev);
    assert_eq!(sim.captain(c).plate.len(), 3);
    assert_eq!(sim.onions()[0].stored_of(PikiColor::Blue), 2);
    let first = ev
        .iter()
        .find_map(|e| match e {
            Event::PikiLeftOnion { piki, .. } => Some(*piki),
            _ => None,
        })
        .unwrap();
    assert_eq!(sim.piki(first).happa, Happa::Flower, "highest leaf stage comes out first");
}

#[test]
fn calling_out_respects_the_field_limit() {
    let mut params = retail_params();
    params.ai.max_pikis_on_field = 10;
    let mut sim = Sim::new(params, 73);
    let c = sim.add_captain(Vec3::new(30.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Yellow]);
    for i in 0..8 {
        sim.spawn_piki(PikiColor::Red, Vec3::new(200.0 + i as f32 * 10.0, 0.0, 0.0));
    }
    sim.store_in_onion(onion, PikiColor::Yellow, Happa::Leaf, 20);
    assert_eq!(sim.onion_counts(onion, c, PikiColor::Yellow).can_call_out(), 2);
    assert_eq!(sim.onion_call_out(c, onion, PikiColor::Yellow, 20), 2);
    assert_eq!(sim.onion_call_out(c, onion, PikiColor::Yellow, 20), 0);
}
