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

    // Open, it twinkles; nothing was announced just for being there.
    assert!(view.take_treasure_cues().is_empty(), "buds are not announced for standing there");
    view.particles(Vec3::X, Vec3::Y);
    assert!(!view.particles.quads.is_empty(), "an open bud twinkles");
    // A Pikmin comes down in it: the sound and the puff.
    let p = sim.spawn_piki(PikiColor::Red, Vec3::new(200.0, 20.0, 0.0)).unwrap();
    sim.piki_mut(p).state = libpikmin::PikiState::Flying { ground_frames: 0 };
    sim.step(&mut Nothing);
    view.update(&sim, 1.0, TICK_DT);
    let cues = view.take_treasure_cues();
    assert_eq!(cues.iter().map(|c| c.id).collect::<Vec<_>>(), [libpikmin_present::bulb::ENTER]);
    let before = view.particles.quads.len();
    view.particles(Vec3::X, Vec3::Y);
    assert!(view.particles.quads.len() > before - 2, "the puff is drawn");
}

struct Nothing;

impl libpikmin::Host for Nothing {
    fn ground_height(&self, _: Vec3) -> Option<f32> {
        Some(0.0)
    }
}

#[test]
fn buds_far_from_every_captain_are_not_drawn() {
    let Some(assets) = Assets::discover(None) else { return };
    let p2 = Assets::default_dir().join("p2");
    if !libpikmin_present::bulb::BulbSet::available(&p2) {
        eprintln!("SKIP: no Pikmin 2 enemy cache");
        return;
    }
    let mut view = View::load(&assets).unwrap();
    view.load_bulbs(&p2).unwrap();
    let mut sim = Sim::default();
    // No captain: everything is drawn. With one, only the bud near him.
    sim.add_bulb(PikiColor::Red, false, Vec3::new(100.0, 0.0, 0.0));
    sim.add_bulb(PikiColor::Blue, false, Vec3::new(libpikmin_present::bulb::DRAW_DISTANCE * 2.0, 0.0, 0.0));
    view.update(&sim, 1.0, TICK_DT);
    let drawn = |view: &View| view.draws.iter().filter(|d| d.kind == kinds::PELLET).count();
    assert_eq!(drawn(&view), 2);
    sim.add_captain(Vec3::ZERO);
    view.update(&sim, 1.0, TICK_DT);
    assert_eq!(drawn(&view), 1, "only the bud near the captain");
}
