//! With the parts on the Dolphin and `set_repaired_ship`, the view draws them on its joints.

use libpikmin::host::FlatHost;
use libpikmin::{Params, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::{kinds, models, set_repaired_ship};

#[test]
fn the_repaired_dolphin_is_drawn_with_its_parts_and_the_wreck_without() {
    let Some(assets) = Assets::discover(None) else { return };
    set_repaired_ship(true);
    let mut view = View::load(&assets).unwrap();
    assert!(view.models.len() > models::COUNT, "part models loaded");
    let mut params = Params::default();
    let _ = params.load_extracted(&assets.root);
    let mut sim = Sim::new(params, 5);
    let ship = sim.add_ship(Vec3::new(0.0, 0.0, 300.0), Vec3::new(0.0, 0.0, 400.0), Vec3::new(0.0, 74.0, 300.0));
    sim.add_captain(Vec3::ZERO);
    sim.step(&mut FlatHost);
    view.update(&sim, 1.0, TICK_DT);
    let parts = |v: &View| v.draws.iter().filter(|d| d.kind == kinds::SHIP_PART).count();
    assert_eq!(parts(&view), 0, "a wreck has no parts on board");
    for part in 0..30 {
        sim.give_ship_part(ship, part);
    }
    // A new View starts the ship's animator with the parts fitted (it starts once per ship).
    let mut view = View::load(&assets).unwrap();
    for _ in 0..3 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
    }
    let n = parts(&view);
    // The 16 exterior parts (soto, fuzoku) have a joint on the ship; the 14 inside (naka) are not seen.
    assert_eq!(n, 16, "the exterior parts are drawn on the ship");
    assert!(view.positions.iter().all(|p| p.is_finite()));
    let ship_draw = view.draws.iter().find(|d| d.kind == kinds::SHIP).unwrap();
    for d in view.draws.iter().filter(|d| d.kind == kinds::SHIP_PART) {
        assert_eq!((d.pos, d.rot), (ship_draw.pos, ship_draw.rot), "parts share the ship's frame");
    }
    assert!(view.draws.iter().any(|d| d.kind == kinds::SHIP), "the ship itself too");
}
