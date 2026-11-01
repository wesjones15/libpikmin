//! A squad Pikmin left on a lower level whose slot is up a cliff: there is no pathfinding, so it
//! would press against the cliff forever. The sim puts it at its slot after a couple of seconds.
use libpikmin::host::Host;
use libpikmin::{Params, PikiColor, PikiState, Sim, Vec3};

/// Floor at y = 0 west of x = 0, and a 400-unit drop east of it with a vertical face.
struct Cliff;

impl Host for Cliff {
    fn ground_height(&self, pos: Vec3) -> Option<f32> {
        Some(if pos.x < 0.0 { 0.0 } else { -400.0 })
    }

    fn walls(&self, centre: Vec3, radius: f32) -> Vec3 {
        if centre.x - radius < 0.0 && centre.x > 0.0 {
            Vec3::new(radius, centre.y, centre.z)
        } else {
            centre
        }
    }
}

#[test]
fn a_squad_pikmin_stuck_below_a_cliff_is_brought_up_to_its_slot() {
    let mut sim = Sim::new(Params::default(), 41);
    let captain = sim.add_captain(Vec3::new(-60.0, 0.0, 0.0));
    let piki = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(40.0, -400.0, 0.0), captain).unwrap();
    for _ in 0..180 {
        sim.step(&mut Cliff);
    }
    let p = sim.piki(piki);
    assert!(p.pos.y > -50.0, "put on the captain's level, not left below: {:?}", p.pos);
    assert!(matches!(p.state, PikiState::Squad { .. }), "still following: {:?}", p.state);
    assert!(p.pos.distance(sim.captain(captain).pos) < 150.0, "back with the squad: {:?}", p.pos);
}
