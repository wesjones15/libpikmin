use std::sync::Arc;
use glam::Vec3;
use libpikmin::{Params, Sim};
use libpikmin_assets::{Assets, sound::{SoundData, tables::JaudioTables}};
use libpikmin_audio::SoundSystem;
use libpikmin_present::{View, teki::{TekiInstance, PELLET_POSY}, view};

fn assets() -> Option<Assets> {
    let assets = Assets { root: Assets::default_dir() };
    if !assets.root.join("tekis/palm/palm.mod").is_file() { eprintln!("SKIP: Pellet Posy disc assets absent"); return None; }
    Some(assets)
}

#[test]
fn flower_materials_really_change_colour_without_changing_model_or_pose() {
    let Some(assets) = assets() else { return };
    view::set_teki_models(true);
    let mut view = View::load(&assets).unwrap();
    let sim = Sim::new(Params::default(), 1);
    let mut variants = Vec::new();
    for colour in 0..3 {
        view.set_tekis(&[TekiInstance { id: 1, kind: PELLET_POSY, pos: Vec3::ZERO, yaw: 0.0,
            motion: 8, frame: 5.0, flash: 0.0, scale: 1.0, alpha: 1.0, material_frame: Some(colour as f32), presentation: 0 }]);
        view.update(&sim, 1.0, 1.0 / 30.0);
        let draw = view.draws.iter().find(|d| d.kind == view::kinds::TEKI).unwrap();
        let n = view.models[draw.model as usize].batches.len();
        let ids = &view.texture_ids[draw.first_color as usize..draw.first_color as usize+n];
        let bytes: Vec<u8> = ids.iter().filter(|&&i| i >= 0).flat_map(|&i| view.textures.list[i as usize].rgba.iter().copied()).collect();
        variants.push(bytes);
    }
    assert!(variants[0] != variants[1] && variants[1] != variants[2] && variants[0] != variants[2]);
}

#[test]
fn posy_flint_and_nectar_sound_cues_resolve_and_render_audibly_from_the_disc() {
    let Some(assets) = assets() else { return };
    let data = Arc::new(SoundData::load(&assets).unwrap());
    for name in ["SE_FLOWER_DOWN", "SE_FLOWER_CONVULSION", "SE_FLOWER_GROW0", "SE_FLOWER_GROW1", "SE_FLOWER_GROW2", "SE_KOGANE_WALK", "SE_KOGANE_DAMAGE", "SE_KURIONE_WATER", "SEF_PIKI_DRINK"] {
        let tables = JaudioTables::load(&assets).unwrap();
        let mut system = SoundSystem::new(data.clone(), tables);
        system.set_listener(glam::Mat4::IDENTITY, Vec3::ZERO);
        assert!(system.play(name, Vec3::ZERO), "missing {name}");
        let mut pcm = Vec::new();
        for _ in 0..60 {
            system.frame();
            pcm.extend(system.render_native(1.0 / 30.0));
        }
        assert!(pcm.iter().all(|f| f.is_finite()));
        assert!(pcm.iter().any(|f| f.abs() > 0.001), "silent {name}");
    }
}

#[test]
fn nectar_drops_and_puddles_reach_the_shared_host_draw_list() {
    let Some(assets) = assets() else {return};
    let mut view=View::load(&assets).unwrap();
    let mut sim=Sim::new(Params::default(),1);
    sim.spawn_nectar_with_velocity(Vec3::new(0.0,80.0,0.0),Vec3::Y*200.0);
    sim.add_nectar_puddle(Vec3::new(100.0,0.0,0.0));
    view.update(&sim,1.0,1.0/30.0);
    let drop=view.draws.iter().find(|d|d.kind==view::kinds::NECTAR_DROP).expect("falling nectar is visible in RoR2's draw path");
    let puddle=view.draws.iter().find(|d|d.kind==view::kinds::NECTAR_PUDDLE).expect("drinkable puddle is visible in RoR2's draw path");
    assert_ne!(drop.model,puddle.model);
    assert_eq!(drop.pos.y,80.0,"retain the authored flying-drop origin");
    assert_eq!(puddle.pos.x,100.0);
    for draw in [drop,puddle] {assert!(view.models[draw.model as usize].vertex_count>0);}
}
