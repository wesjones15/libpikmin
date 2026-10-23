//! Pikmin that fall off the map are brought back to their captain, as the captain himself is
//! (`Sim::rescue_fallen`, a host extension): out of a void in any job, and off far lower ground
//! when they were only standing about.

use libpikmin::piki::PikiState;
use libpikmin::{Event, Host, Params, PikiColor, Sim, TICK_DT, Vec3};

/// A platform around the origin, a second platform 1500 units lower out at x > 2000, and nothing
/// anywhere else (the probe only sees floors at or below the point it starts from).
struct Platforms;

impl Host for Platforms {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        let on_start = pos.x.abs() <= 600.0 && pos.z.abs() <= 600.0;
        if on_start && pos.y >= 0.0 {
            return Some(0.0);
        }
        (pos.x > 2000.0 && pos.y >= -1500.0).then_some(-1500.0)
    }
}

fn run(sim: &mut Sim, secs: f32) -> Vec<Event> {
    let mut host = Platforms;
    let mut events = Vec::new();
    for _ in 0..(secs / TICK_DT).round() as u32 {
        sim.step(&mut host);
        events.extend(sim.drain_events());
    }
    events
}

fn died(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::PikiDied { .. }))
}

#[test]
fn a_pikmin_that_falls_into_a_void_comes_back_unharmed() {
    let mut sim = Sim::new(Params::default(), 21);
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-50.0, 0.0, 0.0), c).unwrap();
    // Out past the platform with nothing below it: it falls forever unless it is brought back.
    sim.piki_mut(id).pos = Vec3::new(-1500.0, 40.0, 0.0);
    let events = run(&mut sim, 12.0);
    let p = sim.piki(id);
    assert!(!died(&events), "the Pikmin died");
    assert!(p.pos.y > -100.0 && p.pos.x.abs() < 700.0, "still lost: {:?}", p.pos);
}

#[test]
fn a_busy_pikmin_lets_go_and_comes_back_from_a_void() {
    let mut sim = Sim::new(Params::default(), 22);
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(-50.0, 0.0, 0.0), c).unwrap();
    {
        // Flattened under something that has fallen off the map with it.
        let p = sim.piki_mut(id);
        p.pos = Vec3::new(-1500.0, -600.0, 0.0);
        p.state = PikiState::Pressed { stun: 100.0, death_timer: 100.0 };
    }
    let events = run(&mut sim, 3.0);
    let p = sim.piki(id);
    assert!(!died(&events), "the Pikmin died");
    assert!(p.pos.y > -300.0 && p.pos.x.abs() < 700.0, "still lost: {:?}", p.pos);
    assert!(!matches!(p.state, PikiState::Pressed { .. }), "still pressed: {:?}", p.state);
}

#[test]
fn a_loose_pikmin_far_below_on_lower_ground_comes_back_but_a_near_drop_stays() {
    let mut sim = Sim::new(Params::default(), 23);
    sim.add_captain(Vec3::ZERO);
    let far = sim.spawn_piki(PikiColor::Yellow, Vec3::new(-50.0, 0.0, 0.0)).unwrap();
    let near = sim.spawn_piki(PikiColor::Yellow, Vec3::new(-60.0, 0.0, 0.0)).unwrap();
    sim.piki_mut(far).pos = Vec3::new(2500.0, -1400.0, 0.0); // 1400 below the captain, over the lower platform
    sim.piki_mut(near).pos = Vec3::new(2500.0, -500.0, 0.0); // 500 below the captain, over the lower platform
    let events = run(&mut sim, 0.2);
    assert!(!died(&events));
    assert!(sim.piki(far).pos.x.abs() < 700.0, "the far one is still down there: {:?}", sim.piki(far).pos);
    assert!(sim.piki(near).pos.x > 2000.0, "a drop onto nearer ground was undone: {:?}", sim.piki(near).pos);
}
