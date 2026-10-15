//! Host item hooks (items.rs, jump.rs's air jumps and stomps).
use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Event, Params, PelletDesc, PikiColor, PikiState, Sim, Vec2, Vec3};

fn sim() -> Sim {
    let mut params = Params::default();
    params.ai.auto_attack = 0;
    Sim::new(params, 5)
}

fn steps(sim: &mut Sim, n: u32) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..n {
        sim.step(&mut FlatHost);
        out.extend(sim.drain_events());
    }
    out
}

fn press_jump(sim: &mut Sim, c: libpikmin::CaptainId) {
    sim.set_input(c, CaptainInput { jump: true, ..Default::default() });
    sim.step(&mut FlatHost);
    sim.set_input(c, CaptainInput::default());
}

#[test]
fn squad_pikmin_pick_up_a_pellet_they_pass() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-30.0, 0.0, 0.0), c).unwrap();
    sim.add_onion(Vec3::new(-800.0, 0.0, 0.0), &PikiColor::ALL);
    let pellet = PelletDesc::numbered(sim.params(), 1, PikiColor::Red, Vec3::new(-40.0, 0.0, 30.0), 0.0);
    sim.spawn_pellet(pellet);
    steps(&mut sim, 30);
    assert!(sim.piki(p).state.in_squad(), "off by default");

    sim.set_auto_carry_range(80.0);
    steps(&mut sim, 30);
    assert!(matches!(sim.piki(p).state, PikiState::Carry(_)), "{:?}", sim.piki(p).state);
    assert!(!sim.piki(p).dismissed);
}

#[test]
fn an_extinct_owner_gets_emergency_pikmin_down_the_legs_or_dropped_when_flying() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::new(300.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
    sim.set_onion_owner(onion, Some(c));
    sim.set_emergency_recovery(1.0, 10);
    let ev = steps(&mut sim, 25);
    assert!(!ev.iter().any(|e| matches!(e, Event::EmergencyPikmin { .. })));
    let ev = steps(&mut sim, 10);
    assert!(ev.iter().any(|e| matches!(e, Event::EmergencyPikmin { count: 10, .. })));
    steps(&mut sim, 30 * 6);
    assert_eq!(sim.captain_piki_count(c), 10);
    assert!(sim.pikis().iter().all(|p| p.state.in_squad()));

    // Flying: dropped, then they find him.
    let mut sim2 = self::sim();
    let c = sim2.add_captain(Vec3::new(300.0, 0.0, 0.0));
    let onion = sim2.add_onion(Vec3::ZERO, &PikiColor::ALL);
    sim2.set_onion_owner(onion, Some(c));
    sim2.set_auto_rejoin(true);
    sim2.set_onion_pose(onion, Vec3::new(0.0, 100.0, 0.0), true);
    sim2.set_emergency_recovery(0.5, 10);
    let ev = steps(&mut sim2, 20);
    assert!(ev.iter().any(|e| matches!(e, Event::EmergencyPikmin { count: 10, .. })));
    assert!(sim2.pikis().iter().all(|p| matches!(p.state, PikiState::Flying { .. })));
    steps(&mut sim2, 30 * 6);
    assert_eq!(sim2.pikis().iter().filter(|p| p.state.in_squad()).count(), 10);
}

#[test]
fn a_stocked_onion_sends_no_emergency_pikmin() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::new(300.0, 0.0, 0.0));
    let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
    sim.set_onion_owner(onion, Some(c));
    sim.store_in_onion(onion, PikiColor::Red, libpikmin::Happa::Leaf, 1);
    sim.set_emergency_recovery(0.1, 10);
    let ev = steps(&mut sim, 60);
    assert!(!ev.iter().any(|e| matches!(e, Event::EmergencyPikmin { .. })));
}

#[test]
fn hopoo_air_jumps_are_repeated_by_the_squad() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-60.0, 0.0, 0.0), c).unwrap();
    steps(&mut sim, 2);
    // Without feathers a second press in the air does nothing.
    press_jump(&mut sim, c);
    steps(&mut sim, 6);
    let vy = sim.captain(c).vel.y;
    press_jump(&mut sim, c);
    assert!(sim.captain(c).vel.y < vy);
    steps(&mut sim, 90);

    sim.set_captain_air_jumps(c, 1);
    press_jump(&mut sim, c);
    steps(&mut sim, 8);
    let low = sim.piki(p).vel.y;
    press_jump(&mut sim, c);
    let ev: Vec<_> = sim.drain_events().collect();
    let _ = ev;
    assert!(sim.captain(c).vel.y > 0.0 && sim.captain(c).air_jumps_used == 1);
    assert!(sim.piki(p).vel.y > low, "the airborne follower jumped again");
    // A third press has no jumps left.
    steps(&mut sim, 3);
    let vy = sim.captain(c).vel.y;
    press_jump(&mut sim, c);
    assert!(sim.captain(c).vel.y < vy);
    steps(&mut sim, 120);
    assert!(sim.captain(c).on_ground && sim.captain(c).air_jumps_used == 0);
}

#[test]
fn headstompers_stomp_brings_the_airborne_squad_down() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-60.0, 0.0, 0.0), c).unwrap();
    sim.set_captain_stomp(c, true);
    steps(&mut sim, 2);
    press_jump(&mut sim, c);
    steps(&mut sim, 8);
    press_jump(&mut sim, c);
    assert!(sim.captain(c).stomp.is_some());
    let ev = steps(&mut sim, 30);
    assert!(ev.iter().any(|e| matches!(e, Event::CaptainStomped { drop, .. } if *drop > 10.0)));
    assert!(ev.iter().any(|e| matches!(e, Event::PikiStomped { piki, .. } if *piki == p)));
}

#[test]
fn quail_boost_carries_the_jump_further() {
    let far = |boost: f32| {
        let mut sim = sim();
        let c = sim.add_captain(Vec3::ZERO);
        sim.set_input(c, CaptainInput { stick: Vec2::new(1.0, 0.0), ..Default::default() });
        steps(&mut sim, 30);
        let start = sim.captain(c).pos.x;
        sim.set_input(c, CaptainInput { stick: Vec2::new(1.0, 0.0), jump: true, ..Default::default() });
        sim.step(&mut FlatHost);
        if boost > 1.0 {
            sim.boost_captain_jump(c, boost);
        }
        sim.set_input(c, CaptainInput { stick: Vec2::new(1.0, 0.0), ..Default::default() });
        for _ in 0..200 {
            sim.step(&mut FlatHost);
            if sim.captain(c).on_ground {
                break;
            }
        }
        sim.captain(c).pos.x - start
    };
    assert!(far(2.0) > far(1.0) + 20.0);
}

#[test]
fn rates_speed_up_attacks_moves_and_growth() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-400.0, 0.0, 0.0), c).unwrap();
    steps(&mut sim, 15);
    let slow = sim.piki(p).vel.length();
    sim.set_squad_speed(c, 2.0);
    steps(&mut sim, 15);
    assert!(sim.piki(p).vel.length() > slow * 1.5);

    // Growth: a planted seed shows sooner.
    let grow = |rate: f32| {
        let mut sim = self::sim();
        let onion = sim.add_onion(Vec3::ZERO, &PikiColor::ALL);
        sim.set_growth_rate(rate);
        sim.give_onion_seeds(onion, PikiColor::Red, 1);
        for t in 0..30 * 120 {
            sim.step(&mut FlatHost);
            if sim.sprouts().any(|s| s.pluckable()) {
                return t;
            }
        }
        u32::MAX
    };
    assert!(grow(2.0) < grow(1.0));
}

#[test]
fn healing_and_teleporting_the_squad() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-30.0, 0.0, 0.0), c).unwrap();
    steps(&mut sim, 2);
    sim.attack_piki(p, 30.0);
    let hurt = sim.piki(p).health;
    assert_eq!(sim.heal_squad(c, 10.0), 10.0);
    assert_eq!(sim.piki(p).health, hurt + 10.0);
    sim.heal_pikis_in_sphere(sim.piki(p).pos, 5.0, 1000.0);
    assert_eq!(sim.piki(p).health, sim.params().piki.piki_max_health);

    sim.teleport_squad(c, Vec3::new(5000.0, 0.0, 0.0));
    steps(&mut sim, 2);
    assert!((sim.piki(p).pos - Vec3::new(5000.0, 0.0, 0.0)).length() < 60.0);
}

#[test]
fn faster_attack_rate_plucks_sooner() {
    let pull = |rate: f32| {
        let mut sim = sim();
        let c = sim.add_captain(Vec3::ZERO);
        sim.set_captain_attack_rate(c, rate);
        sim.plant_sprout(PikiColor::Red, Vec3::new(10.0, 0.0, 0.0));
        sim.set_input(c, CaptainInput { throw: true, ..Default::default() });
        for t in 0..300 {
            sim.step(&mut FlatHost);
            if sim.drain_events().any(|e| matches!(e, Event::Plucked { .. })) {
                return t;
            }
        }
        u32::MAX
    };
    assert!(pull(2.0) < pull(1.0));
}

/// Floor for x < 1000 and nothing beyond: a pit.
struct PitHost;

impl libpikmin::Host for PitHost {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        (pos.x < 1000.0).then_some(0.0)
    }
}

#[test]
fn a_pikmin_that_falls_into_a_pit_comes_back_to_its_captain() {
    let mut sim = sim();
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(1100.0, -500.0, 0.0), c).unwrap();
    let health = sim.piki(p).health;
    for _ in 0..3 {
        sim.step(&mut PitHost);
    }
    let at = sim.piki(p).pos;
    assert!(at.x < 100.0 && at.y > -50.0, "still in the pit: {at:?}");
    assert_eq!(sim.piki(p).health, health, "no damage");
    assert!(sim.piki(p).is_alive());

    // Over solid ground, a Pikmin far below a captain is left alone (it is on a lower floor).
    let mut sim = sim();
    sim.add_captain(Vec3::new(0.0, 600.0, 0.0));
    let p = sim.spawn_piki(PikiColor::Red, Vec3::new(50.0, 0.0, 0.0)).unwrap();
    for _ in 0..3 {
        sim.step(&mut PitHost);
    }
    assert!(sim.piki(p).pos.y < 100.0, "taken off a floor it stands on");
}

