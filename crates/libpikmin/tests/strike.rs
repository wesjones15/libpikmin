//! The dismiss button as a melee strike, and Pikmin pace set against their captain's speed.

use libpikmin::{CaptainInput, CreatureFrame, CreatureId, CreaturePart, DamageSource, Event, Host, Params, PikiColor, Sim, Vec3};

#[derive(Default)]
struct Arena {
    creatures: Vec<(CreatureId, Vec3)>,
    hits: Vec<(CreatureId, f32, DamageSource)>,
}

impl Host for Arena {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        for (id, pos) in &self.creatures {
            let part = CreaturePart { centre: *pos + Vec3::Y * 10.0, radius: 12.0, stickable: true };
            out.push(*id, *pos, 0.0, true, false, &[part]);
        }
    }

    fn damage(&mut self, creature: CreatureId, amount: f32, _part: Option<u32>, from: DamageSource) -> bool {
        self.hits.push((creature, amount, from));
        true
    }
}

fn press(sim: &mut Sim, c: libpikmin::CaptainId, arena: &mut Arena) {
    sim.set_input(c, CaptainInput { dismiss: true, ..Default::default() });
    sim.step(arena);
    sim.set_input(c, CaptainInput::default());
}

fn run(sim: &mut Sim, arena: &mut Arena, seconds: f32) {
    for _ in 0..(seconds * libpikmin::TICK_HZ as f32) as u32 {
        sim.step(arena);
    }
}

fn setup(strike: bool) -> (Sim, Arena, libpikmin::CaptainId) {
    let mut params = Params::default();
    params.navi.punch_damage = 10.0;
    let mut sim = Sim::new(params, 3);
    let c = sim.add_captain(Vec3::ZERO);
    for k in 0..4 {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-30.0 - k as f32 * 8.0, 0.0, 0.0), c);
    }
    let mut arena = Arena::default();
    arena.creatures = vec![(1, Vec3::new(70.0, 0.0, 0.0)), (2, Vec3::new(-50.0, 0.0, 30.0)), (3, Vec3::new(900.0, 0.0, 0.0))];
    if strike {
        sim.set_dismiss_strike(3.0, 60.0, 4.0, 2.0);
    }
    run(&mut sim, &mut arena, 2.0);
    (sim, arena, c)
}

#[test]
fn without_strikes_the_button_still_dismisses() {
    let (mut sim, mut arena, c) = setup(false);
    let squad = |sim: &Sim| sim.pikis().iter().filter(|p| p.state.in_squad()).count();
    assert_eq!(squad(&sim), 4);
    press(&mut sim, c, &mut arena);
    assert_eq!(squad(&sim), 0);
    assert!(arena.hits.is_empty());
}

#[test]
fn a_strike_hits_everything_around_for_more_than_a_punch_and_leaves_the_squad_alone() {
    let (mut sim, mut arena, c) = setup(true);
    press(&mut sim, c, &mut arena);
    assert!(sim.drain_events().any(|e| matches!(e, Event::StrikeStarted { .. })));
    assert!(sim.captain(c).strike.is_some());
    run(&mut sim, &mut arena, 1.0);
    let mut ids: Vec<_> = arena.hits.iter().map(|h| h.0).collect();
    ids.sort();
    assert_eq!(ids, [1, 2], "both nearby creatures, not the far one");
    for (_, amount, from) in &arena.hits {
        assert_eq!(*amount, 30.0, "3x a 10 damage punch");
        assert_eq!(*from, DamageSource::Strike(c));
    }
    assert_eq!(sim.pikis().iter().filter(|p| p.state.in_squad()).count(), 4, "nobody is dismissed");
    assert!(sim.captain(c).strike.is_none());
}

#[test]
fn the_cooldown_blocks_a_second_strike_for_four_seconds() {
    let (mut sim, mut arena, c) = setup(true);
    press(&mut sim, c, &mut arena);
    run(&mut sim, &mut arena, 1.0);
    let first = arena.hits.len();
    press(&mut sim, c, &mut arena);
    run(&mut sim, &mut arena, 1.0);
    assert_eq!(arena.hits.len(), first, "still cooling down 2 s in");
    run(&mut sim, &mut arena, 2.2);
    press(&mut sim, c, &mut arena);
    run(&mut sim, &mut arena, 1.0);
    assert!(arena.hits.len() > first, "ready after 4 s");
}

#[test]
fn a_strike_sends_the_squad_charging_the_way_he_struck_for_a_couple_of_seconds() {
    let (mut sim, mut arena, c) = setup(true);
    // He faces +x (yaw 90 degrees); the squad trails behind at -x.
    let pose = libpikmin::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
    sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
    run(&mut sim, &mut arena, 0.5);
    assert!(!sim.captain(c).swarming);
    sim.set_input(c, CaptainInput { pose: Some(pose), dismiss: true, ..Default::default() });
    sim.step(&mut arena);
    sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
    run(&mut sim, &mut arena, 0.5);
    let c0 = sim.captain(c);
    assert!(c0.swarming, "the squad swarms");
    assert!((c0.plate_yaw - std::f32::consts::FRAC_PI_2).abs() < 0.2, "toward +x, got {}", c0.plate_yaw);
    run(&mut sim, &mut arena, 2.0);
    assert!(!sim.captain(c).swarming, "and settles back after the charge");
}

#[test]
fn a_stick_swarm_beats_the_charge() {
    let (mut sim, mut arena, c) = setup(true);
    let pose = libpikmin::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
    sim.set_input(c, CaptainInput { pose: Some(pose), dismiss: true, ..Default::default() });
    sim.step(&mut arena);
    sim.set_input(c, CaptainInput { pose: Some(pose), swarm: libpikmin::Vec2::new(0.0, -1.0), ..Default::default() });
    run(&mut sim, &mut arena, 0.5);
    let yaw = sim.captain(c).plate_yaw;
    assert!((yaw - std::f32::consts::PI).abs() < 0.3, "toward -z, got {yaw}");
}
