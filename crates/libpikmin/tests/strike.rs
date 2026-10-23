//! The dismiss button as a melee strike, and Pikmin pace set against their captain's speed.

use libpikmin::{CaptainInput, CreatureFrame, CreatureId, CreaturePart, DamageSource, Event, Host, Params, PikiColor, PikiState, Sim, Snapshot, Vec3};
use libpikmin::piki::AttackMode;

#[derive(Default)]
struct Arena {
    creatures: Vec<(CreatureId, Vec3)>,
    hits: Vec<(CreatureId, f32, DamageSource)>,
    flying: bool,
}

impl Host for Arena {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        for (id, pos) in &self.creatures {
            let part = CreaturePart { centre: *pos + Vec3::Y * 10.0, radius: 12.0, stickable: true };
            out.push(*id, *pos, 0.0, true, self.flying, &[part]);
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
fn a_strike_hits_everything_around_for_more_than_a_punch_without_dismissing() {
    let (mut sim, mut arena, c) = setup(true);
    press(&mut sim, c, &mut arena);
    assert!(sim.drain_events().any(|e| matches!(e, Event::StrikeStarted { .. })));
    assert!(sim.captain(c).strike.is_some());
    run(&mut sim, &mut arena, 1.0);
    let mut ids: Vec<_> = arena.hits.iter().filter(|h| matches!(h.2, DamageSource::Strike(_))).map(|h| h.0).collect();
    ids.sort();
    assert_eq!(ids, [1, 2], "both nearby creatures, not the far one");
    for (_, amount, from) in arena.hits.iter().filter(|h| matches!(h.2, DamageSource::Strike(_))) {
        assert_eq!(*amount, 30.0, "3x a 10 damage punch");
        assert_eq!(*from, DamageSource::Strike(c));
    }
    assert_eq!(sim.pikis().iter().filter(|p| p.dismissed).count(), 0, "nobody is dismissed");
    assert!(sim.captain(c).strike.is_none());
}

#[test]
fn the_cooldown_blocks_a_second_strike_for_four_seconds() {
    let (mut sim, mut arena, c) = setup(true);
    press(&mut sim, c, &mut arena);
    run(&mut sim, &mut arena, 1.0);
    let first = arena.hits.iter().filter(|h| matches!(h.2, DamageSource::Strike(_))).count();
    press(&mut sim, c, &mut arena);
    run(&mut sim, &mut arena, 1.0);
    assert_eq!(arena.hits.iter().filter(|h| matches!(h.2, DamageSource::Strike(_))).count(), first, "still cooling down 2 s in");
    run(&mut sim, &mut arena, 2.2);
    press(&mut sim, c, &mut arena);
    run(&mut sim, &mut arena, 1.0);
    assert!(arena.hits.iter().filter(|h| matches!(h.2, DamageSource::Strike(_))).count() > first, "ready after 4 s");
}

#[test]
fn strike_sends_squad_to_forward_enemy_to_jump_and_latch() {
    let (mut sim, mut arena, c) = setup(true);
    arena.creatures = vec![(1, Vec3::new(120.0, 0.0, 0.0)), (2, Vec3::new(-80.0, 0.0, 0.0))];
    let pose = libpikmin::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
    sim.set_input(c, CaptainInput { pose: Some(pose), dismiss: true, ..Default::default() });
    sim.step(&mut arena);
    assert!(sim.pikis().iter().all(|p| matches!(p.state, PikiState::Attack(a) if a.target == 1 && matches!(a.mode, AttackMode::StrikeRush | AttackMode::StrikeJump { .. }))));
    sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
    run(&mut sim, &mut arena, 2.0);
    assert!(sim.pikis().iter().all(|p| p.state.latched_to() == Some(1)), "all Pikmin should stick to the forward enemy: {:?}", sim.pikis().iter().map(|p| p.state).collect::<Vec<_>>());
    assert!(arena.hits.iter().any(|h| h.0 == 1 && matches!(h.2, DamageSource::Piki(_))));
    assert!(!arena.hits.iter().any(|h| h.0 == 2 && matches!(h.2, DamageSource::Piki(_))));
}

#[test]
fn strike_jump_tracks_a_moving_airborne_enemy() {
    let (mut sim, mut arena, c) = setup(true);
    arena.creatures = vec![(1, Vec3::new(110.0, 145.0, 0.0))];
    arena.flying = true;
    let pose = libpikmin::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
    sim.set_input(c, CaptainInput { pose: Some(pose), dismiss: true, ..Default::default() });
    sim.step(&mut arena);
    sim.set_input(c, CaptainInput { pose: Some(pose), ..Default::default() });
    for _ in 0..90 {
        arena.creatures[0].1.z += 0.6;
        sim.step(&mut arena);
    }
    assert!(sim.pikis().iter().all(|p| p.state.latched_to() == Some(1)), "guided jump should stick to a moving flying target: {:?}", sim.pikis().iter().map(|p| p.state).collect::<Vec<_>>());
}

#[test]
fn a_hundred_charging_pikmin_fit_in_a_small_replica_snapshot() {
    let mut sim = Sim::new(Params::default(), 3);
    let c = sim.add_captain(Vec3::ZERO);
    for i in 0..100 {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-30.0 - i as f32 * 0.2, 0.0, 0.0), c);
    }
    sim.set_dismiss_strike(3.0, 60.0, 4.0, 2.0);
    let mut arena = Arena { creatures: vec![(1, Vec3::new(200.0, 0.0, 0.0))], ..Default::default() };
    let pose = libpikmin::CaptainPose { pos: Vec3::ZERO, vel: Vec3::ZERO, face: std::f32::consts::FRAC_PI_2 };
    sim.set_input(c, CaptainInput { pose: Some(pose), dismiss: true, ..Default::default() });
    sim.step(&mut arena);
    assert_eq!(sim.pikis().iter().filter(|p| matches!(p.state, PikiState::Attack(a) if a.target == 1)).count(), 100);
    let wire = Snapshot::of(&sim, &[]).encode_replica();
    assert!(wire.len() < 4_000, "100 charging Pikmin need {} snapshot bytes", wire.len());
    assert_eq!(Snapshot::decode(&wire).unwrap().pikis.len(), 100);
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
