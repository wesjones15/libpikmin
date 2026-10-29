//! A treasure pellet is drawn with its own Pikmin 2 model, sparkles and announces itself. Needs the Pikmin 1 assets and
//! the Pikmin 2 treasure cache (`libsms::extract::p2_treasure`); skipped without them.

use libpikmin::{PelletDesc, Sim, TICK_DT, TREASURE_PART_BASE, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::treasure::APPEAR;
use libpikmin_present::view::{kinds, models};

#[test]
fn a_treasure_pellet_is_drawn_with_its_own_model_and_sparkles() {
    let Some(assets) = Assets::discover(None) else { return };
    let p2 = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libsms/assets/p2");
    if !p2.join("treasure/config/otakara_config.txt").is_file() {
        eprintln!("SKIP: no Pikmin 2 treasure cache");
        return;
    }
    let mut view = View::load(&assets).unwrap();
    let before = view.models.len();
    let first = view.load_treasures(&p2).expect("treasures") as usize;
    assert_eq!((first, view.models.len()), (before, before + 188), "188 models appended");
    assert_eq!(view.load_treasures(&p2).unwrap() as usize, first, "loading twice is a no-op");
    assert!(view.models[first..].iter().all(|m| !m.batches.is_empty() && m.batch_texture.iter().any(|&t| t >= 0)));
    assert!(view.textures.list.len() > 188, "their bakes are in the view's textures");

    let mut sim = Sim::default();
    sim.add_ship(Vec3::ZERO, Vec3::X * 100.0, Vec3::Y * 60.0);
    let mut desc = PelletDesc::numbered(sim.params(), 1, libpikmin::PikiColor::Red, Vec3::X * 200.0, 0.0);
    desc.radius = 35.0;
    desc.ship_part = Some(TREASURE_PART_BASE + 5);
    sim.spawn_pellet(desc);
    // An ordinary ship part and a numbered pellet beside it keep their own models.
    let mut part = desc;
    part.pos = Vec3::X * 300.0;
    part.ship_part = Some(0);
    sim.spawn_pellet(part);
    view.update(&sim, 1.0, TICK_DT);
    let draws: Vec<_> = view.draws.iter().filter(|d| d.kind == kinds::PELLET).collect();
    assert_eq!(draws.len(), 2);
    assert!(draws.iter().any(|d| d.model as usize == first + 5), "treasure 5 drawn with model {}", first + 5);
    assert!(draws.iter().any(|d| (d.model as usize) < models::COUNT || (d.model as usize) < first), "the part keeps its model");
    assert!(view.positions.iter().all(|p| p.is_finite()));
    // The lowest point of the model rests on the pellet's ground.
    let t = draws.iter().find(|d| d.model as usize == first + 5).unwrap();
    let body = view.models[first + 5].vertex_count;
    let ys: Vec<f32> = view.positions[t.first_vertex as usize..t.first_vertex as usize + body].iter().map(|p| p.y).collect();
    assert!(ys.iter().cloned().fold(f32::INFINITY, f32::min).abs() < 1e-3 && ys.iter().cloned().fold(0.0, f32::max) > 1.0);
    // It announced itself once, and sparkles over the map.
    let cues = view.take_treasure_cues();
    assert_eq!(cues.iter().filter(|c| c.id == APPEAR).count(), 1, "{cues:?}");
    view.update(&sim, 1.0, TICK_DT);
    assert!(view.take_treasure_cues().is_empty(), "lying still is quiet");
    view.particles(Vec3::X, Vec3::Y);
    assert!(!view.particles.quads.is_empty(), "sparkle and the appearing dust");
}
