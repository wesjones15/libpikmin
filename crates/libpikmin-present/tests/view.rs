//! The view against the user's extracted assets (skipped when `assets/` isn't there).

use libpikmin::host::FlatHost;
use libpikmin::{Params, PelletDesc, PikiColor, PikiState, Sim, TICK_DT, Vec3};
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

#[test]
fn ice_climbers_hide_growth_attachments_without_affecting_normal_pikmin() {
    use libpikmin::{CaptainInput, Happa};
    use libpikmin_present::pikachu::SKIN_PIKACHU;
    let Some(assets) = assets() else { return };
    let melee = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../libssbm/assets");
    if !libssbm_assets::disc::ice_climbers_present(&melee) {
        eprintln!("SKIP: no Melee cache with the Ice Climbers");
        return;
    }
    let mut view = View::load(&assets).unwrap();
    let pikachu = view.load_pikachu(&melee).unwrap();
    let mut sim = Sim::default();
    let captains = [sim.add_captain(Vec3::ZERO), sim.add_captain(Vec3::X * 200.0)];
    for captain in captains {
        for (i, stage) in [Happa::Leaf, Happa::Bud, Happa::Flower].into_iter().enumerate() {
            let id = sim.spawn_piki_in_squad(PikiColor::from_index(i), Vec3::new(i as f32 * 20.0, 0.0, captain.0 as f32 * 100.0), captain).unwrap();
            sim.piki_mut(id).happa = stage;
        }
        sim.set_input(captain, CaptainInput { stick: libpikmin::Vec2::Y, ..Default::default() });
    }
    view.set_captain_skin(captains[0].0, SKIN_PIKACHU);
    for _ in 0..3 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
        let pikis: Vec<_> = view.draws.iter().filter(|d| d.kind == kinds::PIKI).collect();
        assert_eq!(pikis.len(), 6);
        assert_eq!(pikis.iter().filter(|d| d.model > pikachu).count(), 3);
        let growth: Vec<_> = view.draws.iter().filter(|d| d.kind == kinds::HAPPA).collect();
        assert_eq!(growth.len(), 3, "only the ordinary captain's Pikmin have attachments");
        for model in models::HAPPA {
            assert!(growth.iter().any(|d| d.model == model));
        }
    }
    // Skin changes restore all growth stages, then hide the climbers' again.
    for (skin, attachments) in [(0, 6), (SKIN_PIKACHU, 3)] {
        view.set_captain_skin(captains[0].0, skin);
        view.update(&sim, 1.0, TICK_DT);
        assert_eq!(view.draws.iter().filter(|d| d.kind == kinds::HAPPA).count(), attachments);
    }
}

#[test]
fn distant_pose_budget_keeps_draws_and_clocks_but_skips_vertex_work() {
    use libpikmin_present::view::flags::RETAIN_POSE;
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::default();
    sim.spawn_piki(PikiColor::Red, Vec3::X * 5000.0).unwrap();
    view.update(&sim, 1.0, TICK_DT);
    let full_vertices = view.positions.len();
    assert!(full_vertices > 0);
    let draws = view.draws.len();
    view.pose_budget = Some((Vec3::ZERO, 3500.0));
    let mut fresh = 0;
    for _ in 0..6 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
        assert_eq!(view.draws.len(), draws);
        let retained = view.draws.iter().all(|d| d.flags & RETAIN_POSE != 0);
        if retained { assert!(view.positions.is_empty()); }
        else { fresh += 1; assert_eq!(view.positions.len(), full_vertices); }
    }
    assert_eq!(fresh, 2);
    view.pose_budget = None;
    view.update(&sim, 1.0, TICK_DT);
    assert_eq!(view.positions.len(), full_vertices);
    assert!(view.draws.iter().all(|d| d.flags & RETAIN_POSE == 0));
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

/// Whistling streams the whistle's particles; a Pikmin that dies leaves a rising ghost.
#[test]
fn whistle_and_ghosts_make_particles() {
    use libpikmin::captain::CaptainInput;
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).expect("models load");
    assert!(view.effects.is_some(), "effects didn't load");
    let mut sim = Sim::new(Params::default(), 3);
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(-20.0, 0.0, 0.0), c).expect("room for a Pikmin");
    let mut host = FlatHost;
    let (right, up) = (Vec3::X, Vec3::Y);
    let mut step = |sim: &mut Sim, view: &mut View, input: CaptainInput| {
        sim.set_input(c, input);
        sim.step(&mut host);
        for e in sim.drain_events() {
            view.on_event(&e);
        }
        view.update(sim, 0.0, TICK_DT);
        view.particles(right, up);
    };

    for _ in 0..10 {
        step(&mut sim, &mut view, CaptainInput { whistle: true, ..Default::default() });
    }
    let whistle = view.particles.quads.len();
    assert!(whistle > 0, "no whistle particles");
    for q in &view.particles.quads {
        assert!((q.texture as usize) < view.particles.textures.len());
        assert!(q.corners.iter().all(|c| c.is_finite()));
    }

    // Let the whistle run down, then kill the Pikmin and wait for its ghost.
    for _ in 0..90 {
        step(&mut sim, &mut view, CaptainInput::default());
    }
    sim.kill_piki(p);
    let mut ghost = 0;
    for _ in 0..120 {
        step(&mut sim, &mut view, CaptainInput::default());
        ghost = ghost.max(view.particles.quads.len());
    }
    assert!(ghost > 0, "no ghost after the Pikmin died");
}

/// Olimar's helmet is environment mapped: its texture coordinates follow the camera, stay on
/// the map, and its alpha is lit.
#[test]
fn the_helmet_reflects_the_camera() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).expect("models load");
    let olimar = &view.models[models::OLIMAR as usize];
    let env: Vec<usize> = (0..olimar.batches.len()).filter(|&b| olimar.batch_env[b].is_some()).collect();
    assert!(!env.is_empty(), "nothing on Olimar is environment mapped");
    assert!(env.iter().all(|&b| olimar.batch_light(b) & 2 != 0), "the helmet's alpha isn't lit");
    let first: u32 = olimar.batches[..env[0]].iter().map(|b| b.vertices.len() as u32).sum();
    let count = olimar.batches[env[0]].vertices.len();

    let mut sim = Sim::new(Params::default(), 1);
    sim.add_captain(Vec3::ZERO);
    let uvs_from = |view: &mut View, right: Vec3, up: Vec3, fwd: Vec3| {
        view.set_camera(right, up, fwd);
        view.update(&sim, 0.0, TICK_DT);
        let d = view.draws.iter().find(|d| d.kind == kinds::CAPTAIN).unwrap();
        view.uvs[(d.first_vertex + first) as usize..][..count].to_vec()
    };
    let front = uvs_from(&mut view, Vec3::X, Vec3::Y, Vec3::Z);
    let above = uvs_from(&mut view, Vec3::X, Vec3::Z, -Vec3::Y);
    assert!(front.iter().flatten().all(|c| (-0.01..=1.01).contains(c)), "off the map: {front:?}");
    assert_ne!(front, above, "the reflection doesn't move with the camera");
}

#[test]
fn cursors_can_be_hidden() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).expect("models load");
    let sim = world(&assets);
    view.show_cursors = false;
    view.update(&sim, 0.5, TICK_DT);
    let count = |k: u8| view.draws.iter().filter(|d| d.kind == k).count();
    assert_eq!(count(kinds::CAPTAIN), 1);
    assert_eq!(count(kinds::CURSOR) + count(kinds::MARKER), 0);
}

#[test]
fn dolphin_and_unified_onion_draw_in_flight_and_after_landing_without_parts() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 13);
    let ship = sim.add_ship(Vec3::ZERO, Vec3::Z * 100.0, Vec3::Y * 74.0);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Blue, PikiColor::Red, PikiColor::Yellow]);
    for colour in [PikiColor::Red, PikiColor::Yellow, PikiColor::Blue] {
        sim.set_onion_color(onion, colour);
        assert_eq!(sim.onions()[onion.0 as usize].colors.len(), 3);
        assert_eq!(sim.onions()[onion.0 as usize].colors[0], colour);
        for flying in [true, false] {
            let pos = if flying { Vec3::Y * 500.0 } else { Vec3::ZERO };
            sim.set_onion_pose(ship, pos, flying);
            sim.set_onion_pose(onion, pos + Vec3::X * 200.0, flying);
            sim.step(&mut FlatHost);
            view.update(&sim, 1.0, TICK_DT);
            let d = view.draws.iter().find(|d| d.kind == kinds::SHIP).unwrap();
            assert_eq!(d.model, models::SHIP);
            assert_eq!(d.pos, pos);
            assert!(sim.onions()[ship.0 as usize].ship.as_ref().unwrap().parts.is_empty());
            assert_eq!(view.draws.iter().filter(|d| d.kind == kinds::ONION).count(), 1);
            assert!(view.positions.iter().all(|p| p.is_finite()));
            let vertices = &view.positions[d.first_vertex as usize..][..view.models[models::SHIP as usize].vertex_count];
            let bounds = vertices.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            assert!((bounds.1 - bounds.0).max_element() > 50.0, "Dolphin must have visible geometry");
        }
    }
}

#[test]
fn entry_pikmin_follow_the_onions_posed_leg_not_its_sampled_frame() {
    let Some(assets) = assets() else { return };
    for color in [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow] {
        // A fresh view per colour: ids repeat between sims and the view's interpolation
        // state is keyed by id.
        let mut view = View::load(&assets).unwrap();
        let mut sim = Sim::new(Params::default(), 31);
        let captain = sim.add_captain(Vec3::ZERO);
        let at = Vec3::new(20.0, 120.0, -30.0);
        let onion = sim.add_onion(at, &[color]);
        sim.set_onion_pose(onion, at, true);
        let piki = sim.spawn_piki_in_squad(color, Vec3::new(at.x, 0.0, at.z), captain).unwrap();
        assert_eq!(sim.onion_put_away(captain, onion, color, 1), 1);
        let mut checked = false;
        for _ in 0..600 {
            sim.step(&mut FlatHost);
            let p = sim.piki(piki);
            if let PikiState::Enter { onion, leg, climbing: true, ratio } = p.state {
                view.update(&sim, 1.0, TICK_DT);
                let draw = view.draws.iter().find(|d| d.kind == kinds::PIKI).expect("the climber is drawn");
                // The sim's loop is sampled; the drawn leg is the actual pose, which turns
                // through the loop and differs by colour. The Pikmin must be on the drawn leg.
                let on_leg = view.onion_leg_point(onion, leg, ratio).expect("the Onion's legs were drawn");
                assert!(draw.pos.distance(on_leg) < 0.01,
                    "{color:?} climber at {:?}, drawn leg at {:?}", draw.pos, on_leg);
                checked = true;
                break;
            }
        }
        assert!(checked, "{color:?} Pikmin climbed onto the Onion");
    }
}

/// The jump between the walk and the climb is drawn continuously with both. Mirroring the
/// jump's world start (or subtracting the sampled midpoint and then adding the drawn one)
/// pops the Pikmin across the Onion at takeoff and again when the climb begins.
#[test]
fn the_onion_jump_is_continuous_with_the_walk_and_the_climb() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 33);
    sim.set_mirror_onion_legs(true); // the Unity host: models and leg points are mirrored
    let captain = sim.add_captain(Vec3::ZERO);
    let at = Vec3::new(20.0, 120.0, -30.0);
    let onion = sim.add_onion(at, &[PikiColor::Red]);
    sim.set_onion_pose(onion, at, true);
    // Offset from the Onion's axis, where mirroring the start would show up as a long pop.
    let piki = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::new(at.x - 60.0, 0.0, at.z + 40.0), captain).unwrap();
    assert_eq!(sim.onion_put_away(captain, onion, PikiColor::Red, 1), 1);
    let mut last: Option<(Vec3, PikiState, Vec3)> = None;
    let mut climbed = false;
    for _ in 0..600 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
        let p = sim.piki(piki);
        let draw = view.draws.iter().find(|d| d.kind == kinds::PIKI)
            .expect("the returning Pikmin is drawn").pos;
        if let Some((prev, prev_state, prev_sim)) = last {
            // The jump covers its whole arc in 0.65 s; one tick is at most a tenth of it.
            assert!(draw.distance(prev) < 25.0,
                "tick {}: the drawn Pikmin jumped {:.1} units ({prev:?} -> {draw:?}); sim {prev_sim:?} -> {:?}; state {prev_state:?} -> {:?}",
                sim.tick(), draw.distance(prev), p.pos, p.state);
        }
        last = Some((draw, p.state.clone(), p.pos));
        if let PikiState::Enter { climbing: true, .. } = p.state { climbed = true; break; }
    }
    assert!(climbed, "the Pikmin reached the climb");
}

#[test]
fn host_flight_uses_articulated_disc_clips_without_cinematic_position_drift() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 13);
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Blue, PikiColor::Red, PikiColor::Yellow]);
    let ship = sim.add_ship(Vec3::ZERO, Vec3::Z * 100.0, Vec3::Y * 74.0);
    let air = Vec3::Y * 150.0;
    let vertices = |view: &View, kind| {
        let draw = view.draws.iter().find(|d| d.kind == kind).unwrap();
        view.positions[draw.first_vertex as usize..][..view.models[draw.model as usize].vertex_count].to_vec()
    };
    for colour in [PikiColor::Blue, PikiColor::Red, PikiColor::Yellow] {
        sim.set_onion_color(onion, colour);
        sim.set_onion_pose(onion, air, true);
        view.update(&sim, 1.0, TICK_DT);
        let first = vertices(&view, kinds::ONION);
        for _ in 0..140 { // crosses the flight-loop seam twice
            // The sim owns the airborne loop's frame now (the Pikmin's leg ropes use it).
            sim.step(&mut FlatHost);
            view.update(&sim, 1.0, TICK_DT);
            assert!(view.positions.iter().all(|v| v.is_finite()));
        }
        let later = vertices(&view, kinds::ONION);
        assert!(first.iter().zip(&later).any(|(a, b)| a.distance(*b) > 0.2), "flying Onion must animate");
        assert!(later.iter().all(|v| v.length() < 600.0), "flight must not climb thousands of cinematic units");
        let mut landing = Vec::new();
        for progress in [0.0, 0.4, 0.8, 1.0] {
            sim.set_onion_flight_motion(onion, true, progress);
            view.update(&sim, 1.0, TICK_DT);
            let player = view.animator.players[&libpikmin_present::animator::onion_key(onion.0 as usize)];
            assert_eq!(player.anim, libpikmin::anim::ANIM_GOAL_LAND + colour.index());
            landing.push(vertices(&view, kinds::ONION));
        }
        assert!(landing[0].iter().zip(&landing[2]).any(|(a, b)| a.distance(*b) > 1.0), "landing must deploy legs");
    }
    let mut frames = Vec::new();
    for progress in [0.0, 0.2, 0.6, 0.9] {
        sim.set_onion_pose(ship, air, true);
        sim.set_onion_flight_motion(ship, true, progress);
        for _ in 0..10 { view.update(&sim, 1.0, TICK_DT); }
        frames.push(vertices(&view, kinds::SHIP));
        assert!(view.positions.iter().all(|v| v.is_finite()));
        view.particles(Vec3::X, Vec3::Y);
        if progress == 0.2 { assert!(!view.particles.quads.is_empty(), "rocket flames must render"); }
    }
    assert!(frames[0].iter().zip(&frames[2]).any(|(a, b)| a.distance(*b) > 0.2), "rocket landing must animate its skeleton");
    assert!(frames.iter().flatten().all(|v| v.length() < 1000.0), "host supplies rocket travel exactly once");
    assert!(view.demo_sounds.iter().any(Option::is_some), "rocket sound starts");
    sim.set_onion_pose(ship, Vec3::ZERO, false);
    view.update(&sim, 1.0, TICK_DT);
    assert!(view.demo_sounds.contains(&None), "rocket sound stops at touchdown");
}

#[test]
fn dolphin_touches_ground_without_inverting_at_touchdown() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 13);
    let ship = sim.add_ship(Vec3::ZERO, Vec3::Z * 100.0, Vec3::Y * 74.0);
    let vertices = |view: &View| {
        let d = view.draws.iter().find(|d| d.kind == kinds::SHIP).unwrap();
        view.positions[d.first_vertex as usize..][..view.models[d.model as usize].vertex_count].to_vec()
    };
    sim.set_onion_pose(ship, Vec3::ZERO, true);
    sim.set_onion_flight_motion(ship, true, 0.9999);
    view.update(&sim, 1.0, TICK_DT);
    let before = vertices(&view);
    sim.set_onion_pose(ship, Vec3::ZERO, false);
    for _ in 0..60 {
        view.update(&sim, 1.0, TICK_DT);
        let after = vertices(&view);
        let bottom = after.iter().map(|v| v.y).fold(f32::INFINITY, f32::min);
        assert!(bottom.abs() < 0.05, "rocket bottom must touch terrain: {bottom}");
        let jump = before.iter().zip(&after).map(|(a,b)| a.distance(*b)).fold(0.0, f32::max);
        assert!(jump < 0.1, "touchdown must retain the upright pose: {jump}");
    }
}

#[test]
fn jumps_use_disc_geyser_and_pikmin_airborne_motions_then_resume_walking() {
    use libpikmin::CaptainInput;
    use libpikmin_assets::motion::Motion;
    use libpikmin_present::animator::{captain_key, piki_key};
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 35);
    let c = sim.add_captain(Vec3::ZERO);
    let p = sim.spawn_piki_in_squad(PikiColor::Red, Vec3::X * -60.0, c).unwrap();
    sim.step(&mut FlatHost);
    view.update(&sim, 1.0, TICK_DT);
    let vertices = |view: &View, kind| {
        let d = view.draws.iter().find(|d| d.kind == kind).unwrap();
        view.positions[d.first_vertex as usize..][..view.models[d.model as usize].vertex_count].to_vec()
    };
    let standing_c = vertices(&view, kinds::CAPTAIN);
    let standing_p = vertices(&view, kinds::PIKI);
    sim.set_input(c, CaptainInput { jump: true, ..Default::default() });
    let mut poses_c = Vec::new();
    let mut poses_p = Vec::new();
    for _ in 0..8 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
        assert_eq!(view.animator.players[&captain_key(c)].anim, Motion::OCarry.anim_index().unwrap());
        assert_eq!(view.animator.players[&piki_key(sim.piki(p))].anim, Motion::RollJmp.anim_index().unwrap());
        poses_c.push(vertices(&view, kinds::CAPTAIN));
        poses_p.push(vertices(&view, kinds::PIKI));
        assert!(view.positions.iter().all(|p| p.is_finite()));
    }
    for (standing, poses) in [(standing_c, poses_c), (standing_p, poses_p)] {
        assert!(standing.iter().zip(&poses[0]).any(|(a,b)| a.distance(*b) > 0.1), "airborne pose differs from walking");
        assert!(poses[0].iter().zip(&poses[7]).any(|(a,b)| a.distance(*b) > 0.1), "jump animation articulates");
        assert!(poses.iter().flatten().all(|v| v.length() < 80.0), "animation must not add cinematic travel");
    }
    for _ in 0..90 { sim.step(&mut FlatHost); }
    view.update(&sim, 1.0, TICK_DT);
    assert_ne!(view.animator.players[&captain_key(c)].anim, Motion::OCarry.anim_index().unwrap());
    assert_ne!(view.animator.players[&piki_key(sim.piki(p))].anim, Motion::RollJmp.anim_index().unwrap());
}

#[test]
fn a_flying_onion_leans_into_its_travel_and_lands_upright() {
    let Some(assets) = assets() else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::default();
    let onion = sim.add_onion(Vec3::ZERO, &[PikiColor::Red]);
    let mut at = Vec3::new(0.0, 100.0, 0.0);
    sim.set_onion_pose(onion, at, true);
    view.update(&sim, 1.0, TICK_DT);
    let tilt = |view: &View| view.draws.iter().find(|d| d.kind == kinds::ONION).unwrap().rot;
    assert!(tilt(&view).angle_between(glam::Quat::IDENTITY) < 1e-3, "still at first");
    // 20 sim units a tick is 600 a second: a full lean.
    for _ in 0..12 {
        at.x += 20.0;
        sim.set_onion_pose(onion, at, true);
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
    }
    let lean = tilt(&view);
    let (axis, angle) = lean.to_axis_angle();
    assert!(angle > 0.15, "leans into its travel: {angle}");
    assert!(axis.z < -0.9 && axis.y.abs() < 0.1, "a pitch about the travel's horizontal axis: {axis:?}");
    // Landed, it stands upright again.
    sim.set_onion_pose(onion, at.with_y(0.0), false);
    for _ in 0..24 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
    }
    assert!(tilt(&view).angle_between(glam::Quat::IDENTITY) < 0.02, "upright once landed");
}
