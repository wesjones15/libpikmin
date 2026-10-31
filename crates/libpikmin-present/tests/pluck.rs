use std::sync::Arc;
use glam::{Mat4, Vec3};
use libpikmin::{CaptainInput, Event, Params, PikiColor, Sim, TICK_DT};
use libpikmin::host::FlatHost;
use libpikmin_assets::{Assets, sound::{SoundData, tables::JaudioTables}};
use libpikmin_audio::SoundSystem;
use libpikmin_present::{View, view::kinds};

fn assets() -> Option<Assets> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("pikis/animMgr.bin").is_file() {
        eprintln!("SKIP: pluck disc assets absent"); return None;
    }
    Some(assets)
}

#[test]
fn pluck_keys_reach_the_shared_draw_path_with_animated_body_and_both_dirt_textures() {
    let Some(assets) = assets() else { return };
    let mut params = Params::default();
    params.load_p1_dir(&assets.root).unwrap();
    let mut sim = Sim::new(params, 9);
    let c = sim.add_captain(Vec3::ZERO);
    sim.plant_sprout(PikiColor::Red, Vec3::Z * 6.0);
    let mut view = View::load(&assets).unwrap();
    let mut before = None;
    let mut changed = false;
    let mut burst = false;
    let mut dirt_textures = std::collections::HashSet::new();
    for _ in 0..65 {
        sim.set_input(c, CaptainInput { throw: true, ..Default::default() });
        sim.step(&mut FlatHost);
        for e in sim.drain_events().collect::<Vec<_>>() {
            burst |= matches!(e, Event::PluckBurst { .. });
            view.on_event(&e);
        }
        view.update(&sim, 1.0, TICK_DT);
        view.particles(Vec3::X, Vec3::Y);
        if let Some(draw) = view.draws.iter().find(|d| d.kind == kinds::PIKI) {
            let vertices = view.positions[draw.first_vertex as usize..][..view.models[draw.model as usize].vertex_count].to_vec();
            assert!(vertices.iter().all(|p| p.is_finite()));
            if let Some(old) = &before { changed |= *old != vertices; }
            before = Some(vertices);
        }
        if burst { for q in &view.particles.quads { dirt_textures.insert(q.texture); } }
    }
    assert!(changed && burst);
    assert!(dirt_textures.len() >= 2, "both dirt cloud and debris reach the host");
}

#[test]
fn all_three_source_pluck_sounds_resolve_and_produce_pcm() {
    let Some(assets) = assets() else { return };
    let data = Arc::new(SoundData::load(&assets).unwrap());
    for name in ["SE_PIKI_PULLING", "SE_PIKI_PULLED2", "SE_PIKI_PULLED"] {
        let mut sys = SoundSystem::new(data.clone(), JaudioTables::load(&assets).unwrap());
        sys.set_listener(Mat4::IDENTITY, Vec3::ZERO);
        assert!(sys.play(name, Vec3::ZERO), "missing {name}");
        let mut audible = false;
        for _ in 0..60 {
            sys.frame();
            let pcm = sys.render_native(TICK_DT);
            assert!(pcm.iter().all(|f| f.is_finite()));
            audible |= pcm.iter().any(|f| f.abs() > 0.001);
        }
        assert!(audible, "silent {name}");
    }
}
