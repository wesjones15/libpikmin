//! The view against the user's extracted assets (skipped when `assets/` isn't there).

use libpikmin::host::FlatHost;
use libpikmin::{Params, PelletDesc, PikiColor, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::{kinds, models};

fn assets() -> Option<Assets> {
    let a = Assets::discover(None);
    if a.is_none() {
        eprintln!("no extracted assets; skipping");
    }
    a
}

fn world(assets: &Assets) -> Sim {
    let mut params = Params::default();
    let _ = params.load_extracted(&assets.root);
    let mut sim = Sim::new(params, 7);
    let c = sim.add_captain(Vec3::ZERO);
    for (i, colour) in [PikiColor::Red, PikiColor::Yellow, PikiColor::Blue].into_iter().enumerate() {
        sim.spawn_piki_in_squad(colour, Vec3::new(-20.0 - 10.0 * i as f32, 0.0, 0.0), c);
    }
    sim.add_onion(Vec3::new(300.0, 0.0, 0.0), &[PikiColor::Red]);
    sim.spawn_pellet(PelletDesc {
        ship_part: None,
        pos: Vec3::new(0.0, 0.0, 200.0),
        yaw: 0.0,
        radius: 20.0,
        height: 10.0,
        min_carriers: 5,
        max_carriers: 10,
        seeds_matching: 2,
        seeds_other: 1,
        color: None,
        creature: None,
    });
    sim.plant_sprout(PikiColor::Yellow, Vec3::new(100.0, 0.0, 100.0));
    let mut host = FlatHost;
    for _ in 0..10 {
        sim.step(&mut host);
    }
    sim
}

#[test]
fn draws_everything_with_consistent_buffers() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).expect("models load");
    let sim = world(&assets);
    view.update(&sim, 0.5, TICK_DT);

    let count = |k: u8| view.draws.iter().filter(|d| d.kind == k).count();
    assert_eq!(count(kinds::CAPTAIN), 1);
    assert_eq!(count(kinds::PIKI), 3);
    assert_eq!(count(kinds::HAPPA), 3);
    assert_eq!(count(kinds::ONION), 1);
    assert_eq!(count(kinds::PELLET), 1);
    assert_eq!(count(kinds::SPROUT), 1);
    assert_eq!(count(kinds::CURSOR), 1);
    assert_eq!(count(kinds::MARKER), 1);

    let mut next_vertex = 0;
    let mut next_color = 0;
    for d in &view.draws {
        let vm = &view.models[d.model as usize];
        assert_eq!(d.first_vertex as usize, next_vertex, "{d:?}");
        assert_eq!(d.first_color as usize, next_color, "{d:?}");
        next_vertex += vm.vertex_count;
        next_color += vm.batches.len();
        assert!(d.pos.is_finite() && d.rot.is_finite(), "{d:?}");
    }
    assert_eq!(view.positions.len(), next_vertex);
    assert_eq!(view.normals.len(), next_vertex);
    assert_eq!(view.colors.len(), next_color);
    assert!(view.positions.iter().all(|p| p.is_finite()));
    assert!(view.colors.iter().flatten().all(|c| (0.0..=1.0).contains(c)));

    // A posed Olimar is about as tall as the bind pose says, standing on his feet.
    let d = view.draws.iter().find(|d| d.kind == kinds::CAPTAIN).unwrap();
    let verts = &view.positions[d.first_vertex as usize..][..view.models[models::OLIMAR as usize].vertex_count];
    let top = verts.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let bottom = verts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!(top - bottom > 15.0 && top - bottom < 80.0, "Olimar from {bottom} to {top}");
}

/// The Onion's colour comes from an animated TEV register at frame colour * 2; its flat
/// colours must follow the Onion's colour.
#[test]
fn onion_colours_follow_its_colour() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).expect("models load");
    let mut tint = Vec::new();
    for colour in [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow] {
        let mut sim = Sim::new(Params::default(), 1);
        sim.add_onion(Vec3::ZERO, &[colour]);
        view.update(&sim, 0.0, TICK_DT);
        let d = view.draws.iter().find(|d| d.kind == kinds::ONION).unwrap();
        let n = view.models[models::ONION as usize].batches.len();
        let cs = &view.colors[d.first_color as usize..][..n];
        let sum = cs.iter().fold([0.0f32; 3], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2]]);
        eprintln!("{colour:?}: {cs:?}");
        tint.push(sum);
    }
    let (blue, red, yellow) = (tint[0], tint[1], tint[2]);
    assert!(blue[2] > blue[0], "blue Onion isn't blue: {blue:?}");
    assert!(red[0] > red[2], "red Onion isn't red: {red:?}");
    assert!(yellow[1] > yellow[2], "yellow Onion isn't yellow: {yellow:?}");
}

fn average(t: &libpikmin_present::tev::Baked) -> [f32; 4] {
    let n = (t.rgba.len() / 4) as f32;
    let mut s = [0.0f32; 4];
    for px in t.rgba.as_chunks::<4>().0 {
        for c in 0..4 {
            s[c] += px[c] as f32 / 255.0 / n;
        }
    }
    s
}

/// Hosts get each material baked: a leaf's green comes from a second texture, a pellet's
/// colour from a register its pattern blends towards, and Olimar's helmet glass is see-through.
#[test]
fn materials_bake_their_colours() {
    use libpikmin_present::view::blend;
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).expect("models load");

    let leaf = &view.models[models::HAPPA[0] as usize];
    let id = leaf.batch_texture[0];
    assert!(id >= 0, "the leaf isn't textured");
    let g = average(&view.textures.list[id as usize]);
    assert!(g[1] > g[0] * 1.5 && g[1] > g[2] * 1.5, "the leaf isn't green: {g:?}");
    assert_eq!(leaf.batch_blend[0], blend::CUTOUT);

    let olimar = &view.models[models::OLIMAR as usize];
    assert!(olimar.batch_blend.contains(&blend::TRANSLUCENT), "no see-through helmet");
    for pellet in models::PELLETS {
        assert!(view.models[pellet as usize].batch_blend.iter().all(|&b| b == blend::OPAQUE), "pellet {pellet}");
    }

    // A red pellet's texture is red, a fresh variant of the pellet's material.
    let mut sim = Sim::new(Params::default(), 1);
    sim.spawn_pellet(PelletDesc {
        ship_part: None,
        pos: Vec3::ZERO,
        yaw: 0.0,
        radius: 20.0,
        height: 10.0,
        min_carriers: 1,
        max_carriers: 2,
        seeds_matching: 2,
        seeds_other: 1,
        color: Some(PikiColor::Red),
        creature: None,
    });
    view.update(&sim, 0.0, TICK_DT);
    let d = view.draws.iter().find(|d| d.kind == kinds::PELLET).unwrap();
    let id = view.texture_ids[d.first_color as usize];
    assert!(id >= 0);
    assert_ne!(id, view.models[d.model as usize].batch_texture[0]);
    let r = average(&view.textures.list[id as usize]);
    assert!(r[0] > r[2] * 1.5, "the red pellet isn't red: {r:?}");
    assert_eq!(view.texture_ids.len(), view.colors.len());
}
