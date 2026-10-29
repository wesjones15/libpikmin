//! A Candypop Bud is drawn with Pikmin 2's model in its colour and follows its phases. Needs the Pikmin 1 assets and the
//! Pikmin 2 enemy cache (`libsms::extract::p2_enemy`); skipped without them.

use libpikmin::{PikiColor, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::kinds;

#[test]
fn buds_are_drawn_in_their_colour_and_their_petals_move() {
    let Some(assets) = Assets::discover(None) else {
        eprintln!("SKIP: no Pikmin 1 assets");
        return;
    };
    let p2 = Assets::default_dir().join("p2");
    if !libpikmin_present::bulb::BulbSet::available(&p2) {
        eprintln!("SKIP: no Pikmin 2 enemy cache");
        return;
    }
    let mut view = View::load(&assets).unwrap();
    let before = view.models.len();
    let first = view.load_bulbs(&p2).expect("buds") as usize;
    assert_eq!((first, view.models.len()), (before, before + 5), "five models appended");
    assert_eq!(view.load_bulbs(&p2).unwrap() as usize, first, "loading twice is a no-op");

    let mut sim = Sim::default();
    sim.add_bulb(PikiColor::Purple, false, Vec3::new(200.0, 0.0, 0.0));
    sim.add_bulb(PikiColor::Blue, true, Vec3::new(300.0, 0.0, 0.0));
    view.update(&sim, 1.0, TICK_DT);
    let draws: Vec<_> = view.draws.iter().filter(|d| d.kind == kinds::PELLET).collect();
    assert_eq!(draws.len(), 2);
    assert!(draws.iter().any(|d| d.model as usize == first + PikiColor::Purple.index()));
    assert!(draws.iter().any(|d| d.model as usize == first + PikiColor::Blue.index()));
    assert!(view.positions.iter().all(|p| p.is_finite()));

    // Later the same bud is open and unfolded: the vertices have moved.
    let at = |view: &View, model: usize| -> Vec<Vec3> {
        let d = view.draws.iter().find(|d| d.model as usize == model).unwrap();
        view.positions[d.first_vertex as usize..d.first_vertex as usize + view.models[model].vertex_count].to_vec()
    };
    let opening = at(&view, first + PikiColor::Purple.index());
    for _ in 0..40 {
        sim.step(&mut Nothing);
        view.update(&sim, 1.0, TICK_DT);
    }
    let open = at(&view, first + PikiColor::Purple.index());
    assert!(opening.iter().zip(&open).any(|(a, b)| a.distance(*b) > 1.0), "the petals unfolded");
}

struct Nothing;

impl libpikmin::Host for Nothing {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
}
