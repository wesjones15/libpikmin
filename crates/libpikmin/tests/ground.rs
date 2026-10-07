//! Ground probes against a host that, like a raycasting 3D host, only sees floors below the
//! probe point. Flat test hosts answer the same height wherever the probe starts, which hid
//! bodies sinking into slopes (found in Risk of Rain 2, where everything fell through the map).

use libpikmin::{CaptainInput, Event, Host, Params, PikiColor, Sim, TICK_DT, Vec2, Vec3};

/// Flat until x = 0, then a ramp rising `slope` per unit of x. A probe that starts under the
/// surface misses it, and there's nothing below: a void.
struct RampHost {
    slope: f32,
}

impl RampHost {
    fn surface(&self, x: f32) -> f32 {
        x.max(0.0) * self.slope
    }
}

impl Host for RampHost {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        let g = self.surface(pos.x);
        (pos.y >= g).then_some(g)
    }
}

fn run(sim: &mut Sim, host: &mut RampHost, secs: f32, ev: &mut Vec<Event>) {
    for _ in 0..(secs / TICK_DT).round() as u32 {
        sim.step(host);
        ev.extend(sim.drain_events());
    }
}

#[test]
fn captain_and_squad_walk_up_a_slope() {
    let mut sim = Sim::new(Params::default(), 11);
    let mut host = RampHost { slope: 0.4 };
    let c = sim.add_captain(Vec3::new(-50.0, 0.0, 0.0));
    for i in 0..10 {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-70.0 - i as f32 * 5.0, 0.0, 0.0), c);
    }
    let mut ev = Vec::new();
    sim.set_input(c, CaptainInput { stick: Vec2::new(1.0, 0.0), ..Default::default() });
    run(&mut sim, &mut host, 4.0, &mut ev);

    let cap = sim.captain(c).pos;
    assert!(cap.x > 200.0, "should have walked up the ramp, at {cap:?}");
    assert!((cap.y - host.surface(cap.x)).abs() < 1.0, "off the ramp: {cap:?}");
    for p in sim.pikis() {
        let g = host.surface(p.pos.x);
        assert!(p.pos.y > g - 1.0, "{:?} sank into the ramp at {:?}", p.id, p.pos);
        assert!(p.pos.x > 100.0, "{:?} didn't keep up: {:?}", p.id, p.pos);
    }
}

#[test]
fn a_pikmin_dropped_from_high_up_lands() {
    let mut sim = Sim::new(Params::default(), 12);
    let mut host = RampHost { slope: 0.0 };
    let c = sim.add_captain(Vec3::ZERO);
    let id = sim.spawn_piki_in_squad(PikiColor::Blue, Vec3::new(-20.0, 2000.0, 0.0), c).unwrap();
    let mut ev = Vec::new();
    run(&mut sim, &mut host, 6.0, &mut ev);

    let p = sim.piki(id);
    assert!(p.pos.y.abs() < 1.0, "fell through the floor: {:?}", p.pos);
}
