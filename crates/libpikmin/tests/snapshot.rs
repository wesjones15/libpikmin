//! A replica built from snapshots is the sim the server had: every entity, field for field.

use libpikmin::captain::ThrowState;
use libpikmin::carry::{Carry, CarryPhase};
use libpikmin::onion::{OnionId, SproutId};
use libpikmin::piki::{Attack, AttackMode};
use libpikmin::plucker::{Plucker, PluckerJob};
use libpikmin::{
    CaptainId, CaptainInput, CreatureFrame, CreaturePart, Event, Happa, Host, KnockPhase, Params, PelletDesc, PelletId, PikiColor,
    PikiId, PikiState, Sim, Snapshot, Vec2, Vec3,
};

struct Arena;

impl Host for Arena {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn creatures(&mut self, out: &mut CreatureFrame) {
        out.clear();
        let part = CreaturePart { centre: Vec3::new(150.0, 15.0, 30.0), radius: 14.0, stickable: true };
        out.push(7, Vec3::new(150.0, 0.0, 30.0), 0.0, true, false, &[part]);
    }
}

fn dump(sim: &Sim) -> String {
    format!(
        "{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}",
        sim.captains(),
        sim.pikis(),
        sim.pellets(),
        sim.onions(),
        sim.sprouts().collect::<Vec<_>>(),
        sim.nectar_drops(),
        sim.puddles(),
    )
}

/// A sim with two captains, a squad each, Onions (one a ship), pellets, a sprout, an enemy to
/// throw at and a whistle or two: enough variety that most Pikmin states come up.
fn busy() -> (Sim, Vec<Event>) {
    let mut sim = Sim::new(Params::default(), 9);
    let a = sim.add_captain(Vec3::ZERO);
    let b = sim.add_captain(Vec3::new(-200.0, 0.0, 80.0));
    for i in 0..60 {
        let color = PikiColor::from_index(i);
        let at = Vec3::new(-30.0 - (i % 10) as f32 * 9.0, 0.0, (i / 10) as f32 * 11.0);
        sim.spawn_piki_in_squad(color, at, if i % 3 == 0 { b } else { a });
    }
    sim.add_onion(Vec3::new(-300.0, 0.0, -100.0), &[PikiColor::Red, PikiColor::Yellow, PikiColor::Blue]);
    sim.add_ship(Vec3::new(-460.0, 0.0, 40.0), Vec3::new(-340.0, 0.0, 40.0), Vec3::new(-440.0, 80.0, 40.0));
    let params = sim.params().clone();
    sim.spawn_pellet(PelletDesc::numbered(&params, 5, PikiColor::Red, Vec3::new(60.0, 0.0, -40.0), 0.7));
    sim.spawn_pellet(PelletDesc::numbered(&params, 1, PikiColor::Blue, Vec3::new(80.0, 0.0, 50.0), 0.1));
    sim.plant_sprout(PikiColor::Yellow, Vec3::new(20.0, 0.0, -60.0));
    let mut events = Vec::new();
    let mut host = Arena;
    for tick in 0..400 {
        let mut input = CaptainInput { stick: Vec2::new(0.3, 0.1), ..Default::default() };
        input.throw = tick % 40 < 3;
        input.whistle = tick % 97 == 0;
        input.dismiss = tick == 250;
        sim.set_input(a, input);
        sim.set_input(b, CaptainInput { whistle: tick % 53 == 0, throw: tick % 70 < 2, ..Default::default() });
        sim.step(&mut host);
        events.extend(sim.drain_events());
    }
    (sim, events)
}

#[test]
fn a_round_trip_reproduces_every_entity() {
    let (sim, events) = busy();
    assert!(sim.pikis().len() >= 60);
    assert!(
        events.len() > 10,
        "the scenario should raise events ({})",
        events.len()
    );
    let snap = Snapshot::of(&sim, &events);
    let bytes = snap.encode();
    println!("busy snapshot: {} bytes for {} Pikmin and {} events", bytes.len(), sim.pikis().len(), events.len());
    assert!(bytes.len() < 60_000, "a busy snapshot is {} bytes", bytes.len());
    let decoded = Snapshot::decode(&bytes).expect("decodes");
    assert_eq!(decoded.events, events);
    assert_eq!(decoded.creatures, snap.creatures);
    assert_eq!(decoded.creatures.len(), 1, "the arena's one creature went over");
    assert_eq!(decoded.tick, sim.tick());

    let mut replica = Sim::default();
    replica.apply_snapshot(&decoded);
    assert_eq!(dump(&replica), dump(&sim));
    assert_eq!(replica.tick(), sim.tick());
    // And re-encoding the replica's own entities gives the same bytes back.
    assert_eq!(Snapshot::of(&replica, &events).encode(), bytes);
}

#[test]
fn many_moments_round_trip() {
    // Not just the end state: states mid-throw, mid-carry and so on, every so often.
    let mut sim = Sim::new(Params::default(), 3);
    let a = sim.add_captain(Vec3::ZERO);
    for i in 0..30 {
        sim.spawn_piki_in_squad(PikiColor::from_index(i), Vec3::new(-20.0 - i as f32 * 3.0, 0.0, (i % 5) as f32 * 8.0), a);
    }
    sim.add_onion(Vec3::new(-250.0, 0.0, 0.0), &[PikiColor::Red, PikiColor::Yellow, PikiColor::Blue]);
    let params = sim.params().clone();
    sim.spawn_pellet(PelletDesc::numbered(&params, 5, PikiColor::Red, Vec3::new(70.0, 0.0, 0.0), 0.0));
    let mut host = Arena;
    let mut seen_states = std::collections::HashSet::new();
    let mut replica = Sim::default();
    for tick in 0..1200 {
        sim.set_input(
            a,
            CaptainInput {
                stick: Vec2::new((tick as f32 * 0.01).sin(), (tick as f32 * 0.013).cos()),
                throw: tick % 25 < 2,
                whistle: tick % 120 == 0,
                ..Default::default()
            },
        );
        sim.step(&mut host);
        let events: Vec<Event> = sim.drain_events().collect();
        for p in sim.pikis() {
            seen_states.insert(std::mem::discriminant(&p.state));
        }
        if tick % 37 == 0 {
            let bytes = Snapshot::of(&sim, &events).encode();
            let snap = Snapshot::decode(&bytes).expect("decodes");
            replica.apply_snapshot(&snap);
            assert_eq!(dump(&replica), dump(&sim), "tick {tick}");
        }
    }
    assert!(seen_states.len() >= 5, "only {} kinds of Pikmin state came up", seen_states.len());
    let held = sim.captain(CaptainId(0)).throw;
    assert!(matches!(held, ThrowState::Idle | ThrowState::Wait { .. }));
}

#[test]
fn bad_input_is_refused_not_trusted() {
    let (sim, events) = busy();
    let bytes = Snapshot::of(&sim, &events).encode();
    assert!(Snapshot::decode(&[]).is_none());
    assert!(Snapshot::decode(&bytes[..bytes.len() - 1]).is_none(), "one byte short");
    let mut long = bytes.clone();
    long.push(0);
    assert!(Snapshot::decode(&long).is_none(), "a trailing byte");
    let mut version = bytes.clone();
    version[0] ^= 0xFF;
    assert!(Snapshot::decode(&version).is_none(), "another version");
    // Truncated everywhere, and every byte flipped: never a panic.
    for cut in (0..bytes.len()).step_by(13) {
        assert!(Snapshot::decode(&bytes[..cut]).is_none(), "cut at {cut}");
    }
    for i in (0..bytes.len()).step_by(7) {
        let mut flipped = bytes.clone();
        flipped[i] = flipped[i].wrapping_add(0x55);
        let _ = Snapshot::decode(&flipped);
    }
}

#[test]
fn blending_moves_entities_between_two_snapshots() {
    let (mut sim, events) = busy();
    let from = Snapshot::of(&sim, &events);
    let mut host = Arena;
    for _ in 0..6 {
        sim.step(&mut host);
    }
    let to = Snapshot::of(&sim, &[]);
    let mut replica = Sim::default();
    replica.apply_snapshot_blend(&from, &to, 0.5);
    let moved = to
        .pikis
        .iter()
        .zip(&from.pikis)
        .position(|(b, a)| a.id == b.id && a.pos.distance(b.pos) > 1.0)
        .expect("something moved");
    let half = replica.pikis()[moved].pos;
    let (a, b) = (from.pikis[moved].pos, to.pikis[moved].pos);
    assert!(half.distance(a.lerp(b, 0.5)) < 0.01, "halfway between the two");
    // The rest of the state is `to`'s.
    assert_eq!(replica.pikis()[moved].state, to.pikis[moved].state);
    // t = 1 is `to` exactly.
    replica.apply_snapshot_blend(&from, &to, 1.0);
    assert_eq!(replica.pikis()[moved].pos, b);
}

#[test]
fn every_pikmin_state_round_trips() {
    let c = CaptainId(0);
    let states = vec![
        PikiState::Idle,
        PikiState::Squad { lost_timer: 1.5, near_counter: 3, reslotted: true, formed: true },
        PikiState::GoHang { captain: c },
        PikiState::Held { captain: c },
        PikiState::Flying { ground_frames: 4 },
        PikiState::Attack(Attack { target: 77, mode: AttackMode::Approach }),
        PikiState::Attack(Attack { target: 77, mode: AttackMode::JumpPrep }),
        PikiState::Attack(Attack { target: 77, mode: AttackMode::Jumping }),
        PikiState::Attack(Attack { target: 77, mode: AttackMode::Swing { frame: 3.0, hit: true } }),
        PikiState::Attack(Attack {
            target: 78,
            mode: AttackMode::Latched { part: 5, offset: Vec3::new(1.0, 2.0, 3.0), frame: 9.5, hit: false },
        }),
        PikiState::Knocked { phase: KnockPhase::Flying { frame: 1.0 }, whistled: Some(c) },
        PikiState::Knocked { phase: KnockPhase::Landing { frame: 2.0 }, whistled: None },
        PikiState::Knocked { phase: KnockPhase::Down { timer: 3.0 }, whistled: None },
        PikiState::Knocked { phase: KnockPhase::GettingUp { frame: 4.0 }, whistled: None },
        PikiState::Carry(Carry { pellet: PelletId(2), slot: 3, phase: CarryPhase::Go { progress: 9 } }),
        PikiState::Carry(Carry { pellet: PelletId(2), slot: 3, phase: CarryPhase::Lift { frame: 2.5, lifted: true } }),
        PikiState::Carry(Carry { pellet: PelletId(2), slot: 3, phase: CarryPhase::Move }),
        PikiState::Pressed { stun: 1.0, death_timer: 2.0 },
        PikiState::Swallowed { creature: 9, part: 2, offset: Vec3::new(0.5, 0.25, 0.125) },
        PikiState::Dying { frame: 3.0, end: 40.0 },
        PikiState::Fading { timer: 0.5 },
        PikiState::Absorb { puddle: libpikmin::NectarId(4), frame: 5.0, drinking: true, drank: false, finishing: true },
        PikiState::GrowUp { frame: 6.0, second: true, bloomed: false },
        PikiState::Enter { onion: OnionId(1), leg: 2, climbing: true, ratio: 0.5 },
        PikiState::Exit { onion: OnionId(1), leg: 1, ratio: 0.25 },
        PikiState::Plucker(Plucker { onion: OnionId(0), job: PluckerJob::Descend { leg: 1, ratio: 0.1 } }),
        PikiState::Plucker(Plucker { onion: OnionId(0), job: PluckerJob::Walk { sprout: SproutId(3), best: 4.0, stuck: 0.5 } }),
        PikiState::Plucker(Plucker { onion: OnionId(0), job: PluckerJob::Pull { sprout: SproutId(3), frame: 7.0, popped: true } }),
        PikiState::Dead,
    ];
    let mut sim = Sim::new(Params::default(), 1);
    sim.add_captain(Vec3::ZERO);
    for i in 0..states.len() {
        sim.spawn_piki(PikiColor::from_index(i), Vec3::new(i as f32 * 5.0, 0.0, 0.0));
    }
    let mut snap = Snapshot::of(&sim, &[]);
    for (p, s) in snap.pikis.iter_mut().zip(&states) {
        p.state = *s;
        p.happa = Happa::Flower;
        p.captain = Some(c);
        p.guided_throw = Some(libpikmin::piki::GuidedThrow { creature: Some(5), point: Some(Vec3::ONE), remaining: 2.0 });
    }
    // Every kind of event, too.
    let (p, o, cap, pe, sp) = (PikiId(1), OnionId(1), c, PelletId(1), SproutId(1));
    snap.events = vec![
        Event::PikiEnteredOnion { piki: p, onion: o },
        Event::PikiLeftOnion { piki: p, onion: o },
        Event::OnionLanded { onion: o },
        Event::NectarSplash { pos: Vec3::ONE },
        Event::NectarGone,
        Event::PikiDrinking { piki: p },
        Event::Bloomed { piki: p },
        Event::Whistle { captain: cap, centre: Vec3::ONE, radius: 90.0 },
        Event::Recruited { piki: p, captain: cap, stolen_from: Some(CaptainId(1)) },
        Event::Recruited { piki: p, captain: cap, stolen_from: None },
        Event::Thrown { piki: p, captain: cap },
        Event::Landed { piki: p },
        Event::Latched { piki: p, creature: 3 },
        Event::Hit { piki: p, creature: 3, damage: 4.0, latched: true },
        Event::KnockedOff { piki: p, creature: Some(3) },
        Event::KnockedOff { piki: p, creature: None },
        Event::PikiHurt { piki: p, damage: 1.0 },
        Event::ShipPartCollected { pellet: pe, ship: o, part: 7, total: 8 },
        Event::CarrierGrabbed { piki: p, pellet: pe },
        Event::Deflowered { piki: p },
        Event::PikiGone { piki: p, pos: Vec3::ONE },
        Event::PikiDied { piki: p, cause: libpikmin::DeathCause::Eaten },
        Event::Swallowed { piki: p, creature: 3 },
        Event::SpitOut { piki: p },
        Event::CaptainHurt { captain: cap, damage: 2.0, health: 50.0 },
        Event::CaptainDown { captain: cap },
        Event::Dismissed { captain: cap },
        Event::Punched { captain: cap, creature: 3, damage: 9.0 },
        Event::PelletLifted { pellet: pe, onion: o },
        Event::PelletRerouted { pellet: pe, onion: o },
        Event::PelletDropped { pellet: pe },
        Event::PelletArrived { pellet: pe, onion: o },
        Event::PelletDelivered { pellet: pe, onion: o, color: PikiColor::Yellow, seeds: -2 },
        Event::SeedLaunched { onion: o, sprout: sp, color: PikiColor::Blue },
        Event::SeedStored { onion: o, color: PikiColor::Red },
        Event::SproutPlanted { sprout: sp },
        Event::SproutGrew { sprout: sp, happa: Happa::Bud },
        Event::SproutWilted { sprout: sp },
        Event::Plucked { captain: cap, piki: p, sprout: sp },
        Event::EmergencyPikmin { onion: o, captain: cap, count: 3 },
        Event::PluckerOut { piki: p, onion: o },
        Event::PluckerPulled { plucker: p, plucked: PikiId(2), captain: Some(cap) },
        Event::CaptainJumped { captain: cap, air: true },
        Event::CaptainStomped { captain: cap, pos: Vec3::ONE, drop: 3.0 },
        Event::PikiGrew { piki: p, happa: Happa::Flower },
        Event::StrikeStarted { captain: cap },
        Event::CaptainLanded { captain: cap, drop: 4.0, stomp: false },
        Event::PikiStomped { piki: p, captain: cap, pos: Vec3::ONE },
    ];
    let bytes = snap.encode();
    let back = Snapshot::decode(&bytes).expect("decodes");
    assert_eq!(back.events, snap.events);
    assert_eq!(format!("{:#?}", back.pikis), format!("{:#?}", snap.pikis));
}
