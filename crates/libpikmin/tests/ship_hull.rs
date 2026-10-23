//! A landed Dolphin is solid to the Pikmin walking about on the ground; one in the air is not.

use libpikmin::host::FlatHost;
use libpikmin::{Params, PikiColor, Sim, Vec3};

fn settled(flying: bool) -> f32 {
    let mut sim = Sim::new(Params::default(), 3);
    let ship = sim.add_ship(Vec3::new(500.0, 0.0, 0.0), Vec3::new(600.0, 0.0, 0.0), Vec3::new(500.0, 74.0, 0.0));
    if flying {
        sim.set_onion_pose(ship, Vec3::new(500.0, 0.0, 0.0), true);
    }
    let c = sim.add_captain(Vec3::new(0.0, 0.0, 2000.0));
    let id = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(510.0, 0.0, 5.0), c).unwrap();
    sim.piki_mut(id).pos = Vec3::new(510.0, 0.0, 5.0);
    for _ in 0..5 {
        sim.step(&mut FlatHost);
    }
    let p = sim.piki(id).pos;
    ((p.x - 500.0).powi(2) + p.z.powi(2)).sqrt()
}

#[test]
fn a_pikmin_in_the_way_is_pushed_out_of_a_landed_ship() {
    let at = settled(false);
    assert!(at >= libpikmin::carry::SHIP_HULL_RADIUS, "{at}");
}

#[test]
fn a_ship_in_the_air_does_not_push() {
    assert!(settled(true) < libpikmin::carry::SHIP_HULL_RADIUS);
}
