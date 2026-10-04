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
    run(&mut sim, &mut host, 3.0, &mut ev);

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
fn attack_once_drops_off_after_one_hit() {
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
    run(&mut sim, &mut host, 3.0, &mut ev);
    assert_eq!(host.hits.iter().filter(|(id, _)| *id == target).count(), 1);
    assert!(sim.pikis().iter().all(|p| p.state.latched_to().is_none()));
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
