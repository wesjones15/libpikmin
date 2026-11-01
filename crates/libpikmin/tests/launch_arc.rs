//! A host launch (jump pad / geyser): the captain's arc must use the host gravity and keep its
//! horizontal throw, not decay toward a standing target.
use libpikmin::host::FlatHost;
use libpikmin::{Params, Sim, Vec3};

#[test]
fn a_launched_captain_keeps_his_throw_and_the_hosts_gravity() {
    let mut sim = Sim::new(Params::default(), 51);
    let c = sim.add_captain(Vec3::ZERO);
    let v = Vec3::new(300.0, 500.0, 0.0);
    let g = 245.25;
    sim.launch_captain(c, v, g, false);
    let start = sim.captain(c).pos;
    let mut apex = start.y;
    let mut landed = None;
    for _ in 0..600 {
        sim.step(&mut FlatHost);
        let p = sim.captain(c).pos;
        apex = apex.max(p.y);
        if landed.is_none() && sim.captain(c).on_ground && p.y <= start.y + 1.0 {
            landed = Some(p);
            break;
        }
    }
    let landed = landed.expect("came back down");
    let expected_apex = 500.0 * 500.0 / (2.0 * g);
    assert!((apex - start.y - expected_apex).abs() < 15.0, "apex {} vs {expected_apex}", apex - start.y);
    assert!(landed.x > 1000.0, "kept the horizontal throw: {landed:?}");
}
