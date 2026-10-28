//! Purple and White Pikmin (Pikmin 2): the disc's tuning, carrying, speed, the pound and the poison.

use libpikmin::captain::ThrowState;
use libpikmin::piki::PikiState;
use libpikmin::{CaptainInput, CreatureFrame, CreaturePart, Event, Host, Params, PelletDesc, PikiColor, Sim, Vec3};

/// A flying creature (id 7) hovering over `(100, ., 0)`.
struct Flier;

impl Host for Flier {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        let part = CreaturePart { centre: Vec3::new(100.0, 60.0, 0.0), radius: 14.0, stickable: true };
        out.push(7, Vec3::new(100.0, 40.0, 0.0), 0.0, true, true, &[part]);
    }

    fn damage(&mut self, _: u64, _: f32, _: Option<u32>, _: libpikmin::DamageSource) -> bool {
        true
    }
}

#[test]
fn pikmin2_text_parms_replace_the_code_defaults() {
    let mut p = Params::default();
    assert_eq!(p.p2.purple_attack_power, 10.0);
    p.apply_pikmin2_text(
        b"# Piki::Parms\n{\n\t{P003} 4 20.000000 \t# \x8d\x95\n\t{P004} 4 10.000000\n\t{P001} 4 1.200000\n\t{P002} 4 0.800000\n\t{P006} 4 3.000000\n\t{P005} 4 0.600000\n\t{zzzz} 4 9.0\n\t{_eof} \n}\n",
    );
    assert_eq!((p.p2.purple_attack_power, p.p2.white_attack_power), (20.0, 10.0));
    assert_eq!((p.p2.white_speed_multiplier, p.p2.purple_speed_multiplier), (1.2, 0.8));
    assert_eq!((p.p2.white_carry_power, p.p2.purple_carry_power), (3.0, 0.6));
}

#[test]
fn the_discs_pikiparms_load_when_extracted() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/p2/parms");
    let mut p = Params::default();
    if p.load_pikmin2_dir(&dir).is_empty() {
        eprintln!("SKIP: no Pikmin 2 parms");
        return;
    }
    assert_eq!((p.p2.purple_attack_power, p.p2.white_attack_power), (20.0, 10.0));
    assert_eq!((p.p2.white_speed_multiplier, p.p2.purple_speed_multiplier), (1.2, 0.8));
    assert_eq!((p.p2.white_carry_power, p.p2.purple_carry_power), (3.0, 0.6));
    assert_eq!((p.p2.bud_carry_bonus, p.p2.flower_carry_bonus), (0.5, 1.0));
    assert_eq!((p.p2.pound_radius, p.p2.pound_damage), (60.0, 20.0));
}

/// A corpse needing `min` carriers, an Onion taking `color`, and one Pikmin of `color` sent at it.
fn lift(color: PikiColor, min: u32) -> bool {
    let mut sim = Sim::new(Params::default(), 3);
    sim.add_onion(Vec3::new(300.0, 0.0, 0.0), &[color]);
    let pellet = sim.spawn_pellet(PelletDesc {
        ship_part: None,
        pos: Vec3::ZERO,
        yaw: 0.0,
        radius: 12.0,
        height: 10.0,
        min_carriers: min,
        max_carriers: min.max(10),
        seeds_matching: 1,
        seeds_other: 1,
        color: None,
        creature: None,
    });
    sim.spawn_piki(color, Vec3::new(30.0, 0.0, 0.0));
    let mut host = libpikmin::host::FlatHost;
    assert_eq!(sim.recruit_to_pellet(pellet, None, 200.0), 1);
    let mut lifted = false;
    for _ in 0..600 {
        sim.step(&mut host);
        lifted |= sim.drain_events().any(|e| matches!(e, Event::PelletLifted { .. }));
    }
    lifted
}

#[test]
fn a_lone_purple_lifts_what_ten_blues_would() {
    assert!(lift(PikiColor::Purple, 10), "a Purple counts as ten carriers");
    assert!(!lift(PikiColor::Blue, 10), "one Blue cannot");
    assert!(lift(PikiColor::White, 1));
}

/// How far along +x a lone Pikmin of `color` gets in 3 s behind a captain running off at 400 units a second.
fn follows(color: PikiColor) -> f32 {
    let mut sim = Sim::new(Params::default(), 3);
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(color, Vec3::new(-30.0, 0.0, 0.0), c).unwrap();
    let mut host = libpikmin::host::FlatHost;
    for _ in 0..30 {
        sim.step(&mut host);
    }
    let mut pos = Vec3::ZERO;
    for _ in 0..90 {
        pos.x += 400.0 / 30.0;
        let pose = libpikmin::captain::CaptainPose { pos, vel: Vec3::new(400.0, 0.0, 0.0), face: std::f32::consts::FRAC_PI_2 };
        sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
        sim.step(&mut host);
    }
    sim.piki(id).pos.x
}

#[test]
fn white_runs_faster_and_purple_slower_than_blue() {
    let (blue, white, purple) = (follows(PikiColor::Blue), follows(PikiColor::White), follows(PikiColor::Purple));
    assert!(white > blue && blue > purple, "{white} {blue} {purple}");
}

/// A Purple in a squad thrown at `at`, with the cursor homing so it lands where aimed.
fn throw_purple(at: Vec3, lock: bool) -> Vec<Event> {
    let mut sim = Sim::new(Params::default(), 5);
    sim.set_cursor_homing(true);
    let c = sim.add_captain(Vec3::ZERO);
    sim.spawn_piki_in_squad(PikiColor::Purple, Vec3::new(-20.0, 0.0, 0.0), c).unwrap();
    let mut host = Flier;
    let step = |sim: &mut Sim, host: &mut Flier, throw: bool| {
        sim.set_input(c, CaptainInput { cursor: Some(at), throw, ..Default::default() });
        sim.step(host);
    };
    for _ in 0..60 {
        step(&mut sim, &mut host, false);
    }
    if lock {
        sim.set_lock_target(c, Some(7));
    }
    for _ in 0..40 {
        step(&mut sim, &mut host, true);
        if matches!(sim.captain(c).throw, ThrowState::Wait { .. }) {
            break;
        }
    }
    sim.drain_events().for_each(drop);
    step(&mut sim, &mut host, false);
    let mut events = Vec::new();
    for _ in 0..200 {
        step(&mut sim, &mut host, false);
        events.extend(sim.drain_events());
    }
    events
}

#[test]
fn a_purple_landing_beside_a_flier_brings_it_down() {
    let events = throw_purple(Vec3::new(110.0, 0.0, 0.0), false);
    assert!(events.iter().any(|e| matches!(e, Event::Pounded { .. })), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, Event::Grounded { creature: 7, .. })), "{events:?}");
}

#[test]
fn a_purple_landing_far_from_a_flier_does_not() {
    let events = throw_purple(Vec3::new(-150.0, 0.0, 120.0), false);
    assert!(events.iter().any(|e| matches!(e, Event::Pounded { .. })), "it still pounds the ground");
    assert!(!events.iter().any(|e| matches!(e, Event::Grounded { .. })), "{events:?}");
}

#[test]
fn a_purple_that_reaches_a_flier_in_the_air_pounds_it() {
    let events = throw_purple(Vec3::new(100.0, 60.0, 0.0), true);
    assert!(events.iter().any(|e| matches!(e, Event::Hit { .. })), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, Event::Grounded { creature: 7, .. })), "{events:?}");
}

#[test]
fn a_white_that_is_eaten_poisons_its_eater() {
    struct Eater;
    impl Host for Eater {
        fn ground_height(&self, _: Vec3) -> Option<f32> {
            Some(0.0)
        }
        fn creatures(&mut self, out: &mut CreatureFrame) {
            out.clear();
            let part = CreaturePart { centre: Vec3::new(10.0, 10.0, 0.0), radius: 14.0, stickable: true };
            out.push(9, Vec3::new(10.0, 0.0, 0.0), 0.0, true, false, &[part]);
        }
    }
    let mut sim = Sim::new(Params::default(), 5);
    let mut host = Eater;
    let white = sim.spawn_piki(PikiColor::White, Vec3::new(8.0, 0.0, 0.0)).unwrap();
    sim.spawn_piki(PikiColor::Blue, Vec3::new(12.0, 0.0, 0.0)).unwrap();
    sim.step(&mut host);
    assert_eq!(sim.swallow_pikis(9, 0, Vec3::new(10.0, 0.0, 0.0), 30.0, 5), 2);
    assert!(matches!(sim.piki(white).state, PikiState::Swallowed { .. }));
    assert_eq!(sim.chew(9), 2);
    let events: Vec<_> = sim.drain_events().collect();
    let poisoned = events.iter().filter(|e| matches!(e, Event::PoisonedEater { .. })).count();
    assert_eq!(poisoned, 1, "only the White: {events:?}");
}
