//! Walls ([`Host::walls`]) and the host-set cursor ([`CaptainInput::cursor`]).

use libpikmin::{CaptainInput, Event, Host, Params, PikiColor, PikiState, Sim, TICK_DT, Vec2, Vec3};

/// Flat floor with a wall along x = `wall`, facing -x.
struct WallHost {
    wall: f32,
}

impl Host for WallHost {
    fn ground_height(&self, _pos: Vec3) -> Option<f32> {
        Some(0.0)
    }

    fn walls(&self, centre: Vec3, radius: f32) -> Vec3 {
        Vec3::new(centre.x.min(self.wall - radius), centre.y, centre.z)
    }
}

fn run(sim: &mut Sim, host: &mut WallHost, secs: f32, ev: &mut Vec<Event>) {
    for _ in 0..(secs / TICK_DT).round() as u32 {
        sim.step(host);
        ev.extend(sim.drain_events());
    }
}

#[test]
fn captain_and_squad_stop_at_a_wall() {
    let mut sim = Sim::new(Params::default(), 3);
    let mut host = WallHost { wall: 200.0 };
    let c = sim.add_captain(Vec3::ZERO);
    for i in 0..10 {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-20.0 - i as f32 * 5.0, 0.0, 0.0), c);
    }
    let mut ev = Vec::new();
    sim.set_input(c, CaptainInput { stick: Vec2::new(1.0, 0.0), ..Default::default() });
    run(&mut sim, &mut host, 6.0, &mut ev);

    let radius = sim.params().navi.body_collision_radius;
    let cap = sim.captain(c).pos;
    assert!(cap.x > 150.0, "should have walked up to the wall, at {cap:?}");
    assert!(cap.x <= 200.0 - radius + 0.01, "walked into the wall: {cap:?}");
    for p in sim.pikis() {
        assert!(p.pos.x <= 200.0 - p.size + 0.01, "{:?} in the wall at {:?}", p.id, p.pos);
    }
}

#[test]
fn a_pikmin_thrown_at_a_wall_lands_in_front_of_it() {
    let mut sim = Sim::new(Params::default(), 4);
    let mut host = WallHost { wall: 120.0 };
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(PikiColor::Yellow, Vec3::new(-20.0, 0.0, 0.0), c).unwrap();
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 1.0, &mut ev);

    // Aim well past the wall: the throw is aimed there, the wall catches it.
    let aim = Vec3::new(400.0, 0.0, 0.0);
    sim.set_input(c, CaptainInput { throw: true, cursor: Some(aim), ..Default::default() });
    run(&mut sim, &mut host, 1.0, &mut ev);
    sim.set_input(c, CaptainInput { cursor: Some(aim), ..Default::default() });
    run(&mut sim, &mut host, 2.0, &mut ev);

    assert!(ev.iter().any(|e| matches!(e, Event::Thrown { piki, .. } if *piki == id)));
    let p = sim.piki(id);
    assert!(!matches!(p.state, PikiState::Flying { .. }), "still flying: {:?}", p.state);
    assert!(p.pos.x > 60.0, "should have flown toward the wall, at {:?}", p.pos);
    assert!(p.pos.x <= 120.0 - p.size + 0.01, "went through the wall: {:?}", p.pos);
}

#[test]
fn host_cursor_is_kept_within_reach() {
    let mut sim = Sim::new(Params::default(), 5);
    let mut host = WallHost { wall: f32::INFINITY };
    let c = sim.add_captain(Vec3::new(10.0, 0.0, 10.0));
    let reach = sim.params().navi.cursor_max_radius;
    let mut ev = Vec::new();

    sim.set_input(c, CaptainInput { cursor: Some(Vec3::new(10.0, 0.0, 60.0)), ..Default::default() });
    run(&mut sim, &mut host, 0.1, &mut ev);
    let w = sim.captain(c).cursor_world;
    assert!((w.x - 10.0).abs() < 1e-3 && (w.z - 60.0).abs() < 1e-3, "cursor at {w:?}");

    sim.set_input(c, CaptainInput { cursor: Some(Vec3::new(10.0 + reach * 4.0, 0.0, 10.0)), ..Default::default() });
    run(&mut sim, &mut host, 0.1, &mut ev);
    let off = sim.captain(c).cursor;
    assert!((off.length() - reach).abs() < 1e-2, "cursor offset {off:?}, reach {reach}");
    assert!(off.x > 0.0);
}
