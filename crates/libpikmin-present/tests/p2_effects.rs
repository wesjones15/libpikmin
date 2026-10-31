use glam::{Mat4, Vec3};
use libpikmin_present::{
    effects::Flat,
    p2effects::{IDS, P2Effects},
};
fn load() -> Option<P2Effects> {
    let d = libpikmin_assets::Assets::default_dir().join("p2");
    if !d.join("effects/game.jpc").is_file() {
        eprintln!("SKIP: extracted JPC absent");
        return None;
    }
    Some(P2Effects::load(&d).unwrap())
}
#[test]
fn exact_enemy_emitters_animate_and_stop_without_invalid_geometry() {
    let Some(mut fx) = load() else { return };
    assert!(fx.missing.is_empty(), "missing {:?}", fx.missing);
    let mut flat = Flat::default();
    let mut early = 0;
    let mut late = 0;
    let mut refracts = false;
    for tick in 0..90 {
        if tick < 30 {
            for (i, &id) in IDS.iter().enumerate() {
                fx.request(
                    i as u32,
                    id,
                    Mat4::from_translation(Vec3::X * i as f32 * 30.0),
                    1.0,
                );
            }
        }
        fx.advance(tick);
        flat.quads.clear();
        fx.draw(&mut flat, Vec3::X, Vec3::Y);
        if tick == 10 {
            early = flat.quads.len();
        }
        if tick == 80 {
            late = flat.quads.len();
        }
        refracts |= flat.quads.iter().any(|q| q.refraction);
        assert!(flat.quads.len() < 30000, "unbounded particles at {tick}");
        for q in &flat.quads {
            assert!(q.corners.iter().all(|c| c.is_finite()));
            assert!(q.uvs.iter().flatten().all(|v| v.is_finite()));
            assert!(q.color.iter().all(|v| v.is_finite() && *v >= 0.0));
            assert!((q.texture as usize) < flat.textures.len());
        }
    }
    assert!(
        early > 0 && late < early,
        "emitters did not release: {early} -> {late}"
    );
    assert!(refracts, "Fiery Bulblax heat layer disappeared");
    assert!(flat.textures.iter().all(|t| t.sampler.is_some()
        && t.image.rgba.len() == (t.image.width * t.image.height * 4) as usize));
}
#[test]
fn followed_shell_times_out_without_refresh() {
    let Some(mut fx) = load() else { return };
    let mut flat = Flat::default();
    fx.follow(9, 0x9b, Vec3::ZERO, Vec3::X, 1.0, true);
    let mut first = 0;
    for tick in 0..100 {
        fx.advance(tick);
        flat.quads.clear();
        fx.draw(&mut flat, Vec3::X, Vec3::Y);
        if tick == 5 {
            first = flat.quads.len();
        }
    }
    assert!(first > 0);
    assert!(flat.quads.is_empty(), "orphan shell still emits");
}
