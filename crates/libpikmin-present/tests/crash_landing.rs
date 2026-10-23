//! Ship Repair's first stage: the bare wreck comes down in the pose it lies in, not upright to be turned
//! over on touching the ground; a ship with parts on board lands upright as before.

use libpikmin::host::FlatHost;
use libpikmin::{Params, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::{kinds, models};

fn shape(view: &View) -> Vec<Vec3> {
    let d = view.draws.iter().find(|d| d.kind == kinds::SHIP).unwrap();
    let v = &view.positions[d.first_vertex as usize..][..view.models[models::SHIP as usize].vertex_count];
    let mid = v.iter().copied().sum::<Vec3>() / v.len() as f32;
    v.iter().map(|p| *p - mid).collect()
}

fn gap(a: &[Vec3], b: &[Vec3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a.distance(*b)).fold(0.0, f32::max)
}

fn landing_then_resting(parts: &[u32]) -> (Vec<Vec3>, Vec<Vec3>) {
    let assets = Assets::discover(None).unwrap();
    // As the game does, so the parts on board are known.
    libpikmin_present::view::set_repaired_ship(true);
    let mut view = View::load(&assets).unwrap();
    view.repair_mode = true;
    let mut sim = Sim::new(Params::default(), 13);
    let ship = sim.add_ship(Vec3::ZERO, Vec3::Z * 100.0, Vec3::Y * 74.0);
    for &p in parts {
        sim.give_ship_part(ship, p);
    }
    let air = Vec3::Y * 300.0;
    // Coming down.
    sim.set_onion_pose(ship, air, true);
    sim.set_onion_flight_motion(ship, true, 0.0);
    sim.step(&mut FlatHost);
    view.update(&sim, 1.0, TICK_DT);
    sim.set_onion_flight_motion(ship, true, 0.5);
    for _ in 0..5 { view.update(&sim, 1.0, TICK_DT); }
    let landing = shape(&view);
    // Down for good.
    sim.set_onion_pose(ship, Vec3::ZERO, false);
    for _ in 0..30 * 5 { sim.step(&mut FlatHost); view.update(&sim, 1.0, TICK_DT); }
    (landing, shape(&view))
}

#[test]
fn the_bare_wreck_lands_in_its_resting_pose() {
    if Assets::discover(None).is_none() { return; }
    let (landing, resting) = landing_then_resting(&[]);
    assert!(gap(&landing, &resting) < 1.0, "it flips on landing: {}", gap(&landing, &resting));
}

#[test]
fn a_ship_with_a_part_on_board_does_not_come_down_as_the_wreck() {
    if Assets::discover(None).is_none() { return; }
    let (_, wreck) = landing_then_resting(&[]);
    let (landing, _) = landing_then_resting(&[4]);
    assert!(gap(&landing, &wreck) > 5.0, "a ship with a part comes down upright, as it did");
}
