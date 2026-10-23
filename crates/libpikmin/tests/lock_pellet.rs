//! Locked onto a pellet (or a corpse, or a ship part) rather than an enemy: the cursor stands in front of
//! it, and a thrown Pikmin goes to it and starts carrying.

use libpikmin::host::FlatHost;
use libpikmin::carry::{PelletDesc, PelletId};
use libpikmin::{CaptainInput, Params, PikiColor, Sim, Vec3};

fn pellet(sim: &mut Sim, at: Vec3) -> PelletId {
    sim.spawn_pellet(PelletDesc {
        pos: at,
        yaw: 0.0,
        radius: 12.0,
        height: 20.0,
        min_carriers: 1,
        max_carriers: 4,
        seeds_matching: 2,
        seeds_other: 1,
        color: Some(PikiColor::Red),
        creature: None,
        ship_part: None,
    })
}

fn step(sim: &mut Sim, c: libpikmin::CaptainId, id: Option<PelletId>, throw: bool) {
    sim.set_lock_pellet(c, id);
    sim.set_input(c, CaptainInput { throw, ..Default::default() });
    sim.step(&mut FlatHost);
}

#[test]
fn the_cursor_stands_in_front_of_the_locked_pellet() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let pel = pellet(&mut sim, Vec3::new(0.0, 0.0, 200.0));
    step(&mut sim, c, None, false);
    assert!(sim.captain(c).lock_view.is_none());
    step(&mut sim, c, Some(pel), false);
    let cap = sim.captain(c);
    let view = cap.lock_view.expect("locked on");
    assert!((cap.cursor_world - Vec3::new(0.0, 10.0, 200.0)).length() < 0.5, "{:?}", cap.cursor_world);
    assert!(view.pos.z < 200.0 && view.pos.z > 0.0, "{:?}", view.pos);
    // Letting go gives the cursor back.
    step(&mut sim, c, None, false);
    assert!(sim.captain(c).lock_view.is_none());
}

#[test]
fn a_pikmin_thrown_at_the_locked_pellet_starts_carrying_it() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    let pel = pellet(&mut sim, Vec3::new(0.0, 0.0, 260.0));
    // Something to carry it to.
    sim.add_onion(Vec3::new(600.0, 0.0, 260.0), &[PikiColor::Red]);
    for x in [-20.0, 20.0, 0.0] {
        sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(x, 0.0, -30.0), c).unwrap();
    }
    for _ in 0..40 { step(&mut sim, c, Some(pel), false); }
    for _ in 0..30 { step(&mut sim, c, Some(pel), true); }
    let mut carried = false;
    for _ in 0..200 {
        step(&mut sim, c, Some(pel), false);
        carried |= !sim.pellet(pel).carriers.is_empty();
        if carried { break; }
    }
    let states: Vec<_> = sim.pikis().iter().map(|p| format!("{:?} {:?}", p.state, p.pos)).collect();
    assert!(carried, "nobody picked it up: {states:#?}");
}

#[test]
fn the_lock_lapses_when_the_pellet_is_gone() {
    let mut sim = Sim::new(Params::default(), 5);
    let c = sim.add_captain(Vec3::ZERO);
    step(&mut sim, c, Some(PelletId(999)), false);
    assert!(sim.captain(c).lock_view.is_none());
}
